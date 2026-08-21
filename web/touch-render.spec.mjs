// Plain-Node test for touch/render.ts's armed-paste `Into` styling (ADR 0004
// §1, Task 11): while the clipboard is armed with an `Into` target, the
// matching row must render with the `drop-into` class touch's drag-reorder
// already uses for its own hover cue (`style.css` `.row.drop-into`), and no
// other row may get it. Follows render.spec.mjs's convention: no test
// framework, just `node:assert` + a `check()` tally; touch/render.ts has no
// DOM/wasm top-level side effects, so it bundles and imports directly (no
// jsdom, no new npm dependency).
import path from "node:path";
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

async function bundle(entry) {
  const result = await esbuild.build({
    entryPoints: [path.join(here, entry)],
    bundle: true,
    write: false,
    format: "esm",
    platform: "node",
    target: "es2022",
  });
  const code = result.outputFiles[0].text;
  const modUrl = "data:text/javascript;base64," + Buffer.from(code).toString("base64");
  return import(modUrl);
}

const { treeHTML } = await bundle("touch/render.ts");

// A minimal well-formed ViewRow (types.ts); tests override specific fields.
function makeRow(overrides = {}) {
  return {
    path: [{ Key: "a" }],
    depth: 1,
    is_branch: true,
    key: "a",
    value: undefined,
    scalar_type: undefined,
    format: "Table",
    type_label: "table",
    child_count: 0,
    trailing_comment: undefined,
    read_only: false,
    violations: undefined,
    selected: false,
    is_cursor: false,
    ...overrides,
  };
}

function makeSnap(rows, pasteSlot) {
  return { rows, paste_slot: pasteSlot, clipboard_paths: [], clipboard_cut: false };
}

console.log("-- treeHTML(): armed Into styling keys off paste_slot, per row --");
{
  const rowB = makeRow({ path: [{ Key: "b" }], key: "b" });
  const rowC = makeRow({ path: [{ Key: "c" }], key: "c" });
  const htmlPlain = treeHTML(makeSnap([rowB, rowC], undefined));
  check("no armed slot: neither row gets drop-into", !htmlPlain.includes("drop-into"));

  const htmlInto = treeHTML(makeSnap([rowB, rowC], { Into: [{ Key: "b" }] }));
  const bDiv = htmlInto.split("<div")[1]; // rowB is the first rendered row div
  check("Into-armed row gets the drop-into class", bDiv.includes("drop-into"));
  const cDiv = htmlInto.split("<div")[2];
  check("non-armed sibling row does not get drop-into", !cDiv.includes("drop-into"));
}

console.log("\n-- treeHTML(): After slot does not bake a row class (line is drawn by app.ts) --");
{
  const rowB = makeRow({ path: [{ Key: "b" }], key: "b" });
  const htmlAfter = treeHTML(makeSnap([rowB], { After: [{ Key: "b" }] }));
  check("After-armed row is not given drop-into (that's app.ts's reorder-line job)", !htmlAfter.includes("drop-into"));
}

console.log("\n-- treeHTML(): still emits the trailing .reorder-line the After cue reuses --");
{
  const html = treeHTML(makeSnap([], undefined));
  check('trailing reorder-line element present', html.includes('<div class="reorder-line"></div>'));
}

console.log("\n-- treeHTML(): has_descendant_violation gets warn-branch, stably regardless of expand state --");
{
  const rowB = makeRow({ path: [{ Key: "b" }], key: "b", has_descendant_violation: true });
  const html = treeHTML(makeSnap([rowB], undefined));
  const bDiv = html.split("<div")[1];
  check("collapsed branch with descendant warning gets warn-branch class", bDiv.includes("warn-branch"));

  const rowBExpanded = makeRow({ path: [{ Key: "b" }], key: "b", has_descendant_violation: true });
  const rowChild = makeRow({ path: [{ Key: "b" }, { Key: "c" }], key: "c", depth: 2, is_branch: false });
  const htmlExpanded = treeHTML(makeSnap([rowBExpanded, rowChild], undefined));
  const bDivExpanded = htmlExpanded.split("<div")[1];
  check("expanded branch with descendant warning still gets warn-branch class (stable cue)", bDivExpanded.includes("warn-branch"));
}

console.log(failures === 0 ? "\nALL TOUCH RENDER-CUE CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
