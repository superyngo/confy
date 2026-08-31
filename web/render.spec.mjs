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
    badge_label: "str",
    badge_note: "",
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

// ---- renderRow: Into-armed row gets the paste-target class (ADR 0004 §1) ----
console.log("\n-- renderRow(): paste-armed Into styling --");
{
  const row = makeRow({ key: "b", is_branch: true });
  const htmlPlain = renderRow(row, 0, [row], null, null, "");
  check("plain row has no paste-target class", !htmlPlain.includes("paste-target"));
  const htmlInto = renderRow(row, 0, [row], null, null, "", true);
  check("Into-armed row gets paste-target class", htmlInto.includes("paste-target"));
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

// ---- renderRow: the key label is the authored spelling (key_literal) ----
console.log("\n-- renderRow(): key label uses the authored spelling --");
{
  const row = makeRow({ key: "a b", key_literal: '"a b"' });
  const htmlYaml = renderRow(row, 0, [row], null, null, "", false, "Yaml");
  check('double-quoted key renders as "a b"', htmlYaml.includes(">&quot;a b&quot;<"), htmlYaml);
  // A single-quoted key must keep ITS OWN quote style — the old code
  // synthesized `"` for every quoted key regardless of source.
  const sq = makeRow({ key: "a b", key_literal: "'a b'" });
  const htmlSq = renderRow(sq, 0, [sq], null, null, "", false, "Yaml");
  check("single-quoted key keeps single quotes", htmlSq.includes(">'a b'<"), htmlSq);
  check(
    "single-quoted key gains no double quotes",
    !htmlSq.includes(">&quot;a b&quot;<") && !htmlSq.includes('>"a b"<'),
    htmlSq,
  );
  const htmlBare = renderRow(makeRow({ key: "a", key_literal: "a" }), 0, [row], null, null, "", false, "Yaml");
  check("bare key has no added quotes", htmlBare.includes('data-edit="key">a</span>'), htmlBare);
  // A keyless/undefined literal falls back to the decoded key.
  const noLit = makeRow({ key: "plain" });
  const htmlNoLit = renderRow(noLit, 0, [noLit], null, null, "", false, "Json");
  check("missing key_literal falls back to key", htmlNoLit.includes('data-edit="key">plain</span>'), htmlNoLit);
  // TOML carries exactly one set of quotes; must not be double-wrapped.
  const tomlRow = makeRow({ key: "a b", key_literal: '"a b"' });
  const htmlToml = renderRow(tomlRow, 0, [tomlRow], null, null, "", false, "Toml");
  check("TOML quoted key is not double-quoted", !htmlToml.includes('""a b""'), htmlToml);
}

// ---- renderRow: quoted YAML key rename buffer carries literal quotes ----
console.log("\n-- renderRow(): quoted-key rename input carries literal quotes --");
{
  // The rename buffer itself carries the authored spelling (quotes included),
  // seeded from core's `ViewRow.key_literal`. No separate decoration span.
  const row = makeRow({ key: "a b", key_literal: '"a b"', is_cursor: true });
  const edit = { field: "Name", buffer: '"a b"', cursor: 5, is_element: false, is_comment: false };
  const html = renderRow(row, 0, [row], edit, null, "", false, "Yaml");
  check("quoted-key rename input value carries literal quotes", html.includes('value="&quot;a b&quot;"'), html);
  check("no separate quote-decoration span is drawn", !html.includes("key-quote"), html);

  // A single-quoted key's buffer carries ITS quote style.
  const sqRow = makeRow({ key: "a b", key_literal: "'a b'", is_cursor: true });
  const sqEdit = { field: "Name", buffer: "'a b'", cursor: 5, is_element: false, is_comment: false };
  const sqHtml = renderRow(sqRow, 0, [sqRow], sqEdit, null, "", false, "Yaml");
  check("single-quoted rename input keeps single quotes", sqHtml.includes("value=\"'a b'\""), sqHtml);

  // A bare key's rename input is unaffected.
  const bareRow = makeRow({ key: "a", key_literal: "a", is_cursor: true });
  const bareEdit = { field: "Name", buffer: "a", cursor: 1, is_element: false, is_comment: false };
  const bareHtml = renderRow(bareRow, 0, [bareRow], bareEdit, null, "", false, "Yaml");
  check("bare-key rename input has no quote decoration", !bareHtml.includes("key-quote"), bareHtml);
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

// ---- panelHTML: the editable Key field carries the authored spelling ----
console.log("\n-- panelHTML(): Key field is the authored spelling, not the decoded key --");
{
  // The panel's Key input is editable and committed verbatim, so seeding it
  // with the decoded key would silently restyle a quoted key to bare on an
  // otherwise untouched commit — and the quotes would vanish on reopen.
  const dq = makeRow({ key: "a b", key_literal: '"a b"' });
  const dqHtml = panelHTML(dq, false, "None");
  check(
    "double-quoted key field keeps its quotes",
    dqHtml.includes('data-field="name" value="&quot;a b&quot;"'),
    dqHtml,
  );

  const sq = makeRow({ key: "a b", key_literal: "'a b'" });
  const sqHtml = panelHTML(sq, false, "None");
  check(
    "single-quoted key field keeps ITS quote style",
    sqHtml.includes(`data-field="name" value="'a b'"`) && !sqHtml.includes("&quot;a b&quot;"),
    sqHtml,
  );

  // A row with no literal (JSON, keyless) falls back to the decoded key.
  const noLit = makeRow({ key: "plain" });
  const noLitHtml = panelHTML(noLit, false, "None");
  check(
    "missing key_literal falls back to the decoded key",
    noLitHtml.includes('data-field="name" value="plain"'),
    noLitHtml,
  );
}

console.log(failures === 0 ? "\nALL RENDER-ESCAPING CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
