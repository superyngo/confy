# JSON Schema Support — Design

Date: 2026-08-10
Status: ✅ Shipped (historical reference — see CHANGELOG.md)

## Summary

Add JSON Schema detection, validation, and constraint-aware editing to confy, uniformly
across TOML/JSON/YAML and across all four surfaces (TUI, desktop web, touch/mobile web,
Tauri desktop/mobile). Validation is always **soft** — it never blocks edit or save. The
free-form popup/`$EDITOR` block editor is never constrained by schema, only by the user.

Vocabulary: a **Violation** and the **Soft constraint** principle behind it are now
canonical CONTEXT.md terms (§ Schema), explicitly contrasted with confy's existing
Mutation-mechanics error vocabulary (`Illegal`/`Unsupported`/`Collision`) — a Violation
never blocks a Mutation and never appears in a `MutateError`.

Decisions locked in this session:
- Validation engine: the [`jsonschema`](https://crates.io/crates/jsonschema) Rust crate
  (full draft 2020-12 support incl. `$ref`/`allOf`/`oneOf`/`anyOf`/`if-then-else`/`format`),
  confirmed `wasm32-unknown-unknown`-compatible with the optional `reqwest`
  remote-`$ref`-fetch feature disabled (confy stays fs-free; hosts do all I/O already).
- Detection: **in-file hints only**, no filename-based sibling-file guessing.
- Association lifetime: **session-only** — re-detected on every file open, nothing
  persisted to `config.toml`.
- Recorded as `docs/adr/0002-jsonschema-crate-for-validation.md` — hard to reverse,
  surprising against confy's hand-rolled-parser house style, and a real trade-off; see
  the ADR for the full reasoning.

## Non-goals

- Hard/blocking validation mode (schema violations never prevent editing or saving).
- Cross-session persisted schema associations (à la VS Code's `json.schemas` setting).
- Filename-based sibling-schema guessing (`foo.toml` → probing `foo.schema.json`).
- Automatic sibling/relative-path schema resolution on Android (SAF has no directory
  enumeration from a single persistable document grant — see "Android" below).
- Schema *authoring* — confy consumes schemas, never generates or edits `.schema.json`
  files itself.
- Per-format input masks for `pattern`/`format`/length constraints (soft-warn only, no
  live keystroke rejection).

## Architecture

### Where it lives

New `crates/confy-core/src/schema/` module. **Not** a new crate: `confy-core/Cargo.toml`
already depends on `serde` + `serde_json`, which `jsonschema` builds directly on, so this
is one new leaf dependency, not a new dependency stack.

confy-core stays fully headless (per its existing "no FS, no terminal" contract — see
`crates/confy-core/Cargo.toml` description and the fact that `AnyDocument::from_str_as`
is confy-core's only document constructor today). All schema **loading** (reading a local
file, fetching a URL) is host-owned, exactly like file I/O already is.

Files:
- `schema/hints.rs` — pure, no I/O. Per-`DocFormat` sniffers that scan already-loaded
  document text/AST for a schema reference:
  - JSON/JSONC: a root-level `"$schema"` string member (the JSON Schema spec's own
    self-description convention).
  - YAML: a leading `# yaml-language-server: $schema=<path-or-url>` modeline comment
    (the `redhat.vscode-yaml` / `yaml-language-server` convention, broadly recognized).
  - TOML: a first-line `#:schema <path-or-url>` comment (the Taplo / Even Better TOML
    convention).
  Returns `Option<SchemaSource>` where `SchemaSource = Local(String) | Url(String)`
  (string, not a resolved path — resolution is host-side).
- `schema/value_bridge.rs` — converts confy's format-neutral `Value` tree
  (`model/value.rs`, already used by the format-conversion pipeline) into a **JSON
  projection** (a `serde_json::Value` tree, deliberately *not* called "Value" — that name
  is already taken by `model/value.rs`'s own tree; see `CONTEXT.md` § Schema), plus a
  `Path` (`model/node.rs::Seg`/`Path`) ⇄ JSON-Pointer mapping in both directions. This is
  the single place format-specific type bridging happens:
  - `ScalarType::{Integer,Float,Bool,Str}` → JSON `integer`/`number`/`boolean`/`string`
    directly.
  - TOML `OffsetDatetime`/`LocalDatetime`/`LocalDate`/`LocalTime` → JSON `string`,
    RFC3339-rendered, so `format: date-time`/`date`/`time` keywords validate correctly.
  - TOML has no `Null` scalar — never produced by the TOML backend's `Value` lowering, so
    a schema requiring `type: null` against a TOML-sourced node is structurally
    unsatisfiable; `validate.rs` emits a distinct `keyword: "type"`,
    `category: Representation` violation with an explanatory message rather than a
    generic type-mismatch message.
  - YAML has no datetime type by confy's own design (CONTEXT.md: "a date- or time-looking
    plain scalar is a string") — no special-casing needed, `format` keywords apply to the
    string value as-is.
- `schema/validate.rs` — `fn validate(root: &serde_json::Value, compiled: &Validator) ->
  Vec<Violation>` where `Violation { path: Path, pointer: String, keyword: String,
  message: String, category: Category }` (`Category::{Value, Representation}`). Built on
  `jsonschema::Validator::iter_errors()`. Runs against the whole document's **JSON
  projection** in one pass; full spec semantics (composition, `$ref` to the schema's own
  `$defs`) apply uniformly across TOML/JSON/YAML since it's operating on the projection,
  not source syntax. A `required`-keyword failure has no Path of its own (the missing
  child doesn't exist) — `jsonschema` reports the JSON Pointer of the **parent** object,
  so the Violation's `path` is the parent's Path and the message names the missing
  key(s) (e.g. "missing required field 'port'"). It surfaces as an ordinary parent-row
  Soft constraint warning, same mechanism as every other Violation — no distinct
  affordance for the absent child this pass. (A "quick-add missing field" action from
  that warning is a plausible fast-follow, deliberately out of scope here — it's a new
  interactive surface, not a visual indicator.)
- `schema/hints_edit.rs` — **separate and intentionally simpler** than `validate.rs`: a
  best-effort walk of the *raw* (uncompiled) schema JSON to resolve the applicable
  sub-schema at one target `Path` (only called when a node enters inline edit, not
  eagerly for the whole tree). Resolves through `properties`/`items`/local `$defs` +
  same-document `$ref`, and reads `enum`/`const`/`type`/`minimum`/`maximum`/
  `exclusiveMinimum`/`exclusiveMaximum`/`multipleOf` when present. Returns
  `EditHint::{Enum(Vec<Value>), Const(Value), Bounded{min,max,multiple_of}, None}`.
  Deliberately does **not** attempt `allOf`/`oneOf`/`anyOf`/`not`/`if-then-else` or
  remote `$ref` resolution — those fall through to `EditHint::None` (plain text input),
  while `validate.rs` still fully validates against them regardless. This split keeps the
  editing-widget code simple without limiting validation coverage. **Carve-out:** a
  `oneOf`/`anyOf` where every branch is a bare `{const, title?, description?}` (nothing
  else) resolves to `EditHint::Enum`, using each branch's `title` as the picker label
  when present (else the const value itself) — this is the single most common
  real-world idiom for an enum with per-value descriptions (SchemaStore, code-generated
  schemas), so it earns a narrow special case; any branch carrying anything beyond
  `const`/`title`/`description` still declines to `EditHint::None`.

### Session / snapshot wiring

`crates/confy-core/src/session/session.rs::Session` gains:
```rust
pub schema: Option<SchemaState>,
```
```rust
pub struct SchemaState {
    pub source: SchemaSource,
    pub compiled: Option<jsonschema::Validator>, // None while load_error is set
    pub violations: Vec<Violation>,
    pub load_error: Option<String>,
}
```
Re-`validate()` runs after every successful mutation commit, at the same point
`rebuild_rows()` already re-projects the tree — no new invalidation mechanism.
This is deliberately **unconditional and synchronous** — no size/complexity guard: the
compiled `Validator` is built once per schema load, so a re-`iter_errors()` pass is a
single tree walk, matching confy's existing fully-synchronous mutation pipeline. Revisit
only if profiling proves it a problem.

`crates/confy-core/src/session/view.rs::SessionSnapshot` gains:
```rust
pub schema_status: Option<SchemaStatus>, // { source_label, violation_count, load_error }
```
(additive; does not replace or overload the existing `error: Option<String>` field, which
stays reserved for hard mutation failures).

`ViewRow` (same file) gains:
```rust
pub schema_warn: Option<Vec<String>>, // violation messages whose `path` == this row; None = clean
```
This is the row-level flag every host's renderer threads into its existing "extra row
class/style" mechanism (see "Visual indicator" below) — same shape as how `clip-copy`/
`clip-cut` are threaded today web-side.

`EditState` (session/state.rs) gains:
```rust
pub constraint: Option<EditHint>,
```
populated by `begin_inline_edit()` via `hints_edit::resolve(path)` when `Session.schema`
is `Some`.

### Host↔core async handshake for loading

confy-core cannot read files or fetch URLs. Schema *text* resolution reuses the existing
async-signal pattern already established for `external_edit` (§8.2 in PORTING.md) and
`convert_write`: `SessionSnapshot` gains
```rust
pub schema_fetch_request: Option<SchemaSource>,
```
set when a hint/override is detected but not yet resolved. The host reads the local file
or fetches the URL, then dispatches a new intent:
```rust
Intent::SchemaLoaded { source: SchemaSource, text: Result<String, String> }
```
mirroring `Intent::ApplyReplace`/`ApplyEditComment`'s "host resolves an async request,
hands the result back" shape. On `Err`, core sets `load_error` and leaves `schema: None`
for validation purposes (soft, non-blocking, per the "never hard-fail" convention already
used for config/format detection in `confy-tui/src/cli.rs`).

## 1. Detection + fallback

Resolution order, evaluated once per file open (session-only — never persisted):
1. Explicit override — **in MVP on every surface**, not TUI-only (the locked detection
   decision was "in-file hints + explicit specification", both in scope). TUI: a new
   `--schema <path-or-url>` CLI flag in `confy-tui/src/cli.rs::Args`, threaded the same
   way `--format`/`--lang` already are. Web/touch/Tauri: an "Attach schema…" action next
   to Open, reusing the same file-pick/URL-fetch primitives already wired for opening the
   main config file (`web/fs.ts::pickOpenFile`/`fetchUrlFile`) — no new capability, just a
   second invocation of the existing pick/fetch flow.
2. In-file hint via `schema/hints.rs` (see above), format-specific.
3. Neither → `Session.schema = None`. Editor behaves exactly as it does today; zero
   observable change.

A **local** hint/override's relative path resolves against **the directory of the open
config file** — the only base that's meaningful on every surface (web/Tauri have no
process cwd for a browser- or SAF-picked file) and matches the ecosystem convention
Taplo/yaml-language-server/the JSON Schema spec itself already use for `$schema`-as-a
relative reference.

Load failure at any stage (missing file, network error, the schema document itself
fails to parse as JSON, or fails to compile as a schema) degrades to `schema: None` for
validation/hint purposes but keeps `SchemaStatus.load_error` populated for a single
status-line message. Never blocks opening, editing, or saving the file.

## 2. Value constraints

Full JSON Schema keyword coverage, "for free" from the `jsonschema` crate, applied to the
**JSON projection** lowered from the format-neutral `Value` tree — the same `Value` tree
the format-conversion pipeline already produces via `ConfigDocument::to_value()`.
Cross-format bridging caveats are documented above in `value_bridge.rs`'s description;
all are soft/informational (`Category::Representation`), never a hard rejection.

## 3. Constraint-driven inline edit

`EditHint` is resolved only for the single node entering inline edit (`BeginEdit`), not
eagerly for the whole tree (full-tree scanning is `validate.rs`'s job, for point 4).

| Hint | TUI | Web desktop | Touch |
|---|---|---|---|
| `Enum`/`Const` | Reuse `draw_kind_switch_overlay` (`crates/confy-tui/src/tui/ui.rs` ~line 846) verbatim — same single-select popup as the `K` kind-switch, driven by `EditHint::Enum` values instead of kind options; `Mode::KindSwitch`-shaped state, new `Mode` variant reusing the same rendering path. | `render.ts::renderValue()`'s edit branch (currently emits `<input>`) emits `<select data-editing="value">` when `EditState.constraint` is `Enum`/`Const`; `focusInlineEdit()` (`web/ui.ts` ~1242) gets a matching `<select>`-vs-`<input>` read branch before `CommitEdit`. | Reuse the bottom-sheet list pattern already used by `openKindSheet()` (`web/touch/app.ts` ~478) — one `.kind-opt`-style button per enum value, tap dispatches `CommitEdit`. |
| `Bounded{min,max,multiple_of}` | The existing `←`/`→` numeric nudge (`nudge_scalar` in `session.rs`) clamps to `[min,max]` and snaps to `multiple_of`. Free-text inline typing stays unclamped (soft only — an out-of-range typed value is flagged by point 4, never rejected at commit). | Same clamp applied where the desktop UI drives the nudge; text input unclamped. | Same. |
| `None` (patterns, formats, lengths, unresolved composition/`$ref`) | No widget change — plain inline text input, exactly as today. | Same. | Same. |

The popup/free-form editors — `$EDITOR` (`crates/confy-tui/src/tui/editor.rs::edit_text`),
the web `#ext-modal` textarea (`openExternalEdit()`, `web/ui.ts` ~957), and touch's
equivalent — are **never** constrained by `EditHint`. They stay raw free-text edit exactly
as today; the committed text is simply re-validated afterward by `validate.rs` like any
other mutation, never blocked at commit.

## 4. Soft visual indicator

Row-level, driven by `ViewRow.schema_warn`, reusing each surface's existing "extra row
state" mechanism rather than inventing new ones:

- **Web** (`web/render.ts`, `web/style.css`): new `.row.schema-warn` class, threaded
  through `renderTree()`'s existing Set-membership mechanism (the same one that drives
  `.clip-copy`/`.clip-cut`, `web/style.css` lines ~555-558). Style: a subdued dashed
  outline in a new `--warn` CSS variable (amber-adjacent, distinct from the existing
  clip-copy blue and clip-cut red-ish tones) plus a small corner dot styled after the
  existing `.dirty-dot` (`web/style.css` line ~93) — deliberately not a full alarming
  red row background.
- **TUI** (`crates/confy-tui/src/tui/ui.rs`): `draw_tree`'s row-style `match` (~line 260)
  gains a new soft-warning arm — a subdued Yellow/DarkYellow left-edge accent parallel to
  the existing cursor `▎` bar (not reversed-video, not alarming), and `type_tag` (~line
  712) gains a trailing `!` glyph when the fixed 8-column budget allows, parallel to the
  existing `[opaq ]` read-only-tag precedent.
- **Touch**: same `.row.schema-warn` CSS class applied in `web/touch/render.ts` (touch and
  desktop already share `web/style.css`).
- **Document-level summary**: `SchemaStatus.violation_count` surfaces through each
  surface's existing single status/error line (`setStatus()` web-side, the TUI status
  line) as "N schema warnings"; activating it jumps to the first violating row via the
  existing `Reveal` concept (CONTEXT.md) rather than a new navigation primitive.
- **Violation messages**: shown in the existing per-node Detail popup (`i` key /
  detail panel) as an additional section — no new popup surface.

## 5 & 6. Cross-platform / cross-format parity

Structural, not incidental to this design:
- Validation runs once, against the JSON projection lowered from the format-neutral
  `Value` tree — one code path serves TOML/JSON/YAML.
- Each surface's constrained-input widget for `Enum`/`Const` is a **reuse** of that
  surface's pre-existing single-select mechanism (TUI kind-switch popup, touch bottom
  sheet) or a natural DOM-native addition (`<select>`) — this is wiring, not new UI
  framework work.

## Convert interaction

`Convert` (`C`) produces a new in-memory document in a different `DocFormat` within the
same session. The currently-loaded `SchemaState` (compiled `Validator` + source) carries
forward and re-validates against the converted document's freshly-lowered JSON
projection — the schema is format-neutral and the bridge already normalizes
representation differences, so this is just another `validate()` pass, no special case.
confy does **not** attempt to auto-write an equivalent in-file hint into the converted
document's syntax (e.g. synthesizing a TOML `#:schema` comment from a JSON `$schema`
key) — a fresh open of the converted file re-detects from scratch, consistent with
session-only association.

## Tauri / Android file access

Research findings (`crates/confy-tauri/capabilities/default.json`,
`crates/tauri-plugin-confy-picker/src/mobile.rs`, `web/fs.ts`):

- **Desktop**: `fs:scope` is `{"path": "**"}` (unrestricted) — reading a sibling/
  relative-path schema file or an explicit override path works unmodified, no
  capability changes needed.
- **Android**: `tauri-plugin-confy-picker`'s only commands (`pick_writable`,
  `create_writable`) grant a *persistable* SAF URI to exactly the one document the user
  picked — there is no directory-tree or second-document read capability. Per
  `docs/adr/0001-android-save-as-persistable-grant.md`, `pick_writable` exists to fix a
  **persistable-grant durability gap** (the grant must survive app-kill + relaunch so a
  later in-place save still works) — not, as an earlier draft of this doc claimed,
  because Android categorically withholds write access. Given the session's decision to
  skip filename-based sibling guessing entirely, this narrows to: a
  **local/relative-path** hint or override on Android degrades to a soft
  `SchemaStatus.load_error` (schema unavailable, editing unaffected).
- **URL-based hints work identically to desktop on Android today, with zero new
  capability**: `web/fs.ts::fetchUrlFile()` already does a plain `fetch(url)` (used by
  the existing "Open from URL…" feature); `tauri.conf.json`'s CSP is `null` and
  `gen/android/app/src/main/AndroidManifest.xml` already declares
  `android.permission.INTERNET` unconditionally. No `tauri-plugin-http` or new capability
  entry is required.
- **Out of scope for this design**: a future "manually attach a local schema file"
  action on Android. Noted as a low-risk fast-follow — it would only need a *one-shot*
  read, which has no relaunch-survival requirement and so never needed the
  persistable-grant machinery `pick_writable` exists for (see ADR 0001 above); the
  existing `tauri-plugin-dialog` `dialog.open()` + `fs.readTextFile()` primitives
  (already used for opening the main config file) are expected to suffice without a new
  `confy-picker` command.

## New dependencies

- `confy-core`: `jsonschema` (with default `reqwest` remote-fetch feature disabled).
- `confy-tui`: one new blocking HTTP client (e.g. `ureq`) for resolving URL-based schema
  hints in the terminal — confy-tui currently does zero networking, this is a genuinely
  new capability for that crate. All other hosts (web, Tauri) already have a fetch path
  (`fetchUrlFile`) and reuse it as-is.

## Rollout phasing

1. **confy-core**: `schema/` module (hints, value bridge, validate, hints_edit), `Session`/
   `SchemaState` wiring, new `Intent`/`SessionSnapshot`/`ViewRow`/`EditState` fields, the
   `schema_fetch_request` ↔ `SchemaLoaded` handshake. Fully testable headless (golden
   tests), no host involvement.
2. **confy-tui**: `resolve_schema_source` (local read + `ureq` URL fetch), `--schema` CLI
   flag, KIND-column/row soft-warning styling, `EditHint::Enum` popup reuse, Detail-popup
   violation messages.
3. **Web desktop** (`web/`): `renderValue()`/`focusInlineEdit()` `<select>` branch,
   `.schema-warn` CSS + `--warn` variable, `fs.ts` local-sibling-read + URL-fetch wiring
   for schema hints, status-line summary.
4. **Touch** (`web/touch/`): bottom-sheet enum picker reuse, same CSS class, same `fs.ts`
   wiring (shared with desktop web).
5. **Tauri**: verify desktop `fs:scope` covers schema reads (already does — expected zero
   code change beyond what step 3 wires through the shared `fs.ts`); document the Android
   relative-path limitation in `TAURI.md`.
