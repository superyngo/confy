// Plain-Node test for dnd.ts's drop targeting (ADR 0004 §1 → ADR 0010): the
// grip drag's destination must be the `PasteSlot` the injected core
// `pointerSlot(path, relY)` returns — the WHOLE destination, not just the
// into/not-into half. dnd.ts used to ask core only "is this hover an Into",
// then hand-roll the rest as `parentOf(path)` + `siblingIndex(...) ± 1` with a
// 0.5 split; that contradicted core on two counts (`After` an *expanded*
// branch means its FIRST CHILD, and core's leaf boundary is 0.75), so a drag
// and an armed paste released at the same pixel landed in different places.
// Follows armed-paste.spec.mjs's convention: no test framework, just a
// `check()` tally; dnd.ts is bundled via esbuild and run against a minimal DOM
// shim, so the behavioral checks below exercise the real shipped
// dragstart/dragover/drop handlers, and the wiring checks verify ui.ts's call
// site structurally, same as TOOLBAR_ENTRIES.
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
  "dragover stores the slot core returned, with no local band arithmetic",
  /slot = pointerSlot\(path, \(ev\.clientY - r\.top\) \/ r\.height\) \?\? null;/.test(dndTs),
);
check(
  "dnd.ts no longer derives a parent/index (no siblingIndex / parentOf / 0.5 split)",
  // Comments stripped: the file's header still *describes* the removed math.
  !/siblingIndex|parentOf|rel < 0\.5/.test(dndTs.replace(/^\s*\/\/.*$/gm, "")),
);
check(
  "drop hands the slot to MoveSelectionTo verbatim",
  /send\(\{ MoveSelectionTo: \{ sources: src, slot: dest, cut \} \}\);/.test(dndTs),
);
check(
  "ui.ts wires session.pointerSlot as the 4th arg, cue-restore kept 5th",
  /installDnd\(tree, \(\) => snap, send, \(p, r\) => session!\.pointerSlot\(p, r\), \(\) => \{\s*\n\s*if \(snap\) \{ renderConfirmedPasteCue\(snap\); renderHoverCue\(snap, undefined\); \}\s*\n\s*\}\);/.test(uiTs),
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
    // Real `data-path` lookup: the `After` line is drawn under the SLOT's row,
    // which is not always the hovered one.
    querySelector: (sel) => liveRows.find((r) => sel.includes(r.dataset.path)) ?? null,
  };
  globalThis.document = {
    getElementById: (id) =>
      id === "dropLine" ? dropLine : { getBoundingClientRect: () => ({ top: 0 }), scrollTop: 0 }, // treeWrap
  };
  globalThis.CSS = { escape: (s) => s };
  // `slotLineIndentPx` reads the live `--indent` step; Node has no CSSOM.
  globalThis.getComputedStyle = () => ({ getPropertyValue: () => "22px" });
  const snap = { rows: [{ path: A }, { path: B }] };
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

  console.log("\n-- pointerSlot Into → into-target drop --");
  slot = { Into: B };
  ops.length = 0; // per-block history: the declined hover above already showed the line
  dragover(120);
  check("Into classification outlines the row", ops.includes("b add drag-over-into"), JSON.stringify(ops));
  check("Into classification withholds the dropLine", !ops.includes("dropLine.display=block"));
  drop();
  check(
    "drop after Into sends the Into slot verbatim — core resolves the index",
    eq(lastSend(), { MoveSelectionTo: { sources: [A], slot: { Into: B }, cut: true } }),
    JSON.stringify(lastSend()),
  );
  check("drag lifecycle still fires the onDragEnd cue-restore hook", ops.includes("onDragEnd"));

  console.log("\n-- pointerSlot After → the same slot, no sibling math --");
  sent.length = 0;
  ops.length = 0;
  dragstart();
  slot = { After: B };
  dragover(120);
  check("After classification shows the dropLine", ops.includes("dropLine.display=block"), JSON.stringify(ops));
  check("After classification does not outline the row", !ops.includes("b add drag-over-into"));
  check("line sits at the slot row's bottom edge", dropLine.style.top === "140px", JSON.stringify(dropLine.style));
  check("line sits at the slot row's own indent + 8", dropLine.style.left === "32px", JSON.stringify(dropLine.style));
  drop();
  check(
    "drop sends the After slot verbatim (no parent/index derived here)",
    eq(lastSend(), { MoveSelectionTo: { sources: [A], slot: { After: B }, cut: true } }),
    JSON.stringify(lastSend()),
  );

  // The slot's row can differ from the hovered row: core's top band resolves to
  // the PRECEDING slot in `paste_slots()`'s flattened order, so the cue must
  // follow the slot, not the pointer.
  console.log("\n-- the line follows the slot's row, not the hovered row --");
  ops.length = 0;
  dragstart();
  slot = { After: A }; // hovering rowB (top 100), slot points at rowA (top 0)
  dragover(104);
  check("line drawn under the slot's row A, not hovered row B", dropLine.style.top === "40px", JSON.stringify(dropLine.style));

  // ADR 0010's visual half: `After(<expanded branch>)` inserts as that
  // branch's first child, so the line belongs one level deeper.
  console.log("\n-- After(expanded branch) indents the line one level deeper --");
  ops.length = 0;
  dragstart();
  rowB.classes.add("branch");
  rowB.classes.add("open");
  slot = { After: B };
  dragover(120);
  check(
    "expanded-branch line gains one --indent step (24 + 22 + 8)",
    dropLine.style.left === "54px",
    JSON.stringify(dropLine.style),
  );
  rowB.classes.delete("branch");
  rowB.classes.delete("open");

  console.log("\n-- an unclassifiable hover is not a drop target --");
  sent.length = 0;
  ops.length = 0;
  dragstart();
  slot = undefined;
  dragover(120);
  check("declined hover shows no line", !ops.includes("dropLine.display=block"), JSON.stringify(ops));
  check("declined hover does not outline the row", !ops.includes("b add drag-over-into"));
  drop();
  check("declined hover sends nothing on drop", sent.length === 0, JSON.stringify(sent));
  check("declined drop still runs the cue-restore hook", ops.includes("onDragEnd"));

  console.log("\n-- cross-hover cleanup (clearOver between hovers) --");
  ops.length = 0;
  dragstart();
  slot = { Into: B };
  dragover(120); // Into outlined
  check("Into hover outlines the row", rowB.classes.has("drag-over-into"));
  slot = { After: B };
  dragover(120); // next hover is an After — stale outline must not survive
  check(
    "later After hover clears the stale Into outline",
    !rowB.classes.has("drag-over-into"),
  );
  check("clearOver hid the line before the After branch redrew it", ops.includes("dropLine.display=none"));
  check("After hover leaves the line shown", dropLine.style.display === "block");
  slot = { Into: B };
  dragover(120); // and back to Into — a previously shown line must not survive
  check("later Into hover hides the dropLine", dropLine.style.display === "none");
  check("later Into hover re-outlines the row", rowB.classes.has("drag-over-into"));
}

console.log(failures === 0 ? "\nALL DND-INTO CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
