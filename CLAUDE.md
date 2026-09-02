# CLAUDE.md — confy developer guide

## Build & test commands

```bash
cargo build                   # compile
cargo test                    # unit + integration tests
cargo clippy -- -D warnings   # lint (must be clean before commit)
cargo fmt                     # format
cargo fmt --check             # check formatting without modifying
cargo run -- <file.toml>      # run against a TOML file
cargo bench -p confy-core     # perf harness (no criterion; plain main() + medians)
# Bigger synthetic document. `--bench perf` is required: without it the args
# reach the lib test binary first, which rejects `--nodes`.
cargo bench -p confy-core --bench perf -- --nodes 5000

# Web / touch UI (from web/) - NOT covered by `cargo test`
cd web
npm run typecheck             # tsc --noEmit
npm run build                 # esbuild bundles + wasm-pack copy
npm test                      # plain-Node spec suite (node run-tests.mjs)
# The wasm command channel end-to-end (Intent -> SessionSnapshot):
cd crates/confy-ffi && wasm-pack build --target web && node functional_smoke.mjs
```

**Two test conventions worth knowing.** (1) The **web suite is a plain-Node harness** —
no framework: each `*.spec.mjs` esbuild-bundles the TS module under test and tallies
`check(name, cond)` calls, so render modules must stay importable without the wasm glue
(hence `highlight.ts`'s `setFuzzyMatcher` injection instead of a `pkg/` import).
(2) A **CLI integration test that asserts message text must pin `--lang en`** — with no
flag the binary resolves the language from the *real* `~/.config/confy/config.toml`, so an
unpinned English assertion passes in CI and fails on a zh-TW machine. MESSAGES.md §5.5.

## Release process

**Three version files + CHANGELOG must all move together for every release** —
`.github/workflows/release.yml`'s `verify-versions` job hard-fails the tagged
build if any of them disagree with the tag:

- `Cargo.toml` (`[workspace.package].version` — covers all Rust crates:
  confy-core, confy-tui, confy-ffi, confy-tauri)
- `web/package.json` (`.version`)
- `editors/vscode/package.json` (`.version` — also regenerate
  `editors/vscode/package-lock.json`'s root version via
  `npm install --package-lock-only` in `editors/vscode/`, so `npm ci` doesn't
  warn on a stale lockfile)
- `CHANGELOG.md` must contain a `## [vX.Y.Z]` section for the tag

Bump all four in the same release commit, before tagging. Never tag with only
`Cargo.toml` updated.

**Also update the MSIX Store listing's ReleaseNotes** at
`crates/confy-tauri/msix/listings/listingData-9PLCJGQ3C654.csv` — set the `ReleaseNotes`
column to describe the new version in the same release commit.

## Architecture

**Lossless CST.** `CstDocument` (`model/cst_doc.rs`) holds a `taplo` parse → `rowan` syntax tree
as the single source of truth. Comments, whitespace and newlines are real tokens with real
positions, so `serialize()` is plain token concatenation and an untouched file round-trips
byte-identically. The Node tree is a *projection* (`cst_project.rs`) rebuilt after every
mutation — it is never mutated directly. `apply` edits a `clone_for_update` copy of the tree and
commits only on success, so **every mutation is atomic** (failure leaves the document untouched).
Every successful mutation is also **semantically validated before commit** (taplo DOM
validation — duplicate sections/keys reject as `Collision`, other semantic errors as
`Illegal`), a backstop for edits the targeted pre-checks can't see (e.g. a whole-document or block
`$EDITOR` rewrite introducing a duplicate `[a]`). Validation needs a serialize + re-parse, and
that re-parse *is* the normalization turning the mutable `clone_for_update` tree back into an
immutable one — so it is done **once**: `apply` returns `(SyntaxNode, String)` and the caller
commits both rather than recomputing either. All three backends share this shape; doing the two
jobs separately used to serialize and re-parse the whole document twice per keystroke.

**JSON/JSONC backend.** `JsonDocument` (`model/json/`) is a second concrete `ConfigDocument`
built on a hand-rolled lossless lexer + recursive-descent parser that emits a `rowan` green tree
(the same `rowan` version taplo uses, pinned `=0.15.18`). Load, serialize, and apply are all
atomic-commit; a `validate_semantics` post-check (DOM re-parse for duplicate keys) mirrors the
TOML backstop. JSONC extends `.json` with `//` line comments — which project as first-class
Comment nodes (consecutive lines merge; a blank splits them) or `trailing_comment` — and `/* */`
block comments, which project as **read-only** Comment nodes (new `Node.read_only` flag:
displayed and copyable, but edit/delete/cut/remark reject them). Comments are always legal to
author into any `.json` document -- no upgrade prompt gates it; `//` is used from the first
remark or inserted comment. Trailing commas are accepted
on parse but never emitted by splices. `K` switch covers object/array Inline↔Multiline and float
Plain↔Exponent; the `f` type-filter shows only JSON-reachable facets (`(Q)`/`(-)` key signs,
no `[A/T]`/`[T/D]`/`[T/S]`, no radix/string-style/datetime rows). JSON omits TOML-only
features: no dotted keys, array-of-tables, datetimes, integer radixes, multiline strings, or
string-notation switching; newlines are `\n`-encoded only. New model atoms added for this
backend: `ScalarType::Null` (KIND tag `[S:null]`), `Format::Exponent` (KIND tag `[F:exp ]`),
`KindTarget::TableMultiline` (KIND tag `[T/M]`), `Node.read_only`.

**YAML subset backend.** `YamlDocument` (`model/yaml/`) is a third concrete `ConfigDocument`, also
a hand-rolled lossless lexer + recursive-descent parser onto the same `rowan` green tree; load,
serialize, and apply are atomic-commit with a `validate_semantics` duplicate-key backstop. The
splice core is a **reindent engine** (`reindent` in `edit.rs`) — YAML's analogue of JSON's
comma/brace normalization — that re-flows a fragment from its source indent to the destination's.
**Subset:** a single document (an optional leading `---` is kept verbatim), block + single-line flow
maps/sequences (**nesting is preserved** — the parser builds nested `FLOW_MAP`/`FLOW_SEQ` child nodes
and a `FLOW_ENTRY` node per flow-map member, so a nested `{…}`/`[…]` value is a real recursing child
and each member is individually addressable/editable; replace/insert/delete/rename on a flow member
rebuild the `{…}` inline, while block-producing converts on an inline member are rejected and the `K`
popup hides them), 5 scalar styles (plain, single-quoted, double-quoted, literal `|`, folded `>` with
chomping), `#` comments, and YAML 1.2 **core-schema typing** with **no datetime** (date-looking
scalars are strings). **Out-of-subset constructs** — `&anchor`, `*alias`, `<<:` merge, `!tag`,
multi-line flow — project as **read-only opaque nodes** (`Node.read_only`, KIND tag `[opaq ]`): they
render and copy, but every mutation on or into them (and on any entry whose *value* is opaque —
`entry_has_opaque_value`) returns `Unsupported`, leaving the document untouched. **Multi-document**
files are rejected at load (a whole-document `E` re-parse rejects them too). The resolver maps a path
to a `Target` (`MapEntry`/`Element`/`Comment`/`Opaque`); `is_opaque` walks ancestors so a path inside
an opaque span is blocked. New model atoms: `Format::{Block, SingleQuoted, DoubleQuoted, LiteralBlock,
Folded}` and `KindTarget::{Flow, Block, StringPlain, StringSingle, StringDouble, StringLiteralBlock,
StringFolded}` — driving KIND tags `[A/B]`/`[A/F]` (block/flow seq), `[T/B]`/`[T/F]` (block/flow map;
`[T/F]` is shared by flow map and inline table), `[S:sq  ]`/`[S:dq  ]`/`[S:lit ]`/`[S:fold]`. `K`
covers map/seq block↔flow, the 5 string styles, integer radix (dec/hex/oct), float plain↔exponent.
`scalar_fragment` wraps `key: value` (or a bare `- ` element); `value_kind` projects the value in YAML
syntax for the type-change check.

**`ConfigDocument` trait** abstracts the storage backend so YAML/JSON can be added later; the
concrete backends are `CstDocument` (TOML), `JsonDocument` (JSON/JSONC), and `YamlDocument`
(YAML subset) (the original `toml_edit`-based `TomlDocument` was retired after reaching parity). The trait exposes `project`, `serialize`, `serialize_fragment`,
`serialize_fragment_relative`, `is_dirty`, `apply(Mutation)`, `to_value()`, and three **format facets** —
`format() -> DocFormat`, `comment_prefix()`, `had_comments_at_open()` — plus `kind_options(path)`,
which serves the `K` popup's per-node convertible-kind list (`(label, KindTarget)` pairs) so the
TUI never hard-codes a backend's notations, and two **fragment facets** the inline editor/`nudge`/`a`
use so they don't hard-code a notation either: `scalar_fragment(key, value)` (wraps a value repr as
`key = value` / `"key": value`, or — `key: None` — the backend's *value-Replace* element form, which
TOML wraps as `__elem__ = value`), `array_element_fragment(value)` (the **bare keyless element** form
`a` seeds into an array/seq — TOML/JSON re-wrap a bare value spliced keyless, YAML's `- value` — so all
three seed array elements uniformly), and `value_kind(value)` (projects
the value in the backend's own syntax for the type-change check). **`AnyDocument`** (`model/any_doc.rs`) is a one-enum
dispatcher wrapping every backend (`Toml(CstDocument)`, `Json(JsonDocument)`, `Yaml(YamlDocument)`)
and implementing `ConfigDocument` by match-delegation; the TUI holds a single `AnyDocument`, and a
new format is one more variant. `detect_format(path)` maps the extension to a `DocFormat`
(`.toml`/`.json`/`.jsonc`/`.yaml`/`.yml`); `load_as(path, format)` dispatches to TOML, JSON/JSONC,
or YAML. `Mutation::Insert`/`Replace` carry a format-neutral `fragment:` field (not `toml:`).
Path→node lookup lives on `NodeTree::node_at(path)` (model layer, reused by `kind_options`).

**Document-level conversion** (`model/convert.rs`, spec §Phase 4). `convert(doc, target) ->
Result<ConvertResult, ConvertAbort>` lowers a loaded document to a **format-neutral `Value`
tree** (`model/value.rs`: `Value::{Null,Bool,Int,Float,Str,Datetime,Seq,Map}`, ordered
`Vec<Item>` where `Item::{Comment, Node{key,value,trailing}}` keeps confy's first-class comments
in document order), then renders it back in the *target's* default style. The lowering is one
generic walk — `tree_to_value(&NodeTree, src)` maps containers by `NodeKind` (Table/InlineTable→
`Map`, Array/ArrayOfTables→`Seq`, the Root sniffs keyed-vs-keyless children, a comment→
`Item::Comment` with markers stripped, `trailing_comment`→`Item.trailing`), and per-format
`decode_*` helpers decode each scalar's raw token text (`node.value`) to typed data (TOML/JSON/
YAML radix, escapes, block scalars, inf/nan). Each backend implements `ConfigDocument::to_value`
as `tree_to_value(&self.project(), <fmt>)`; **schema validation** lowers through the sibling
`tree_to_value_lenient`, identical except that a YAML **opaque** node is *skipped* instead of
aborting the document (`value_bridge::walk` skips the same nodes to keep the Node↔Value pairing
1:1) — one anchor used to silence every violation marker in the file. **Loss policy** (the documented lossy contract):
notation/style that the default render drops is collected as deduplicated **warnings** during the
walk (`style_note`: radix, string style, inline/flow, dotted, AoT, exponent); `analyze` adds the
target-specific rules — `null`→TOML and a YAML opaque node→any target **abort** (no output;
null paths listed), TOML datetime→JSON/YAML and non-finite floats→JSON **warn**. A detected
**schema hint** (JSON `"$schema"` key / YAML modeline / TOML `#:schema` comment, via
`schema::hints`) is stripped from the source and re-authored in the *target's* own convention
rather than carried across verbatim as a stray comment or data key; if the target root shape
can't carry that convention (e.g. a non-object JSON root), the hint is dropped with a warning
instead. The three
renderers emit default style only (`render_toml` scope tables + bare keys + `#`, two-phase so
keys precede `[sub]`/`[[aot]]` headers; `render_json` 2-space multiline, `//` comments only when
present ⇒ JSONC; `render_yaml` block + plain-where-safe scalars + `#`). A **reparse safety net**
loads the rendered text with the target backend before returning, so invalid output never reaches
disk. The **source document is never modified**. Two surfaces: the `confy convert <in> <out>
[--from --to --yes]` CLI (`cli.rs`) and a TUI Root-node action on `C` (`Mode::Convert`: pick
format → output path → warning/confirm; the open doc is untouched).

**Addressing.** Keyed nodes are addressed by `Seg::Key(name)`; **positional** nodes — comments,
array elements, AoT entries — by `Seg::Index(i)` over the parent's *full child sequence*
(comments share the slot space, so an element after a comment keeps its full-sequence index).
There are no synthetic keys; the TUI identifies a comment by `NodeKind::Comment`, never by
sniffing the path. `cst_edit::walk` builds the same `path → syntax element` index the projection
uses, so resolver and projection cannot drift (a consistency test ties them).

**`Mutation` enum** — the closed set of document operations: Insert, Delete, Replace, Rename,
Move, Remark, EditComment, InsertComment. Each variant is a rowan green-tree splice with
newline/indent normalization. Per-variant mechanics (forming/clamp, AoT-entry move-out, delete
extent, Rename whole-key rewrite, known edges) are in CONTEXT.md *Mutation mechanics*.

**Projection.** Dotted *keys* (`a.b.c = 1`) nest into a chain of synthetic `[T/D]` tables via
`project_entry_into`/`ensure_dotted_chain` in `cst_project.rs`; the leaf keeps its full
`Target::Entry` path so an **untouched file round-trips byte-identically**. Dotted-key
concepts, inline-dotted machinery, member spans, implicit/mixed tables, `[T/S]` scope nesting,
and Illegal table moves are in CONTEXT.md (*Dotted table*, *Member spans*, *Mixed table*,
*Insert / move legality*, *Mutation mechanics*). `ScalarType`, `Format` enum values,
`KeySign` facet, the `value` repr field, and KIND column rendering (`type_tag`) are in
TUI.md §*Rendering*.

**Key representation.** A key has two projected forms, and every backend agrees on the split:
`Node.key`/`Seg::Key` hold the **decoded** key (semantic identity — path resolution, collision
checks, JSON-Schema `properties` lookup, `to_value`/convert), while `Node.key_literal:
Option<String>` holds it **exactly as authored**, quote characters and escapes intact
(presentation + edit identity — tree row label, Path line, rename/edit buffer, fragment
rebuilding). It is filled once during projection from the key token already in hand, so no
consumer re-walks the CST or synthesizes a quote character; `None` means "authored bare, the
decoded key is the literal". `Node.key_sign` (`KeySign`) survives as a coarse facet for the `f`
type filter, which classifies quoted-vs-bare without needing the text. Going the other way,
`ConfigDocument::rename_key_segs(new_key)` decodes a rename's literal with the backend's own key
lexer, so the post-rename path is rebuilt from decoded segments and a quoted key containing a dot
is never split. `Session::human_path(path)` renders the dotted/bracketed display form, re-wrapping
a quoted-YAML segment in its `"…"` flanks; it is precomputed per row as `ViewRow.path_display` and
drives the TUI Detail popup's `Path:` line and the web panel's Path field.

**Editing.** `e` dispatches via `edit_target_kind`. The **inline-vs-`$EDITOR` boundary** is
governed by BEHAVIOR_MATRIX §6 (universal single-line-scalar inline editing across all scopes;
single-line arrays/inline tables/JSON objects edited as their one-line repr, EOL comment
preserved via `entry_trailing_comment`; the YAML array-ancestor lift where `plugins[1].name` /
`plugins[3]` edit inline and `edit_node` skips array truncation; literal `|`/folded `>` and
everything multiline → `$EDITOR`). Inline editor mechanics (Tab Value↔Name commit order,
type-change detection, caret fields, `←/→` nudge, `a`-add Esc rollback via
`History::cancel_last`) are in TUI.md §*Editing*.

**Kind switch (`K`).** `Mutation::ConvertKind { path, target: KindTarget }` (`convert_kind` in
`cst_edit.rs`) rewrites a node's kind/notation in place; targets come from `kind_options(path)`.
Conversion rules (scalar within-type, table `[T/I]`/`[T/D]`/`[T/S]` D5-checks, `[A/T]`↔array,
Illegal conditions) are in CONTEXT.md *Kind switch (`K`) rules*.

**Comments are first-class nodes** (concepts in CONTEXT.md: *Comment*, *Trailing comment* —
standalone `#` lines merge into one node and are never dragged by an adjacent node's move; a
trailing comment is value-attached decoration). Trailing-comment inline edit flow,
array-element trailing rules, YAML re-assert, and `e`/`E`/`d` comment routing are in
TUI.md §*Comments (TUI)*.

**Navigation.** Expand/collapse mechanics (`expanded` HashSet, root empty-path,
`collapse_all`, `1`/`2` level-at-a-time ascend) — TUI.md §*Navigation*.

**Filter.** Three-state flow, FilterResults dispatch, `last_filter` prefill, Esc peel,
haystack semantics (key/path + Comment text + a scalar leaf's own value), and the NAME+VALUE
per-char highlight — TUI.md §*Filter*. The web/touch trees mark the same chars with the same
wasm-exported matcher — WEBUI.md §*Native modal widgets*.

**Type filter.** TypeToken/classify popup, tristate groups, AND-intersection of text∩type,
FilterLayer peel — TUI.md §*Type filter*.

**Multi-select.** round/committed union and fresh-round folding — TUI.md §*Multi-select*.

**Clipboard / paste.** Scope-relative capture, paste-mode state machine (clipboard freezes
selection; `c`/`x` toggles; Esc peels), failure contract (`do_paste` restores on every
failure), and InsertComment/ArrayUpgrade paths — TUI.md §*Clipboard / paste*.

**JSON Schema.** `schema/` (types.rs: `SchemaSource`/`SchemaState`/`SchemaStatus`/`Violation`/
`EditHint`/`Category`; hints.rs: per-format hint detection — JSON `"$schema"` root key, YAML
`# yaml-language-server: $schema=` modeline, TOML `#:schema` leading comment; value_bridge.rs:
Node+Value → JSON-projection bridging that attaches a Path to every projection node (YAML opaque
nodes omitted on both sides, matching `tree_to_value_lenient`, so an anchor costs only itself);
validate.rs: `jsonschema`-backed validation over that projection, draft 2020-12, uniform across
all three formats since it runs on the projection, never source syntax — ADR 0002; hints_edit.rs:
best-effort sub-schema resolution at one Path for the constrained-value picker, simpler than full
validation, declining to `EditHint::None` for anything beyond `properties`/`items`/local
`$defs`/same-document `$ref`/a narrow `oneOf`/`anyOf`-of-`const` — plus `resolve_schema_info`
(`Session::schema_info`), an orthogonal non-widget lookup on the same resolved sub-schema for
`description`/`type`/`format`/`pattern`, covering the plain-typed case `EditHint` leaves at
`None`; dirty_check.rs: a per-mutation
"does this path carry a schema constraint" check that lets `Session::on_mutation_success` skip a
full revalidation walk when the answer is no) is a **soft constraint** (CONTEXT.md § Schema):
Violations surface as a visual indicator and never block a Mutation or a save. Detection/parsing
is host-agnostic in `confy-core`; hosts resolve the actual bytes — the TUI's
`crates/confy-tui/src/tui/schema_io.rs` (local hint resolves against the open file's directory, a
URL hint fetches over a blocking HTTP client) and its `overlay_schema_enum.rs` popup (reuses the
`K` kind-switch popup's shape); the web layer's `session.schemaHint(path)`/`fetch()`. There is no
manual "attach a schema" UI action on any host — every host goes through the same detection path.
`session/schema_hint.rs` is unrelated by name collision only: it holds `nudge_scalar`'s numeric
clamping for the `←`/`→` shortcut, not schema attachment. `Mode::SchemaEnum` has one
**schema-independent** producer too: `begin_inline_edit` opens the same picker with `true`/`false`
(in the node's authored casing, `inline_edit.rs::bool_picker_options`) for any `bool` scalar,
flagged `from_schema: false` so hosts title it "Value"; a real schema `enum` on that node is
resolved first and wins.

**i18n (internationalization).** The translation catalog lives in `confy-core`, not per-host
(`crates/confy-core/src/session/i18n.rs`): `Lang` (`En`/`ZhTw`, serde `"en"`/`"zh-TW"`,
`Default = En`) plus `tr(lang, key)`/`tr_args(lang, key, args)` look up flat `core.*`/`tui.*`/
`web.*` keys in `include_str!`'d JSON catalogs at the repo root (`i18n/en.json` canonical,
`i18n/zh-TW.json`), falling back to `en` then the raw key so a missing translation never panics
or blanks the UI. `Session.lang: Lang` drives every status/error/detail string `Session`
composes; `Intent::SetLang(String)` (a string, not the enum, to keep the wasm wire contract
simple) is routed in `dispatch.rs`, and `SessionSnapshot.lang: String` mirrors it back to hosts.
Each host layers its own strings on top: the TUI's `crates/confy-tui/src/config.rs` persists a
`lang` preference to `~/.config/confy/config.toml` (`%APPDATA%\confy\config.toml` on Windows),
exposes `--lang` (session-only override; precedence `--lang` > config file > `en`), and an `l`
picker (TUI.md §*Language / i18n (TUI)*); `web/i18n.ts` imports both catalog JSON files directly
(esbuild bundles them), exposes `t()`/`tArgs()` with the identical fallback chain, and persists
the choice in `localStorage["confy-lang"]` (WEBUI.md §*Language / i18n (Web)*).
`state.rs::about_text(lang)` gives each host a translated About body (`ABOUT_TEXT`/
`ABOUT_TEXT_ZH_TW`); the TUI appends host-only `Config:`/`Language:` lines, the web layer
appends a localStorage disclosure line instead.

## Known Risks

**`taplo` is unmaintained upstream.** The maintainer stepped down in Dec 2024
([tamasfe/taplo#715](https://github.com/tamasfe/taplo/issues/715)); the repo is stalled but
not archived, no ownership transfer has happened, and `rowan =0.15.18` is exact-pinned to
match taplo's internal version. `confy`'s taplo surface is small — `taplo::parser::parse`
(47 call sites), `taplo::syntax::*`/`taplo::rowan::NodeOrToken`/`SyntaxElement` (18 sites),
`taplo::dom::Node`/`taplo::dom::Error::ConflictingKeys` (2 sites) — and none of taplo's
1,330-line formatter or 2,800-line DOM is used (duplicate-key detection is already
hand-rolled per-backend in `validate_semantics`). Vendoring only the used surface
(`parser/mod.rs` + `parser/macros.rs` + `syntax.rs`) is estimated at ~1,240 LOC and would
also unpin `rowan` and drop `globset`/`schemars`/`arc-swap`/`itertools`/`once_cell` from the
dependency tree. `tombi`, the community's suggested migration target, is **not** currently a
usable dependency (its crates.io entry is a reserved placeholder; sub-crates unpublished).
**Decision: do not migrate now.** The `cargo audit` CI step (`.github/workflows/rust-ci.yml`)
is the trigger — if it flags a `rowan`/`taplo`/`ahash` advisory, vendoring per the above scope
estimate is the pre-planned contingency.

## Module map

Cargo **workspace** (see `PORTING.md`): `confy-core` is the headless model crate; `confy-tui`
is the ratatui TUI + CLI binary (`confy`) that depends on it and re-exports `model` so its UI
modules keep their `crate::model::…` paths. `confy-ffi` is the WASM wrapper (Web UI); `confy-tauri`
is the Tauri v2 shell over that same web UI — desktop (macOS/Windows) and, since Mobile M1,
Android — adding only native file I/O. `tauri-plugin-confy-picker` is a small first-party mobile
plugin `confy-tauri` depends on for the one Android gap stock Tauri plugins don't cover (see
below).

```
i18n/                     translation catalogs — root i18n/en.json (canonical, en-fallback
                          source) + i18n/zh-TW.json; flat core.*/tui.*/web.* keys, embedded in
                          confy-core via include_str! and imported directly by web/i18n.ts
                          (esbuild bundles JSON)

crates/confy-core/src/   headless core — pure, no terminal/UI/`tempfile` runtime deps
  lib.rs           `pub mod model; pub mod schema; pub mod session;`
  model/
    mod.rs         re-exports
    text_range.rs  TextRange (byte-offset spans for source ranges) shared by rowan projections
    node.rs        Seg, ScalarType, Format, NodeKind, Node, NodeTree (+ node_at lookup)
    document.rs    ConfigDocument trait (+ to_value), DocFormat, Mutation, Target, OnCollision, ConvertAbort, errors
    value.rs       format-neutral Value/Item tree for conversion (has_null/has_datetime)
    convert.rs     document-level conversion: tree_to_value walk (+ tree_to_value_lenient, the
                   opaque-skipping variant schema validation lowers through) + per-format scalar
                   decoders + default-style renderers + loss policy
    any_doc.rs     AnyDocument enum: per-format dispatch + detect_format/from_str_as/set_filename (TOML/JSON/YAML)
    cst_doc.rs     CstDocument holding the taplo/rowan tree: from_str (sole headless ctor) / serialize / apply (atomic commit) / set_filename
    cst_project.rs CST → NodeTree projection (comments as real nodes; golden tests)
    cst_edit/      rowan splice helpers, split by Mutation family (Task 15, 2026-08-11 audit
                   remediation) — mod.rs (dispatch + the path→element walk index),
                   move_paste.rs (Insert/Move), replace_delete.rs (Replace/Delete/Remark/
                   EditComment/InsertComment + table/section/member-span machinery),
                   rename.rs (Rename), convert.rs (ConvertKind), dotted_table.rs (synthetic
                   `[T/D]` table helpers), aot_group.rs (`[[array-of-tables]]` group spans),
                   tree_nav.rs (shared projected-tree/CST-index navigation), escape.rs
                   (basic-string escape helpers)
    json/
      mod.rs       re-exports for the JSON/JSONC backend
      syntax.rs    SyntaxKind enum + rowan Language impl (hand-rolled JSON token/node kinds)
      parse.rs     lossless lexer + recursive-descent parser → rowan GreenTree (JSONC-aware)
      doc.rs       JsonDocument: from_str/serialize/apply (atomic commit + validate_semantics)
      project.rs   GreenTree → NodeTree projection (// comments as real nodes; golden tests)
      edit.rs      rowan splice helpers: one fn per Mutation variant for JSON/JSONC
    yaml/
      mod.rs       re-exports for the YAML-subset backend
      syntax.rs    SyntaxKind enum + rowan Language impl (hand-rolled YAML token/node kinds)
      parse.rs     lossless lexer + recursive-descent parser → rowan GreenTree (subset; multi-doc reject)
      doc.rs       YamlDocument: from_str/serialize/apply (atomic commit + validate_semantics)
      project.rs   GreenTree → NodeTree projection (# comments real nodes; opaque read-only nodes; golden tests)
      edit/        rowan splice helpers, split by construct (Task 15, 2026-08-11 audit
                    remediation) — mod.rs (indent engine/resolver/opaque-guard re-exports +
                    atomic dispatch), block.rs (block-style map/seq Replace/Delete/Insert),
                    flow.rs (`{ … }`/`[ … ]` flow-collection edits), mutations.rs
                    (Rename/Remark/EditComment/InsertComment/Move/SetTrailingComment),
                    convert.rs (ConvertKind: flow/block toggle + scalar notation),
                    resolve.rs (reindent engine, path resolver, opaque guard)
  session/         §5 state-machine lift (Slice 4) — the complete headless Session, split
                   further across single-purpose files (Task 15, 2026-08-11 audit remediation)
    mod.rs         re-exports
    host.rs        Host trait (edit_text callback) + EditTextOutcome
    i18n.rs        Lang enum + tr/tr_args catalog lookup (include_str!'d i18n/*.json, en-fallback)
    intent.rs      Intent enum — every key-mapped action the TUI can dispatch
    notice.rs      Notice (single-slot transient message), Severity, NoticeSource, severity_of table
                   — see MESSAGES.md for the full message-system reference
    session.rs     Session struct (all CORE state + methods): visible_rows/compute_rows, navigation,
                   filter/type-filter, kind-switch, convert (no fs), edit routing,
                   escape, prompt-key dispatch, quit flow; plus free fns: node_type_label,
                   format_label
    clipboard.rs   cut/copy/paste + the paste collision/array-upgrade prompt sub-state-machine
    action_menu.rs core-owned Action menu: one item list + open/cursor state, read by every host
                   via `ModeView::ActionMenu` (ADR 0009) — replaces the desktop `⋮` popup, the
                   detail panel's action row, and the FAB's add-only decision
    add_picker.rs  `Mode::AddPicker`: the legal node kinds for the resolved insertion Target
                   (filtered by parent kind/format), seeding the picked kind's default literal
    diag.rs        DiagLevel, DiagEvent (monotonic seq, kind, detail), DiagRing (bounded 256-event ring)
                   — see MESSAGES.md §4
    inline_edit.rs inline-editor buffer lifecycle (begin_inline_edit*/edit_*/edit_commit) +
                   value/rename/nudge/add-node mutation-application methods that commit through it
    schema_hint.rs nudge_scalar: schema-constraint numeric clamping for the `←`/`→` shortcut
    undo_redo.rs   undo/redo
    status_fmt.rs  kind/type/format label formatting + small scalar-repr/string utilities
    state.rs       Mode, PendingCommit, PendingExternalEdit, EditKind, EditState, History,
                   Clipboard, PasteSlot, FilterLayer, …
    selection.rs   Selection (path-keyed multi-select + range rounds)
    search.rs      fuzzy_match / fuzzy_indices / haystack
    insertion.rs   resolve_target (pure insertion-target logic)
    type_filter.rs TypeFilter, TypeToken, layout/nav helpers
    view.rs        ViewRow (pure view row, no type_tag) +
                   Stage-2 full-state transport: SessionSnapshot (+clipboard_count), ModeView,
                   EditView, ConvertView, KindOptionView, PromptView, ExternalEdit/ExternalEditKind,
                   TypeFilterView/TypeFilterRow/TypeFilterCellView (the WASM wire contract)
    dispatch.rs    Stage-2 command channel: Session::dispatch(Intent) -> SessionSnapshot
                   (mode-dependent Intent→method routing; the only entry point the Web UI uses)
  schema/          JSON Schema detection/validation/constrained-editing — see Architecture
                   *JSON Schema* above for the per-file breakdown
crates/confy-core/tests/  roundtrip*.rs / yaml_scratch.rs + fixtures/ + no_fs_gate.rs (§7 gate)
                          + session_headless.rs (§7 gate #4: headless Session scripted tests;
                          §7 gate #5: fake-Host `$EDITOR` flow; + dispatch() tests) + serde_roundtrip.rs (§7 gate #3)
                          + schema_headless.rs (headless schema-engine tests, same crate-root
                          `#[test]`-fn convention as session_headless.rs) + modal_lock.rs
                          (integration: every guarded method no-ops + sets status while the
                          clipboard is armed, ADR 0005 §5)

crates/confy-ffi/         Stage-2 WASM wrapper over confy-core (wasm-bindgen + serde-wasm-bindgen)
  src/lib.rs     ConfySession: from_text/dispatch/snapshot/serialize/visible_rows/kind_options
                 (the JS-facing handle; serde-wasm-bindgen marshals Intent/SessionSnapshot)
  functional_smoke.mjs     node verification of the Intent→snapshot contract (128 checks)
  (build: `wasm-pack build --target web`; getrandom wasm_js for the ahash-via-taplo chain)

web/                       TypeScript integration + **web-native** UI (see WEBUI.md) — a
                           pointer-first port of `design_index_model.html`, Session-driven
  types.ts       hand-written mirror of the confy-core serde contract (Intent/SessionSnapshot/…)
  confy.ts       typed wrapper around the wasm ConfySession (load + Session class; `kindOptions`)
  fs.ts          File System Access API open/save-in-place + download fallback + `fetchUrlFile`
                 (open a remote config; `?url=` deep-link & "Open from URL") — host-owned I/O
  menu.ts        Tauri native File/Edit/View/Help menu bar (`window.__TAURI__.menu`;
                 `isTauri()` no-op on the pure web build) — see TAURI.md §Desktop menu (Tauri)
  render.ts      pure `SessionSnapshot → DOM` tree: web-native row anatomy (drag grip, rotating
                 caret, key/`—`/value value-type-colored, item count, **kind badge** =
                 label+notation suffix+chevron, comment/trailing, hover ＋/⋮ actions);
                 container & scalar notation suffixes, `escapeAttr` for `data-path`
  highlight.ts   fuzzy-filter match marks: `highlightHtml(text, needle)` → escaped HTML with
                 `<mark class="fz">` runs (coalesced, char-indexed via `Array.from`). Web mirror
                 of the TUI's `highlight_spans`, driven by the SAME matcher — the wasm free export
                 `fuzzy_indices`, injected by confy.ts's `load()` via `setFuzzyMatcher` so
                 render.ts stays wasm-free and node-bundleable. Used by render.ts + touch/render.ts
  i18n.ts        catalog wrapper: t()/tArgs() over ../i18n/*.json, en-fallback chain,
                 getLang()/setLang() persisted in localStorage["confy-lang"]
  select.ts      pure pointer-selection logic → `SetSelection`/`SetCursor`: plain/⇧-range/
                 ⌘-toggle clicks (segmented additive range via an anchor+base snapshot) + marquee
  dnd.ts         HTML5 grip drag-reparent → `MoveSelectionTo {sources,slot,cut}`: the destination is
                 core's `pointerSlot(path,relY)` verbatim (`Into` outline / `After` `#dropLine`),
                 resolved by the same `slot_target` a keyboard Paste uses — no host-side
                 parent/index or band threshold (ADR 0010); self-subtree drop rejected
  slot-line.ts   `slotLineIndentPx()` — the one rule for an insertion line's indent, shared by the
                 web drag/armed cues and touch's `.reorder-line`: `After(<expanded branch>)` inserts
                 as its first child, so the line sits one `--indent` step deeper (as the TUI draws it)
  panel.ts       shared node detail/edit panel (`panelHTML`/`wirePanel`) — one module rendering
                 the desktop Detail aside AND the touch edit sheet identically (locked field order
                 Key/Value/Trailing comment/Kind/Path/Children/Sign); a panel input's Enter/Escape
                 keydown `stopPropagation()`s so a synchronously-opened confirm prompt or the host's
                 global key handler doesn't re-read the same bubbling event
  prompt.ts      shared `Mode::Prompt` y/n(/o/r) answer buttons (`promptButtonsHTML`/
                 `promptQuestion`/`bindPromptClicks`) — desktop renders them in `#overlay`, touch in
                 a `.prompt-sheet`; both answer via the same `PromptKey` intent
  breadcrumb.ts  VS Code-style breadcrumb bar + mini-tree picker: segment click →
                 RevealPath ("Reveal": expand ancestors + set cursor + select, then
                 ui.ts center-scrolls the row; filter-hidden targets keep cursor +
                 report on status); `›` separator click (incl. trailing one) opens a
                 lazy mini document tree (ffi children(path)), row click → same
                 Reveal; popup state is ephemeral
  ui.ts          orchestrator: holds the latest snapshot, renders via render.ts + the modal
                 surfaces (detail aside, native search box, `#tfPop` type-filter grid, `#convDlg`
                 convert dialog, `#overlay` for Help/Prompt/KindSwitch only), Tree|Raw read-only
                 view toggle (`session.serialize()`), keyboard→Intent map (mirrors tui/keys.rs),
                 theme toggle, FS open/save, `#url-modal` Open-from-URL, external-edit modal,
                 paste-mode cursor target; `navSelect` re-targets an undrawn-root cursor via
                 `path-utils.ts`'s `drawnCursorFallback` (shared with touch's `touchNavSelect`) —
                 `Home`/`g` can otherwise leave an invisible cursor, since neither web host draws
                 the root row. Touch's `app.ts` mirrors this plus its own keyboard
                 `scrollFocusIntoView()` (minimal-scroll the tree pane to follow the cursor / the
                 paste-mode `.reorder-line`/`.drop-into` row past a viewport edge — `render()`
                 otherwise restores `scrollTop` verbatim across every re-render)
  toolbar-fold.ts shared header/filter-row "⋯ More" fold registry (`foldedEntries`/
                 `ToolbarEntry`), used identically by `ui.ts` and `touch/app.ts` — button
                 inventory, fold breakpoints, and per-host trimming are in **CHROME.md**
  host-io.ts     host-side I/O + theme flows shared by the two orchestrators (open/save/
                 open-from-URL/theme), so `ui.ts` and `touch/app.ts` don't fork them
  key-intent.ts  pure "which Intent does this (mode, key) pair mean" resolution — the single
                 keymap source both orchestrators dispatch through (KEYMAP.md is its SSOT doc)
  mode.ts        shared `modeTag()` helper over the `ModeView` union
  escape.ts      the one HTML escaper (`escapeHtml`/`escapeAttr`) every render module uses
  kind-labels.ts shared `ViewRow` lookups/predicates (value-hue labels, row-anatomy helpers)
  samples.ts     built-in demo doc + sample-mode state (shared backbone tree + per-format
                 showcase branch); `schema-sample.json` is its `$schema` target
  help-content.ts shared Help/About/KIND-legend body for the Help overlay (all web hosts)
  convert-dialog.ts shared Save/Convert dialog, rendered identically desktop + touch
  typefilter.ts  shared `f` type-filter facet grid (same markup/wiring on both hosts)
  fab.ts         shared floating "actions / paste" button (FAB) behavior + markup
  action-menu-items.ts / add-picker-items.ts  shared item rendering for `Mode::ActionMenu` /
                 `Mode::AddPicker`, so the desktop popup and the touch sheet stay identical
  entry-desktop.js / entry-touch.js / register-sw.js  the per-entry boot scripts (pointer-based
                 desktop↔touch router; https-only service-worker registration). **External
                 files, never inline `<script>`** — the Tauri shell's CSP forbids inline script;
                 new ones must be added to `assemble-dist.mjs`'s copy list (TAURI.md §CSP)
  index.html / style.css (design `<style>` **verbatim** + a fenced app-only appendix; dark+light
                 via :root[data-theme]; header/filter-row button layout — see CHROME.md) /
                 build.mjs (esbuild) / serve.mjs / cf-build.sh
                 (Cloudflare Workers Builds build command → runtime-only web/dist; deployed with
                 root `wrangler.toml` to confy.turkeyang.net — see WEBUI.md §Deployment)

crates/confy-tui/src/    ratatui TUI + CLI; depends on confy-core, `pub use confy_core::model`
  main.rs          bin `confy`: parse args, load via load_document, run TUI
  lib.rs           `pub use confy_core::model;` + `pub mod cli; pub mod tui;` + the host fs boundary:
                   `load_document` (read → strip UTF-8 BOM → from_str_as → set_filename, returns
                   `LoadedDocument { doc, bom }`) and `write_document` (BOM re-emit + atomic
                   temp-file + rename, preserving the destination's Unix mode)
  cli.rs           clap args: default `confy <file> [--format]` (TUI) + `confy convert <in> <out>` subcommand
                   + `--lang <code>` (session-only language override)
  config.rs        host-owned config file I/O: load_config/save_config for
                   `~/.config/confy/config.toml` (`%APPDATA%\confy\config.toml` on Windows);
                   `lang = "…"` today, missing/unparsable file ⇒ defaults, never an error
  tui/
    mod.rs         re-exports; run() entry point + event loop (run_event_loop)
    app.rs         App = thin Host wrapper: `pub session: Session` + 6 HOST-only fields
                   (rows/source_path/bom/detail_scroll/help_scroll/table_offset); App::save = serialize → write_document
    state.rs       thin re-export of confy_core::session::state
    keys.rs        KeyAction mapping + help text
    insertion.rs   thin re-export of confy_core::session::insertion
    selection.rs   thin re-export of confy_core::session::selection
    search.rs      thin re-export of confy_core::session::search
    type_filter.rs thin re-export of confy_core::session::type_filter
    editor.rs      $EDITOR integration (external edit for nested array/table)
    schema_io.rs   host-side schema-source resolution: a local hint resolves against the open
                   file's directory; a URL hint fetches over a blocking HTTP client (the one
                   networking capability the schema feature adds to this crate)
    ui.rs          ratatui rendering: title bar + NAME/TYPE/VALUE column header + tree Table;
                   popup rendering itself was split out into the overlay_*.rs siblings below
                   (Task 10, 2026-08-11 audit remediation — pure code motion)
    overlay_action_menu.rs  the Action menu popup (`Mode::ActionMenu`, ADR 0009)
    overlay_add_picker.rs   the `a` Add-type picker popup (`Mode::AddPicker`)
    overlay_convert.rs      the `C` convert-document popup
    overlay_detail.rs       the `i` Detail popup (+ appended Schema: violations section)
    overlay_diag.rs         the `~` read-only diag ring overlay
    overlay_help.rs         the `?` Help | About popup
    overlay_kind_switch.rs  the `K` kind-switch popup
    overlay_lang_picker.rs  the `l` language-picker popup
    overlay_schema_enum.rs  the schema-constrained enum/const picker (reuses `K`'s popup shape)
    overlay_type_filter.rs  the `f` type-filter facet popup
crates/confy-tui/tests/   convert_cli.rs (`confy convert` happy/lossy/abort/overwrite-guard paths,
                          source-unchanged), open_url_cli.rs, schema_io.rs

crates/confy-tauri/       desktop + Android app shell (Tauri v2) over the web UI — **native file
                          I/O only** (the native menu bar, recent-files, and Android
                          picker/file-association mechanics are in **`TAURI.md`**)
  src/lib.rs     `confy_tauri_lib` — the real crate body (mobile needs a `#[cfg_attr(mobile,
                 tauri::mobile_entry_point)] pub fn run()` in a `[lib]`, not `main.rs`): Tauri
                 builder + `tauri_plugin_fs`/`tauri_plugin_dialog` + 2 custom
                 `#[tauri::command]`s — `startup_file` (desktop CLI-arg open) and `opened_urls`
                 (Android cold-start "Open with" drain; a warm app instead gets an `"opened"`
                 window event). Editing stays in the in-webview wasm Session (dispatch is sync;
                 not moved over IPC) — Rust owns only real open/save/read/write. Real open/save
                 on desktop goes through `tauri_plugin_dialog`/`tauri_plugin_fs` directly (no
                 custom command needed, unlike the pre-M1 5-command design); Android's
                 write-in-place picker instead routes through `tauri-plugin-confy-picker`
                 (below) since stock `tauri-plugin-dialog`'s Android `open()` uses
                 `ACTION_GET_CONTENT`, which never grants write access (a confirmed, unresolved
                 upstream gap as of `tauri-plugin-dialog` 2.7.1).
  src/main.rs    thin bin `confy-desktop`, just calls `confy_tauri_lib::run()`.
  tauri.conf.json  frontendDist=../../web/dist, beforeBuildCommand=cf-build.sh (via git toplevel),
                   bundle targets ["dmg"], identifier net.turkeyang.confy.
                   `dragDropEnabled: false` on the main window is REQUIRED: Tauri v2
                   defaults it to `true`, and that OS-level file-drop handler swallows every
                   drag session before the webview sees it, killing `web/dnd.ts`'s HTML5
                   grip-drag (greyed rows + forbidden cursor, no `dragover`/`drop`) on
                   Windows and macOS alike. The app uses no native file drops, so leave it
                   off — re-enabling it regresses desktop node drag-and-drop.
                   `security.csp` is set (not Tauri's `null` default); it forbids inline
                   `<script>`, so every HTML entry's boot script must stay an external file
                   — see `docs/reference/TAURI.md §Content Security Policy`.
  tauri.windows.conf.json  Windows platform override (Tauri v2 auto-merge): empty
                   before-commands (bash/git rev-parse don't run under the Windows build
                   shell — build web/dist manually first) + bundle targets ["nsis"]
  tauri.android.conf.json  Android-only platform-merge override: `bundle.fileAssociations` for
                   `.toml`/`.json`/`.jsonc`/`.yaml`/`.yml` (kept out of the shared config —
                   `bundle` also governs the macOS `.dmg`, and Finder would register the
                   association there with nothing wired up to handle it). Several MIME entries
                   per extension (`text/plain` fallback, YAML's 4 near-synonym MIME strings) —
                   `.toml`/`.yaml` have no IANA-registered type, so different Android file
                   managers guess differently when resolving a file's MIME for intent matching;
                   this broadens the match without guaranteeing every one. Tauri's Android build
                   generates the intent-filter from this automatically — no manual
                   `AndroidManifest.xml` edit.
  capabilities/    default.json — core:default + dialog:default + explicit
                   `fs:allow-read-text-file`/`fs:allow-write-text-file` + scope, plus
                   `confy-picker:default` (Android only) for the custom plugin below.
  icons/           brand set (32/128/@2x png + icon.icns/.ico), regen via `cargo tauri icon`.
                   Android's launcher icon deliberately uses the plain per-density
                   `ic_launcher.png` mipmaps, **not** the adaptive-icon foreground/background
                   split `cargo tauri icon` also generates — the source PNG has zero alpha
                   transparency, so the adaptive foreground fills the entire icon with no margin
                   for a background color to show through and reads as a flat block; the
                   adaptive-icon resources are removed from `gen/android`.
  gen/android/     Tauri-generated Android Studio project (committed, generated `.gitignore`
                   already excludes build outputs/keystores). A few files are **hand-edited and
                   must be reapplied if `cargo tauri icon`/`android init` regenerates this
                   directory**: `values{,-night}/themes.xml` add
                   `android:windowOptOutEdgeToEdgeEnforcement` (targetSdk 36 forces edge-to-edge
                   by default, drawing content under the status bar otherwise); the
                   `mipmap-anydpi-v26/ic_launcher.xml` adaptive-icon definition (plus its
                   now-orphaned `drawable-v24`/`values` foreground/background resources) is
                   deleted per the icon note above.

crates/tauri-plugin-confy-picker/   first-party Tauri mobile plugin, Android-only real
                          implementation (desktop stub returns `Error::Unsupported` — desktop
                          keeps using `tauri-plugin-dialog` directly, which has no such gap)
  src/models.rs  `PickWritableResponse { uri: Option<String>, name: Option<String> }` — **every
                 field the Kotlin side puts on its response object must be declared here**,
                 since mobile-plugin responses deserialize from the JNI/Kotlin JSON into this
                 typed Rust struct before being re-serialized back to JS; serde silently drops
                 anything undeclared (a real bug hit in M1: the Kotlin side computed `name`
                 correctly the whole time, but it never reached JS until this struct declared
                 the field).
  android/.../ConfyPickerPlugin.kt  one command, `pickWritable`: `ACTION_OPEN_DOCUMENT` +
                 `FLAG_GRANT_{READ,WRITE,PERSISTABLE_URI_PERMISSION}`, then
                 `takePersistableUriPermission` on the result so the URI survives a full app
                 restart, then queries the real display name via `ContentResolver`'s SAF
                 `DISPLAY_NAME` column (null projection — some providers, e.g. the Downloads
                 provider's `msf:` media-store-file passthrough IDs, don't honor a narrow one)
                 since `content://` URIs are opaque and don't reliably embed a filename/extension
                 for format detection.

editors/vscode/          third host shell (M1.5, sideload-only, no Marketplace): a
                          `CustomTextEditorProvider` VS Code extension embedding `web/dist`
                          verbatim in a webview, over VS Code's own `TextDocument` (single source
                          of truth for content/dirty/undo/save/revert/hot-exit) via a shared
                          `web/vscode-protocol.ts` message contract — mechanics, the protocol
                          table, and the 0.2.1 tab-swap fix are in **`VSCODE.md`**.
                          `web/vscode-protocol.ts`/`web/vscode.ts` are imported here as
                          `../../../web/vscode-protocol.ts`, so protocol drift is a compile error;
                          every other `web/` behavior difference is gated on `ui.ts`'s `VSHOST`
                          flag (`isVsCode()`). `media/` is a build-time copy of `web/dist`
                          (gitignored, staged by `build.mjs`) — the extension ships no web source
                          of its own. Like the web bundle, **the extension's esbuild must run from
                          a scratchpad copy**; see `editors/vscode/README.md` for the exact
                          commands.
```

**Desktop + mobile host I/O.** `web/fs.ts` detects Tauri (`window.__TAURI__`) and routes
open/save through `tauri_plugin_fs`/`tauri_plugin_dialog`'s JS bindings instead of the browser
File System Access API. The path/URI string is the durable "handle", wrapped in an object that
**conforms to the existing `FsHandle` shape** (getFile/createWritable → `invoke`), so `ui.ts`/
`touch/app.ts` (writeFile/readHandle/deriveName/convert) are unchanged regardless of platform.
`tauriStartupFile()` opens a CLI-arg file at boot (desktop only). `fs.ts::isTauriAndroid()` picks
the one Android-specific fork: `pickOpenFile()` calls `plugin:confy-picker|pick_writable` instead
of `dialog.open()` (see the crate note above); `canSaveAs()` is false on Tauri mobile — picking a
*new* save destination (Save As, first-save-after-New, Convert output) isn't supported in M1, so
those paths show a translated hint instead of opening a picker, while writing in place to an
already-open handle is unaffected. `fileAssociations` + `opened_urls`/`"opened"` deliver a file
picked from Android's "Open with" chooser through the same `openTauriPath`-style read path.

A plain `cargo build -p confy-tauri --release` must add `--features custom-protocol` (embeds
`web/dist`; without it the exe loads devUrl → "localhost refused"); `cargo tauri build`/
`cargo tauri android build` enable it automatically. Build a desktop bundle with `cargo tauri
build` from `crates/confy-tauri` (the workspace `[profile.release]` uses
`opt-level 3`+`lto`+`codegen-units=1` — optimized for **runtime speed**, so the build
itself is slow; `--debug` is fast for local checks. Only the wasm leg wants a small
artifact, so `web/cf-build.sh` overrides that one build with
`CARGO_PROFILE_RELEASE_OPT_LEVEL=z`; dropping the override inflates the wasm by ~39%,
and applying `z` everywhere used to cost ~2.2x native runtime).
macOS produces `.app`/`.dmg`; **Windows must be built on a Windows host** (the
webview is WebView2; no cross-build); Android needs the SDK/NDK + `cargo tauri android build
--debug --apk` for a sideload-able debug APK (no keystore setup needed — debug builds auto-sign).
Linux is not targeted yet, nor is iOS.

`confy-core` is pure and **filesystem-free at runtime** — no TUI/terminal deps, no `fs`/`process`/
`env`/`tempfile`, fully unit-testable in isolation (enforced by `tests/no_fs_gate.rs`). The sole
constructor is `from_str(text)` / `AnyDocument::from_str_as(text, format)`; there is no `load`/`save`
and no `path` field (backends keep a host-set `filename` display label via `set_filename`). **The
host owns all file I/O:** `confy_tui::load_document(path, format)` reads the bytes, strips a
leading UTF-8 BOM (remembered as `LoadedDocument::bom` — Windows tools write one routinely and no
parser accepts it as content), parses via `from_str_as`, and sets the path-derived label (the
extension drives no comment-related setup — comments are legal in every `.json`/`.jsonc`
document); `App::save` and `confy convert` write through `confy_tui::write_document`, which
re-emits the BOM and writes atomically (sibling temp file + rename, so a crash mid-write never
truncates the user's config). `detect_format(path)` (pure extension match, no I/O) stays in
core. The headless-core port (§3 cursor reshape, §5 state-machine lift) is complete — see
`PORTING.md`.

## Terminology

See **`CONTEXT.md`** for the canonical glossary. Key rule: use **Node** (not "Entry"). Subtypes
are **Root**, **Branch node**, **Leaf node**, **Scalar**, and **Comment**. The operation that
toggles a live Node to/from a Comment is **Remark** (key `r`).
