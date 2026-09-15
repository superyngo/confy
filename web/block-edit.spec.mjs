// Plain-Node test for ui.ts's external-edit modal after the Block switchover
// (docs/plan/2026-09-15-block-edit-whole-file-reparse-plan.md Task 7;
// BEHAVIOR_MATRIX.md §6.3). The rule under test is the one the TUI expresses
// by re-spawning `$EDITOR`: a **rejected** Block must leave the pop-up open
// holding the user's own text, with the error on the notice channel and never
// inside the buffer.
//
// Follows touch-ext-apply.spec.mjs's convention: `openExternalEdit` is
// extracted verbatim from source and type-stripped via esbuild, so these
// checks run the real shipped function body rather than a reimplementation.
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

const uiTs = readFileSync(path.join(here, "ui.ts"), "utf8");
const fnMatch = uiTs.match(/^function openExternalEdit\([\s\S]*?\n\}/m)?.[0];
check("openExternalEdit extracted verbatim", !!fnMatch);
check("its confirm handler commits a Block", /ApplyBlockText/.test(fnMatch ?? ""));
check("success is read off doc_revision, not the notice (F4)", /doc_revision/.test(fnMatch ?? ""));
check(
  "it no longer closes before knowing the outcome",
  !/close\(\);\s*\n\s*if \(kind\.Value\)/.test(fnMatch ?? ""),
);

// A DOM stub narrow enough to run the real body: the four elements it touches.
const src = `let snap, sendFn, els;
function $(id) { return els[id]; }
function send(i) { return sendFn(i); }
export function setEnv(e) {
  if ("snap" in e) snap = e.snap;
  if ("send" in e) sendFn = e.send;
  if ("els" in e) els = e.els;
}
export ${fnMatch}
`;
const out = esbuild.transformSync(src, { loader: "ts", format: "esm" }).code;
const mod = await import(`data:text/javascript;base64,${Buffer.from(out).toString("base64")}`);

function freshEnv() {
  const classes = new Set(["hidden"]);
  const els = {
    "ext-modal": {
      classList: {
        add: (c) => classes.add(c),
        remove: (c) => classes.delete(c),
        contains: (c) => classes.has(c),
      },
    },
    "ext-text": { value: "", focus() { this.focused = (this.focused ?? 0) + 1; }, onkeydown: null },
    "ext-confirm": { onclick: null },
    "ext-cancel": { onclick: null },
  };
  return { els, isOpen: () => !classes.has("hidden") };
}

const sent = [];

// ---- 1. A rejected Block: pop-up stays open, text preserved ----
console.log("\n-- openExternalEdit(): a rejected Block keeps the pop-up and the text --");
{
  const env = freshEnv();
  mod.setEnv({
    els: env.els,
    snap: { doc_revision: 5 }, // stays flat = rejected
    send: (i) => sent.push(i),
  });
  mod.openExternalEdit({ initial: "k1 = 1\n", kind: { Value: { path: [{ Key: "k1" }] } } });
  check("the pop-up opened", env.isOpen());
  check("the textarea is seeded with the Block", env.els["ext-text"].value === "k1 = 1\n");
  env.els["ext-text"].value = "k2 = 7\n"; // a duplicate key — core rejects it
  env.els["ext-confirm"].onclick();
  check(
    "ApplyBlockText was dispatched with the node's path",
    JSON.stringify(sent[0]) === JSON.stringify({ ApplyBlockText: { path: [{ Key: "k1" }], text: "k2 = 7\n" } }),
  );
  check("a rejected Block keeps the pop-up open", env.isOpen());
  check("a rejected Block keeps the user's own text", env.els["ext-text"].value === "k2 = 7\n");
  check("the error is never written into the buffer", !env.els["ext-text"].value.includes("not applied"));
  check("focus returns to the textarea for the retry", (env.els["ext-text"].focused ?? 0) >= 2);
}

// ---- 2. An accepted Block (a key rename) closes ----
console.log("\n-- openExternalEdit(): an accepted Block closes --");
{
  const env = freshEnv();
  sent.length = 0;
  let rev = 5;
  mod.setEnv({
    els: env.els,
    snap: { get doc_revision() { return rev; } },
    send: (i) => { sent.push(i); rev = 6; },
  });
  mod.openExternalEdit({ initial: "k1 = 1\n", kind: { Value: { path: [{ Key: "k1" }] } } });
  env.els["ext-text"].value = "renamed = 1\n"; // the gesture the old route dropped
  env.els["ext-confirm"].onclick();
  check("an accepted Block closes the pop-up", !env.isOpen());
  check("the rename crossed the wire", sent[0]?.ApplyBlockText?.text === "renamed = 1\n");
}

// ---- 3. A Comment row is a Block too (no separate route any more) ----
console.log("\n-- openExternalEdit(): a comment is committed as a Block --");
{
  const env = freshEnv();
  sent.length = 0;
  let rev = 5;
  mod.setEnv({ els: env.els, snap: { get doc_revision() { return rev; } }, send: (i) => { sent.push(i); rev = 6; } });
  mod.openExternalEdit({ initial: "# c\n", kind: { Value: { path: [{ Index: 0 }] } } });
  env.els["ext-text"].value = "c = 1\n"; // un-commenting, which the old route could not express
  env.els["ext-confirm"].onclick();
  check("ApplyBlockText was dispatched for a comment row", !!sent[0]?.ApplyBlockText);
  check("no comment-only intent is used any more", !sent.some((i) => i.ApplyEditComment));
  check("it closes on success", !env.isOpen());
}

console.log(failures === 0 ? "\nALL BLOCK-EDIT CHECKS PASSED" : `\n${failures} FAILURE(S)`);
process.exit(failures === 0 ? 0 : 1);
