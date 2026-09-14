// Plain-Node test for ui.ts's Raw breadcrumb jump (T8 of
// docs/plan/2026-09-14-raw-write-mode.md, RS2c — R12-R17/R29). Follows
// raw-write.spec.mjs's convention: `byteToCodeUnit`/`findOutlineByPath` are
// pure and DOM-free, so `text-offset.ts` is imported directly; `ui.ts`'s
// `jumpSelectRawSpan`/`renderRawControls` are extracted verbatim and
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
const { byteToCodeUnit, findOutlineByPath } = await import(
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

// ---- 2. findOutlineByPath: nested lookup, whole-member range (R29) ----
console.log("\n-- findOutlineByPath() --");
{
  const tree = [
    { key: "a", path: [{ Key: "a" }], type_label: "table", value: undefined, text_range: [0, 5], key_text_range: undefined, children: [
      { key: "b", path: [{ Key: "a" }, { Key: "b" }], type_label: "string", value: "x", text_range: [10, 20], key_text_range: [10, 11], children: [] },
    ] },
  ];
  check("finds a top-level node by path", findOutlineByPath(tree, [{ Key: "a" }])?.text_range[0] === 0);
  const nested = findOutlineByPath(tree, [{ Key: "a" }, { Key: "b" }]);
  check("finds a nested node by path", nested?.text_range[0] === 10 && nested?.text_range[1] === 20);
  check("returns undefined for a path with no match", findOutlineByPath(tree, [{ Key: "missing" }]) === undefined);
}

// ---- 3. jumpSelectRawSpan / renderRawControls: extracted verbatim ----
const uiTs = readFileSync(path.join(here, "ui.ts"), "utf8");
const names = ["jumpSelectRawSpan", "renderRawControls"];
const fns = names.map((n) => uiTs.match(new RegExp(`^function ${n}\\([\\s\\S]*?\\n\\}`, "m"))?.[0]);
fns.forEach((s, i) => check(`${names[i]} extracted verbatim`, !!s));

const src = `let snap, session, rawState = "off", rawWriteBaseline = null, statusEl, VSHOST = false;
function t(key) { return key; }
function findOutlineByPath(nodes, path) {
  const pathEq = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  for (const n of nodes) {
    if (pathEq(n.path, path)) return n;
    if (path.length > n.path.length) {
      const hit = findOutlineByPath(n.children, path);
      if (hit) return hit;
    }
  }
  return undefined;
}
function byteToCodeUnit(text, byteOffset) { return byteOffset; } // ASCII-only fixtures below
${fns[0]}
${fns[1]}
export { jumpSelectRawSpan, renderRawControls, setEnv };
function setEnv(e) { snap = e.snap; session = e.session; rawState = e.rawState; rawWriteBaseline = e.rawWriteBaseline; statusEl = e.statusEl; VSHOST = e.vshost ?? false; }
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
  return { value: "", textContent: "", classList: mkClassList(), focus() {}, setSelectionRange() {}, firstChild: null, scrollTop: 0, getBoundingClientRect: () => ({ top: 0, bottom: 100 }), ...overrides };
}

let els;
let statusTextCalls;
let rangeCalls;
let selectionCalls;
function freshGlobalEnv(outline, opts = {}) {
  els = { rawEdit: mkEl({ value: opts.editValue ?? "text" }), raw: mkEl({ firstChild: opts.rawHasTextNode === false ? null : { nodeType: 3 } }), rawControls: mkEl(), btnRawView: mkEl(), btnRawEdit: mkEl(), btnRawApply: mkEl(), btnRawSave: mkEl() };
  statusTextCalls = [];
  rangeCalls = [];
  selectionCalls = [];
  globalThis.$ = (id) => els[id];
  globalThis.document = {
    createRange: () => {
      const r = { setStart: (n, o) => rangeCalls.push(["setStart", o]), setEnd: (n, o) => rangeCalls.push(["setEnd", o]), getClientRects: () => [] };
      return r;
    },
  };
  globalThis.window = { getSelection: () => ({ removeAllRanges: () => selectionCalls.push("clear"), addRange: () => selectionCalls.push("add") }) };
  const sessionStub = { outline: () => outline, serialize: () => opts.text ?? "target = \"needle\"\n" };
  mod.setEnv({ snap: {}, session: sessionStub, rawState: opts.rawState ?? "view", rawWriteBaseline: opts.baseline ?? "text", statusEl: { set textContent(v) { statusTextCalls.push(v); } }, vshost: opts.vshost });
}

// ---- Raw view: a jump selects the node's span via Range/Selection ----
console.log("\n-- jumpSelectRawSpan(): Raw view selects via Range/Selection --");
{
  const outline = [{ key: "target", path: [{ Key: "target" }], type_label: "string", value: "needle", text_range: [0, 17], key_text_range: [0, 6], children: [] }];
  freshGlobalEnv(outline, { rawState: "view" });
  mod.jumpSelectRawSpan([{ Key: "target" }]);
  check("Range.setStart called with the node's start offset", rangeCalls.some((c) => c[0] === "setStart" && c[1] === 0));
  check("Range.setEnd called with the node's end offset", rangeCalls.some((c) => c[0] === "setEnd" && c[1] === 17));
  check("Selection cleared then a range added", selectionCalls[0] === "clear" && selectionCalls[1] === "add");
}
{
  // R29/F5: the selected range is the whole member (key included), not a
  // value-only span — the outline's own `text_range` already encodes this,
  // and this call site must use it verbatim rather than `key_text_range`.
  const outline = [{ key: "target", path: [{ Key: "target" }], type_label: "string", value: "needle", text_range: [0, 17], key_text_range: [0, 6], children: [] }];
  freshGlobalEnv(outline, { rawState: "view" });
  mod.jumpSelectRawSpan([{ Key: "target" }]);
  check("selects text_range (whole member), not key_text_range", rangeCalls.some((c) => c[0] === "setEnd" && c[1] === 17));
}

// ---- Raw write, clean buffer: selects via setSelectionRange ----
console.log("\n-- jumpSelectRawSpan(): Raw write (clean buffer) sets the textarea selection --");
{
  const outline = [{ key: "target", path: [{ Key: "target" }], type_label: "string", value: "needle", text_range: [0, 17], key_text_range: [0, 6], children: [] }];
  freshGlobalEnv(outline, { rawState: "write", editValue: "text", baseline: "text" });
  let sel = null;
  els.rawEdit.setSelectionRange = (s, e) => (sel = [s, e]);
  mod.jumpSelectRawSpan([{ Key: "target" }]);
  check("setSelectionRange called with the node's span", sel && sel[0] === 0 && sel[1] === 17, sel);
  check("no status text set (buffer was clean)", statusTextCalls.length === 0);
}

// ---- R17: Raw write, dirty buffer — reports instead of moving the caret ----
console.log("\n-- jumpSelectRawSpan(): R17 dirty write buffer is gated --");
{
  const outline = [{ key: "target", path: [{ Key: "target" }], type_label: "string", value: "needle", text_range: [0, 17], key_text_range: [0, 6], children: [] }];
  freshGlobalEnv(outline, { rawState: "write", editValue: "edited text", baseline: "text" });
  let selCalled = false;
  els.rawEdit.setSelectionRange = () => (selCalled = true);
  mod.jumpSelectRawSpan([{ Key: "target" }]);
  check("setSelectionRange is never called on a dirty buffer", !selCalled);
  check("status reports web.raw.jump-needs-apply instead", statusTextCalls.length === 1);
}

// ---- renderRawControls(): band visibility per rawState ----
console.log("\n-- renderRawControls(): band + pair + apply/save visibility --");
{
  freshGlobalEnv([], { rawState: "off" });
  mod.renderRawControls();
  check("band hidden when rawState is off", els.rawControls.classList.contains("hidden"));
}
{
  freshGlobalEnv([], { rawState: "view" });
  mod.renderRawControls();
  check("band shown in view", !els.rawControls.classList.contains("hidden"));
  check("view pair marked active", els.btnRawView.classList.contains("active"));
  check("edit pair not active", !els.btnRawEdit.classList.contains("active"));
  check("apply hidden in view", els.btnRawApply.classList.contains("hidden"));
  check("save hidden in view", els.btnRawSave.classList.contains("hidden"));
}
{
  freshGlobalEnv([], { rawState: "write" });
  mod.renderRawControls();
  check("edit pair marked active in write", els.btnRawEdit.classList.contains("active"));
  check("apply shown in write", !els.btnRawApply.classList.contains("hidden"));
  check("save shown in write", !els.btnRawSave.classList.contains("hidden"));
}
{
  // R10: VS Code's own TextDocument owns whole-document editing — the Raw
  // pane's Edit control is suppressed there, View is unaffected.
  freshGlobalEnv([], { rawState: "view", vshost: true });
  mod.renderRawControls();
  check("edit control hidden under VSHOST", els.btnRawEdit.classList.contains("hidden"));
  check("view control still shown under VSHOST", !els.rawControls.classList.contains("hidden"));
}


console.log(failures === 0 ? "\nALL RAW JUMP CHECKS PASSED" : `\n${failures} FAILURE(S)`);
process.exit(failures === 0 ? 0 : 1);
