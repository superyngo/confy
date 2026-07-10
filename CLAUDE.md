# CLAUDE.md — confy developer guide

## Build & test commands

```bash
cargo build                   # compile
cargo test                    # unit + integration tests
cargo clippy -- -D warnings   # lint (must be clean before commit)
cargo fmt                     # format
cargo fmt --check             # check formatting without modifying
cargo run -- <file.toml>      # run against a TOML file
```

## Architecture

**Lossless CST.** `CstDocument` (`model/cst_doc.rs`) holds a `taplo` parse → `rowan` syntax tree
as the single source of truth. Comments, whitespace and newlines are real tokens with real
positions, so `serialize()` is plain token concatenation and an untouched file round-trips
byte-identically. The Node tree is a *projection* (`cst_project.rs`) rebuilt after every
mutation — it is never mutated directly. `apply` edits a `clone_for_update` copy of the tree and
commits only on success, so **every mutation is atomic** (failure leaves the document untouched).
Every successful mutation is also **semantically validated before commit** (`validate_semantics`:
taplo DOM validation — duplicate sections/keys reject as `Collision`, other semantic errors as
`Illegal`), a backstop for edits the targeted pre-checks can't see (e.g. a whole-document or block
`$EDITOR` rewrite introducing a duplicate `[a]`).

**JSON/JSONC backend.** `JsonDocument` (`model/json/`) is a second concrete `ConfigDocument`
built on a hand-rolled lossless lexer + recursive-descent parser that emits a `rowan` green tree
(the same `rowan` version taplo uses, pinned `=0.15.18`). Load, serialize, and apply are all
atomic-commit; a `validate_semantics` post-check (DOM re-parse for duplicate keys) mirrors the
TOML backstop. JSONC extends `.json` with `//` line comments — which project as first-class
Comment nodes (consecutive lines merge; a blank splits them) or `trailing_comment` — and `/* */`
block comments, which project as **read-only** Comment nodes (new `Node.read_only` flag:
displayed and copyable, but edit/delete/cut/remark reject them). A pure `.json` file whose first
remark is triggered prompts `Mode::Prompt(JsoncUpgrade)`; `y` flips `supports_comments()` true
and `//` is used thereafter (the file extension is never rewritten). Trailing commas are accepted
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
(YAML subset) (the original `toml_edit`-based `TomlDocument` was retired after reaching parity). The trait exposes `load`, `project`, `serialize`, `serialize_fragment`,
`serialize_fragment_relative`, `is_dirty`, `apply(Mutation)`, and three **format facets** —
`format() -> DocFormat`, `comment_prefix()`, `supports_comments()` — plus `kind_options(path)`,
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
as `tree_to_value(&self.project(), <fmt>)`. **Loss policy** (the documented lossy contract):
notation/style that the default render drops is collected as deduplicated **warnings** during the
walk (`style_note`: radix, string style, inline/flow, dotted, AoT, exponent); `analyze` adds the
target-specific rules — `null`→TOML and a YAML opaque node→any target **abort** (no output;
null paths listed), TOML datetime→JSON/YAML and non-finite floats→JSON **warn**. The three
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
haystack semantics (key/path + Comment text; value never matched), and highlight — TUI.md §*Filter*.

**Type filter.** TypeToken/classify popup, tristate groups, AND-intersection of text∩type,
FilterLayer peel — TUI.md §*Type filter*.

**Multi-select.** round/committed union and fresh-round folding — TUI.md §*Multi-select*.

**Clipboard / paste.** Scope-relative capture, paste-mode state machine (clipboard freezes
selection; `c`/`x` toggles; Esc peels), failure contract (`do_paste` restores on every
failure), and InsertComment/ArrayUpgrade paths — TUI.md §*Clipboard / paste*.

## Module map

Cargo **workspace** (see `PORTING.md`): `confy-core` is the headless model crate; `confy-tui`
is the ratatui TUI + CLI binary (`confy`) that depends on it and re-exports `model` so its UI
modules keep their `crate::model::…` paths. `confy-ffi` is the WASM wrapper (Web UI); `confy-tauri`
is the Tauri v2 desktop shell over that same web UI, adding only native file I/O.

```
crates/confy-core/src/   headless core — pure, no terminal/UI/`tempfile` runtime deps
  lib.rs           `pub mod model; pub mod session;`
  model/
    mod.rs         re-exports
    node.rs        Seg, ScalarType, Format, NodeKind, Node, NodeTree (+ node_at lookup)
    document.rs    ConfigDocument trait (+ to_value), DocFormat, Mutation, Target, OnCollision, ConvertAbort, errors
    value.rs       format-neutral Value/Item tree for conversion (has_null/has_datetime)
    convert.rs     document-level conversion: tree_to_value walk + per-format scalar decoders + default-style renderers + loss policy
    any_doc.rs     AnyDocument enum: per-format dispatch + detect_format/from_str_as/set_filename (TOML/JSON/YAML)
    cst_doc.rs     CstDocument holding the taplo/rowan tree: from_str (sole headless ctor) / serialize / apply (atomic commit) / set_filename
    cst_project.rs CST → NodeTree projection (comments as real nodes; golden tests)
    cst_edit.rs    rowan splice helpers: one fn per Mutation variant + the path→element walk index
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
      edit.rs      rowan splice helpers: reindent engine + one fn per Mutation variant; opaque guard
  session/         §5 state-machine lift (Slice 4) — the complete headless Session
    mod.rs         re-exports
    host.rs        Host trait (edit_text callback) + EditTextOutcome
    intent.rs      Intent enum — every key-mapped action the TUI can dispatch
    session.rs     Session struct (all CORE state + methods): visible_rows/compute_rows, navigation,
                   filter/type-filter, kind-switch, convert (no fs), edit routing, inline-edit,
                   mutations (apply_replace/insert/delete/copy/cut/paste/remark/undo/redo/nudge),
                   escape, prompt-key dispatch, quit flow; plus free fns: node_type_label,
                   format_label, nudge_scalar
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
crates/confy-core/tests/  roundtrip*.rs / yaml_scratch.rs + fixtures/ + no_fs_gate.rs (§7 gate)
                          + session_headless.rs (§7 gate #4: headless Session scripted tests;
                          §7 gate #5: fake-Host `$EDITOR` flow; + dispatch() tests) + serde_roundtrip.rs (§7 gate #3)

crates/confy-ffi/         Stage-2 WASM wrapper over confy-core (wasm-bindgen + serde-wasm-bindgen)
  src/lib.rs     ConfySession: from_text/dispatch/snapshot/serialize/visible_rows/kind_options
                 (the JS-facing handle; serde-wasm-bindgen marshals Intent/SessionSnapshot)
  functional_smoke.mjs     node verification of the Intent→snapshot contract (36 checks)
  (build: `wasm-pack build --target web`; getrandom wasm_js for the ahash-via-taplo chain)

web/                       TypeScript integration + **web-native** UI (see WEBUI.md) — a
                           pointer-first port of `design_index_model.html`, Session-driven
  types.ts       hand-written mirror of the confy-core serde contract (Intent/SessionSnapshot/…)
  confy.ts       typed wrapper around the wasm ConfySession (load + Session class; `kindOptions`)
  fs.ts          File System Access API open/save-in-place + download fallback + `fetchUrlFile`
                 (open a remote config; `?url=` deep-link & "Open from URL") — host-owned I/O
  render.ts      pure `SessionSnapshot → DOM` tree: web-native row anatomy (drag grip, rotating
                 caret, key/`—`/value value-type-colored, item count, **kind badge** =
                 label+notation suffix+chevron, comment/trailing, hover ＋/⋮ actions);
                 container & scalar notation suffixes, `escapeAttr` for `data-path`
  select.ts      pure pointer-selection logic → `SetSelection`/`SetCursor`: plain/⇧-range/
                 ⌘-toggle clicks (segmented additive range via an anchor+base snapshot) + marquee
  dnd.ts         HTML5 grip drag-reparent → `MoveSelectionTo`: into-branch vs before/after sibling
                 (`#dropLine`), self-subtree drop rejected
  panel.ts       shared node detail/edit panel (`panelHTML`/`wirePanel`) — one module rendering
                 the desktop Detail aside AND the touch edit sheet identically (locked field order
                 Key/Value/Trailing comment/Kind/Path/Children/Sign); a panel input's Enter/Escape
                 keydown `stopPropagation()`s so a synchronously-opened confirm prompt or the host's
                 global key handler doesn't re-read the same bubbling event
  prompt.ts      shared `Mode::Prompt` y/n(/o/r) answer buttons (`promptButtonsHTML`/
                 `promptQuestion`/`bindPromptClicks`) — desktop renders them in `#overlay`, touch in
                 a `.prompt-sheet`; both answer via the same `PromptKey` intent
  ui.ts          orchestrator: holds the latest snapshot, renders via render.ts + the modal
                 surfaces (detail aside, native search box, `#tfPop` type-filter grid, `#convDlg`
                 convert dialog, `#overlay` for Help/Prompt/KindSwitch only), Tree|Raw read-only
                 view toggle (`session.serialize()`), keyboard→Intent map (mirrors tui/keys.rs),
                 theme toggle, FS open/save, `#url-modal` Open-from-URL, external-edit modal,
                 paste-mode cursor target
  index.html / style.css (design `<style>` **verbatim** + a fenced app-only appendix; dark+light
                 via :root[data-theme]) / build.mjs (esbuild) / serve.mjs / cf-build.sh
                 (Cloudflare Workers Builds build command → runtime-only web/dist; deployed with
                 root `wrangler.toml` to confy.turkeyang.net — see WEBUI.md §Deployment)

crates/confy-tui/src/    ratatui TUI + CLI; depends on confy-core, `pub use confy_core::model`
  main.rs          bin `confy`: parse args, load via load_document, run TUI
  lib.rs           `pub use confy_core::model;` + `pub mod cli; pub mod tui;` + `load_document` (the host fs boundary: read → from_str_as → set_filename → .jsonc enable)
  cli.rs           clap args: default `confy <file> [--format]` (TUI) + `confy convert <in> <out>` subcommand
  tui/
    mod.rs         re-exports; run() entry point + event loop (run_event_loop)
    app.rs         App = thin Host wrapper: `pub session: Session` + 5 HOST-only fields
                   (rows/source_path/detail_scroll/help_scroll/table_offset); App::save = serialize → fs::write
    state.rs       thin re-export of confy_core::session::state
    keys.rs        KeyAction mapping + help text
    insertion.rs   thin re-export of confy_core::session::insertion
    selection.rs   thin re-export of confy_core::session::selection
    search.rs      thin re-export of confy_core::session::search
    type_filter.rs thin re-export of confy_core::session::type_filter
    editor.rs      $EDITOR integration (external edit for nested array/table)
    ui.rs          ratatui rendering: title bar + NAME/TYPE/VALUE column header + tree Table, detail popup, help, prompts
crates/confy-tui/tests/   convert_cli.rs integration: `confy convert` happy/lossy/abort paths, source-unchanged

crates/confy-tauri/       desktop app shell (Tauri v2) over the web UI — **native file I/O only**
  src/main.rs    bin `confy-desktop`: Tauri builder + dialog plugin + 5 `#[tauri::command]`s —
                 open_dialog / save_dialog / read_file_text / write_file / startup_file (CLI-arg open).
                 Editing stays in the in-webview wasm Session (dispatch is sync; not moved over IPC);
                 the Rust side owns only real open/save/read/write so the desktop gets native paths,
                 in-place save, and CLI-arg open — no download fallback.
  tauri.conf.json  frontendDist=../../web/dist, beforeBuildCommand=cf-build.sh (via git toplevel),
                   bundle targets ["dmg"], identifier net.turkeyang.confy
  tauri.windows.conf.json  Windows platform override (Tauri v2 auto-merge): empty
                   before-commands (bash/git rev-parse don't run under the Windows build
                   shell — build web/dist manually first) + bundle targets ["nsis"]
  capabilities/    default.json — core:default + dialog:default for the main window
  icons/           placeholder brand set (32/128/@2x png + icon.icns/.ico), regen via `cargo tauri icon`
```

**Desktop host I/O.** `web/fs.ts` detects Tauri (`window.__TAURI__`) and routes open/save through the
Rust commands instead of the browser File System Access API. The path string is the durable "handle",
wrapped in an object that **conforms to the existing `FsHandle` shape** (getFile/createWritable →
`invoke`), so `ui.ts` (writeFile/readHandle/deriveName/convert) is unchanged. `tauriStartupFile()`
opens a CLI-arg file at boot. Build a desktop bundle with `cargo tauri build` from `crates/confy-tauri`
(the workspace `[profile.release]` is aggressive — `lto`+`codegen-units=1`+`opt-level z` — so the
release bundle is slow; `--debug` is fast for local checks). macOS produces `.app`/`.dmg`; **Windows
must be built on a Windows host** (the webview is WebView2; no cross-build). Linux is not targeted yet.

`confy-core` is pure and **filesystem-free at runtime** — no TUI/terminal deps, no `fs`/`process`/
`env`/`tempfile`, fully unit-testable in isolation (enforced by `tests/no_fs_gate.rs`). The sole
constructor is `from_str(text)` / `AnyDocument::from_str_as(text, format)`; there is no `load`/`save`
and no `path` field (backends keep a host-set `filename` display label via `set_filename`). **The
host owns all file I/O:** `confy_tui::load_document(path, format)` reads the bytes, parses via
`from_str_as`, sets the path-derived label, and enables JSONC comments for a `.jsonc` extension;
`App::save` serializes and writes to `App::source_path`. `detect_format(path)` (pure extension
match, no I/O) stays in core. The next slice is the §3 cursor reshape / §5 state-machine lift — see
`PORTING.md`.

## Terminology

See **`CONTEXT.md`** for the canonical glossary. Key rule: use **Node** (not "Entry"). Subtypes
are **Root**, **Branch node**, **Leaf node**, **Scalar**, and **Comment**. The operation that
toggles a live Node to/from a Comment is **Remark** (key `r`).
