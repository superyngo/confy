// Plain-Node test for touch/app.ts's external-edit sheet (T9 of
// docs/plan/2026-09-14-raw-write-mode.md, RS3 — R18/R19). Touch keeps the
// existing bottom sheet for every path, including the empty path used by a
// whole-file edit (R18, unchanged routing); this spec covers the one new
// rule R18 costs (R19): a failed whole-file Apply must keep the sheet open
// with the buffer intact, instead of closing like a per-node edit does.
// Follows raw-write.spec.mjs's convention: `openExternalEdit` is extracted
// verbatim from source and type-stripped via esbuild — these checks run the
// real shipped function body, not a reimplementation.
import path from "node:path";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import * as esbuild from "esbuild";

const here = path.dirname(fileURLToPath(import.meta.url));

let failures = 0;
function check(name, cond, extra = "") {
  if (cond) {
    console.log(`  \u2713 ${name}`);
  } else {
    failures++;
    console.log(`  \u2717 ${name} ${extra}`);
  }
}

const appTs = readFileSync(path.join(here, "touch/app.ts"), "utf8");
const fnMatch = appTs.match(/^function openExternalEdit\([\s\S]*?\n\}/m)?.[0];
check("openExternalEdit extracted verbatim", !!fnMatch);
check("openExternalEdit's Apply handler checks doc_revision (R19 failure detection)", /doc_revision/.test(fnMatch ?? ""));
check(
  "openExternalEdit's Apply handler does not unconditionally close before sending (R19)",
  !/^\s*closeSheets\(\);\s*\n\s*if \(kind\.Value\)/m.test(fnMatch ?? ""),
);

const src = `let snap, sheetsExt;
let sendFn, openSheetFn, closeSheetsFn;
const sheets = { get ext() { return sheetsExt; } };
function t(k) { return k; }
function tArgs(k, args) { return k + ":" + JSON.stringify(args); }
function esc(s) { return s; }
function lastKey(p) {
  const s = p[p.length - 1];
  if (!s) return "";
  return "Key" in s ? s.Key : \`[\${s.Index}]\`;
}
const IC = { close: "x" };
function send(i) { return sendFn(i); }
function openSheet(name) { return openSheetFn(name); }
function closeSheets() { return closeSheetsFn(); }
export function setEnv(e) {
  if ("snap" in e) snap = e.snap;
  if ("sheetsExt" in e) sheetsExt = e.sheetsExt;
  if ("send" in e) sendFn = e.send;
  if ("openSheet" in e) openSheetFn = e.openSheet;
  if ("closeSheets" in e) closeSheetsFn = e.closeSheets;
}
export ${fnMatch}
`;

const built = await esbuild.build({
  stdin: { contents: src, resolveDir: here, loader: "ts" },
  write: false,
  format: "esm",
  target: "es2022",
});
const mod = await import("data:text/javascript;base64," + Buffer.from(built.outputFiles[0].text).toString("base64"));

// ---- fakes: an .ext sheet with a textarea + an apply button, wired the way
// the real DOM is (innerHTML then querySelector for ".ext-text"/".ext-apply") ----
function mkClassList() {
  const set = new Set();
  return {
    add: (c) => set.add(c),
    remove: (c) => set.delete(c),
    contains: (c) => set.has(c),
  };
}
function mkExtSheet() {
  const txt = { value: "", focus() {} };
  const applyBtn = { onclick: null };
  return {
    classList: mkClassList(),
    innerHTML: "",
    querySelector(sel) {
      if (sel === ".ext-text") return txt;
      if (sel === ".ext-apply") return applyBtn;
      return null;
    },
    _txt: txt,
    _applyBtn: applyBtn,
  };
}

let sent, openSheetCalls, closeSheetsCalls;
function freshEnv() {
  sent = [];
  openSheetCalls = [];
  closeSheetsCalls = 0;
  const sheetsExt = mkExtSheet();
  mod.setEnv({
    snap: { clipboard_count: 0 },
    sheetsExt,
    send: (i) => sent.push(i),
    openSheet: (n) => openSheetCalls.push(n),
    closeSheets: () => closeSheetsCalls++,
  });
  return sheetsExt;
}

// ---- 1. R18: every path (including []) routes to the sheet ----
console.log("-- openExternalEdit(): R18 routes every path, including [] --");
{
  const sheetsExt = freshEnv();
  mod.openExternalEdit({ initial: "whole file text", kind: { Value: { path: [] } } });
  check("the sheet opens for the empty path", openSheetCalls.includes("ext"));
  check("the textarea is seeded with the initial text", sheetsExt._txt.value === "whole file text");
}
{
  const sheetsExt = freshEnv();
  mod.openExternalEdit({ initial: "leaf value", kind: { Value: { path: [{ Key: "name" }] } } });
  check("the sheet opens for a per-node path", openSheetCalls.includes("ext"));
  check("the textarea is seeded with the initial text (per-node)", sheetsExt._txt.value === "leaf value");
}

// ---- 2. R19: a failed whole-file Apply keeps the sheet open, buffer intact ----
console.log("\n-- openExternalEdit(): R19 failed whole-file Apply --");
{
  const sheetsExt = freshEnv();
  mod.setEnv({
    snap: { clipboard_count: 0, doc_revision: 5 },
    send: (i) => {
      sent.push(i);
      // Failure: doc_revision does not move.
    },
  });
  mod.openExternalEdit({ initial: "before", kind: { Value: { path: [] } } });
  sheetsExt._txt.value = "unparsable {{{ text";
  sheetsExt._applyBtn.onclick();
  check("ApplyReplace was dispatched with the empty path", JSON.stringify(sent[0]) === JSON.stringify({ ApplyReplace: { path: [], text: "unparsable {{{ text" } }));
  check("closeSheets was NOT called on a failed whole-file Apply (buffer stays open)", closeSheetsCalls === 0);
}

// ---- 3. R19 control case: a succeeding whole-file Apply closes normally ----
console.log("\n-- openExternalEdit(): a succeeding whole-file Apply closes --");
{
  const sheetsExt = freshEnv();
  let rev = 5;
  mod.setEnv({
    snap: { clipboard_count: 0, get doc_revision() { return rev; } },
    send: (i) => {
      sent.push(i);
      rev = 6; // Success: doc_revision moves.
    },
  });
  mod.openExternalEdit({ initial: "before", kind: { Value: { path: [] } } });
  sheetsExt._txt.value = "valid = true";
  sheetsExt._applyBtn.onclick();
  check("closeSheets WAS called after a successful whole-file Apply", closeSheetsCalls === 1);
}

// ---- 4. A per-node Apply is a Block commit, and a REJECTED one keeps the
//         sheet open holding the user's text (BEHAVIOR_MATRIX §6.3). This
//         case asserted the opposite until the Block switchover: closing
//         unconditionally is exactly the lost-work bug R19 named, and there
//         is no longer a reason to tolerate it per-node. ----
console.log("\n-- openExternalEdit(): a per-node Apply is a Block commit --");
{
  const sheetsExt = freshEnv();
  mod.setEnv({
    snap: { clipboard_count: 0, doc_revision: 5 }, // stays flat = rejected
  });
  mod.openExternalEdit({ initial: "name = 1\n", kind: { Value: { path: [{ Key: "name" }] } } });
  sheetsExt._txt.value = "other = 1\n";
  sheetsExt._applyBtn.onclick();
  check(
    "ApplyBlockText was dispatched with the node's path",
    JSON.stringify(sent[0]) === JSON.stringify({ ApplyBlockText: { path: [{ Key: "name" }], text: "other = 1\n" } }),
  );
  check("a rejected Block keeps the sheet open", closeSheetsCalls === 0);
  check("a rejected Block keeps the user's own text in the buffer", sheetsExt._txt.value === "other = 1\n");
  check("the error is never written into the buffer", !sheetsExt._txt.value.includes("not applied"));
}

// ---- 4b. The accepted Block closes, same signal (doc_revision moved) ----
console.log("\n-- openExternalEdit(): an accepted Block closes --");
{
  const sheetsExt = freshEnv();
  let rev = 5;
  mod.setEnv({
    snap: { clipboard_count: 0, get doc_revision() { return rev; } },
    send: (i) => { sent.push(i); rev = 6; },
  });
  mod.openExternalEdit({ initial: "name = 1\n", kind: { Value: { path: [{ Key: "name" }] } } });
  sheetsExt._txt.value = "renamed = 1\n";
  sheetsExt._applyBtn.onclick();
  check("an accepted Block closes the sheet", closeSheetsCalls === 1);
}

// ---- 5. Comment edits (always per-node — never empty path) keep closing unconditionally ----
console.log("\n-- openExternalEdit(): a comment edit still closes unconditionally --");
{
  const sheetsExt = freshEnv();
  mod.setEnv({ snap: { clipboard_count: 0, doc_revision: 5 } });
  mod.openExternalEdit({ initial: "a comment", kind: { Comment: { path: [{ Key: "name" }] } } });
  sheetsExt._txt.value = "an edited comment";
  sheetsExt._applyBtn.onclick();
  check("closeSheets is still called unconditionally for a comment edit", closeSheetsCalls === 1);
  check("ApplyEditComment was dispatched", JSON.stringify(sent[0]) === JSON.stringify({ ApplyEditComment: { path: [{ Key: "name" }], text: "an edited comment" } }));
}

console.log(failures === 0 ? "\nALL TOUCH EXT-APPLY CHECKS PASSED" : `\n${failures} FAILURE(S)`);
process.exit(failures === 0 ? 0 : 1);
