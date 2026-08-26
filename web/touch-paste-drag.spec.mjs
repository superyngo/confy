// Plain-Node test for touch/app.ts's drag-to-target (ADR 0005 §6b, Phase 5):
// while the clipboard is armed, a body-drag that crosses the tap-vs-scroll
// dead zone must continuously reclassify + repaint the paste-target cue via
// `session.pointerSlot(path, relY)`, client-only (no `dispatch`), and commit
// exactly one `SetPasteSlot`/`SetCursor` on release — never a `Paste` (the
// FAB alone dispatches that). A pointerdown landing on `.caret` must bail out
// of the new drag-preview loop so a stationary caret press still reaches
// `handleTap`'s existing `act === "caret"` branch unchanged.
//
// Follows touch-pointer-slot.spec.mjs's convention: no test framework, just a
// `check()` tally; touch/app.ts can't be imported in Node (wasm + DOM boot at
// module top level), so the real function bodies are extracted verbatim and
// type-stripped via esbuild into wrapper modules supplying the module-level
// state they close over — the behavioral checks below run the real shipped
// function bodies, not reimplementations.
//
// Two wrapper builds:
//   A. `installTreeGestures` + `handleTap` + `onPasteDragMove` +
//      `finishPasteDrag` + `pathOf`, all real — proves the actual pointer
//      event wiring/gating, with `renderPasteSlotCue` and `send` as spies (so
//      the drag-preview loop's classify-and-repaint effects are observable
//      without needing a full DOM `.row`/`.reorder-line` fixture).
//   B. `renderPasteSlotCue` alone, real, with a DOM-ish fake `treeEl` — proves
//      the new `.drop-into` sweep in isolation (build A's spy can't).
import path from "node:path";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import * as esbuild from "esbuild";

const here = path.dirname(fileURLToPath(import.meta.url));

let failures = 0;
function check(name, cond, extra = "") {
  if (cond) {
    console.log(`  ✓ ${name}`);
  } else {
    console.log(`  ✗ ${name} ${extra}`);
    failures++;
  }
}

const appTs = readFileSync(path.join(here, "touch/app.ts"), "utf8");

// ---- wiring: source-level shape (regex, same convention as touch-pointer-slot.spec.mjs) ----
check(
  "PasteSlot is imported from types.js",
  /import type \{[\s\S]*?\bPasteSlot\b[\s\S]*?\} from "\.\.\/types\.js";/.test(appTs),
);
check(
  "renderPasteSlotCue takes an optional PasteSlot override",
  /function renderPasteSlotCue\(snap: SessionSnapshot, slotOverride\?: PasteSlot\) \{/.test(appTs),
);
const cueBlock = appTs.match(/^function renderPasteSlotCue\([\s\S]*?\n\}/m)?.[0] ?? "";
check("renderPasteSlotCue found in source", cueBlock.length > 0);
check(
  "renderPasteSlotCue resolves slot from the override, falling back to the committed one",
  /const slot = slotOverride \?\? snap\.paste_slot;/.test(cueBlock),
);
check(
  "renderPasteSlotCue sweeps stale .drop-into rows before applying the new one",
  /treeEl\.querySelectorAll[^\n]*\.drop-into[^\n]*\.forEach\(\(el\) => el\.classList\.remove\("drop-into"\)\);/.test(
    cueBlock,
  ),
);
check("onPasteDragMove found in source", /^function onPasteDragMove\(y: number\) \{/m.test(appTs));
check("finishPasteDrag found in source", /^function finishPasteDrag\(y: number\) \{/m.test(appTs));
const gesturesBlock = appTs.match(/^function installTreeGestures\(\)[\s\S]*?\n\}\n/m)?.[0] ?? "";
check("installTreeGestures found in source", gesturesBlock.length > 0);
check(
  "pointerdown computes pasteDragActive from armed && !closest('.caret'), after the grip check",
  /if \(grip\) \{[\s\S]*?\n {4}\}\n {4}const armed = \(snap\?\.clipboard_count \?\? 0\) > 0;\n {4}pasteDragActive = armed && !\(e\.target as HTMLElement\)\.closest\("\.caret"\);\n {4}pasteDragStartY = e\.clientY;\n {4}pasteDragMoved = false;\n {4}pasteDragRow = null;/.test(
    gesturesBlock,
  ),
  gesturesBlock,
);
check(
  "pointermove's new branch calls onPasteDragMove only when pasteDragActive && dragging, before swipe/scroll tracking",
  /if \(reordering\) \{[\s\S]*?\n {4}\}\n {4}if \(pasteDragActive && dragging\) \{\n {6}e\.preventDefault\(\);\n {6}onPasteDragMove\(e\.clientY\);\n {6}return;\n {4}\}\n {4}if \(!dragging \|\| !dragRow\) return;/.test(
    gesturesBlock,
  ),
  gesturesBlock,
);
check(
  "pointerup's new branch fires finishPasteDrag before the existing !moved tap branch",
  /\} else if \(pasteDragActive && pasteDragMoved\) \{\n {6}finishPasteDrag\(e\.clientY\);\n {4}\} else if \(dragging && dragRow && !moved\) \{\n {6}handleTap\(e\.target as HTMLElement, dragRow, e\.clientY, e\);\n {4}\}/.test(
    gesturesBlock,
  ),
  gesturesBlock,
);
check(
  "pointerup resets the three pasteDrag* fields alongside the existing drag/swipe resets",
  /dragRow = null;\n {4}swiping = false;\n {4}swipeMain = null;\n {4}pasteDragActive = false;\n {4}pasteDragMoved = false;\n {4}pasteDragRow = null;\n {2}\}\);/.test(
    gesturesBlock,
  ),
  gesturesBlock,
);
check(
  "pointercancel restores the committed cue after a painted preview, then resets pasteDrag* fields",
  /if \(pasteDragMoved && snap\) renderPasteSlotCue\(snap\);\n {4}pasteDragActive = false;\n {4}pasteDragMoved = false;\n {4}pasteDragRow = null;/.test(
    gesturesBlock,
  ),
  gesturesBlock,
);

// ---- Build A: real installTreeGestures/handleTap/onPasteDragMove/finishPasteDrag/pathOf,
// spied renderPasteSlotCue/send (drag-preview effects observable without a full DOM fixture) ----
const NAMES_A = ["pathOf", "onPasteDragMove", "finishPasteDrag", "handleTap", "installTreeGestures"];
const fnsA = NAMES_A.map((n) => appTs.match(new RegExp(`^function ${n}\\([\\s\\S]*?\\n\\}`, "m"))?.[0]);
fnsA.forEach((s, i) => check(`${NAMES_A[i]} extracted verbatim (build A)`, !!s));

const H = (globalThis.__touchHooks = { sent: [], ops: [], cue: [] });
let modA = null;
{
  const src = `let session = null;
let snap = null;
let treeEl = null;
let treePane = null;
let sx = 0, sy = 0, dragRow = null, dragging = false, moved = false;
let pasteDragActive = false, pasteDragStartY = 0, pasteDragMoved = false, pasteDragRow = null;
let edgeScrollY = 0, edgeScrollRAF = null;
function edgeAutoScrollStep() {}
function kickEdgeAutoScroll() {}
function requestAnimationFrame(fn) { return 0; }
let lastTapKey = null, lastTapTime = 0;
const DOUBLE_TAP_MS = 300;
let swiping = false, swipeMain = null, swipeBase = 0, swipeOff = 0, openSwipeMain = null;
const SWIPE_W = 96;
let reordering = false;
const H = globalThis.__touchHooks;
const send = (i) => H.sent.push(i);
const sendR = (i) => { H.sent.push(i); return H.snapAfter ?? {}; };
const selectOnly = (p) => H.ops.push("selectOnly " + JSON.stringify(p));
const openPanel = (p) => H.ops.push("openPanel " + JSON.stringify(p));
const toast = (m) => H.ops.push("toast " + m);
const t = (k) => k;
const setDelRevealed = (m, on) => H.ops.push("setDelRevealed " + on);
function startReorder() {}
function onReorderMove() {}
function endReorder() {}
function renderPasteSlotCue(snapArg, slotOverride) {
  H.cue.push(slotOverride);
}
export function setEnv(e) {
  if ("session" in e) session = e.session;
  if ("snap" in e) snap = e.snap;
  if ("treeEl" in e) treeEl = e.treeEl;
  if ("treePane" in e) treePane = e.treePane;
}
export function resetTap() { lastTapKey = null; lastTapTime = 0; }
export function pasteDragState() { return { pasteDragActive, pasteDragMoved, pasteDragRow }; }
export function dragState() { return { dragging, dragRow, moved }; }
export ${fnsA[0]}
export ${fnsA[1]}
export ${fnsA[2]}
export ${fnsA[3]}
export ${fnsA[4]}
`;
  const built = await esbuild.build({
    stdin: { contents: src, resolveDir: here, loader: "ts" },
    write: false,
    format: "esm",
    target: "es2022",
  });
  const modUrl = "data:text/javascript;base64," + Buffer.from(built.outputFiles[0].text).toString("base64");
  modA = await import(modUrl);
}

// Minimal fakes: a row tracks dataset.path + a bounding rect; a target's
// `closest()` fans out the same selectors installTreeGestures/handleTap
// actually query (`.drag-handle`, `.caret`, `.row-main`, `.row-del`,
// `[data-act]`). No jsdom.
function mkRow(key, top, height) {
  return {
    dataset: { path: JSON.stringify([{ Key: key }]) },
    getBoundingClientRect: () => ({ top, height, bottom: top + height }),
    offsetHeight: height,
    querySelector: () => null,
  };
}
function mkTarget(row, { onCaret = false } = {}) {
  return {
    closest: (sel) => {
      if (sel === ".drag-handle") return null;
      if (sel === ".caret") return onCaret ? {} : null;
      if (sel === ".row-main") return { closest: (s) => (s === ".row" ? row : null) };
      if (sel === ".row-del") return null;
      if (sel === "[data-act]") return onCaret ? { dataset: { act: "caret" } } : null;
      return null;
    },
  };
}
function mkTreeEl(rows) {
  const handlers = {};
  return {
    addEventListener: (type, fn) => {
      handlers[type] = fn;
    },
    handlers,
    querySelectorAll: (sel) => (sel === ".row" ? rows : []),
  };
}
const noop = () => {};

function sessionStub(classify) {
  return { pointerSlot: (path, relY) => classify(path, relY) };
}

let treeEl, rowB, rowC;
function installFresh(snapArg, classify) {
  rowB = mkRow("b", 100, 40);
  rowC = mkRow("c", 200, 40);
  treeEl = mkTreeEl([rowB, rowC]);
  H.sent.length = 0;
  H.ops.length = 0;
  H.cue.length = 0;
  modA.resetTap();
  modA.setEnv({
    treeEl,
    treePane: { addEventListener: noop },
    session: sessionStub(classify ?? (() => undefined)),
    snap: snapArg,
  });
  modA.installTreeGestures();
}
function down(target, clientY) {
  treeEl.handlers.pointerdown({ target, clientX: 0, clientY, preventDefault: noop });
}
function move(target, clientY) {
  treeEl.handlers.pointermove({ target, clientX: 0, clientY, preventDefault: noop });
}
function up(target, clientY) {
  treeEl.handlers.pointerup({ target, clientX: 0, clientY, preventDefault: noop });
}

console.log("\n-- pointerdown: pasteDragActive wiring --");
{
  installFresh({ clipboard_count: 1 });
  down(mkTarget(rowB), 120);
  check("armed pointerdown off .caret sets pasteDragActive", modA.pasteDragState().pasteDragActive === true);
}
{
  installFresh({ clipboard_count: 1 });
  down(mkTarget(rowB, { onCaret: true }), 120);
  check(
    "armed pointerdown on .caret leaves pasteDragActive false",
    modA.pasteDragState().pasteDragActive === false,
  );
}
{
  installFresh({ clipboard_count: 0 });
  down(mkTarget(rowB), 120);
  check("disarmed pointerdown never sets pasteDragActive", modA.pasteDragState().pasteDragActive === false);
}

console.log("\n-- dead zone: no repaint/flag flip under 6px --");
{
  installFresh({ clipboard_count: 1 }, () => ({ Into: [{ Key: "b" }] }));
  down(mkTarget(rowB), 120);
  move(mkTarget(rowB), 123); // 3px — under the threshold
  check("movement under 6px does not mark pasteDragMoved", modA.pasteDragState().pasteDragMoved === false);
  check("movement under 6px does not repaint the cue", H.cue.length === 0);
}

console.log("\n-- live preview across two rows: repaints, never dispatches --");
{
  installFresh({ clipboard_count: 1 }, (p) =>
    p[0].Key === "b" ? { Into: [{ Key: "b" }] } : { After: [{ Key: "c" }] },
  );
  down(mkTarget(rowB), 120);
  move(mkTarget(rowB), 130); // still over row b (rect 100-140), past the dead zone
  check("crossing the dead zone marks pasteDragMoved", modA.pasteDragState().pasteDragMoved === true);
  check(
    "first classify repaints the cue with row b's slot",
    H.cue.length === 1 && JSON.stringify(H.cue[0]) === JSON.stringify({ Into: [{ Key: "b" }] }),
    JSON.stringify(H.cue),
  );
  move(mkTarget(rowB), 220); // now over row c (rect 200-240)
  check(
    "dragging onto a different row repaints with its own classified slot",
    H.cue.length === 2 && JSON.stringify(H.cue[1]) === JSON.stringify({ After: [{ Key: "c" }] }),
    JSON.stringify(H.cue),
  );
  check("no intent dispatched while the drag is only previewing", H.sent.length === 0);
}

console.log("\n-- release: commits exactly one set, never a Paste --");
{
  installFresh({ clipboard_count: 1 }, () => ({ Into: [{ Key: "b" }] }));
  down(mkTarget(rowB), 120);
  move(mkTarget(rowB), 130);
  up(mkTarget(rowB), 120);
  check(
    "release sends exactly one SetPasteSlot, no Paste",
    H.sent.length === 1 && JSON.stringify(H.sent[0]) === JSON.stringify({ SetPasteSlot: { Into: [{ Key: "b" }] } }),
    JSON.stringify(H.sent),
  );
  check("handleTap's tap path did not also fire", !H.ops.some((o) => o.startsWith("selectOnly")));
}
{
  installFresh({ clipboard_count: 1 }, () => undefined); // pointerSlot declines
  down(mkTarget(rowB), 120);
  move(mkTarget(rowB), 130);
  up(mkTarget(rowB), 120);
  check(
    "release falls back to SetCursor when pointerSlot declines",
    H.sent.length === 1 && JSON.stringify(H.sent[0]) === JSON.stringify({ SetCursor: [{ Key: "b" }] }),
    JSON.stringify(H.sent),
  );
}

console.log("\n-- caret bail: stationary press-release on .caret reaches handleTap's caret branch --");
{
  installFresh({ clipboard_count: 1 }, () => ({ Into: [{ Key: "b" }] }));
  const caretTarget = mkTarget(rowB, { onCaret: true });
  down(caretTarget, 120);
  up(caretTarget, 120); // stationary: no pointermove in between
  check(
    "armed caret tap sends SetPasteSlot then SetCursor then ToggleExpand (unchanged today's behavior)",
    H.sent.length === 3 &&
      JSON.stringify(H.sent[0]) === JSON.stringify({ SetPasteSlot: { Into: [{ Key: "b" }] } }) &&
      JSON.stringify(H.sent[1]) === JSON.stringify({ SetCursor: [{ Key: "b" }] }) &&
      H.sent[2] === "ToggleExpand",
    JSON.stringify(H.sent),
  );
}

console.log("\n-- regression: plain stationary tap on an armed row body is unchanged --");
{
  installFresh({ clipboard_count: 1 }, () => ({ Into: [{ Key: "b" }] }));
  const target = mkTarget(rowB);
  down(target, 120);
  up(target, 120); // stationary: pasteDragMoved never flips true
  check(
    "stationary armed tap still resolves through handleTap's armedTarget(), exactly once",
    H.sent.length === 1 && JSON.stringify(H.sent[0]) === JSON.stringify({ SetPasteSlot: { Into: [{ Key: "b" }] } }),
    JSON.stringify(H.sent),
  );
}

console.log("\n-- disarmed: body-drags are untouched by any of this phase's new code --");
{
  installFresh({ clipboard_count: 0 });
  const target = mkTarget(rowB);
  down(target, 100);
  move(target, 130); // dy = 30 > 8 — the old scroll-cancel threshold
  check("disarmed drag still flips the old `moved` flag (scroll-cancel path runs)", modA.dragState().moved === true);
  check("disarmed drag never touches the paste-drag preview", H.cue.length === 0);
  up(target, 130);
  check("disarmed release sends nothing (moved was true, no tap)", H.sent.length === 0);
}

// ---- Build B: real renderPasteSlotCue, DOM-ish fake treeEl (proves the sweep) ----
console.log("\n-- renderPasteSlotCue(): .drop-into sweep across repeated mid-gesture calls --");
const NAMES_B = ["renderPasteSlotCue"];
const fnsB = NAMES_B.map((n) => appTs.match(new RegExp(`^function ${n}\\([\\s\\S]*?\\n\\}`, "m"))?.[0]);
fnsB.forEach((s, i) => check(`${NAMES_B[i]} extracted verbatim (build B)`, !!s));

globalThis.CSS = { escape: (s) => s };
let modB = null;
{
  const src = `let treeEl = null;
let reordering = false;
export function setEnv(e) { if ("treeEl" in e) treeEl = e.treeEl; }
export ${fnsB[0]}
`;
  const built = await esbuild.build({
    stdin: { contents: src, resolveDir: here, loader: "ts" },
    write: false,
    format: "esm",
    target: "es2022",
  });
  const modUrl = "data:text/javascript;base64," + Buffer.from(built.outputFiles[0].text).toString("base64");
  modB = await import(modUrl);
}
function mkRowB(key, top, height) {
  const live = new Set();
  return {
    dataset: { path: JSON.stringify([{ Key: key }]) },
    classList: { add: (c) => live.add(c), remove: (c) => live.delete(c) },
    classes: live,
    getBoundingClientRect: () => ({ top, height, bottom: top + height }),
  };
}
{
  const rowB2 = mkRowB("b", 100, 40);
  const rowC2 = mkRowB("c", 200, 40);
  const reorderLine = { style: {} };
  const treeElB = {
    querySelectorAll: (sel) => (sel === ".drop-into" ? [rowB2, rowC2].filter((r) => r.classes.has("drop-into")) : []),
    querySelector: (sel) => {
      if (sel === ".reorder-line") return reorderLine;
      const m = sel.match(/^\.row\[data-path='(.+)'\]$/);
      return m ? [rowB2, rowC2].find((r) => r.dataset.path === m[1]) ?? null : null;
    },
    getBoundingClientRect: () => ({ top: 0 }),
  };
  modB.setEnv({ treeEl: treeElB });
  modB.renderPasteSlotCue({ paste_slot: undefined }, { Into: [{ Key: "b" }] });
  check(
    "first mid-gesture classify arms row b's drop-into",
    rowB2.classes.has("drop-into") && !rowC2.classes.has("drop-into"),
  );
  modB.renderPasteSlotCue({ paste_slot: undefined }, { Into: [{ Key: "c" }] });
  check(
    "next classify's sweep removes row b's stale drop-into before arming row c (the collision this phase would otherwise reintroduce)",
    !rowB2.classes.has("drop-into") && rowC2.classes.has("drop-into"),
  );
}

console.log(failures === 0 ? "\nALL TOUCH PASTE-DRAG CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
