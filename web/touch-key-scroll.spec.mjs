// Plain-Node test for the touch UI's keyboard-nav scroll follow
// (`scrollFocusIntoView`) and the shared undrawn-root cursor correction
// (`drawnCursorFallback`).
//
// The defect: `render()` re-applies the captured `treePane.scrollTop` verbatim
// after the `innerHTML` rebuild (so a tap never snaps the pane to the top),
// which also meant a cursor/paste-slot step past a viewport edge left the
// focus off-screen with NO scroll at all — desktop gets that for free from
// `renderTree`'s `scrollIntoView` (`web/render.ts`), touch had no equivalent.
// Reproduced in a real Chromium first (pane 594px, cursor row at y≈1657 with
// `scrollTop` stuck at 0 after `End`).
//
// Contract proven below (sticky-cursor scrolling: minimal scroll, one path for
// every input):
//   1. an anchor fully inside the viewport never moves `scrollTop`;
//   2. an anchor past the bottom/top edge scrolls by EXACTLY the overflow
//      (never centered);
//   3. only `treePane.scrollTop` is written — never `Element.scrollIntoView`,
//      which would also scroll the page and slide the `position:absolute` app
//      shell out from under its bottom-anchored sheets;
//   4. the anchor follows what the focus visually *is*: the cursor row
//      normally, but in paste mode the `.reorder-line` for `After` /
//      `Into(root)` and the target row for a deeper `Into` (core routes arrows
//      to `move_paste_slot`, so the cursor does not move at all);
//   5. `Into(root)` — the slot paste-mode `Home` lands on — is drawn as an
//      insertion line at the tree's top, since `treeHTML` never draws the root
//      row;
//   6. `drawnCursorFallback` re-targets a cursor left on the undrawn root row
//      (`g`/Home, `k` from the first row), in BOTH web hosts.
//
// Follows touch-paste-cue.spec.mjs's convention: no test framework, just a
// `check()` tally; `touch/app.ts` can't be imported in Node (wasm + DOM boot at
// module top level), so `scrollFocusIntoView` and `renderPasteSlotCue` are
// extracted verbatim from the source and type-stripped via esbuild into a
// wrapper module supplying the module-level state they close over — the
// behavioral checks run the real shipped function bodies, not
// reimplementations. `drawnCursorFallback` is imported from the real
// `path-utils.ts`.
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
const uiTs = readFileSync(path.join(here, "ui.ts"), "utf8");

// ---- wiring: every resolved key runs the scroll follow, exactly once ----
console.log("-- wiring: onKey / navSelect --");
const onKeyBlock = appTs.match(/^function onKey\(ev: KeyboardEvent\) \{[\s\S]*?\n\}/m)?.[0] ?? "";
check("onKey found in source", onKeyBlock.length > 0);
check(
  "onKey dispatches the resolved key, then scrolls the focus back into view",
  /if \(!result\) return;\n {2}handleKeyResult\(result, ev\);\n[\s\S]*?\n {2}scrollFocusIntoView\(\);\n\}/.test(
    onKeyBlock,
  ),
  onKeyBlock,
);
check(
  "the scroll follow sits AFTER the field/unresolved-key guards (typing never scrolls the tree)",
  onKeyBlock.indexOf('tag === "INPUT"') < onKeyBlock.indexOf("scrollFocusIntoView()") &&
    onKeyBlock.indexOf("if (!result) return;") < onKeyBlock.indexOf("scrollFocusIntoView()"),
);
check(
  "handleKeyResult carries the whole resolved-key switch",
  /^function handleKeyResult\(result: NonNullable<KeyResolution>, ev: KeyboardEvent\) \{\n {2}switch \(result\.kind\) \{/m.test(
    appTs,
  ),
);
check(
  "scrollFocusIntoView never uses Element.scrollIntoView (would scroll the page/app shell)",
  !/scrollIntoView/.test(appTs.match(/^function scrollFocusIntoView\(\)[\s\S]*?\n\}/m)?.[0] ?? "x scrollIntoView"),
);

const touchNavBlock = appTs.match(/^function touchNavSelect\(i: Intent\) \{[\s\S]*?\n\}/m)?.[0] ?? "";
const uiNavBlock = uiTs.match(/^function navSelect\(i: Intent\) \{[\s\S]*?\n\}/m)?.[0] ?? "";
for (const [host, block] of [["touch touchNavSelect", touchNavBlock], ["desktop navSelect", uiNavBlock]]) {
  check(`${host} found in source`, block.length > 0);
  check(
    `${host} re-targets an undrawn-root cursor BEFORE collapsing the selection onto it`,
    /const drawn = drawnCursorFallback\(snap\);\n\s*if \(drawn\) send\(\{ SetCursor: drawn \}\);\n\s*send\(\{ SetSelection: \{ paths: \[snap!\.cursor\] \} \}\);/.test(
      block,
    ),
    block,
  );
  check(
    `${host} skips the correction in paste mode (arrows move the slot, cursor is frozen)`,
    /if \(snap && \(snap\.clipboard_count \?\? 0\) === 0\) \{/.test(block),
    block,
  );
}

// ---- extract + execute the real functions from touch/app.ts ----
console.log("\n-- extraction --");
const NAMES = ["renderPasteSlotCue", "scrollFocusIntoView"];
const fns = NAMES.map((n) => appTs.match(new RegExp(`^function ${n}\\([\\s\\S]*?\\n\\}`, "m"))?.[0]);
fns.forEach((s, i) => check(`${NAMES[i]} extracted verbatim`, !!s));

globalThis.CSS = { escape: (s) => s };
globalThis.getComputedStyle = (el) => ({
  paddingLeft: el?._paddingLeft ?? "10px",
  getPropertyValue: (prop) => (prop === "--indent" ? (el?._indentStep ?? "18px") : ""),
});

const src = `import { slotLineIndentPx } from "./slot-line.js";
export { drawnCursorFallback } from "./path-utils.js";
let snap = null;
let treeEl = null;
let treePane = null;
let rawView = false;
let reordering = false;
export function setEnv(e) {
  if ("snap" in e) snap = e.snap;
  if ("treeEl" in e) treeEl = e.treeEl;
  if ("treePane" in e) treePane = e.treePane;
  if ("rawView" in e) rawView = e.rawView;
  if ("reordering" in e) reordering = e.reordering;
}
export ${fns[0]}
export ${fns[1]}
`;
const built = await esbuild.build({
  stdin: { contents: src, resolveDir: here, loader: "ts" },
  bundle: true,
  write: false,
  format: "esm",
  platform: "node",
  target: "es2022",
});
const mod = await import(
  "data:text/javascript;base64," + Buffer.from(built.outputFiles[0].text).toString("base64")
);

// ---- fakes: rows/line/pane with live geometry, no jsdom ----
const ROW_H = 54;
function mkRow(pathJson, top, padPx = 10) {
  const live = new Set();
  const rowMain = { _paddingLeft: `${padPx}px` };
  return {
    dataset: { path: pathJson },
    _top: top,
    classList: { add: (c) => live.add(c), remove: (c) => live.delete(c), contains: (c) => live.has(c) },
    classes: live,
    getBoundingClientRect: () => ({ top: top - pane.scrollTop, height: ROW_H, bottom: top - pane.scrollTop + ROW_H }),
    querySelector: (sel) => (sel === ".row-main" ? rowMain : null),
  };
}
// The reorder-line lives in the tree's own (scrolled) coordinate space: its
// `style.top` is content-relative, exactly as renderPasteSlotCue writes it.
const line = {
  style: { display: "none", top: "0px", left: "0px" },
  getBoundingClientRect: () => {
    const top = parseFloat(line.style.top) - pane.scrollTop;
    return { top, height: 2.5, bottom: top + 2.5 };
  },
};
let pane;
function mkPane(height, scrollTop) {
  return { scrollTop, getBoundingClientRect: () => ({ top: 0, height, bottom: height }) };
}
// Rows at content y = 0, 54, 108, … (viewport shows ~4 of them).
const rows = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9].map((i) =>
  mkRow(JSON.stringify([{ Index: i }]), i * ROW_H),
);
const treeEl = {
  querySelector: (sel) => {
    if (sel === ".reorder-line") return line;
    if (sel === ".row") return rows[0];
    const m = sel.match(/^\.row\[data-path='(.+)'\]$/);
    return m ? (rows.find((r) => r.dataset.path === m[1]) ?? null) : null;
  },
  querySelectorAll: (sel) => (sel === ".drop-into" ? rows.filter((r) => r.classes.has("drop-into")) : []),
  getBoundingClientRect: () => ({ top: -pane.scrollTop }),
};
const cursorAt = (i) => ({ cursor: [{ Index: i }], paste_slot: undefined, rows: [] });
const PANE_H = 216; // exactly 4 rows

function run(snap, { scrollTop = 0, rawView = false } = {}) {
  pane = mkPane(PANE_H, scrollTop);
  mod.setEnv({ snap, treeEl, treePane: pane, rawView, reordering: false });
  if (snap) mod.renderPasteSlotCue(snap);
  mod.scrollFocusIntoView();
  return pane.scrollTop;
}

// ---- 1. cursor row: minimal scroll at each edge, no-op inside ----
console.log("\n-- scrollFocusIntoView(): cursor row --");
check("cursor fully inside the viewport does not scroll", run(cursorAt(2), { scrollTop: 0 }) === 0);
check(
  "cursor exactly filling the last visible row does not scroll",
  run(cursorAt(3), { scrollTop: 0 }) === 0,
);
check(
  "cursor one row past the bottom scrolls by exactly the overflow (54), not centered",
  run(cursorAt(4), { scrollTop: 0 }) === 54,
  String(run(cursorAt(4), { scrollTop: 0 })),
);
check(
  "cursor far past the bottom scrolls only enough to reveal it",
  run(cursorAt(9), { scrollTop: 0 }) === 9 * ROW_H + ROW_H - PANE_H,
  String(run(cursorAt(9), { scrollTop: 0 })),
);
check(
  "cursor above the top scrolls up by exactly the overflow",
  run(cursorAt(1), { scrollTop: 200 }) === ROW_H,
  String(run(cursorAt(1), { scrollTop: 200 })),
);
check("cursor at the first row scrolls to the very top", run(cursorAt(0), { scrollTop: 500 }) === 0);
check(
  "raw view is left alone (no tree on screen)",
  run(cursorAt(9), { scrollTop: 0, rawView: true }) === 0,
);
check(
  "a cursor with no drawn row (undrawn root) never moves the scroll",
  run({ cursor: [], paste_slot: undefined, rows: [] }, { scrollTop: 120 }) === 120,
);

// ---- 2. paste mode: the slot, not the cursor, is the focus ----
console.log("\n-- scrollFocusIntoView(): paste mode ----");
const afterSlot = (i) => ({ cursor: [{ Index: 0 }], paste_slot: { After: [{ Index: i }] }, rows: [] });
const intoSlot = (p) => ({ cursor: [{ Index: 0 }], paste_slot: { Into: p }, rows: [] });
check(
  "After(row past the bottom) scrolls the insertion LINE into view, not just its row",
  run(afterSlot(4), { scrollTop: 0 }) === 54 + 2.5,
  String(run(afterSlot(4), { scrollTop: 0 })),
);
check(
  "After slot inside the viewport does not scroll",
  run(afterSlot(2), { scrollTop: 0 }) === 0,
);
check(
  "Into(deep row past the bottom) scrolls that row into view",
  run(intoSlot([{ Index: 5 }]), { scrollTop: 0 }) === 5 * ROW_H + ROW_H - PANE_H,
  String(run(intoSlot([{ Index: 5 }]), { scrollTop: 0 })),
);
check(
  "Into(root) — paste-mode Home — scrolls the tree back to the top",
  run(intoSlot([]), { scrollTop: 400 }) === 0,
  String(run(intoSlot([]), { scrollTop: 400 })),
);
check(
  "paste-mode navigation ignores where the (frozen) cursor is",
  run({ cursor: [{ Index: 9 }], paste_slot: { After: [{ Index: 1 }] }, rows: [] }, { scrollTop: 0 }) === 0,
);

// ---- 3. renderPasteSlotCue: Into(root) is drawn at the tree's top ----
console.log("\n-- renderPasteSlotCue(): Into(root) ----");
{
  pane = mkPane(PANE_H, 0);
  mod.setEnv({ snap: null, treeEl, treePane: pane, rawView: false, reordering: false });
  mod.renderPasteSlotCue({ paste_slot: { Into: [] } });
  check("Into(root) shows the insertion line", line.style.display === "block");
  check("Into(root) draws it at the very top of the tree", line.style.top === "0px", line.style.top);
  check("Into(root) uses the first row's own indent", line.style.left === "10px", line.style.left);
  mod.renderPasteSlotCue({ paste_slot: { Into: [{ Index: 2 }] } });
  check(
    "a deeper Into still outlines the row and hides the line (unchanged)",
    rows[2].classes.has("drop-into") && line.style.display === "none",
  );
}

// ---- 4. drawnCursorFallback (shared by both hosts) ----
console.log("\n-- drawnCursorFallback() ----");
const snapRows = [{ path: [] }, { path: [{ Index: 0 }] }, { path: [{ Key: "a" }] }];
check(
  "cursor on the undrawn root row re-targets the first drawn row",
  JSON.stringify(mod.drawnCursorFallback({ cursor: [], rows: snapRows })) ===
    JSON.stringify([{ Index: 0 }]),
);
check(
  "cursor already on a drawn row is left alone",
  mod.drawnCursorFallback({ cursor: [{ Key: "a" }], rows: snapRows }) === null,
);
check(
  "empty document: nothing to re-target",
  mod.drawnCursorFallback({ cursor: [], rows: [{ path: [] }] }) === null,
);

console.log(failures === 0 ? "\nALL TOUCH KEY-SCROLL CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
