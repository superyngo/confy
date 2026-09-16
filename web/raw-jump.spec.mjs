// Plain-Node test for ui.ts's Raw breadcrumb jump (T8 of
// docs/plan/2026-09-14-raw-write-mode.md, RS2c — R12-R17/R29). Follows
// raw-write.spec.mjs's convention: `byteToCodeUnit` is pure and DOM-free, so
// `text-offset.ts` is imported directly; `ui.ts`'s
// `jumpSelectRawSpan`/`renderRawControls`/`revertRawEdit` are extracted verbatim and
// type-stripped via esbuild, run against a fake `document`/`window`.
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

const textOffsetBuilt = await esbuild.build({
  entryPoints: [path.join(here, "text-offset.ts")],
  bundle: true,
  write: false,
  format: "esm",
  target: "es2022",
});
const { byteToCodeUnit } = await import(
  "data:text/javascript;base64," + Buffer.from(textOffsetBuilt.outputFiles[0].text).toString("base64")
);

// ---- 1. byteToCodeUnit: the F5/T2.3 CJK+emoji fixture (drift === 8) ----
console.log("-- byteToCodeUnit() --");
{
  // note = "設定檔 🎉 comment"\ntarget = "needle"\n
  const text = 'note = "設定檔 🎉 comment"\ntarget = "needle"\n';
  const byteOffset = Buffer.byteLength(text.slice(0, text.indexOf("target")), "utf8");
  const codeUnitOffset = text.indexOf("target");
  const drift = byteOffset - codeUnitOffset;
  check("drift is exactly 8 on the CJK+emoji fixture", drift === 8, drift);
  check(
    "byteToCodeUnit converts the byte offset back to the correct code-unit index",
    byteToCodeUnit(text, byteOffset) === codeUnitOffset,
    byteToCodeUnit(text, byteOffset),
  );
}
{
  const ascii = "a = 1\nb = 2\n";
  check("byteToCodeUnit is the identity on pure ASCII", byteToCodeUnit(ascii, 6) === 6);
}
{
  const text = "x = \"🎉\"\n";
  const afterEmoji = text.indexOf("\"", text.indexOf("🎉"));
  const byteOffset = Buffer.byteLength(text.slice(0, afterEmoji), "utf8");
  check(
    "byteToCodeUnit handles a lone astral surrogate pair mid-string",
    byteToCodeUnit(text, byteOffset) === afterEmoji,
  );
}

// ---- 2. jumpSelectRawSpan / renderRawControls / revertRawEdit ----
const uiTs = readFileSync(path.join(here, "ui.ts"), "utf8");
const names = ["jumpSelectRawSpan", "scrollRawToOffset", "renderRawControls", "revertRawEdit"];
const fns = names.map((n) => uiTs.match(new RegExp(`^function ${n}\\([\\s\\S]*?\\n\\}`, "m"))?.[0]);
fns.forEach((s, i) => check(`${names[i]} extracted verbatim`, !!s));

const src = `let snap, session, rawState = "off", rawWriteBaseline = null, statusEl, VSHOST = false, staleTree = false;
// The jump sets this so the caret → cursor sync (raw-caret-sync.spec.mjs)
// does not bounce our own selection back as a cursor move.
let rawJumpLatch = false;
function t(key) { return key; }
function byteToCodeUnit(text, byteOffset) { return byteOffset; } // ASCII-only fixtures below
${fns[0]}
${fns[1]}
${fns[2]}
${fns[3]}
export { jumpSelectRawSpan, renderRawControls, revertRawEdit, setEnv, latch };
function setEnv(e) { snap = e.snap; session = e.session; rawState = e.rawState; rawWriteBaseline = e.rawWriteBaseline; statusEl = e.statusEl; VSHOST = e.vshost ?? false; staleTree = e.staleTree ?? false; rawJumpLatch = false; }
function latch() { return rawJumpLatch; }
`;

const built = await esbuild.build({
  stdin: { contents: src, loader: "ts" },
  bundle: false,
  write: false,
  format: "esm",
  target: "es2022",
});
const mod = await import("data:text/javascript;base64," + Buffer.from(built.outputFiles[0].text).toString("base64"));

function mkClassList() {
  const s = new Set();
  return { toggle: (c, on) => (on ? s.add(c) : s.delete(c)), contains: (c) => s.has(c) };
}
function mkEl(overrides = {}) {
  return {
    value: "", textContent: "", disabled: false, classList: mkClassList(), focus() {},
    setSelectionRange() {}, scrollTop: 0, clientHeight: 300,
    ...overrides,
  };
}

let els;
let statusTextCalls;
function freshGlobalEnv(spans, opts = {}) {
  // One element in both Raw states (2026-09-14): no `#raw` `<pre>`, no
  // Range/Selection branch — the jump is `setSelectionRange` + an explicit
  // scroll in view mode exactly as in write mode.
  els = {
    rawEdit: mkEl({ value: opts.editValue ?? "text" }),
    rawControls: mkEl(),
    btnRawEdit: mkEl(),
    btnRawEditLabel: mkEl(),
    btnRawCancel: mkEl(),
  };
  statusTextCalls = [];
  globalThis.$ = (id) => els[id];
  globalThis.getComputedStyle = () => ({ lineHeight: "20px", paddingTop: "8px" });
  // `spanOf` replaced the `outline()` walk (2026-09-14): outline omits
  // Comment nodes, so a jump to a comment row used to be a silent no-op.
  const sessionStub = { spanOf: (p) => spans[JSON.stringify(p)], serialize: () => opts.text ?? "target = \"needle\"\n" };
  mod.setEnv({ snap: {}, session: sessionStub, rawState: opts.rawState ?? "view", rawWriteBaseline: opts.baseline ?? "text", statusEl: { set textContent(v) { statusTextCalls.push(v); } }, vshost: opts.vshost, staleTree: opts.staleTree });
}

// ---- Both Raw states: a jump selects the node's span and scrolls to it ----
console.log("\n-- jumpSelectRawSpan(): one path for view and write --");
for (const state of ["view", "write"]) {
  const spans = { '[{"Key":"target"}]': [0, 17] };
  freshGlobalEnv(spans, { rawState: state, editValue: "text", baseline: "text" });
  let sel = null;
  els.rawEdit.setSelectionRange = (s, e) => (sel = [s, e]);
  mod.jumpSelectRawSpan([{ Key: "target" }]);
  check(`${state}: setSelectionRange gets the node's span`, sel && sel[0] === 0 && sel[1] === 17, JSON.stringify(sel));
  check(`${state}: R29/F5 selects the whole member's span`, sel && sel[1] === 17);
  check(`${state}: no status text set (clean buffer)`, statusTextCalls.length === 0);
  check(`${state}: the jump arms rawJumpLatch (Q4 loop guard)`, mod.latch() === true);
}
{
  // The defect fixed 2026-09-14: `outline()` omits Comment nodes, so the old
  // `findOutlineByPath(session.outline(), path)` lookup made a jump to a
  // comment row a silent no-op (measured: selection unmoved, scrollTop 0, no
  // status). `span_of` answers for comments too — the host asks per path now.
  const commentPath = [{ Index: 0 }];
  freshGlobalEnv({ '[{"Index":0}]': [0, 6] }, { rawState: "view", text: "# lead\nname = 1\n" });
  let sel = null;
  els.rawEdit.setSelectionRange = (s, e) => (sel = [s, e]);
  mod.jumpSelectRawSpan(commentPath);
  check("a comment row's span is selected, not skipped", sel && sel[0] === 0 && sel[1] === 6, JSON.stringify(sel));
}
{
  // An unknown path is still a no-op (core returns undefined).
  freshGlobalEnv({}, { rawState: "view" });
  let selCalled = false;
  els.rawEdit.setSelectionRange = () => (selCalled = true);
  mod.jumpSelectRawSpan([{ Key: "missing" }]);
  check("an unresolvable path selects nothing", !selCalled);
}
{
  // The scroll is explicit: `setSelectionRange` alone never scrolls (measured
  // 2026-09-14 — the span landed 4.9k px below the viewport in both states).
  // Line 20 of the text, 20px lines, 8px padding, 300px pane → 8+400-100.
  const text = Array.from({ length: 40 }, (_, i) => `line_${i} = ${i}`).join("\n") + "\n";
  const offset = text.split("\n").slice(0, 20).join("\n").length + 1;
  freshGlobalEnv({ '[{"Key":"line_20"}]': [offset, offset + 12] }, { rawState: "view", text });
  mod.jumpSelectRawSpan([{ Key: "line_20" }]);
  check("the span's line is scrolled a third of the pane down", els.rawEdit.scrollTop === 8 + 20 * 20 - 100, els.rawEdit.scrollTop);
}
{
  // A span already near the top clamps at 0 rather than scrolling negative.
  freshGlobalEnv({ '[{"Key":"target"}]': [0, 17] }, { rawState: "view" });
  mod.jumpSelectRawSpan([{ Key: "target" }]);
  check("a span at the top clamps the scroll at 0", els.rawEdit.scrollTop === 0);
}

// ---- R17: Raw write, dirty buffer — reports instead of moving the caret ----
console.log("\n-- jumpSelectRawSpan(): R17 dirty write buffer is gated --");
{
  freshGlobalEnv({ '[{"Key":"target"}]': [0, 17] }, { rawState: "write", editValue: "edited text", baseline: "text" });
  let selCalled = false;
  els.rawEdit.setSelectionRange = () => (selCalled = true);
  mod.jumpSelectRawSpan([{ Key: "target" }]);
  check("setSelectionRange is never called on a dirty buffer", !selCalled);
  check("the pane is not scrolled on a dirty buffer", els.rawEdit.scrollTop === 0);
  check("status reports web.raw.jump-needs-apply instead", statusTextCalls.length === 1);
}

// ---- renderRawControls(): the primary action + Cancel (2026-09-15) ----
console.log("\n-- renderRawControls(): a primary action plus Cancel --");
{
  freshGlobalEnv({}, { rawState: "off" });
  mod.renderRawControls();
  check("band hidden when rawState is off", els.rawControls.classList.contains("hidden"));
}
{
  freshGlobalEnv({}, { rawState: "view" });
  mod.renderRawControls();
  check("band shown in view", !els.rawControls.classList.contains("hidden"));
  check("the primary control reads Edit while viewing", els.btnRawEditLabel.textContent === "web.raw.controls.edit");
  check("the primary control's title matches its label", els.btnRawEdit.title === "web.raw.controls.edit");
  check("Edit carries the accent fill in view", els.btnRawEdit.classList.contains("primary"));
  check("the primary control is enabled in view mode", els.btnRawEdit.disabled === false);
  check("cancel is present, never hidden", !els.btnRawCancel.classList.contains("hidden"));
  check("cancel is disabled in view mode", els.btnRawCancel.disabled === true);
}
{
  // Write mode, clean buffer: both controls are still the way OUT of the mode.
  freshGlobalEnv({}, { rawState: "write", editValue: "text", baseline: "text" });
  mod.renderRawControls();
  check("the primary control reads Apply while editing", els.btnRawEditLabel.textContent === "web.raw.controls.apply");
  check("Apply drops the accent fill, level with Cancel", !els.btnRawEdit.classList.contains("primary"));
  check("apply is enabled on a clean buffer (it is also the exit)", els.btnRawEdit.disabled === false);
  check("cancel is enabled on a clean buffer (it is also the exit)", els.btnRawCancel.disabled === false);
}
{
  freshGlobalEnv({}, { rawState: "write", editValue: "edited", baseline: "text" });
  mod.renderRawControls();
  check("a dirty buffer changes nothing about the enable rule", els.btnRawEdit.disabled === false && els.btnRawCancel.disabled === false);
}
{
  // Whole-document editing is now supported under VS Code too (the Raw
  // pane's Apply routes through the same Session mutation + `edit` message
  // as every other edit, so VS Code's TextDocument stays the single owner).
  freshGlobalEnv({}, { rawState: "view", vshost: true });
  mod.renderRawControls();
  check("the primary control is enabled under VSHOST", els.btnRawEdit.disabled === false);
  check("the primary control is not hidden under VSHOST", !els.btnRawEdit.classList.contains("hidden"));
  check("the band is still shown under VSHOST", !els.rawControls.classList.contains("hidden"));
}

{
  // Stale tree (VS Code: the side-by-side text doesn't parse): entering write
  // mode then would produce an Apply that `notifyHost` never posts, so the
  // primary control is disabled — while an already-open write mode keeps its
  // way out (Apply/Cancel both still exit).
  freshGlobalEnv({}, { rawState: "view", vshost: true, staleTree: true });
  mod.renderRawControls();
  check("the Edit control is disabled while the tree is stale", els.btnRawEdit.disabled === true);
}
{
  freshGlobalEnv({}, { rawState: "write", vshost: true, staleTree: true, editValue: "text", baseline: "text" });
  mod.renderRawControls();
  check("a write mode already open stays exitable while stale", els.btnRawEdit.disabled === false && els.btnRawCancel.disabled === false);
}

// ---- revertRawEdit(): Cancel's buffer half (the mode half is cancelRawEdit,
//      covered in raw-write.spec.mjs) ----
console.log("\n-- revertRawEdit(): Apply's mirror image --");
{
  freshGlobalEnv({}, { rawState: "write", editValue: "edited", baseline: "text" });
  els.rawEdit.scrollTop = 500;
  mod.revertRawEdit();
  check("the buffer returns to the last applied text", els.rawEdit.value === "text");
  check("the scroll position survives the re-seed", els.rawEdit.scrollTop === 500);
}
{
  freshGlobalEnv({}, { rawState: "view", editValue: "whatever", baseline: "text" });
  mod.revertRawEdit();
  check("revert is inert in Raw view", els.rawEdit.value === "whatever");
}


console.log(failures === 0 ? "\nALL RAW JUMP CHECKS PASSED" : `\n${failures} FAILURE(S)`);
process.exit(failures === 0 ? 0 : 1);
