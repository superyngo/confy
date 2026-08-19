// Plain-Node test for touch/app.ts's pointer-target classification (ADR 0004
// §1, Task 10): while the clipboard is armed, a tap must position the paste
// target via `Session.pointerSlot(path, relY)` -> `Intent.SetPasteSlot`
// (`Into`/`After` from the tap's row-relative Y) instead of a bare `SetCursor`,
// and reorder hover into-eligibility must come from that same core
// classification instead of the hand-rolled `.branch`/0.28/0.72 band
// thresholds — which never checked `Format::Inline`, so an inline table's
// mid-band wrongly offered "into" (the per-surface drift ADR 0004 eliminates).
// Follows armed-paste.spec.mjs's convention: no test framework, just a
// `check()` tally. touch/app.ts can't be imported in Node (wasm + DOM boot at
// module top level), so `handleTap`, `onReorderMove`, `clearInto`, `pathOf`
// and `startsWith` are extracted verbatim from the source and type-stripped
// via esbuild into one wrapper module supplying the module-level state they
// close over — the behavioral checks below run the real shipped function
// bodies, not reimplementations — and the call sites are verified
// structurally, same as TOOLBAR_ENTRIES.
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

// ---- wiring: signature + call sites ----
check(
  "pointerup call site threads e.clientY into handleTap",
  /handleTap\(e\.target as HTMLElement, dragRow, e\.clientY\);/.test(appTs),
);
check(
  "handleTap takes clientY as its 3rd param",
  /function handleTap\(target: HTMLElement, row: HTMLElement, clientY: number\) \{/.test(appTs),
);
const handleTapBlock = appTs.match(/^function handleTap\([\s\S]*?\n\}/m)?.[0] ?? "";
check("handleTap found in source", handleTapBlock.length > 0);
check(
  "handleTap routes armed taps through session.pointerSlot -> SetPasteSlot",
  /session\.pointerSlot\(path, relY\)/.test(handleTapBlock) && /\{ SetPasteSlot: slot \}/.test(handleTapBlock),
);
check(
  "handleTap computes relY from clientY over the row's bounding rect",
  /\(clientY - r\.top\) \/ \(r\.height \|\| 1\)/.test(handleTapBlock),
);
check(
  "both armed branches send armedTarget(), not a bare SetCursor",
  (handleTapBlock.match(/send\(armedTarget\(\)\)/g) ?? []).length === 2 &&
    !/clipboard_count[^;\n]*\)\s*> 0[^\n]*SetCursor/.test(handleTapBlock),
);
const reorderBlock = appTs.match(/^function onReorderMove\([\s\S]*?\n\}/m)?.[0] ?? "";
check("onReorderMove found in source", reorderBlock.length > 0);
check(
  "onReorderMove classifies via session?.pointerSlot(pathOf(hit)!, rel)",
  /session\?\.pointerSlot\(pathOf\(hit\)!, rel\)/.test(reorderBlock),
);
check(
  'onReorderMove offers "into" only when the slot is Into ("Into" in slot)',
  /"Into" in slot/.test(reorderBlock) && /reMode = "into"/.test(reorderBlock),
);
check(
  "onReorderMove keeps the plain 0.5 before/after split when the slot declines",
  /reMode = rel < 0\.5 \? "before" : "after"/.test(reorderBlock),
);
check(
  "onReorderMove no longer hand-rolls thresholds (.branch / 0.28 / 0.72 gone)",
  !/classList\.contains\("branch"\)|0\.28|0\.72/.test(reorderBlock),
);

// ---- extract + execute the real functions from touch/app.ts ----
// The wrapper module declares the module-level state the extracted bodies
// close over (session/snap/treeEl, the reorder state machine, the tap
// double-tap bookkeeping) and stubs the app helpers (send/selectOnly/...) to
// record into globalThis hooks, so each block below can arm its own scenario.
const fns = ["pathOf", "startsWith", "clearInto", "onReorderMove", "handleTap"]
  .map((n) => appTs.match(new RegExp(`^function ${n}\\([\\s\\S]*?\\n\\}`, "m"))?.[0])
  .map((s, i) => {
    check(`${["pathOf", "startsWith", "clearInto", "onReorderMove", "handleTap"][i]} extracted verbatim`, !!s);
    return s ?? `function ${["pathOf", "startsWith", "clearInto", "onReorderMove", "handleTap"][i]}() {}`;
  });

// Hook state must exist before the wrapper module below evaluates (its
// send/selectOnly stubs capture `globalThis.__touchHooks` at eval time).
const H = (globalThis.__touchHooks = { sent: [], ops: [] });
let mod = null;
{
  const src = `let session = null;
let snap = null;
let treeEl = null;
let openSwipeMain = null;
let lastTapKey = null;
let lastTapTime = 0;
const DOUBLE_TAP_MS = 300;
let reLine = null;
let reSrcPath = null;
let reStartY = 0;
let reMoved = false;
let reTarget = null;
let reMode = "before";
let reInto = null;
let edgeScrollY = 0;
const H = globalThis.__touchHooks;
const send = (i) => H.sent.push(i);
const sendR = (i) => { H.sent.push(i); return H.snapAfter ?? {}; };
const selectOnly = (p) => H.ops.push("selectOnly " + JSON.stringify(p));
const openPanel = (p) => H.ops.push("openPanel " + JSON.stringify(p));
const toast = (m) => H.ops.push("toast " + m);
const setDelRevealed = (m, on) => H.ops.push("setDelRevealed " + on);
export function setEnv(e) { session = e.session ?? session; snap = e.snap ?? snap; treeEl = e.treeEl ?? treeEl; }
export function resetTap() { lastTapKey = null; lastTapTime = 0; }
export function setReorder(l, p, y) { reLine = l; reSrcPath = p; reStartY = y; reMoved = true; reTarget = null; reMode = "before"; reInto = null; }
export function reorderState() { return { reMode, reTarget, reInto }; }
export ${fns[0]}
export ${fns[1]}
export ${fns[2]}
export ${fns[3]}
export ${fns[4]}
`;
  const built = await esbuild.build({
    stdin: { contents: src, resolveDir: here, loader: "ts" },
    write: false,
    format: "esm",
    target: "es2022",
  });
  const modUrl = "data:text/javascript;base64," + Buffer.from(built.outputFiles[0].text).toString("base64");
  mod = await import(modUrl);
}

// Minimal fakes: only the surface the extracted functions touch — dataset.path,
// bounding rects, live class sets, closest()/querySelector() lookups, and the
// stubbed core pointerSlot. No jsdom (no new npm dependency).
const P = [{ Key: "a" }, { Index: 1 }];
const eq = (a, b) => JSON.stringify(a) === JSON.stringify(b);

function sessionStub(classify, captured) {
  return { pointerSlot: (path, relY) => { captured.push({ path, relY }); return classify(path, relY); } };
}
const rowAt = (key, top, height, classes = []) => {
  const live = new Set(classes);
  return {
    dataset: { path: JSON.stringify([{ Key: key }]) },
    classList: {
      add: (c) => live.add(c),
      remove: (c) => live.delete(c),
      contains: (c) => live.has(c),
    },
    classes: live,
    getBoundingClientRect: () => ({ top, height, bottom: top + height }),
    offsetHeight: height,
    querySelector: () => null,
  };
};
const tapOn = (row, clientY, target = { closest: () => null }) =>
  mod.handleTap(target, row, clientY);

console.log("\n-- armed tap -> pointerSlot classification (plain tap) --");
{
  const captured = [];
  H.sent.length = 0;
  mod.resetTap();
  mod.setEnv({ session: sessionStub(() => ({ Into: P }), captured), snap: { clipboard_count: 1 } });
  tapOn(rowAt("a", 100, 40), 120); // 50% down the row — core's Into mid-band
  check(
    "armed mid-band tap sends SetPasteSlot { Into }",
    H.sent.length === 1 && eq(H.sent[0], { SetPasteSlot: { Into: P } }),
    JSON.stringify(H.sent),
  );
  check("relY is (clientY - rowTop) / rowHeight", captured.at(-1)?.relY === 0.5, JSON.stringify(captured.at(-1)));
  check("pointerSlot receives the tapped path", eq(captured.at(-1)?.path, [{ Key: "a" }]));
}
{
  const captured = [];
  H.sent.length = 0;
  mod.resetTap();
  mod.setEnv({ session: sessionStub(() => ({ After: [{ Key: "a" }] }), captured), snap: { clipboard_count: 1 } });
  tapOn(rowAt("a", 100, 40), 135); // 87.5% down — core's After bottom band
  check(
    "armed bottom-band tap sends SetPasteSlot { After }",
    H.sent.length === 1 && eq(H.sent[0], { SetPasteSlot: { After: [{ Key: "a" }] } }),
    JSON.stringify(H.sent),
  );
}
{
  const captured = [];
  H.sent.length = 0;
  // pointer_slot returns None for a row that is no longer visible (stale tap):
  // the tap must fall back to a bare cursor move, same as ui.ts.
  mod.resetTap();
  mod.setEnv({ session: sessionStub(() => undefined, captured), snap: { clipboard_count: 1 } });
  tapOn(rowAt("a", 100, 40), 120);
  check(
    "unclassifiable armed tap falls back to SetCursor",
    H.sent.length === 1 && eq(H.sent[0], { SetCursor: [{ Key: "a" }] }),
    JSON.stringify(H.sent),
  );
}

console.log("\n-- caret tap while armed positions the paste target too --");
{
  H.sent.length = 0;
  H.ops.length = 0;
  const caretBtn = { dataset: { act: "caret" } };
  const caretTap = { closest: (sel) => (sel === "[data-act]" ? caretBtn : null) };
  mod.resetTap();
  mod.setEnv({ session: sessionStub(() => ({ Into: [{ Key: "a" }] }), []), snap: { clipboard_count: 1 } });
  mod.handleTap(caretTap, rowAt("a", 100, 40), 120);
  check(
    "armed caret tap sends SetPasteSlot then SetCursor then ToggleExpand",
    H.sent.length === 3 &&
      eq(H.sent[0], { SetPasteSlot: { Into: [{ Key: "a" }] } }) &&
      eq(H.sent[1], { SetCursor: [{ Key: "a" }] }) &&
      H.sent[2] === "ToggleExpand",
    JSON.stringify(H.sent),
  );
  check("armed caret tap does not re-freeze the selection (no selectOnly)", !H.ops.some((o) => o.startsWith("selectOnly")));
}
{
  H.sent.length = 0;
  H.ops.length = 0;
  const caretBtn = { dataset: { act: "caret" } };
  const caretTap = { closest: (sel) => (sel === "[data-act]" ? caretBtn : null) };
  mod.resetTap();
  mod.setEnv({ session: sessionStub(() => undefined, []), snap: { clipboard_count: 0 } });
  mod.handleTap(caretTap, rowAt("a", 100, 40), 120);
  check(
    "disarmed caret tap still selects + expands",
    H.sent.length === 2 &&
      eq(H.sent[0], { SetCursor: [{ Key: "a" }] }) &&
      H.sent[1] === "ToggleExpand" &&
      H.ops.some((o) => o.startsWith("selectOnly")),
    JSON.stringify({ sent: H.sent, ops: H.ops }),
  );
}

console.log("\n-- disarmed tap + double-tap precedence --");
{
  H.sent.length = 0;
  H.ops.length = 0;
  mod.resetTap();
  mod.setEnv({ session: sessionStub(() => ({ Into: P }), []), snap: { clipboard_count: 0 } });
  tapOn(rowAt("a", 100, 40), 120);
  check("disarmed tap selects only (no intent sent by handleTap itself)", H.sent.length === 0 && H.ops.some((o) => o.startsWith("selectOnly")), JSON.stringify({ sent: H.sent, ops: H.ops }));
}
{
  H.sent.length = 0;
  H.ops.length = 0;
  mod.resetTap();
  mod.setEnv({ session: sessionStub(() => ({ Into: P }), []), snap: { clipboard_count: 1 } });
  const row = rowAt("a", 100, 40);
  tapOn(row, 120); // first tap: armed -> positions paste target
  tapOn(row, 120); // second tap within DOUBLE_TAP_MS -> panel wins
  check(
    "double-tap still opens the panel even while armed (no second SetPasteSlot)",
    H.ops.some((o) => o.startsWith("openPanel")) && H.sent.filter((i) => "SetPasteSlot" in i).length === 1,
    JSON.stringify({ sent: H.sent, ops: H.ops }),
  );
}

console.log("\n-- reorder hover: into-eligibility from core pointerSlot --");
{
  // Live class sets make the stale-cue checks below fail if clearInto() ever
  // goes missing from onReorderMove — the exact analog of the dnd.ts
  // clearOver() regression the Task 9 browser smoke test caught.
  const rowSrc = rowAt("src", 0, 40); // dragged row (excluded as a candidate)
  const rowB = rowAt("b", 100, 40, ["branch"]); // .branch so the OLD hand-rolled path would fire
  const rowC = rowAt("c", 200, 40, ["branch"]);
  const reLine = { style: { display: "", top: "" } };
  const treeEl = {
    querySelectorAll: (sel) => (sel === ".row" ? [rowSrc, rowB, rowC] : []),
    getBoundingClientRect: () => ({ top: 0 }),
  };
  let slotFor = () => undefined; // what the stubbed core classifies
  const captured = [];
  mod.setEnv({
    treeEl,
    session: sessionStub((p) => slotFor(p), captured),
  });
  const move = (y) => mod.onReorderMove(y);
  const reset = () => {
    mod.setReorder(reLine, [{ Key: "src" }], 0);
    rowB.classes.delete("drop-into");
    rowC.classes.delete("drop-into");
    reLine.style.display = "";
  };

  reset();
  slotFor = () => ({ Into: [{ Key: "b" }] });
  move(120); // rel 0.5 on rowB — core's Into mid-band
  check(
    "pointerSlot Into at mid-band -> reMode into + drop-into class, line hidden",
    mod.reorderState().reMode === "into" && rowB.classes.has("drop-into") && reLine.style.display === "none",
    JSON.stringify({ st: mod.reorderState(), display: reLine.style.display }),
  );
  check("pointerSlot consulted with the hovered path and relY", captured.at(-1)?.relY === 0.5 && eq(captured.at(-1)?.path, [{ Key: "b" }]), JSON.stringify(captured.at(-1)));

  reset();
  // An inline table (`Format::Inline`): core answers After even mid-band. The
  // old hand-rolled .branch/0.28/0.72 thresholds wrongly offered "into" here.
  slotFor = () => ({ After: [{ Key: "b" }] });
  move(120);
  check(
    "inline branch mid-band no longer offers into (core says After)",
    mod.reorderState().reMode === "after" && !rowB.classes.has("drop-into") && reLine.style.display === "block",
    JSON.stringify({ st: mod.reorderState(), display: reLine.style.display }),
  );

  reset();
  slotFor = () => undefined; // path not visible — pointerSlot declines
  move(105); // rel 0.125 -> before
  check("declined slot keeps the plain 0.5 split (top -> before)", mod.reorderState().reMode === "before" && reLine.style.top === "100px", JSON.stringify({ st: mod.reorderState(), top: reLine.style.top }));
  move(120); // rel 0.5 -> after
  check("declined slot keeps the plain 0.5 split (mid -> after)", mod.reorderState().reMode === "after");

  reset();
  slotFor = (p) => (eq(p, [{ Key: "b" }]) ? { Into: [{ Key: "b" }] } : undefined);
  move(120); // into on B
  check("into on B arms the cue", rowB.classes.has("drop-into") && !rowC.classes.has("drop-into"));
  move(220); // rel 0.5 on C, declined -> before/after: B's cue must be cleared
  check(
    "hovering away clears the previous row's drop-into (clearInto kept)",
    !rowB.classes.has("drop-into") && !rowC.classes.has("drop-into") && reLine.style.display === "block",
    JSON.stringify({ b: [...rowB.classes], c: [...rowC.classes], display: reLine.style.display }),
  );
  slotFor = () => ({ Into: [{ Key: "c" }] });
  move(220); // into on C — the reInto !== hit path must swap the cue
  check(
    "into on a different row swaps the cue (old row cleared, new armed)",
    !rowB.classes.has("drop-into") && rowC.classes.has("drop-into") && reLine.style.display === "none",
    JSON.stringify({ b: [...rowB.classes], c: [...rowC.classes], display: reLine.style.display }),
  );
}

console.log(failures === 0 ? "\nALL TOUCH POINTER-SLOT CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
