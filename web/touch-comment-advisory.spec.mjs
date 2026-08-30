// Plain-Node test for touch/render.ts's comment-advisory rendering (touch UI
// parity: a `strict_json` document's comment gets the same wavy-underline
// signal as desktop) and the swipe-to-remark button. Follows
// touch-render.spec.mjs's convention: no test framework, just
// `node:assert` + a `check()` tally; touch/render.ts has no DOM/wasm
// top-level side effects, so it bundles and imports directly (no jsdom, no
// new npm dependency).
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
    is_branch: false,
    key: "a",
    value: "1",
    scalar_type: "int",
    format: "Table",
    type_label: "int",
    badge_label: "int",
    badge_note: "",
    child_count: 0,
    trailing_comment: undefined,
    comment_advisory: undefined,
    read_only: false,
    violations: undefined,
    selected: false,
    is_cursor: false,
    ...overrides,
  };
}

function makeSnap(rows) {
  return { rows, paste_slot: undefined, clipboard_paths: [], clipboard_cut: false, doc_format: "Json" };
}

console.log("-- treeHTML(): comment-advisory wavy-underline class --");
{
  const row = makeRow({ trailing_comment: "# note", comment_advisory: "strict JSON: comment ignored on save" });
  const html = treeHTML(makeSnap([row]));
  check(
    "leaf row with comment_advisory + trailing_comment gets class=\"comment comment-advisory\"",
    html.includes('class="comment comment-advisory"'),
    html,
  );
}
{
  const row = makeRow({ trailing_comment: "# note", comment_advisory: undefined });
  const html = treeHTML(makeSnap([row]));
  check("leaf row with no comment_advisory does not get the comment-advisory class", !html.includes("comment-advisory"), html);
}
{
  const row = makeRow({ trailing_comment: undefined, comment_advisory: "strict JSON: comment ignored on save" });
  const html = treeHTML(makeSnap([row]));
  check(
    "leaf row with comment_advisory set but no trailing_comment does not get the comment-advisory class",
    !html.includes("comment-advisory"),
    html,
  );
}
{
  const row = makeRow({
    is_branch: true,
    child_count: 2,
    type_label: "table",
    badge_label: "table",
    trailing_comment: "# note",
    comment_advisory: "strict JSON: comment ignored on save",
  });
  const html = treeHTML(makeSnap([row]));
  check(
    "branch row with comment_advisory + trailing_comment gets class=\"comment comment-advisory\"",
    html.includes('class="comment comment-advisory"'),
    html,
  );
}
{
  const row = makeRow({
    type_label: "comment",
    value: "# a standalone comment",
    comment_advisory: "strict JSON: comment ignored on save",
  });
  const html = treeHTML(makeSnap([row]));
  check(
    "standalone comment row with comment_advisory gets class=\"comment comment-advisory\"",
    html.includes('class="comment comment-advisory"'),
    html,
  );
}

console.log("\n-- treeHTML(): swipe-to-remark button --");
{
  const row = makeRow({ read_only: false });
  const html = treeHTML(makeSnap([row]));
  check('non-read-only row emits data-act="rowremark"', html.includes('data-act="rowremark"'), html);
}
{
  const row = makeRow({ read_only: true });
  const html = treeHTML(makeSnap([row]));
  check('read-only row omits data-act="rowremark"', !html.includes('data-act="rowremark"'), html);
}

console.log(failures === 0 ? "\nALL TOUCH COMMENT-ADVISORY CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
