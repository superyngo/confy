// Plain-Node test for dnd.ts's dragover into-eligibility routing (ADR 0004 §1,
// Task 9): the "should this hover offer an Into drop" decision must come from
// the injected core `pointerSlot(path, relY)` callback (Task 4's
// `Session.pointerSlot`), replacing the hand-rolled `vr?.is_branch &&
// vr.format !== "Inline"` copy — the exact drift point the ADR calls out, since
// touch/app.ts's own copy had already diverged. Follows armed-paste.spec.mjs's
// convention: no test framework, just a `check()` tally; dnd.ts is bundled via
// esbuild and run against a minimal DOM shim, so the behavioral checks below
// exercise the real shipped dragstart/dragover/drop handlers, and the wiring
// checks verify ui.ts's call site structurally, same as TOOLBAR_ENTRIES.
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

const dndTs = readFileSync(path.join(here, "dnd.ts"), "utf8");
const uiTs = readFileSync(path.join(here, "ui.ts"), "utf8");

// ---- wiring: signature + call site ----
check(
  "installDnd takes pointerSlot as the 4th param, before optional onDragEnd",
  /send: \(i: Intent\) => void,\s*\n\s*pointerSlot: \(path: Path, relY: number\) => PasteSlot \| undefined,[\s\S]*?onDragEnd\?: \(\) => void,/.test(dndTs),
);
check(
  "dnd.ts no longer hand-rolls into-eligibility (is_branch / Format::Inline check gone)",
  !/\bis_branch\b|format !== "Inline"/.test(dndTs),
);
check(
  "dragover classifies via isInto(pointerSlot(path, rel))",
  /isInto\(pointerSlot\(path, rel\)\)/.test(dndTs),
);
check(
  "ui.ts wires session.pointerSlot as the 4th arg, cue-restore kept 5th",
  /installDnd\(tree, \(\) => snap, send, \(p, r\) => session!\.pointerSlot\(p, r\), \(\) => \{\s*\n\s*if \(snap\) renderPasteSlotCue\(snap\);\s*\n\s*\}\);/.test(uiTs),
);

// ---- behavior: the real dragover/drop handlers against a DOM shim ----
// Minimal fakes: only the surface the handlers touch — closest(".row") /
// closest("[data-grip]") on the event target, dataset.path, bounding rects,
// classList, dropLine style writes, and the injected pointerSlot. No jsdom
// (no new npm dependency).
async function bundleTs(entry) {
  const built = await esbuild.build({
    entryPoints: [path.join(here, entry)],
    bundle: true,
    write: false,
    format: "esm",
    platform: "node",
    target: "es2022",
  });
  const modUrl = "data:text/javascript;base64," + Buffer.from(built.outputFiles[0].text).toString("base64");
  return import(modUrl);
}

const eq = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const A = [{ Key: "a" }];
const B = [{ Key: "b" }];

{
  const ops = [];
  const sent = [];
  const slotCalls = [];
  let slot; // what the stubbed core pointer_slot classifies the hover as
  // Class tracking is REAL (not just op-logged): rows keep a live class set
  // and `treeEl.querySelectorAll(".drag-over-into")` consults it, so the
  // cross-hover cleanup checks below fail if `clearOver()` ever goes missing
  // from the dragover handler (exactly the regression the browser smoke test
  // caught once: a stale Into outline surviving later before/after hovers).
  const classList = (name, classes) => ({
    add: (c) => (ops.push(`${name} add ${c}`), classes.add(c)),
    remove: (c) => (ops.push(`${name} remove ${c}`), classes.delete(c)),
    contains: (c) => classes.has(c),
  });
  const mkRow = (key, top, height) => {
    const classes = new Set();
    const row = {
      dataset: { path: JSON.stringify([{ Key: key }]) },
      classList: classList(key, classes),
      getBoundingClientRect: () => ({ top, height, bottom: top + height }),
      querySelector: (sel) => (sel === ".indent" ? { offsetWidth: 24 } : null),
      classes,
    };
    row.closest = (sel) => (sel === ".row" || sel === "[data-grip]" ? row : null);
    return row;
  };
  const rowA = mkRow("a", 0, 40); // dragged
  const rowB = mkRow("b", 100, 40); // hovered: mid-band at clientY 120 (rel 0.5)
  const liveRows = [rowA, rowB];
  const dropLine = {
    style: new Proxy({}, { set: (t, k, v) => (ops.push(`dropLine.${k}=${v}`), (t[k] = v), true) }),
  };
  const listeners = {};
  const treeEl = {
    addEventListener: (t, fn) => (listeners[t] ??= []).push(fn),
    querySelectorAll: (sel) =>
      sel === ".drag-over-into" ? liveRows.filter((r) => r.classes.has("drag-over-into")) : [],
    querySelector: () => null,
  };
  globalThis.document = {
    getElementById: (id) =>
      id === "dropLine" ? dropLine : { getBoundingClientRect: () => ({ top: 0 }), scrollTop: 0 }, // treeWrap
  };
  globalThis.CSS = { escape: (s) => s };
  const snap = { rows: [{ path: A }, { path: B }] }; // siblingIndex(A,B) => a=0, b=1
  const { installDnd: install } = await bundleTs("dnd.ts");
  install(
    treeEl,
    () => snap,
    (i) => sent.push(i),
    (p, relY) => (slotCalls.push({ p, relY }), slot),
    () => ops.push("onDragEnd"),
  );

  const dragstart = () =>
    listeners.dragstart[0]({ target: rowA, dataTransfer: { setData() {} } });
  const dragover = (clientY) =>
    listeners.dragover[0]({ target: rowB, clientY, dataTransfer: {}, preventDefault() {} });
  const drop = () => listeners.drop[0]({ preventDefault() {} });
  const lastSend = () => sent.at(-1);

  console.log("\n-- dragover consults the injected pointerSlot --");
  dragstart();
  dragover(120); // rel = (120 - 100) / 40 = 0.5 — the old mid-band
  check(
    "dragover asks pointerSlot with the hovered path and relY",
    slotCalls.length === 1 && eq(slotCalls[0].p, B) && slotCalls[0].relY === 0.5,
    JSON.stringify(slotCalls),
  );

  console.log("\n-- pointerSlot Into → into-target drop (child append) --");
  slot = { Into: B };
  ops.length = 0; // per-block history: the declined hover above already showed the line
  dragover(120);
  check("Into classification outlines the row", ops.includes("b add drag-over-into"), JSON.stringify(ops));
  check("Into classification withholds the dropLine", !ops.includes("dropLine.display=block"));
  drop();
  check(
    "drop after Into sends MoveSelectionTo into the hovered path (last child)",
    eq(lastSend(), { MoveSelectionTo: { sources: [A], target: B, index: 0, cut: true } }),
    JSON.stringify(lastSend()),
  );
  check("drag lifecycle still fires the onDragEnd cue-restore hook", ops.includes("onDragEnd"));

  console.log("\n-- pointerSlot declines → before/after sibling math untouched --");
  sent.length = 0;
  ops.length = 0;
  dragstart();
  slot = undefined;
  dragover(120); // rel 0.5 → after
  check("declined Into shows the dropLine instead", ops.includes("dropLine.display=block"), JSON.stringify(ops));
  check("declined Into does not outline the row", !ops.includes("b add drag-over-into"));
  drop();
  check(
    "declined at rel 0.5 drops AFTER the hovered sibling (sib+1)",
    eq(lastSend(), { MoveSelectionTo: { sources: [A], target: [], index: 2, cut: true } }),
    JSON.stringify(lastSend()),
  );
  sent.length = 0;
  ops.length = 0;
  dragstart();
  dragover(104); // rel 0.1 → before
  drop();
  check(
    "top band (rel 0.1) still drops BEFORE the hovered sibling (sib)",
    eq(lastSend(), { MoveSelectionTo: { sources: [A], target: [], index: 1, cut: true } }),
    JSON.stringify(lastSend()),
  );

  console.log("\n-- cross-hover cleanup (clearOver between hovers) --");
  ops.length = 0;
  dragstart();
  slot = { Into: B };
  dragover(120); // Into outlined
  check("Into hover outlines the row", rowB.classes.has("drag-over-into"));
  slot = undefined;
  dragover(120); // next hover declines — stale outline must not survive
  check(
    "later declined hover clears the stale Into outline",
    !rowB.classes.has("drag-over-into"),
  );
  check("clearOver hid the line before the else-branch redrew it", ops.includes("dropLine.display=none"));
  check("declined hover leaves the line shown", dropLine.style.display === "block");
  slot = { Into: B };
  dragover(120); // and back to Into — a previously shown line must not survive
  check("later Into hover hides the dropLine", dropLine.style.display === "none");
  check("later Into hover re-outlines the row", rowB.classes.has("drag-over-into"));
}

console.log(failures === 0 ? "\nALL DND-INTO CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
