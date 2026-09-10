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
npm run build                 # esbuild bundles + wasm-pack COPY (never a wasm rebuild:
                              # a confy-core change reaches the browser only after
                              # `cd crates/confy-ffi && wasm-pack build --target web`.
                              # build.mjs warns when pkg/ is older than the .rs sources)
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
- `CHANGELOG.md` must contain a `## [vX.Y.Z]` section for the tag. It holds `[Unreleased]`
  plus the **current series only** — completed series are archived verbatim under
  `docs/reference/changelog/` (its index says when and how). Never archive the series the
  next tag belongs to.

Bump all four in the same release commit, before tagging. Never tag with only
`Cargo.toml` updated.

**Also update the MSIX Store listing's ReleaseNotes** at
`crates/confy-tauri/msix/listings/listingData-9PLCJGQ3C654.csv` — set the `ReleaseNotes`
column to describe the new version in the same release commit.

## Architecture — where each contract is documented

This file is the **conduct** file: commands, release mechanics, risks, and the module map. It
deliberately does **not** restate reference content. Start at [`CONTEXT.md`](CONTEXT.md), the
documentation index, and read [`docs/reference/glossary.md`](docs/reference/glossary.md) before
touching model code — the terms are not interchangeable with their synonyms (use **Node**, never
"Entry").

The shape in one paragraph: a **headless, filesystem-free core** (`confy-core`) owns the
document model and all editor state; every host — TUI, web, Tauri desktop/Android, VS Code —
drives it through one command channel (`Session::dispatch(Intent) -> SessionSnapshot`) and owns
only its own I/O and presentation. Three concrete backends (TOML via `taplo`, JSON/JSONC and a
YAML subset via hand-rolled lossless parsers) sit behind one `ConfigDocument` trait, all on
`rowan` green trees, all atomic-commit, so an untouched file round-trips byte-identically.

| Topic | Where it is specified |
|---|---|
| Vocabulary: Node/Root/Branch/Leaf/Scalar/Comment, key literal vs decoded key, read-only & opaque nodes, `DocFormat`, the `Value` tree, schema terms, KIND tags | [`docs/reference/glossary.md`](docs/reference/glossary.md) |
| Every `Mutation` variant's mechanics, insert/move legality, `e` block-edit scope, multiline-array layout, kind switch (`K`) rules | [`docs/reference/MUTATIONS.md`](docs/reference/MUTATIONS.md) |
| How nesting **scope** governs each editing behavior across the three backends; the inline-vs-`$EDITOR` boundary; the `ConfigDocument` facet layer | [`docs/reference/BEHAVIOR_MATRIX.md`](docs/reference/BEHAVIOR_MATRIX.md) |
| TUI rendering, editing, comments, navigation, filters, multi-select, clipboard, overlays | [`docs/reference/TUI.md`](docs/reference/TUI.md) |
| WASM FFI wire contract (`Intent`/`SessionSnapshot`/`ViewRow`), web-native architecture, touch UI, deployment | [`docs/reference/WEBUI.md`](docs/reference/WEBUI.md) |
| Header/toolbar button inventory, fold order, per-host trimming | [`docs/reference/CHROME.md`](docs/reference/CHROME.md) |
| Keyboard bindings and the deliberate TUI↔Web divergences | [`docs/reference/KEYMAP.md`](docs/reference/KEYMAP.md) |
| Notice/prompt/diagnostics message system, severity table, per-host channels | [`docs/reference/MESSAGES.md`](docs/reference/MESSAGES.md) |
| Row cursor/selection/clipboard state model and its modal lock | [`docs/reference/ROW_STATE_MODEL.md`](docs/reference/ROW_STATE_MODEL.md) |
| Desktop + Android shell: native menu, file I/O, recent files, the Android picker plugin | [`docs/reference/TAURI.md`](docs/reference/TAURI.md) |
| VS Code extension host and its `TextDocument` protocol | [`docs/reference/VSCODE.md`](docs/reference/VSCODE.md) |
| Distribution channels, triggers, current status per platform | [`docs/reference/RELEASES.md`](docs/reference/RELEASES.md) |
| Why the shape is what it is | [`docs/adr/README.md`](docs/adr/README.md) |

**Two invariants worth stating here, because breaking either is a review failure rather than a
doc lookup.** (1) `confy-core` is **filesystem-free at runtime** — no `fs`/`process`/`env`/
`tempfile`, no terminal deps; the sole constructor is `from_str`/`AnyDocument::from_str_as`, and
`crates/confy-core/tests/no_fs_gate.rs` enforces it. The host owns all file I/O
(`confy_tui::load_document` / `write_document`, which also handle the UTF-8 BOM and the atomic
temp-file rename). (2) Every mutation is **atomic and semantically validated before commit** —
edited on a `clone_for_update` copy, committed only on success, so a failure leaves the document
untouched.

## Known Risks

**`taplo` is unmaintained upstream.** The maintainer stepped down in Dec 2024
([tamasfe/taplo#715](https://github.com/tamasfe/taplo/issues/715)); the repo is stalled but
not archived, no ownership transfer has happened, and `rowan =0.15.18` is exact-pinned to
match taplo's internal version. `confy`'s taplo surface is small and measurable —
`taplo::parser::parse` (49 call sites), `taplo::syntax::*`/`taplo::rowan::*` (28 sites), and
`taplo::dom` (2 sites: `into_dom()` + matching `taplo::dom::Error::ConflictingKeys` in
`cst_edit/mod.rs`'s `validate_dom`, the TOML backend's post-splice duplicate-key backstop —
JSON and YAML hand-roll the equivalent in their own `validate_semantics`). None of taplo's
1,330-line formatter is used, and of its 2,800-line DOM only `Node::validate` is.
Vendoring the used surface
(`parser/mod.rs` + `parser/macros.rs` + `syntax.rs`) is estimated at ~1,240 LOC and would
also unpin `rowan` and drop `globset`/`schemars`/`arc-swap`/`itertools`/`once_cell` from the
dependency tree — the one `dom::Node::validate` call would have to be replaced by the
hand-rolled duplicate-key check the other two backends already have, so it does not widen the
vendoring scope. Estimate last measured 2026-09-09; recount before acting on it.
`tombi`, the community's suggested migration target, is **not** currently a
usable dependency (its crates.io entry is a reserved placeholder; sub-crates unpublished).
**Decision: do not migrate now.** The `cargo audit` CI step (`.github/workflows/rust-ci.yml`)
is the trigger — if it flags a `rowan`/`taplo`/`ahash` advisory, vendoring per the above scope
estimate is the pre-planned contingency.

## Module map

Cargo **workspace** (the extraction design record is
[`docs/spec/2026-06-17-headless-core-port.md`](docs/spec/2026-06-17-headless-core-port.md)):
`confy-core` is the headless model crate; `confy-tui`
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
    blank_lines.rs the one format-neutral trailing-blank-run text splice (count_after/splice),
                   shared by all three backends' SetTrailingBlankLines, plus the
                   with_trailing_run/split_trailing_run pair that packages a node's run into
                   the multiline editor's buffer and splits it back off on commit
    kind_label.rs  align_options: the one `"<name>  <sample>"` picker-label format, name column
                   padded in display cells (unicode-width, so a translated CJK name still lines
                   up). Used by all three backends' kind_options AND the datetime type picker
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
      edit/        rowan splice helpers, split by construct (F6, 2026-09-09) — mod.rs
                   (atomic dispatch + validate_semantics), resolve.rs (path→Target +
                   fragment serialization), fragment.rs (fragment parsing/adaptation +
                   trailing-comment extraction), container.rs (destination OBJECT/ARRAY
                   lookup, item read-back, inline/multiline rebuild + indent detection),
                   replace_delete.rs (Replace/Delete), insert.rs (Insert/Move),
                   mutations.rs (Rename/Remark/EditComment/InsertComment/
                   SetTrailingComment/SetTrailingBlankLines + extent helpers),
                   convert.rs (ConvertKind: Inline↔Multiline, float Plain↔Exponent)
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
    datetime.rs    TOML datetime component surgery for the `K` datetime switch (ADR 0012):
                   parse_toml_datetime decomposes a literal (both separators, seconds-less
                   HH:MM, verbatim frac/offset so `.5` never becomes `.500`), retype re-renders
                   it as any of the four datetime types and reports each dropped/auto-filled
                   component. Fill policy: time → a fixed 00:00:00, offset → `Z`, and only an
                   absent date reads the (UTC) clock
    diag.rs        DiagLevel, DiagEvent (monotonic seq, kind, detail), DiagRing (bounded 256-event ring)
                   — see MESSAGES.md §4
    inline_edit.rs inline-editor buffer lifecycle (begin_inline_edit*/edit_*/edit_commit) +
                   value/rename/nudge/add-node mutation-application methods that commit through it
    schema_hint.rs nudge_scalar + format_nudged: the `←`/`→` value step (a schema `multipleOf`
                   becomes the step; bounds clamp inward to that grid), plus parse_repr /
                   format_nudged_like — the notation-aware decode/render the schema clamp
                   uses so a `0x`/`0o`/`0b` or `1_000`-grouped repr survives it
    undo_redo.rs   undo/redo
    status_fmt.rs  kind/type/format label formatting + small scalar-repr/string utilities;
                   `badge_label_note(kind, format, is_branch, scalar_type, doc)` is the one
                   source of every host's kind badge — a container's label is the outline
                   glyph, the notation goes in the note, and the `doc: DocFormat` exists so
                   YAML's `Format::Inline` reads `flow` while TOML/JSON read `inline`
                   (`datetime_note` gives the four TOML datetime types the note
                   `Format` can't: `date·odt`/`date·ldt`/`date·ldat`/`date·ltim`,
                   the TUI KIND column's own suffixes, and the `K` picker's rows
                   carry the bracketed `[D:…]` form as their second column just
                   as a table's carry `[T/D]`)
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
  schema/          JSON Schema detection/validation/constrained-editing: mod.rs (re-exports),
                   types.rs (SchemaSource/SchemaState/SchemaStatus/Violation/EditHint),
                   hints.rs (per-format hint
                   detection), value_bridge.rs (Node+Value → JSON projection with a Path per
                   node), validate.rs (`jsonschema`, draft 2020-12, ADR 0002), hints_edit.rs
                   (sub-schema resolution for the constrained-value picker + `schema_info`),
                   dirty_check.rs (per-mutation "does this path carry a constraint" skip)
crates/confy-core/tests/  19 integration suites + fixtures/. The gates named in the port design
                          record: no_fs_gate.rs (§7), serde_roundtrip.rs (§7 #3),
                          session_headless.rs (§7 #4 scripted Session tests, #5 fake-Host
                          `$EDITOR` flow, + dispatch() tests). Round-trip/byte-fidelity:
                          roundtrip.rs (incl. the multiline-array layout rules),
                          roundtrip_json.rs, roundtrip_yaml.rs, roundtrip_proptest.rs,
                          yaml_scratch.rs, key_repr.rs, hostile_input.rs (nesting cap),
                          insert_after_trailing_comment.rs,
                          external_edit_clears_trailing_comment.rs. Cross-format:
                          format_parity.rs (one behavior, three backends — 9 behaviors incl.
                          Remark's own-line rule and the `MutateError` variant taxonomy).
                          Session/schema/notice:
                          schema_headless.rs, session_schema_fetch_request.rs, session_notice.rs,
                          session_snapshot_notice.rs, prompt_question.rs, modal_lock.rs (every
                          guarded method no-ops + sets status while the clipboard is armed,
                          ADR 0005 §5).
                          Unit tests also live in-tree next to the code they cover:
                          model/cst_edit/tests.rs, model/json/edit/tests.rs,
                          model/yaml/edit/tests.rs (and
                          crates/confy-tui/src/tui/tests.rs for the TUI).

crates/confy-ffi/         Stage-2 WASM wrapper over confy-core (wasm-bindgen + serde-wasm-bindgen)
  src/lib.rs     ConfySession: from_text/dispatch/snapshot/serialize/visible_rows/kind_options
                 (the JS-facing handle; serde-wasm-bindgen marshals Intent/SessionSnapshot)
  functional_smoke.mjs     node verification of the Intent→snapshot contract (129 checks)
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
                 label+notation note+chevron, comment/trailing, hover ＋/⋮ actions). A
                 container's badge label is an **outline glyph** (`{}` every table/map
                 notation, `[]` every array/sequence one) with the notation in the note
                 (`{}·scope`, `[]·multi`, `[]·AoT`, YAML `·block`/`·flow`); scalars keep
                 short words (`str·"…"`, `int·0x`). The kind as a *word* lives only where
                 there is room for one — the badge's hover title (`ui.ts`) and the panel's
                 Kind field — via `kind-labels.ts`'s `kindWord`. `escapeAttr` for `data-path`
  highlight.ts   fuzzy-filter match marks: `highlightHtml(text, needle)` → escaped HTML with
                 `<mark class="fz">` runs (coalesced, char-indexed via `Array.from`). Web mirror
                 of the TUI's `highlight_spans`, driven by the SAME matcher — the wasm free export
                 `fuzzy_indices`, injected by confy.ts's `load()` via `setFuzzyMatcher` so
                 render.ts stays wasm-free and node-bundleable. Used by render.ts + touch/render.ts
  i18n.ts        catalog wrapper: t()/tArgs() over ../i18n/*.json, en-fallback chain,
                 getLang()/setLang() persisted in localStorage["confy-lang"]
  diag.ts        the `?diag=1` console drain, shared by BOTH orchestrators:
                 `drainDiagIfEnabled(session)` per render + `resetDiagCursor()` at each
                 host's session swap. One module because the copy that lived in ui.ts
                 made the trace desktop-only (F15)
  select.ts      pure pointer-selection logic → `SetSelection`/`SetCursor`: plain/⇧-range/
                 ⌘-toggle clicks (segmented additive range via an anchor+base snapshot) + marquee
  dnd.ts         HTML5 grip drag-reparent → `MoveSelectionTo {sources,slot,cut}`: the destination is
                 core's `pointerSlot(path,relY)` verbatim (`Into` outline / `After` `#dropLine`),
                 resolved by the same `slot_target` a keyboard Paste uses — no host-side
                 parent/index or band threshold (ADR 0010); self-subtree drop rejected
  slot-line.ts   the two shared rules for an insertion line's placement, used by the web
                 drag/armed cues and touch's `.reorder-line`. `slotLineIndentPx()` owns the
                 indent: `After(<expanded branch>)` inserts as its first child, so the line
                 sits one `--indent` step deeper (as the TUI draws it). `rootSlotLine()` owns
                 the **undrawn root row** — neither web host draws it, so its two slots borrow
                 a row edge: `After(root)` (document top) the first row's top edge,
                 `Into(root)` (append at the document end) the last row's bottom edge
  panel.ts       shared node detail/edit panel (`panelHTML`/`wirePanel`) — one module rendering
                 the desktop Detail aside AND the touch edit sheet identically (locked field order
                 Key/Value/Trailing comment/Kind/Path/Children/Sign/Blank after); a panel input's Enter/Escape
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
  path-utils.ts  shared path helpers; `drawnCursorFallback` re-targets a cursor sitting on the
                 undrawn root (neither web host draws it), used by ui.ts and touch/app.ts
  vscode-protocol.ts  the typed host↔webview message contract (`HostToWebview`/`WebviewToHost`),
                 the ONE file both `web/vscode.ts` and the extension import — see VSCODE.md
  vscode.ts      the in-webview VS Code client: posts/receives that protocol, tracks the
                 editor theme, and routes schema reads through the extension host
  escape.ts      the one HTML escaper (`escapeHtml`/`escapeAttr`) every render module uses
  kind-labels.ts shared `ViewRow` lookups/predicates (value-hue labels, row-anatomy helpers,
                 `kindWord` — the kind as a word for the two surfaces with room for one,
                 correcting core's notation-named `type_label` "inline" back to `table`)
  samples.ts     built-in demo doc + sample-mode state (shared backbone tree + per-format
                 showcase branch); `schema-sample.json` is its `$schema` target
  help-content.ts shared Help/About/KIND-legend body for the Help overlay (all web hosts)
  convert-dialog.ts shared Save/Convert dialog, rendered identically desktop + touch
  typefilter.ts  shared `f` type-filter facet grid (same markup/wiring on both hosts)
  fab.ts         shared floating "actions / paste" button (FAB) behavior + markup
  action-menu-items.ts / add-picker-items.ts  shared item rendering for `Mode::ActionMenu` /
                 `Mode::AddPicker`, so the desktop popup and the touch sheet stay identical
  entry-desktop.js / entry-touch.js / register-sw.js / sw.js  the per-entry boot scripts
                 (pointer-based desktop↔touch router; https-only service-worker registration)
                 plus the service worker itself (offline app-shell cache). **External
                 files, never inline `<script>`** — the Tauri shell's CSP forbids inline script;
                 new ones must be added to `assemble-dist.mjs`'s copy list (TAURI.md §CSP)
  touch/         the touch host: app.ts (orchestrator), render.ts (row tree), style.css —
                 a separate UI over the SAME Session, sharing the modules marked shared above
  index.html / touch.html / style.css (design `<style>` **verbatim** + a fenced app-only
                 appendix; dark+light
                 via :root[data-theme]; header/filter-row button layout — see CHROME.md) /
                 build.mjs (esbuild) / assemble-dist.mjs (the runtime-only web/dist file list,
                 run by build.mjs) / serve.mjs / cf-build.sh
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

editors/vscode/          third host shell, published to the VS Marketplace and Open VSX
                          (`wenanlin.confy-vscode`, versioned in lockstep with the app): a
                          `CustomTextEditorProvider` VS Code extension embedding `web/dist`
                          verbatim in a webview, over VS Code's own `TextDocument` (single source
                          of truth for content/dirty/undo/save/revert/hot-exit) via a shared
                          `web/vscode-protocol.ts` message contract — mechanics, the protocol
                          table, and the 0.2.1 tab-swap fix are in **`VSCODE.md`**.
                          `web/vscode-protocol.ts` is imported here as
                          `../../../web/vscode-protocol.js` (and by `web/vscode.ts` on the
                          webview side), so protocol drift is a compile error;
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

`confy-core` is pure and **filesystem-free at runtime** (see the invariants above); the host owns
all file I/O. `confy_tui::load_document(path, format)` reads the bytes, strips a leading UTF-8
BOM (remembered as `LoadedDocument::bom` — Windows tools write one routinely and no parser
accepts it as content), parses via `from_str_as`, and sets the path-derived display label.
`App::save` and `confy convert` write through `confy_tui::write_document`, which re-emits the
BOM and writes atomically (sibling temp file + rename, so a crash mid-write never truncates the
user's config). `detect_format(path)` is a pure extension match and stays in core.

## Terminology

See [`docs/reference/glossary.md`](docs/reference/glossary.md) for the canonical vocabulary.
Key rule: use **Node** (not "Entry"). Subtypes are **Root**, **Branch node**, **Leaf node**,
**Scalar**, and **Comment**. The operation that toggles a live Node to/from a Comment is
**Remark** (key `r`). Introducing a new term means adding its glossary entry in the same commit.
