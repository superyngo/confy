// Plain-Node test for dnd.ts's copy-modifier support (ADR 0004 §1, Task 12):
// holding ⌥ (altKey) or Ctrl (ctrlKey) during a drag-drop copies instead of
// moving — the native `dropEffect` reflects the held modifier during
// `dragover`, and `drop` threads `cut: !(altKey || ctrlKey)` into both
// `MoveSelectionTo` sends (into, and before/after). Follows dnd-into.spec.mjs's
// convention: no test framework, just a `check()` tally; dnd.ts is bundled via
// esbuild and run against a minimal DOM shim, so the checks below exercise the
// real shipped dragover/drop handlers.
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
  let slot; // what the stubbed core pointer_slot classifies the hover as
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
    (p, relY) => slot,
    () => ops.push("onDragEnd"),
  );

  const dragstart = () =>
    listeners.dragstart[0]({ target: rowA, dataTransfer: { setData() {} } });
  const dragover = (clientY, mods = {}) => {
    const dt = {};
    listeners.dragover[0]({ target: rowB, clientY, dataTransfer: dt, preventDefault() {}, ...mods });
    return dt;
  };
  const drop = (mods = {}) => listeners.drop[0]({ preventDefault() {}, ...mods });
  const lastSend = () => sent.at(-1);

  console.log("\n-- dragover dropEffect reflects the held modifier --");
  dragstart();
  let dt = dragover(120); // no modifier
  check("plain dragover sets dropEffect to move", dt.dropEffect === "move", dt.dropEffect);
  dt = dragover(120, { altKey: true });
  check("alt-held dragover sets dropEffect to copy", dt.dropEffect === "copy", dt.dropEffect);
  dt = dragover(120, { ctrlKey: true });
  check("ctrl-held dragover sets dropEffect to copy", dt.dropEffect === "copy", dt.dropEffect);
  dt = dragover(120, { altKey: false, ctrlKey: false });
  check("dropEffect reverts to move once the modifier is released", dt.dropEffect === "move", dt.dropEffect);

  console.log("\n-- drop threads cut through the Into send --");
  slot = { Into: B };
  sent.length = 0;
  dragstart();
  dragover(120);
  drop();
  check(
    "plain drop (no modifier) sends cut: true (a move) into the Into target",
    eq(lastSend(), { MoveSelectionTo: { sources: [A], target: B, index: 0, cut: true } }),
    JSON.stringify(lastSend()),
  );

  sent.length = 0;
  dragstart();
  dragover(120, { altKey: true });
  drop({ altKey: true });
  check(
    "alt-held drop sends cut: false (a copy) into the Into target",
    eq(lastSend(), { MoveSelectionTo: { sources: [A], target: B, index: 0, cut: false } }),
    JSON.stringify(lastSend()),
  );

  sent.length = 0;
  dragstart();
  dragover(120, { ctrlKey: true });
  drop({ ctrlKey: true });
  check(
    "ctrl-held drop sends cut: false (a copy) into the Into target",
    eq(lastSend(), { MoveSelectionTo: { sources: [A], target: B, index: 0, cut: false } }),
    JSON.stringify(lastSend()),
  );

  console.log("\n-- drop threads cut through the before/after sibling send --");
  slot = undefined; // declines Into -> before/after sibling math
  sent.length = 0;
  dragstart();
  dragover(120); // rel 0.5 -> after, no modifier
  drop();
  check(
    "plain sibling drop sends cut: true (a move)",
    eq(lastSend(), { MoveSelectionTo: { sources: [A], target: [], index: 2, cut: true } }),
    JSON.stringify(lastSend()),
  );

  sent.length = 0;
  dragstart();
  dragover(120, { altKey: true }); // rel 0.5 -> after, alt held
  drop({ altKey: true });
  check(
    "alt-held sibling drop sends cut: false (a copy)",
    eq(lastSend(), { MoveSelectionTo: { sources: [A], target: [], index: 2, cut: false } }),
    JSON.stringify(lastSend()),
  );

  console.log("\n-- drop reads the modifier from the drop event itself, not the last dragover --");
  sent.length = 0;
  dragstart();
  dragover(120, { altKey: true }); // last dragover held alt
  drop(); // but drop itself has no modifier
  check(
    "drop's own modifier state wins over the last dragover's",
    eq(lastSend(), { MoveSelectionTo: { sources: [A], target: [], index: 2, cut: true } }),
    JSON.stringify(lastSend()),
  );
}

console.log(failures === 0 ? "\nALL DND-COPY CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
