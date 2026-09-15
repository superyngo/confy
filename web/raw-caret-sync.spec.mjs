// Plain-Node test for the Raw pane's caret → cursor sync (Q4, the inverse of
// the breadcrumb jump covered by raw-jump.spec.mjs). Same convention as its
// sibling: `codeUnitToByte` is pure and DOM-free so `text-offset.ts` is
// imported directly, while `onRawCaretMove`/`syncCursorFromRawCaret` are
// extracted verbatim from ui.ts and type-stripped via esbuild.
import path from "node:path";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import * as esbuild from "esbuild";

const here = path.dirname(fileURLToPath(import.meta.url));

let failures = 0;
function check(name, cond, extra = "") {
  if (cond) console.log(`  ok   ${name}`);
  else {
    failures++;
    console.log(`  FAIL ${name}${extra === "" ? "" : ` — got ${JSON.stringify(extra)}`}`);
  }
}

const textOffsetBuilt = await esbuild.build({
  entryPoints: [path.join(here, "text-offset.ts")],
  bundle: false,
  write: false,
  format: "esm",
  target: "es2022",
});
const { byteToCodeUnit, codeUnitToByte } = await import(
  "data:text/javascript;base64," + Buffer.from(textOffsetBuilt.outputFiles[0].text).toString("base64")
);

// ---- 1. codeUnitToByte: byteToCodeUnit's inverse on the same fixture ----
console.log("-- codeUnitToByte() --");
{
  const text = 'note = "設定檔 🎉 comment"\ntarget = "needle"\n';
  const codeUnitOffset = text.indexOf("target");
  const byteOffset = Buffer.byteLength(text.slice(0, codeUnitOffset), "utf8");
  check(
    "codeUnitToByte matches Buffer.byteLength on the CJK+emoji fixture",
    codeUnitToByte(text, codeUnitOffset) === byteOffset,
    codeUnitToByte(text, codeUnitOffset),
  );
  check(
    "codeUnitToByte round-trips byteToCodeUnit",
    byteToCodeUnit(text, codeUnitToByte(text, codeUnitOffset)) === codeUnitOffset,
  );
}
{
  const ascii = "a = 1\nb = 2\n";
  check("codeUnitToByte is the identity on pure ASCII", codeUnitToByte(ascii, 6) === 6);
  check("codeUnitToByte(0) is 0", codeUnitToByte(ascii, 0) === 0);
  check(
    "an offset past the end clamps to the whole text's byte length",
    codeUnitToByte(ascii, 999) === Buffer.byteLength(ascii, "utf8"),
  );
}
{
  // The caret can only sit before or after a surrogate pair, never inside it.
  const text = 'x = "🎉"\n';
  const after = text.indexOf('"', text.indexOf("🎉")); // code unit after the pair
  check(
    "codeUnitToByte counts an astral code point as 4 bytes",
    codeUnitToByte(text, after) === Buffer.byteLength(text.slice(0, after), "utf8"),
  );
}

// ---- 2. onRawCaretMove / syncCursorFromRawCaret ----
const uiTs = readFileSync(path.join(here, "ui.ts"), "utf8");
const names = ["onRawCaretMove", "syncCursorFromRawCaret"];
const fns = names.map((n) => uiTs.match(new RegExp(`^function ${n}\\([\\s\\S]*?\\n\\}`, "m"))?.[0]);
fns.forEach((s, i) => check(`${names[i]} extracted verbatim`, !!s));

const src = `let snap, session, rawState = "off", rawWriteBaseline = null, rawJumpLatch = false;
let rawCaretTimer = 0;
let sent = [];
function send(i) { sent.push(i); }
function codeUnitToByte(text, n) { return n; } // ASCII-only fixtures below
${fns[0]}
${fns[1]}
export { onRawCaretMove, syncCursorFromRawCaret, setEnv, takeSent, setLatch };
function setEnv(e) { snap = e.snap; session = e.session; rawState = e.rawState; rawWriteBaseline = e.rawWriteBaseline; rawJumpLatch = false; sent = []; }
function setLatch(v) { rawJumpLatch = v; }
function takeSent() { return sent; }
`;

const built = await esbuild.build({
  stdin: { contents: src, loader: "ts" },
  bundle: false,
  write: false,
  format: "esm",
  target: "es2022",
});
const mod = await import("data:text/javascript;base64," + Buffer.from(built.outputFiles[0].text).toString("base64"));

const TEXT = 'a = 1\n[srv]\nport = 2\n';
// Offset -> path, standing in for core's `node_at_offset`: `a` owns 0..5,
// `[srv]` 6..20 with the `port` leaf 12..20 inside it.
function nodeAtOffset(off) {
  if (off < 5) return [{ Key: "a" }];
  if (off >= 12 && off < 20) return [{ Key: "srv" }, { Key: "port" }];
  if (off >= 6 && off < 20) return [{ Key: "srv" }];
  return undefined;
}

let els;
function freshEnv(opts = {}) {
  els = { rawEdit: { value: opts.editValue ?? TEXT, selectionStart: opts.caret ?? 0 } };
  globalThis.$ = (id) => els[id];
  globalThis.window = { setTimeout: (fn, ms) => setTimeout(fn, ms) };
  mod.setEnv({
    snap: { cursor: opts.cursor ?? [{ Key: "a" }] },
    session: { nodeAtOffset, serialize: () => TEXT },
    rawState: opts.rawState ?? "view",
    rawWriteBaseline: opts.baseline ?? TEXT,
  });
}

console.log("\n-- syncCursorFromRawCaret(): offset → path → RevealPath --");
{
  freshEnv({ caret: 14, cursor: [{ Key: "a" }] });
  mod.syncCursorFromRawCaret();
  const sent = mod.takeSent();
  check(
    "a caret inside the section body reveals the innermost node",
    JSON.stringify(sent) === JSON.stringify([{ RevealPath: [{ Key: "srv" }, { Key: "port" }] }]),
    sent,
  );
}
{
  freshEnv({ caret: 7, cursor: [{ Key: "a" }] });
  mod.syncCursorFromRawCaret();
  check(
    "a caret on the header line reveals the branch node",
    JSON.stringify(mod.takeSent()) === JSON.stringify([{ RevealPath: [{ Key: "srv" }] }]),
  );
}

console.log("\n-- the three loop guards --");
{
  // Guard 3: resolving to the node already under the cursor dispatches nothing.
  freshEnv({ caret: 2, cursor: [{ Key: "a" }] });
  mod.syncCursorFromRawCaret();
  check("identity short-circuit: same node as the cursor sends nothing", mod.takeSent().length === 0);
}
{
  // Guard 1: a jump we performed must not bounce back as a caret move, and
  // the latch is consumed so the *next* real move still syncs.
  freshEnv({ caret: 14, cursor: [{ Key: "a" }] });
  mod.setLatch(true);
  mod.syncCursorFromRawCaret();
  check("latch swallows the caret move our own jump caused", mod.takeSent().length === 0);
  mod.syncCursorFromRawCaret();
  check("the latch is one-shot: the next move syncs again", mod.takeSent().length === 1);
}
{
  // Guard 2: a drag-select or held arrow key collapses to one dispatch.
  freshEnv({ caret: 14, cursor: [{ Key: "a" }] });
  mod.onRawCaretMove();
  mod.onRawCaretMove();
  mod.onRawCaretMove();
  check("debounce: nothing has been sent synchronously", mod.takeSent().length === 0);
  await new Promise((r) => setTimeout(r, 90));
  check("debounce: three caret moves collapse into one dispatch", mod.takeSent().length === 1);
}

console.log("\n-- gates --");
{
  freshEnv({ caret: 14, rawState: "off" });
  mod.syncCursorFromRawCaret();
  check("Raw off: no sync at all", mod.takeSent().length === 0);
}
{
  // R17's reason, applied to this direction: text_ranges describe the last
  // commit, so they do not describe an uncommitted write buffer.
  freshEnv({ caret: 14, rawState: "write", editValue: "dirty", baseline: TEXT });
  mod.syncCursorFromRawCaret();
  check("Raw write with a dirty buffer: no sync", mod.takeSent().length === 0);
}
{
  freshEnv({ caret: 14, rawState: "write", editValue: TEXT, baseline: TEXT });
  mod.syncCursorFromRawCaret();
  check("Raw write with a clean buffer: syncs like view mode", mod.takeSent().length === 1);
}
{
  freshEnv({ caret: 5, cursor: [{ Key: "a" }] });
  mod.syncCursorFromRawCaret();
  check("an offset between nodes (the blank gap) sends nothing", mod.takeSent().length === 0);
}

console.log(failures === 0 ? "\nALL RAW CARET SYNC CHECKS PASSED" : `\n${failures} FAILURE(S)`);
process.exit(failures === 0 ? 0 : 1);
