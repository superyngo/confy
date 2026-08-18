// Plain-Node test for touch/app.ts's armed-paste `After`/`Into` visual cue
// (ADR 0004 §1, Task 11): `render()` must reuse drag-reorder's own
// `.reorder-line` element to show the `After` target, and `renderPasteSlotCue`
// must survive a reorder-drag ending — `endReorder()`'s own cleanup
// (`reLine.style.display = "none"` + `clearInto()`) unconditionally wipes
// whichever half of the SAME cue elements a reorder-drag last touched, even a
// drag unrelated to (or a no-op on) the armed clipboard, with no subsequent
// `render()` to restore it when no intent is sent (self-found defect, fixed
// the same way as the desktop `dnd.ts` `onDragEnd` hook, ADR 0004 §1: a direct
// restoration call right after the wipe). Follows
// touch-pointer-slot.spec.mjs's convention: no test framework, just a
// `check()` tally; `touch/app.ts` can't be imported in Node (wasm + DOM boot
// at module top level), so `renderPasteSlotCue`, `endReorder`, `clearInto`,
// `pathOf` and `rowFor` are extracted verbatim from the source and
// type-stripped via esbuild into one wrapper module supplying the module-level
// state they close over (plus the real `path-utils.js` for `parentOf` /
// `pathEq` / `siblingIndex`) — the behavioral checks below run the real
// shipped function bodies, not reimplementations.
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

// ---- wiring: render() and endReorder() both draw/restore the cue ----
check(
  "render() calls renderPasteSlotCue right after the treeHTML rebuild",
  /treeEl\.innerHTML = treeHTML\(snap\);\s*\n\s*renderPasteSlotCue\(snap\);/.test(appTs),
);
const endReorderBlock = appTs.match(/^function endReorder\(\)[\s\S]*?\n\}/m)?.[0] ?? "";
check("endReorder found in source", endReorderBlock.length > 0);
check(
  "endReorder restores the armed-paste cue AFTER its own wipe (self-found fix)",
  /reLine\.style\.display = "none";\s*\n\s*clearInto\(\);\s*\n[\s\S]*?if \(snap\) renderPasteSlotCue\(snap\);/.test(
    endReorderBlock,
  ),
  endReorderBlock,
);

// ---- extract + execute the real functions from touch/app.ts ----
const NAMES = ["pathOf", "rowFor", "clearInto", "renderPasteSlotCue", "endReorder"];
const fns = NAMES.map((n) => appTs.match(new RegExp(`^function ${n}\\([\\s\\S]*?\\n\\}`, "m"))?.[0]);
fns.forEach((s, i) => check(`${NAMES[i]} extracted verbatim`, !!s));

const H = (globalThis.__touchHooks = { sent: [] });
globalThis.CSS = { escape: (s) => s };
let mod = null;
{
  const src = `import { parentOf, pathEq, siblingIndex } from "./path-utils.js";
let snap = null;
let treeEl = null;
let reordering = false;
let reRow = null;
let reMoved = false;
let reTarget = null;
let reMode = "before";
let reInto = null;
let reLine = null;
let reSrcPath = null;
const H = globalThis.__touchHooks;
const send = (i) => H.sent.push(i);
export function setEnv(e) {
  if ("snap" in e) snap = e.snap;
  if ("treeEl" in e) treeEl = e.treeEl;
}
export function setReorderState(s) {
  if ("reordering" in s) reordering = s.reordering;
  if ("reRow" in s) reRow = s.reRow;
  if ("reMoved" in s) reMoved = s.reMoved;
  if ("reTarget" in s) reTarget = s.reTarget;
  if ("reMode" in s) reMode = s.reMode;
  if ("reInto" in s) reInto = s.reInto;
  if ("reLine" in s) reLine = s.reLine;
  if ("reSrcPath" in s) reSrcPath = s.reSrcPath;
}
export ${fns[0]}
export ${fns[1]}
export ${fns[2]}
export ${fns[3]}
export ${fns[4]}
`;
  const built = await esbuild.build({
    stdin: { contents: src, resolveDir: here, loader: "ts" },
    bundle: true,
    write: false,
    format: "esm",
    platform: "node",
    target: "es2022",
  });
  const modUrl = "data:text/javascript;base64," + Buffer.from(built.outputFiles[0].text).toString("base64");
  mod = await import(modUrl);
}

// Minimal fakes: a row tracks its own live class Set AND logs every add/remove
// into a shared `ops` list (so restore-after-wipe ORDER, not just end state,
// is provable — the exact defect class Task 8 found in desktop's dnd.ts). The
// reorder-line's `style` similarly logs every property write. No jsdom.
function mkRow(key, top, height, ops) {
  const live = new Set();
  return {
    dataset: { path: JSON.stringify([{ Key: key }]) },
    classList: {
      add: (c) => {
        live.add(c);
        ops.push(`add ${c} ${key}`);
      },
      remove: (c) => {
        live.delete(c);
        ops.push(`remove ${c} ${key}`);
      },
    },
    classes: live,
    getBoundingClientRect: () => ({ top, height, bottom: top + height }),
  };
}
function mkReorderLine(ops) {
  return {
    style: new Proxy(
      {},
      {
        set: (t, k, v) => {
          ops.push(`reLine.${k}=${v}`);
          t[k] = v;
          return true;
        },
      },
    ),
  };
}
function mkTreeEl(rows, reorderLine) {
  return {
    querySelector: (sel) => {
      if (sel === ".reorder-line") return reorderLine;
      const m = sel.match(/^\.row\[data-path='(.+)'\]$/);
      return m ? (rows.find((r) => r.dataset.path === m[1]) ?? null) : null;
    },
    querySelectorAll: (sel) => (sel === ".drop-into" ? rows.filter((r) => r.classes.has("drop-into")) : []),
    getBoundingClientRect: () => ({ top: 0 }),
  };
}

// ---- renderPasteSlotCue: standalone behavior ----
console.log("\n-- renderPasteSlotCue(): After/Into positioning --");
{
  const ops = [];
  const rowB = mkRow("b", 100, 40, ops);
  const rowC = mkRow("c", 200, 40, ops);
  const reorderLine = mkReorderLine(ops);
  mod.setReorderState({ reordering: false });
  mod.setEnv({ treeEl: mkTreeEl([rowB, rowC], reorderLine) });
  mod.renderPasteSlotCue({ paste_slot: { Into: [{ Key: "b" }] } });
  check("Into slot adds drop-into to the matching row", rowB.classes.has("drop-into"));
  check("Into slot leaves the other row untouched", !rowC.classes.has("drop-into"));
  check("Into slot (no After) hides the reorder-line", reorderLine.style.display === "none");
}
{
  const ops = [];
  const rowB = mkRow("b", 100, 40, ops);
  const reorderLine = mkReorderLine(ops);
  mod.setReorderState({ reordering: false });
  mod.setEnv({ treeEl: mkTreeEl([rowB], reorderLine) });
  mod.renderPasteSlotCue({ paste_slot: { After: [{ Key: "b" }] } });
  check("After slot shows the reorder-line", reorderLine.style.display === "block");
  check("After slot positions the line at the row's bottom", reorderLine.style.top === "140px", reorderLine.style.top);
}
{
  const ops = [];
  const reorderLine = mkReorderLine(ops);
  reorderLine.style.display = "block";
  mod.setReorderState({ reordering: false });
  mod.setEnv({ treeEl: mkTreeEl([], reorderLine) });
  mod.renderPasteSlotCue({ paste_slot: undefined });
  check("unarmed + not reordering hides the line", reorderLine.style.display === "none");
}
{
  const ops = [];
  const reorderLine = mkReorderLine(ops);
  reorderLine.style.display = "block";
  mod.setReorderState({ reordering: true });
  mod.setEnv({ treeEl: mkTreeEl([], reorderLine) });
  mod.renderPasteSlotCue({ paste_slot: undefined });
  check(
    "unarmed but mid-drag: guard leaves a live drag's own line alone",
    reorderLine.style.display === "block",
  );
}
{
  const ops = [];
  const reorderLine = mkReorderLine(ops);
  reorderLine.style.display = "block";
  mod.setReorderState({ reordering: false });
  mod.setEnv({ treeEl: mkTreeEl([], reorderLine) }); // target row no longer visible
  mod.renderPasteSlotCue({ paste_slot: { After: [{ Key: "gone" }] } });
  check("After target row not visible hides the line", reorderLine.style.display === "none");
}

// ---- endReorder: self-found defect regression (restore-after-wipe) ----
console.log("\n-- endReorder(): restores the armed cue its own cleanup wipes --");
{
  // Grip-tap that never crosses the move threshold (reMoved stays false, as
  // in `onReorderMove`'s own early-return band) while an unrelated row is
  // armed with an `After` target already showing on screen.
  const ops = [];
  const rowX = mkRow("x", 100, 40, ops);
  const reorderLine = mkReorderLine(ops);
  reorderLine.style.display = "block";
  reorderLine.style.top = "140px";
  ops.length = 0;
  H.sent.length = 0;
  mod.setEnv({ treeEl: mkTreeEl([rowX], reorderLine), snap: { paste_slot: { After: [{ Key: "x" }] }, rows: [] } });
  mod.setReorderState({
    reordering: true,
    reMoved: false,
    reTarget: null,
    reSrcPath: [{ Key: "other" }],
    reLine: reorderLine,
    reRow: rowX,
    reInto: null,
  });
  mod.endReorder();
  check(
    "a non-moved drag's cleanup still wipes the line first",
    ops.includes("reLine.display=none"),
    JSON.stringify(ops),
  );
  check("armed After cue is redrawn by the end of endReorder", reorderLine.style.display === "block");
  check(
    "restore runs AFTER the wipe, not before (wipe -> restore order)",
    ops.lastIndexOf("reLine.display=block") > ops.lastIndexOf("reLine.display=none"),
    JSON.stringify(ops),
  );
  check("no move -> no intent sent", H.sent.length === 0);
}
{
  // A self-drag (dropped back onto its own source path): `reMoved` is true so
  // `onReorderMove` did overwrite the shared cue elements, but no intent is
  // sent (`pathEq(tgtPath, reSrcPath)` guard) — so, pre-fix, nothing would
  // ever redraw the armed Into cue `clearInto()` just stripped.
  const ops = [];
  const rowY = mkRow("y", 50, 40, ops);
  const reorderLine = mkReorderLine(ops);
  rowY.classList.add("drop-into"); // as if treeHTML baked in the armed Into cue
  ops.length = 0;
  H.sent.length = 0;
  mod.setEnv({ treeEl: mkTreeEl([rowY], reorderLine), snap: { paste_slot: { Into: [{ Key: "y" }] }, rows: [] } });
  mod.setReorderState({
    reordering: true,
    reMoved: true,
    reTarget: rowY,
    reMode: "into",
    reSrcPath: [{ Key: "y" }], // same path as the hovered target -> self-drag
    reLine: reorderLine,
    reRow: rowY,
    reInto: rowY, // the reorder's own hover landed on the SAME row that's armed
  });
  mod.endReorder();
  check(
    "clearInto strips the row's drop-into during cleanup",
    ops.includes("remove drop-into y"),
    JSON.stringify(ops),
  );
  check("armed Into cue is redrawn on the same row by the end of endReorder", rowY.classes.has("drop-into"));
  check(
    "Into restore runs AFTER clearInto's wipe (wipe -> restore order)",
    ops.lastIndexOf("add drop-into y") > ops.lastIndexOf("remove drop-into y"),
    JSON.stringify(ops),
  );
  check("self-drag (target === source) sends no intent", H.sent.length === 0);
}
{
  // Nothing armed: endReorder must leave everything clean, no stray restore.
  const ops = [];
  const rowZ = mkRow("z", 0, 40, ops);
  const reorderLine = mkReorderLine(ops);
  rowZ.classList.add("drop-into");
  ops.length = 0;
  H.sent.length = 0;
  mod.setEnv({ treeEl: mkTreeEl([rowZ], reorderLine), snap: { paste_slot: undefined, rows: [] } });
  mod.setReorderState({
    reordering: true,
    reMoved: false,
    reTarget: null,
    reSrcPath: [{ Key: "z" }],
    reLine: reorderLine,
    reRow: rowZ,
    reInto: rowZ,
  });
  mod.endReorder();
  check("unarmed endReorder leaves the reorder-line hidden", reorderLine.style.display === "none");
  check("unarmed endReorder does not resurrect a stray drop-into", !rowZ.classes.has("drop-into"));
}
{
  // Regression guard: a normal successful reorder still sends MoveSelectionTo
  // (the fix must not have disturbed the pre-existing move-dispatch path).
  const ops = [];
  const rowA = mkRow("a", 0, 40, ops);
  const rowB2 = mkRow("b2", 100, 40, ops);
  const reorderLine = mkReorderLine(ops);
  H.sent.length = 0;
  mod.setEnv({
    treeEl: mkTreeEl([rowA, rowB2], reorderLine),
    snap: { paste_slot: undefined, rows: [{ path: [{ Key: "b2" }], child_count: 0 }] },
  });
  mod.setReorderState({
    reordering: true,
    reMoved: true,
    reTarget: rowB2,
    reMode: "after",
    reSrcPath: [{ Key: "a" }],
    reLine: reorderLine,
    reRow: rowA,
    reInto: null,
  });
  mod.endReorder();
  check(
    "existing successful-move behavior unaffected: MoveSelectionTo still sent",
    H.sent.length === 1 && "MoveSelectionTo" in H.sent[0],
    JSON.stringify(H.sent),
  );
}

console.log(failures === 0 ? "\nALL TOUCH PASTE-CUE CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
