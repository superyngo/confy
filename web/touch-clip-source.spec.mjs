// Plain-Node test for touch/render.ts's clip-source styling (ADR 0005 §2): while the
// clipboard holds a copy or cut, the source row(s) must get the same `clip-copy`/
// `clip-cut` class desktop's web/render.ts already emits, keyed off
// SessionSnapshot.clipboard_paths/clipboard_cut. Follows touch-render.spec.mjs's
// convention: no test framework, just node:assert-style check() + esbuild bundling.
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

function makeSnap(rows, overrides = {}) {
  return { rows, paste_slot: undefined, clipboard_paths: [], clipboard_cut: false, ...overrides };
}

console.log("-- treeHTML(): clip-copy/clip-cut key off clipboard_paths + clipboard_cut, per row --");
{
  const rowB = makeRow({ path: [{ Key: "b" }], key: "b" });
  const rowC = makeRow({ path: [{ Key: "c" }], key: "c" });
  const htmlPlain = treeHTML(makeSnap([rowB, rowC]));
  check("no clipboard: neither row gets a clip class", !htmlPlain.includes("clip-copy") && !htmlPlain.includes("clip-cut"));

  const htmlCopy = treeHTML(makeSnap([rowB, rowC], { clipboard_paths: [[{ Key: "b" }]], clipboard_cut: false }));
  const bDivCopy = htmlCopy.split("<div")[1];
  check("copy source row gets clip-copy", bDivCopy.includes("clip-copy"));
  const cDivCopy = htmlCopy.split("<div")[2];
  check("non-source sibling does not get clip-copy", !cDivCopy.includes("clip-copy"));

  const htmlCut = treeHTML(makeSnap([rowB, rowC], { clipboard_paths: [[{ Key: "b" }]], clipboard_cut: true }));
  const bDivCut = htmlCut.split("<div")[1];
  check("cut source row gets clip-cut, not clip-copy", bDivCut.includes("clip-cut") && !bDivCut.includes("clip-copy"));
}

console.log(failures === 0 ? "\nALL TOUCH CLIP-SOURCE CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
