// Plain-Node test for ui.ts's client-only armed-paste hover preview
// (ROW_STATE_MODEL.md §6a): while the clipboard is armed, moving the pointer
// over a candidate row paints a dashed/muted `.drag-over-into`/`#dropLine`
// preview, computed live from `session.pointerSlot()` — no `dispatch`/`send`,
// no re-render. This preview is independent from the solid, committed
// `renderConfirmedPasteCue` layer (`#pasteTargetLine`/`.paste-target`, also
// exercised in armed-paste.spec.mjs): hovering a row other than the
// committed target never touches the confirmed cue, hovering the exact
// committed row suppresses the preview (no double-paint), and leaving the
// tree (`mouseleave`) clears only the preview, never the confirmed cue.
// Follows armed-paste.spec.mjs's convention: no test framework, just
// `node:assert` + a `check()` tally; `ui.ts` can't be imported in Node (wasm
// + DOM top-level wiring), so `renderConfirmedPasteCue`, `renderHoverCue`,
// and `onArmedPasteHover` are extracted verbatim from ui.ts's source and
// type-stripped via esbuild — the behavioral checks below run the real
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
const cueMatch = uiTs.match(/^function renderHoverCue\([\s\S]*?\n\}/m);
check("ui.ts defines renderHoverCue()", !!cueMatch);
check(
  "renderHoverCue takes the live hover slot only (no override param)",
  /function renderHoverCue\(snap: SessionSnapshot, slot: PasteSlot \| undefined\)/.test(uiTs),
);
check(
  "renderHoverCue sweeps .drag-over-into off every row first",
  !!cueMatch &&
    /^function renderHoverCue\([^)]*\)\s*\{\s*\n\s*tree\.querySelectorAll\("\.drag-over-into"\)\.forEach\(\(el\) => el\.classList\.remove\("drag-over-into"\)\);/.test(
      cueMatch[0],
    ),
);
check(
  "renderHoverCue suppresses itself when the hovered slot matches the confirmed target",
  !!cueMatch &&
    /const sameAsConfirmed = slot && JSON\.stringify\(slot\) === JSON\.stringify\(effectiveConfirmed\);/.test(
      cueMatch[0],
    ),
);
check(
  "ui.ts imports PasteSlot alongside the other ./types.js type imports",
  /import type \{[\s\S]*?\n  PasteSlot,[\s\S]*?\} from "\.\/types\.js";/.test(uiTs),
);

const confirmedMatch = uiTs.match(/^function renderConfirmedPasteCue\([\s\S]*?\n\}/m);
check("ui.ts defines renderConfirmedPasteCue()", !!confirmedMatch);

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
  "onArmedPasteHover calls renderHoverCue with the computed slot",
  !!hoverMatch && /renderHoverCue\(snap, slot\);/.test(hoverMatch[0]),
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
  "bindGlobal wires mouseleave to clear the hover preview only",
  /\$\("treeWrap"\)\.addEventListener\("mouseleave", \(\) => \{\s*\n\s*if \(snap\) renderHoverCue\(snap, undefined\);\s*\n\s*\}\);/.test(
    bindGlobalBlock,
  ),
);

// ---- extract + execute the real renderConfirmedPasteCue + renderHoverCue + onArmedPasteHover ----
// All three reference module-level `snap`/`session` (ui.ts globals) and a
// shared DOM env ($, tree, rawView, CSS); expose setters so each case can
// arm/disarm the stubs. The fn text is verbatim from source.
let renderConfirmedPasteCue = null;
let renderHoverCue = null;
let onArmedPasteHover = null;
let setSession = null;
let setSnap = null;
let setEnv = null;
if (confirmedMatch && cueMatch && hoverMatch) {
  const src = `import { slotLineIndentPx } from "./slot-line.js";
let session, snap, $, tree, rawView, CSS;
export function setSession(s) { session = s; }
export function setSnap(s) { snap = s; }
export function setEnv(e) { $ = e.$; tree = e.tree; rawView = e.rawView; CSS = e.CSS; }
export ${confirmedMatch[0]}
export ${cueMatch[0]}
export ${hoverMatch[0]}
`;
  const built = await esbuild.build({
    stdin: { contents: src, resolveDir: here, loader: "ts" },
    // `bundle` inlines the real `slot-line.ts` the cues now call for their
    // horizontal placement (ADR 0010) — the checks below exercise the shipped
    // helper, not a stub.
    bundle: true,
    write: false,
    format: "esm",
    target: "es2022",
  });
  const modUrl = "data:text/javascript;base64," + Buffer.from(built.outputFiles[0].text).toString("base64");
  ({ renderConfirmedPasteCue, renderHoverCue, onArmedPasteHover, setSession, setSnap, setEnv } =
    await import(modUrl));
}
if (!onArmedPasteHover) {
  // Pre-implementation (RED): keep the tally flowing so every behavioral
  // check below reports ✗ instead of crashing on undefined imports.
  onArmedPasteHover = () => {};
  renderConfirmedPasteCue = () => {};
  renderHoverCue = () => {};
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
    querySelectorAll: (sel) =>
      sel === ".drag-over-into"
        ? rows.filter((r) => r.classes.has("drag-over-into"))
        : sel === ".paste-target"
          ? rows.filter((r) => r.classes.has("paste-target"))
          : [],
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
  const pasteTargetLine = { style: {} };
  const wrap = { getBoundingClientRect: () => ({ top: 0 }), scrollTop: 0 };
  const tree = mkTree(rows);
  setEnv({
    $: (id) => (id === "dropLine" ? dropLine : id === "pasteTargetLine" ? pasteTargetLine : wrap),
    tree,
    rawView: false,
    CSS: { escape: (s) => s },
  });
  return { dropLine, pasteTargetLine, tree };
}

const A = [{ Key: "a" }];
const B = [{ Key: "b" }];
const C = [{ Key: "c" }];

console.log("\n-- hovering a row that classifies Into --");
{
  const rowB = mkRow(B, 100, 40);
  const { dropLine } = freshEnv([rowB]);
  const captured = [];
  setSnap({ clipboard_count: 1, cursor: [], paste_slot: undefined });
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
  setSnap({ clipboard_count: 1, cursor: [], paste_slot: undefined });
  setSession(sessionStub({ After: B }, []));
  onArmedPasteHover(evOn(rowB, 120)); // 50% down the row
  check("After hover shows #dropLine", dropLine.style.display === "block");
  check("After hover positions the line at the row's bottom edge", dropLine.style.top === "140px", JSON.stringify(dropLine.style));
  check("Into row is left un-outlined", !rowB.classes.has("drag-over-into"));
}

// ADR 0010: `After(p)` on an *expanded branch* lands as p's FIRST CHILD
// (core `resolve_target`), so the line must be drawn one indent level deeper —
// the TUI has always done this (`paste_line_row`'s `row.depth + 1`); the web
// cue used the row's own indent unconditionally and so pointed a level too
// shallow at exactly the gap under an expanded `[table]`.
console.log("\n-- After(expanded branch): the line indents one level deeper --");
{
  // `slotLineIndentPx` reads the live `--indent` step; Node has no CSSOM.
  globalThis.getComputedStyle = () => ({ getPropertyValue: () => "22px" });
  const leaf = mkRow(B, 100, 40); // `.indent` shim reports offsetWidth 24
  const { dropLine } = freshEnv([leaf]);
  setSnap({ clipboard_count: 1, cursor: [], paste_slot: undefined });
  setSession(sessionStub({ After: B }, []));
  onArmedPasteHover(evOn(leaf, 120));
  check("leaf/collapsed row: line at the row's own indent + 8", dropLine.style.left === "32px", JSON.stringify(dropLine.style));

  const branch = mkRow(B, 100, 40);
  branch.classes.add("branch");
  branch.classes.add("open");
  const env2 = freshEnv([branch]);
  setSnap({ clipboard_count: 1, cursor: [], paste_slot: undefined });
  setSession(sessionStub({ After: B }, []));
  onArmedPasteHover(evOn(branch, 120));
  check(
    "expanded branch: line indented one --indent step deeper (24 + 22 + 8)",
    env2.dropLine.style.left === "54px",
    JSON.stringify(env2.dropLine.style),
  );
}

console.log("\n-- declined pointerSlot clears the hover cue (no fallback to the committed target) --");
{
  const rowB = mkRow(B, 100, 40);
  const rowC = mkRow(C, 200, 40);
  const { dropLine } = freshEnv([rowB, rowC]);
  setSnap({ clipboard_count: 1, cursor: [], paste_slot: { Into: C } });
  setSession(sessionStub(undefined, [])); // pointerSlot declines to classify
  onArmedPasteHover(evOn(rowB, 110));
  check("declined hover leaves the hovered row un-outlined", !rowB.classes.has("drag-over-into"));
  check("declined hover does not fall back to the committed Into target", !rowC.classes.has("drag-over-into"));
  check("declined hover hides the dropLine", dropLine.style.display === "none");
}

console.log("\n-- hovering outside any .row clears the hover cue (no fallback to the committed target) --");
{
  const rowC = mkRow(C, 200, 40);
  const { dropLine } = freshEnv([rowC]);
  const captured = [];
  setSnap({ clipboard_count: 1, cursor: [], paste_slot: { Into: C } });
  setSession(sessionStub({ Into: B }, captured)); // would classify if consulted
  onArmedPasteHover(evOutside(150));
  check("pointerSlot is never consulted without a hovered row", captured.length === 0);
  check("the committed Into target is not outlined by the hover layer", !rowC.classes.has("drag-over-into"));
  check("dropLine stays hidden", dropLine.style.display === "none");
}

console.log("\n-- not armed is a no-op --");
{
  const rowB = mkRow(B, 100, 40);
  const { dropLine } = freshEnv([rowB]);
  const captured = [];
  setSnap({ clipboard_count: 0, cursor: [], paste_slot: undefined });
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
  setSnap({ clipboard_count: 1, cursor: [], paste_slot: undefined });
  setSession(sessionStub({ Into: A }, []));
  onArmedPasteHover(evOn(rowA, 10));
  check("hovering row A outlines A", rowA.classes.has("drag-over-into"));
  setSession(sessionStub({ Into: B }, []));
  onArmedPasteHover(evOn(rowB, 110));
  check("hovering row B outlines B", rowB.classes.has("drag-over-into"));
  check("hovering row B sweeps the stale outline off A", !rowA.classes.has("drag-over-into"));
}

console.log("\n-- hovering the row that matches the committed target suppresses the hover cue --");
{
  const rowB = mkRow(B, 100, 40);
  const { dropLine } = freshEnv([rowB]);
  setSnap({ clipboard_count: 1, cursor: [], paste_slot: { Into: B } });
  setSession(sessionStub({ Into: B }, []));
  onArmedPasteHover(evOn(rowB, 10));
  check("hover matching the committed target does not paint drag-over-into (no double-paint)", !rowB.classes.has("drag-over-into"));
  check("hover matching the committed target keeps the dropLine hidden", dropLine.style.display === "none");
}

console.log("\n-- hovering a different row than the committed target leaves the confirmed cue untouched --");
{
  const rowB = mkRow(B, 100, 40);
  const rowC = mkRow(C, 200, 40);
  freshEnv([rowB, rowC]);
  const snap = { clipboard_count: 1, cursor: [], paste_slot: { Into: C } };
  setSnap(snap);
  renderConfirmedPasteCue(snap);
  check("confirmed cue paints paste-target on the committed row C", rowC.classes.has("paste-target"));
  setSession(sessionStub({ Into: B }, []));
  onArmedPasteHover(evOn(rowB, 10));
  check("hover paints drag-over-into on the hovered row B", rowB.classes.has("drag-over-into"));
  check("hovering B leaves the confirmed row C's paste-target class untouched", rowC.classes.has("paste-target"));
}

console.log("\n-- leaving the tree clears only the hover preview, the confirmed cue stays --");
{
  // Exercises renderHoverCue directly the way bindGlobal's mouseleave
  // listener calls it (no slotOverride) after a hover preview painted a
  // different row than the committed target — the wiring itself is covered
  // by the static bindGlobal check above; this proves the mouseleave call
  // clears the stale preview without touching the independent confirmed cue.
  const rowB = mkRow(B, 100, 40);
  const rowC = mkRow(C, 200, 40);
  freshEnv([rowB, rowC]);
  const snap = { clipboard_count: 1, cursor: [], paste_slot: { Into: C } };
  setSnap(snap);
  renderConfirmedPasteCue(snap);
  check("confirmed cue painted C before the hover", rowC.classes.has("paste-target"));
  setSession(sessionStub({ Into: B }, []));
  onArmedPasteHover(evOn(rowB, 110));
  check("hover preview painted B before the simulated mouseleave", rowB.classes.has("drag-over-into"));
  renderHoverCue(snap, undefined); // what bindGlobal's mouseleave handler calls
  check("mouseleave clears the hover preview on B", !rowB.classes.has("drag-over-into"));
  check("mouseleave leaves the confirmed target C's paste-target class untouched", rowC.classes.has("paste-target"));
}

console.log(failures === 0 ? "\nALL PASTE-HOVER CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
