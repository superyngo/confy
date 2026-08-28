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

// ---- renderRow: Into-armed row gets the drag-over-into class (ADR 0004 §1) ----
console.log("\n-- renderRow(): paste-armed Into styling --");
{
  const row = makeRow({ key: "b", is_branch: true });
  const htmlPlain = renderRow(row, 0, [row], null, null, "");
  check("plain row has no drag-over-into class", !htmlPlain.includes("drag-over-into"));
  const htmlInto = renderRow(row, 0, [row], null, null, "", true);
  check("Into-armed row gets drag-over-into class", htmlInto.includes("drag-over-into"));
}

// ---- renderRow: has_descendant_violation gets warn-branch class, stably regardless of expand state ----
console.log("\n-- renderRow(): branch schema-warning marker --");
{
  const row = makeRow({ key: "server", is_branch: true, has_descendant_violation: true });
  const htmlCollapsed = renderRow(row, 0, [row], null, null, "");
  check("collapsed branch with descendant warning gets warn-branch class", htmlCollapsed.includes("warn-branch"));
  const expandedRows = [row, makeRow({ key: "port", depth: 2 })];
  const htmlExpanded = renderRow(row, 0, expandedRows, null, null, "");
  check("expanded branch with descendant warning still gets warn-branch class (stable cue)", htmlExpanded.includes("warn-branch"));
  const noWarnRow = makeRow({ key: "server", is_branch: true, has_descendant_violation: false });
  const htmlNoWarn = renderRow(noWarnRow, 0, [noWarnRow], null, null, "");
  check("branch without descendant warning gets no warn-branch class", !htmlNoWarn.includes("warn-branch"));
}

// ---- renderRow: quoted YAML key shows quote marks, matching TOML (item 2) ----
console.log("\n-- renderRow(): quoted-key display (YAML parity with TOML) --");
{
  const row = makeRow({ key: "a b", key_sign: "quoted" });
  const htmlYaml = renderRow(row, 0, [row], null, null, "", false, "Yaml");
  check('YAML quoted key renders as "a b"', htmlYaml.includes(">&quot;a b&quot;<"), htmlYaml);
  const htmlYamlBare = renderRow(makeRow({ key: "a", key_sign: "bare" }), 0, [row], null, null, "", false, "Yaml");
  check('YAML bare key has no added quotes', htmlYamlBare.includes('data-edit="key">a</span>'), htmlYamlBare);
  // TOML already carries its quotes inside `key` itself; must not be double-wrapped.
  const tomlRow = makeRow({ key: '"a b"', key_sign: "quoted" });
  const htmlToml = renderRow(tomlRow, 0, [tomlRow], null, null, "", false, "Toml");
  check('TOML quoted key is not double-quoted', !htmlToml.includes('""a b""'), htmlToml);
}

// ---- renderRow: quoted YAML key rename buffer carries literal quotes ----
console.log("\n-- renderRow(): quoted-key rename input carries literal quotes --");
{
  // The rename buffer itself now carries the literal source text (quotes
  // included), seeded from core's `key_literal_text` — mirroring TOML's
  // rename buffer, which already carries its literal quotes. No separate
  // decoration span is drawn.
  const row = makeRow({ key: "a b", key_sign: "quoted", is_cursor: true });
  const edit = { field: "Name", buffer: '"a b"', cursor: 5, is_element: false, is_comment: false };
  const html = renderRow(row, 0, [row], edit, null, "", false, "Yaml");
  check("YAML quoted-key rename input value carries literal quotes", html.includes('value="&quot;a b&quot;"'), html);
  check("no separate quote-decoration span is drawn", !html.includes("key-quote"), html);

  // A bare YAML key's rename input is unaffected.
  const bareRow = makeRow({ key: "a", key_sign: "bare", is_cursor: true });
  const bareEdit = { field: "Name", buffer: "a", cursor: 1, is_element: false, is_comment: false };
  const bareHtml = renderRow(bareRow, 0, [bareRow], bareEdit, null, "", false, "Yaml");
  check("YAML bare-key rename input has no quote decoration", !bareHtml.includes("key-quote"), bareHtml);

  // TOML's key already carries its quotes inside `row.key`/`edit.buffer` itself
  // (unrelated to `key_sign` display wrapping) — no extra decoration is added.
  const tomlEditRow = makeRow({ key: '"a b"', key_sign: "quoted", is_cursor: true });
  const tomlEdit = { field: "Name", buffer: '"a b"', cursor: 5, is_element: false, is_comment: false };
  const tomlHtml = renderRow(tomlEditRow, 0, [tomlEditRow], tomlEdit, null, "", false, "Toml");
  check("TOML quoted-key rename input has no extra quote decoration", !tomlHtml.includes("key-quote"), tomlHtml);
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

// ---- panelHTML: Path field uses server-computed path_display ----
console.log("\n-- panelHTML(): Path field shows quoted YAML key segments --");
{
  const quotedRow = makeRow({ path: [{ Key: "a b" }], path_display: '"a b"', key_sign: "quoted" });
  const html = panelHTML(quotedRow, false, "None");
  check("Path field shows literal quotes from path_display", html.includes(">&quot;a b&quot;<"), html);

  // Fallback: no path_display supplied → plain client-side join (unaffected
  // synthetic fixtures / non-YAML rows keep working unchanged).
  const plainRow = makeRow({ path: [{ Key: "a" }, { Key: "b" }] });
  const plainHtml = panelHTML(plainRow, false, "None");
  check("Path field falls back to plain dotted path without path_display", plainHtml.includes(">a.b<"), plainHtml);
}

console.log(failures === 0 ? "\nALL RENDER-ESCAPING CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
