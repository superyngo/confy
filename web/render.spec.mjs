// Plain-Node test pinning the HTML-escaping discipline in `render.ts`/`panel.ts`
// (verified manually in the 2026-08-11 code-auditor audit — see
// docs/superpowers/plans/2026-08-11-web-code-audit-remediation-plan.md, Task 6).
// Follows `toolbar-fold.spec.mjs`'s convention: no test framework, just
// `node:assert` + a `check()` tally. Unlike `toolbar-fold.ts` (zero imports),
// `render.ts`/`panel.ts` pull in `escape.ts`/`kind-labels.ts`/`i18n.ts`, so this
// file bundles (esbuild's `build({ bundle: true, write: false })`, already a
// devDependency; no jsdom, no new npm dependency) instead of transforming a
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

const { renderRow, escapeHtml } = await bundle("render.ts");
const { panelHTML } = await bundle("panel.ts");

// Hostile payloads: each must appear in the rendered HTML only in escaped form.
const KEY_PAYLOAD = `<script>alert("key")</script>`;
const VALUE_PAYLOAD = `<b>&"'</b>`;
const COMMENT_PAYLOAD = `<img src=x onerror="alert('note')">`;

// A minimal well-formed ViewRow (types.ts:34-55); tests override specific fields.
function makeRow(overrides = {}) {
  return {
    path: [{ Key: "a" }],
    depth: 1,
    is_branch: false,
    key: "a",
    value: "v",
    scalar_type: "String",
    format: "String",
    type_label: "string",
    child_count: 0,
    trailing_comment: undefined,
    read_only: false,
    violations: undefined,
    selected: false,
    is_cursor: false,
    ...overrides,
  };
}

function assertEscaped(html, payload, label) {
  check(`${label}: escaped payload present`, html.includes(escapeHtml(payload)), escapeHtml(payload));
  check(`${label}: raw payload absent`, !html.includes(payload), payload);
}

// ---- renderRow: key / value / trailing_comment on a regular scalar row ----
console.log("-- renderRow() escaping: scalar row (key/value/trailing_comment) --");
{
  const row = makeRow({
    key: KEY_PAYLOAD,
    value: VALUE_PAYLOAD,
    trailing_comment: COMMENT_PAYLOAD,
  });
  const html = renderRow(row, 0, [row], null, null, "");
  assertEscaped(html, KEY_PAYLOAD, "key");
  assertEscaped(html, VALUE_PAYLOAD, "value");
  assertEscaped(html, COMMENT_PAYLOAD, "trailing_comment");
}

// ---- renderRow: value on a standalone comment row (isCommentRow path) ----
console.log("\n-- renderRow() escaping: comment row (value) --");
{
  const row = makeRow({
    key: "",
    value: VALUE_PAYLOAD,
    scalar_type: undefined,
    type_label: "comment",
  });
  const html = renderRow(row, 0, [row], null, null, "");
  assertEscaped(html, VALUE_PAYLOAD, "comment-row value");
}

// ---- panelHTML: value / trailing_comment ----
console.log("\n-- panelHTML() escaping (value/comment fields) --");
{
  const row = makeRow({
    key: KEY_PAYLOAD,
    value: VALUE_PAYLOAD,
    trailing_comment: COMMENT_PAYLOAD,
  });
  const html = panelHTML(row, false, "None");
  assertEscaped(html, KEY_PAYLOAD, "panel key");
  assertEscaped(html, VALUE_PAYLOAD, "panel value");
  assertEscaped(html, COMMENT_PAYLOAD, "panel trailing_comment");
}
{
  const row = makeRow({
    key: "",
    value: VALUE_PAYLOAD,
    scalar_type: undefined,
    type_label: "comment",
  });
  const html = panelHTML(row, false, "None");
  assertEscaped(html, VALUE_PAYLOAD, "panel comment-row value");
}

console.log(failures === 0 ? "\nALL RENDER-ESCAPING CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
