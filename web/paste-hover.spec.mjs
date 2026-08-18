// Plain-Node test for ui.ts's client-only armed-paste hover preview
// (ROW_STATE_MODEL.md §6a): while the clipboard is armed, moving the pointer
// over a candidate row must paint the same `.drag-over-into`/`#dropLine` cue
// the committed `snap.paste_slot` already uses, computed live from
// `session.pointerSlot()` — no `dispatch`/`send`, no re-render. When
// `pointerSlot` declines (or the pointer isn't over a row, or the clipboard
// isn't armed), the preview must fall back to the *committed* target instead
// of going blank. Follows armed-paste.spec.mjs's convention: no test
// framework, just `node:assert` + a `check()` tally; `ui.ts` can't be
// imported in Node (wasm + DOM top-level wiring), so `renderPasteSlotCue`
// and the new `onArmedPasteHover` are extracted verbatim from ui.ts's source
// and type-stripped via esbuild — the behavioral checks below run the real
// shipped function bodies, not a reimplementation — and the `bindGlobal`
// wiring (mousemove/mouseleave listeners) is verified structurally against
// the source, same as TOOLBAR_ENTRIES.
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

const uiTs = readFileSync(path.join(here, "ui.ts"), "utf8");

// ---- static checks against the source ----
const cueMatch = uiTs.match(/^function renderPasteSlotCue\([\s\S]*?\n\}/m);
check("ui.ts defines renderPasteSlotCue()", !!cueMatch);
check(
  "renderPasteSlotCue takes an optional slotOverride",
  /function renderPasteSlotCue\(snap: SessionSnapshot, slotOverride\?: PasteSlot\)/.test(uiTs),
);
check(
  "renderPasteSlotCue sweeps .drag-over-into off every row first",
  !!cueMatch &&
    /^function renderPasteSlotCue\([^)]*\)\s*\{\s*\n\s*tree\.querySelectorAll\("\.drag-over-into"\)\.forEach\(\(el\) => el\.classList\.remove\("drag-over-into"\)\);/.test(
      cueMatch[0],
    ),
);
check(
  "renderPasteSlotCue's slot prefers the override over the committed snap.paste_slot",
  !!cueMatch && /const slot = slotOverride \?\? snap\.paste_slot;/.test(cueMatch[0]),
);
check(
  "ui.ts imports PasteSlot alongside the other ./types.js type imports",
  /import type \{[\s\S]*?\n  PasteSlot,[\s\S]*?\} from "\.\/types\.js";/.test(uiTs),
);

const hoverMatch = uiTs.match(/^function onArmedPasteHover\([\s\S]*?\n\}/m);
check("ui.ts defines onArmedPasteHover()", !!hoverMatch);
check(
  "onArmedPasteHover computes relY from the row's bounding rect (armedPasteTarget's pattern)",
  !!hoverMatch && /getBoundingClientRect\(\)/.test(hoverMatch[0]) && /ev\.clientY - r\.top/.test(hoverMatch[0]),
);
check(
  "onArmedPasteHover routes through session.pointerSlot",
  !!hoverMatch && /session\.pointerSlot\(path, relY\)/.test(hoverMatch[0]),
);
check(
  "onArmedPasteHover calls renderPasteSlotCue with the computed slot",
  !!hoverMatch && /renderPasteSlotCue\(snap, slot \?\? snap\.paste_slot/.test(hoverMatch[0]),
);
check(
  "onArmedPasteHover never calls send/dispatch",
  !!hoverMatch && !/\bsend\(/.test(hoverMatch[0]) && !/\bdispatch\(/.test(hoverMatch[0]),
);
check(
  "onArmedPasteHover early-returns unless armed",
  !!hoverMatch && /\(snap\.clipboard_count \?\? 0\) === 0\) return/.test(hoverMatch[0]),
);

const bindGlobalBlock = uiTs.match(/^function bindGlobal\([\s\S]*?\n\}/m)?.[0] ?? "";
check("bindGlobal exists", bindGlobalBlock.length > 0);
check(
  "bindGlobal wires mousemove to onArmedPasteHover",
  /\$\("treeWrap"\)\.addEventListener\("mousemove", onArmedPasteHover\);/.test(bindGlobalBlock),
);
check(
  "bindGlobal wires mouseleave to restore the committed cue",
  /\$\("treeWrap"\)\.addEventListener\("mouseleave", \(\) => \{\s*\n\s*if \(snap\) renderPasteSlotCue\(snap\);\s*\n\s*\}\);/.test(
    bindGlobalBlock,
  ),
);

// ---- extract + execute the real renderPasteSlotCue + onArmedPasteHover ----
// Both reference module-level `snap`/`session` (ui.ts globals) and
// renderPasteSlotCue's own DOM env ($, tree, rawView, CSS); expose setters so
// each case can arm/disarm the stubs. The fn text is verbatim from source.
let renderPasteSlotCue = null;
let onArmedPasteHover = null;
let setSession = null;
let setSnap = null;
let setEnv = null;
if (cueMatch && hoverMatch) {
  const src = `let session, snap, $, tree, rawView, CSS;
export function setSession(s) { session = s; }
export function setSnap(s) { snap = s; }
export function setEnv(e) { $ = e.$; tree = e.tree; rawView = e.rawView; CSS = e.CSS; }
export ${cueMatch[0]}
export ${hoverMatch[0]}
`;
  const built = await esbuild.build({
    stdin: { contents: src, resolveDir: here, loader: "ts" },
    write: false,
    format: "esm",
    target: "es2022",
  });
  const modUrl = "data:text/javascript;base64," + Buffer.from(built.outputFiles[0].text).toString("base64");
  ({ renderPasteSlotCue, onArmedPasteHover, setSession, setSnap, setEnv } = await import(modUrl));
}
if (!onArmedPasteHover) {
  // Pre-implementation (RED): keep the tally flowing so every behavioral
  // check below reports ✗ instead of crashing on undefined imports.
  onArmedPasteHover = () => {};
  renderPasteSlotCue = () => {};
  setSession = () => {};
  setSnap = () => {};
  setEnv = () => {};
}

// ---- minimal DOM/session shims (no jsdom, no new npm dependency) ----
const classList = (classes) => ({
  add: (c) => classes.add(c),
  remove: (c) => classes.delete(c),
  contains: (c) => classes.has(c),
});
function mkRow(pathArr, top, height) {
  const classes = new Set();
  const row = {
    dataset: { path: JSON.stringify(pathArr) },
    classes,
    classList: classList(classes),
    getBoundingClientRect: () => ({ top, height, bottom: top + height }),
    querySelector: (sel) => (sel === ".indent" ? { offsetWidth: 24 } : null),
  };
  row.closest = (sel) => (sel === ".row" ? row : null);
  return row;
}
function mkTree(rows) {
  return {
    querySelector: (sel) => rows.find((r) => sel.includes(r.dataset.path)) ?? null,
    querySelectorAll: (sel) => (sel === ".drag-over-into" ? rows.filter((r) => r.classes.has("drag-over-into")) : []),
  };
}
const evOn = (rowEl, clientY) => ({
  target: { closest: (sel) => (sel === ".row" ? rowEl : null) },
  clientY,
});
const evOutside = (clientY) => ({ target: { closest: () => null }, clientY });

function sessionStub(slot, captured) {
  return { pointerSlot: (path, relY) => { captured.push({ path, relY }); return slot; } };
}

function freshEnv(rows) {
  const dropLine = { style: {} };
  const wrap = { getBoundingClientRect: () => ({ top: 0 }), scrollTop: 0 };
  const tree = mkTree(rows);
  setEnv({
    $: (id) => (id === "dropLine" ? dropLine : wrap),
    tree,
    rawView: false,
    CSS: { escape: (s) => s },
  });
  return { dropLine, tree };
}

const A = [{ Key: "a" }];
const B = [{ Key: "b" }];
const C = [{ Key: "c" }];

console.log("\n-- hovering a row that classifies Into --");
{
  const rowB = mkRow(B, 100, 40);
  const { dropLine } = freshEnv([rowB]);
  const captured = [];
  setSnap({ clipboard_count: 1, paste_slot: undefined });
  setSession(sessionStub({ Into: B }, captured));
  onArmedPasteHover(evOn(rowB, 110)); // top quarter, still classified Into by the stub
  check("Into hover paints .drag-over-into on the hovered row", rowB.classes.has("drag-over-into"));
  check("Into hover hides #dropLine", dropLine.style.display === "none");
  check("pointerSlot received the hovered path + relY", captured.length === 1 && captured[0].relY === 0.25);
}

console.log("\n-- hovering a row that classifies After --");
{
  const rowB = mkRow(B, 100, 40);
  const { dropLine } = freshEnv([rowB]);
  setSnap({ clipboard_count: 1, paste_slot: undefined });
  setSession(sessionStub({ After: B }, []));
  onArmedPasteHover(evOn(rowB, 120)); // 50% down the row
  check("After hover shows #dropLine", dropLine.style.display === "block");
  check("After hover positions the line at the row's bottom edge", dropLine.style.top === "140px", JSON.stringify(dropLine.style));
  check("Into row is left un-outlined", !rowB.classes.has("drag-over-into"));
}

console.log("\n-- declined pointerSlot falls back to the committed paste_slot --");
{
  const rowB = mkRow(B, 100, 40);
  const rowC = mkRow(C, 200, 40);
  const { dropLine } = freshEnv([rowB, rowC]);
  setSnap({ clipboard_count: 1, paste_slot: { Into: C } });
  setSession(sessionStub(undefined, [])); // pointerSlot declines to classify
  onArmedPasteHover(evOn(rowB, 110));
  check("declined hover leaves the hovered row un-outlined", !rowB.classes.has("drag-over-into"));
  check("declined hover falls back to the committed Into target", rowC.classes.has("drag-over-into"));
  check("declined hover hides the dropLine (committed target is Into)", dropLine.style.display === "none");
}
{
  const rowB = mkRow(B, 100, 40);
  const { dropLine } = freshEnv([rowB]);
  setSnap({ clipboard_count: 1, paste_slot: undefined }); // no committed target either
  setSession(sessionStub(undefined, []));
  onArmedPasteHover(evOn(rowB, 110));
  check("declined hover with no committed target leaves the row un-outlined", !rowB.classes.has("drag-over-into"));
  check("declined hover with no committed target hides the dropLine", dropLine.style.display === "none");
}

console.log("\n-- hovering outside any .row falls back to the committed paste_slot --");
{
  const rowC = mkRow(C, 200, 40);
  const { dropLine } = freshEnv([rowC]);
  const captured = [];
  setSnap({ clipboard_count: 1, paste_slot: { Into: C } });
  setSession(sessionStub({ Into: B }, captured)); // would classify if consulted
  onArmedPasteHover(evOutside(150));
  check("pointerSlot is never consulted without a hovered row", captured.length === 0);
  check("falls back to the committed Into target", rowC.classes.has("drag-over-into"));
  check("dropLine stays hidden (committed target is Into)", dropLine.style.display === "none");
}

console.log("\n-- not armed is a no-op --");
{
  const rowB = mkRow(B, 100, 40);
  const { dropLine } = freshEnv([rowB]);
  const captured = [];
  setSnap({ clipboard_count: 0, paste_slot: undefined });
  setSession(sessionStub({ Into: B }, captured));
  onArmedPasteHover(evOn(rowB, 110));
  check("unarmed (clipboard_count 0) never calls pointerSlot", captured.length === 0);
  check("unarmed hover leaves the row un-outlined", !rowB.classes.has("drag-over-into"));
  check("unarmed hover leaves the dropLine untouched", dropLine.style.display === undefined);
}
{
  const rowB = mkRow(B, 100, 40);
  freshEnv([rowB]);
  const captured = [];
  setSnap({ paste_slot: undefined }); // clipboard_count missing entirely
  setSession(sessionStub({ Into: B }, captured));
  onArmedPasteHover(evOn(rowB, 110));
  check("missing clipboard_count also never calls pointerSlot", captured.length === 0);
  check("missing clipboard_count leaves the row un-outlined", !rowB.classes.has("drag-over-into"));
}

console.log("\n-- cross-hover sweep: hovering row A then row B leaves only B outlined --");
{
  const rowA = mkRow(A, 0, 40);
  const rowB = mkRow(B, 100, 40);
  freshEnv([rowA, rowB]);
  setSnap({ clipboard_count: 1, paste_slot: undefined });
  setSession(sessionStub({ Into: A }, []));
  onArmedPasteHover(evOn(rowA, 10));
  check("hovering row A outlines A", rowA.classes.has("drag-over-into"));
  setSession(sessionStub({ Into: B }, []));
  onArmedPasteHover(evOn(rowB, 110));
  check("hovering row B outlines B", rowB.classes.has("drag-over-into"));
  check("hovering row B sweeps the stale outline off A", !rowA.classes.has("drag-over-into"));
}

console.log("\n-- leaving the tree restores the committed cue --");
{
  // Exercises renderPasteSlotCue directly (the way bindGlobal's mouseleave
  // listener calls it: no slotOverride) after a hover preview painted a
  // different row — the wiring itself is covered by the static bindGlobal
  // check above; this proves the restore call clears the stale preview.
  const rowB = mkRow(B, 100, 40);
  const rowC = mkRow(C, 200, 40);
  const { dropLine } = freshEnv([rowB, rowC]);
  const snap = { clipboard_count: 1, paste_slot: { Into: C } };
  setSnap(snap);
  setSession(sessionStub({ Into: B }, []));
  onArmedPasteHover(evOn(rowB, 110));
  check("hover preview painted B before the simulated mouseleave", rowB.classes.has("drag-over-into"));
  renderPasteSlotCue(snap); // what bindGlobal's mouseleave handler calls
  check("mouseleave restore clears the hover preview on B", !rowB.classes.has("drag-over-into"));
  check("mouseleave restore repaints the committed target C", rowC.classes.has("drag-over-into"));
  check("mouseleave restore hides the dropLine (committed target is Into)", dropLine.style.display === "none");
}

console.log(failures === 0 ? "\nALL PASTE-HOVER CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
