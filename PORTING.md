# PORTING.md — Headless Core extraction & multi-platform port

Design record for refactoring confy from a single TUI binary into a **Headless Core**
(`confy-core`) consumed by a TUI, a Tauri desktop app, a web app, and a VSCode extension —
all sharing one Web UI compiled against the core via WebAssembly.

This file is the durable companion to the doc set: `CONTEXT.md` (model glossary),
`BEHAVIOR_MATRIX.md` (nested behavior), `TUI.md` (ratatui mechanics). It records *what moves
where and why*; the eventual `WEBUI.md` will document the shared Web UI against this contract.

> **Status (2026-06-17).** Slices 1 **and 2** landed. Slice 1: the §1 workspace split
> (`confy-core` + `confy-tui`) and the §2 **A1** (`from_str` / `AnyDocument::from_str_as`) and
> **A3** (tempfile-free conversion reparse-net) fixes. Slice 2: §2 **A2/A4/A5** + the §7 gate —
> `confy-core` is now **fully filesystem-free at runtime**. `ConfigDocument::load`, every backend's
> `load`/`save`, and `AnyDocument::load_as`/`save` are gone; the `path` field is dropped (`filename`
> remains a host-set display label via `set_filename`). The host owns I/O: `confy_tui::load_document`
> (read → `from_str_as` → label → `.jsonc` enable) and `App::save` (`serialize` → `fs::write` to
> `App::source_path`). The §7 boundary gate (`crates/confy-core/tests/no_fs_gate.rs`) enforces it,
> and `tempfile` is no longer a `confy-core` dependency. The binary still builds as `confy`; the full
> suite passes. **Not yet done:** the §3 cursor reshape and the §5 state-machine lift are untouched —
> that is the next slice.

---

## 0. Decisions taken

- **The editor state machine is lifted into Rust core** (not reimplemented in TypeScript).
  Rust owns modes, cursor, selection, filters, clipboard, undo; the UI is a pure render of an
  emitted view model + a stream of intents back. This preserves the tested logic and gives a
  single source of truth (kills the dual-state drift risk between platforms).
- **Cursor and selection are identified by `Path`, not by row index.** See §3 — this is the
  pervasive reshape that the lift depends on.
- **Three concrete UI hosts, one core.** FS, terminal, and `$EDITOR` never enter the core; they
  are host capabilities (§4).

---

## 1. Target workspace layout

```
confy/                       (cargo workspace root)
  crates/
    confy-core/              pure state machine — no fs, process, env, crossterm, ratatui, tempfile
      model/                 existing src/model/ verbatim (after §2 fs fixes)
      session/               lifted from src/tui/ (see §5 map)
      host.rs                trait Host (the one mid-operation callback: edit_text)
    confy-tui/               ratatui render of the core's view model + Host impl + CLI
    confy-ffi/               wasm-bindgen wrapper over confy-core (Stage 2)
  apps/                      (Stage 2/3, JS monorepo)
    web/  desktop/ (Tauri)  vscode/   packages/ui  packages/core-wasm
```

Local path dependencies only; nothing published to crates.io. The standalone TUI binary must
keep building and passing the full existing test suite at every step (the Stage-1 exit gate).

---

## 2. FS boundary — sever these (the data layer is otherwise pure)

The entire `model/ → environment` surface. No process/env/stdio exists in `model/`; only these
file touches:

| # | Where | Fix |
|---|---|---|
| A1 | `ConfigDocument::load(path)` reads the file internally (`cst_doc.rs`, `json/doc.rs`, `yaml/doc.rs`) | Add `from_str(text, …)` as the primitive; `load` = host `fs::read` + `from_str`. The parser already re-parses from a string (`replace_from_str`), so this is mechanical. |
| A2 | `*Document::save()` → `std::fs::write(&self.path, …)` | Core exposes only `serialize() -> String`. The **host** owns the path and writes. |
| A3 | `convert.rs` reparse safety-net uses a `NamedTempFile` (no tempfile in WASM) | Reparse the rendered string via `from_str` instead of round-tripping through a temp file. |
| A4 | `self.path: PathBuf` field on each doc | Remove — document becomes a pure bytes-in → string-out value. |
| A5 | `AnyDocument::{save, mark_saved, replace_from_str, enable_comments}` (inherent, not on the trait) | Only `save` is an env op → host. The other three are pure → keep on the core session. |

CI gate for "headless": `confy-core` must contain **no** `std::fs`, `std::process`, `std::env`,
`tempfile`, `crossterm`, `ratatui`.

---

## 3. The identity reshape: row-index → Path

Today `App.cursor: usize` indexes `rows: Vec<RowSnapshot>` — the *rendered* visible list — and
`Selection` is a set of `usize`. Nearly every navigation/selection/edit/mutation function reads
`self.cursor` / `self.rows[cursor]`. A declarative UI re-renders from a projection, so an index
cursor cannot survive.

**Reshape:**
- Core holds `cursor: Path` and `selection: HashSet<Path>` (plus the existing `expanded: HashSet<Path>`).
- Core computes **`visible_rows() -> Vec<ViewRow>`** (tree × expanded × filter → ordered semantic rows).
- Index ↔ Path translation lives in the **UI** (`ViewRow` carries its `Path`; the UI maps a
  clicked/highlighted index back to a path when sending an intent).

This threads through ~50 functions. It is the bulk of the lift — pervasive, not algorithmically
hard. Everything in §5 assumes it is done first.

---

## 4. Host capabilities (never in core)

A small `Host` trait the TUI / Tauri / VSCode each implement. Only **one** call is needed
*mid-operation*; the rest are fire-and-forget the host performs around the core.

```rust
// confy-core/host.rs
pub trait Host {
    /// Open `initial` in an external/multi-line editor, return the edited text.
    /// TUI → $EDITOR; Web/VSCode → an in-app multi-line modal. Core issues the
    /// resulting Mutation::Replace. This is the BEHAVIOR_MATRIX §6 multi-line path.
    fn edit_text(&self, initial: String) -> EditTextOutcome; // sync (TUI) or future (web)
}
```

Host-owned, outside the trait: **file read/write** (load/save, the `C` convert write), the
**terminal** (crossterm raw mode, alternate screen, the event loop, width/height), and all
**viewport scroll** (`table_offset`, detail/help `u16` scroll, horizontal `clamp_scroll`,
page sizing from terminal height).

---

## 5. Portability map (per source file)

Destinations: **CORE** (→ `confy-core/session`), **HOST** (stays `confy-tui` + `Host`),
**SPLIT** (portable logic to core, presentation/viewport shell to host).

| File | Destination | Notes |
|---|---|---|
| `state.rs` | 🟢 CORE verbatim | `History` (string snapshots), `EditState`, `Mode`, `Clipboard`, `PasteSlot`, `PromptKind`, `cancel_last`. Already serializable. |
| `selection.rs` | 🟢 CORE | `normalize`, `toggle`, round begin/extend/commit/union. Re-key `usize` → `Path`. |
| `search.rs` | 🟢 CORE verbatim | `haystack`, `fuzzy_match`, `fuzzy_indices` (UI styles the returned positions). |
| `insertion.rs` | 🟡 SPLIT (mostly CORE) | `resolve_target` is pure §6.1 logic; take `(path, is_branch, expanded)` instead of `&RowSnapshot`. |
| `type_filter.rs` | 🟡 SPLIT | CORE: `classify`, `TypeFilter` predicate + popup *state* (`matches/toggle/group_state/cell_state/move_cursor/...`). HOST: `layout`, `nav_rows`, `LayoutRow`, `Cell` geometry, display labels — the Web UI lays out its own popup from the facet model. |

### `app.rs`

**CORE — state transitions:**
- Navigation: `cursor_down/up/home/end`, `toggle_expand`, `collapse_all`, `expand_all`,
  `expand_level`, `collapse_level`, `is_expanded`, `true_sibling_index`, `resting_mode`.
- Selection: `toggle_select`, `extend_select_up/down`, `selected_paths`, `cursor_is_read_only`.
- Filter: `enter/commit/exit_filter`, `exit_filter_results`, `filter_char/backspace/delete`,
  `filter_cursor_*`, `recompute_filter`.
- Type filter: `enter/commit/exit_type_filter`, `type_filter_move/toggle`.
- Kind switch: `open_kind_switch`, `kind_switch_move`, `kind_switch_commit`, `exit_kind_switch`.
- Inline edit: `begin_inline_edit`, `begin_inline_rename`, `edit_toggle_field`,
  `edit_input_char/backspace/delete`, `edit_cursor_*`, `edit_cancel`, `cancel_added_node`,
  `edit_commit`, `apply_deferred_rename`.
- Edit routing **decision**: `edit_target_kind`, `no_array_ancestor` (the inline-vs-external
  decision is core; the *spawn* is host).
- Mutations: `nudge`, `add_node`, `add_comment_sibling`, `delete_selected`, `copy_selected`,
  `cut_selected`, `paste`, `remark`, `do_remark`, `on_mutation_success` (minus row rebuild).
- Clipboard/paste: `paste_slots`, `effective_paste_slot`, `move_paste_slot`, `slot_target`.
- Convert orchestration: `open_convert`, `convert_move`, `convert_pick_format`,
  `convert_path_*`, `convert_run`, `convert_confirm`, `exit_convert`.
- Lifecycle: `undo`, `redo`, `escape`, `handle_prompt_key`, `confirm_quit`, `quit_requested`.
- Value math (free fns): `nudge_scalar`, `regroup_int/float`, `group_left/right`, `unique_key`,
  `char_byte_idx`, `project_first_label`, `node_type_label`, `branch_type_format`.

**HOST:**
- `save`, `convert_write` (fs); the `edit_node` spawn portion → `Host::edit_text`.
- `detail_scroll_by/set_scroll`, `help_scroll_by/set_scroll`, `enter/exit_help`,
  `toggle/exit_detail` viewport scroll; `page_up/page_down` (page size from terminal height);
  `edit_clamp_scroll` + free `clamp_scroll` (horizontal viewport). *(Mode enter/exit is core; the
  scroll offset is host.)*

**SPLIT — `rebuild_rows` / `RowSnapshot` / `type_tag`:**
- CORE: the flatten (tree × expanded × filter → ordered visible nodes) becomes `visible_rows()`.
- HOST: `RowSnapshot.type_tag` (fixed-pitch 12-char `(B) [S:str ]`), padded value column,
  `table_offset: Cell`, the `type_tag` free fn — ratatui presentation.
- `open_detail` splits the same way: build the detail text (CORE) vs. scroll offset (HOST).

**Approx. share:** ~70% CORE, ~20% SPLIT, ~10% HOST. Cost is dominated by the §3 index→path
inversion and the `rebuild_rows` split, not by the (small, isolated) host work.

---

## 6. Core session API sketch (Stage-1 contract)

```rust
// confy-core/session/session.rs
pub struct Session {
    doc: AnyDocument,              // owns the document (no path inside)
    cursor: Path,
    expanded: HashSet<Path>,
    selection: HashSet<Path>,
    mode: Mode,                    // Edit / Filter / KindSwitch / Convert / Prompt / …
    clipboard: Option<Clipboard>,
    filter: FilterState,
    type_filter: TypeFilter,
    history: History,             // serialized-string snapshots
    // … inline-edit / pending-commit state as today
}

impl Session {
    pub fn open(text: &str, format: DocFormat) -> anyhow::Result<Self>;   // §2 A1
    pub fn serialize(&self) -> String;                                    // host writes bytes
    pub fn is_dirty(&self) -> bool;

    /// Ordered, semantic visible rows — the view model both UIs render.
    pub fn visible_rows(&self) -> Vec<ViewRow>;
    pub fn detail(&self) -> Option<DetailView>;     // text only; host owns scroll
    pub fn kind_options(&self) -> Vec<(String, KindTarget)>;
    pub fn type_filter_facets(&self) -> TypeFilterView;   // facet model, no geometry

    /// One entry point: UI sends an Intent, core mutates state, returns what changed.
    pub fn dispatch(&mut self, intent: Intent, host: &dyn Host) -> Update;
}

/// What the UI tells the core happened. (Key→Intent mapping lives in each UI.)
pub enum Intent {
    CursorUp, CursorDown, CursorHome, CursorEnd,
    ToggleExpand, ExpandLevel, CollapseLevel, CollapseAll, ExpandAll,
    Select, ExtendUp, ExtendDown,
    BeginEdit, BeginRename, EditKey(EditKey), CommitEdit, CancelEdit,
    Nudge(i64), Add, Delete, Copy, Cut, Paste,
    Remark, Undo, Redo, Escape,
    OpenKindSwitch, KindSwitchMove(i32), KindSwitchCommit,
    EnterFilter, FilterKey(EditKey), CommitFilter,
    EnterTypeFilter, TypeFilterMove(i32, i32), TypeFilterToggle, CommitTypeFilter,
    OpenConvert, ConvertKey(EditKey), ConvertRun, ConvertConfirm,
    SetCursorPath(Path),   // UI translated a click/index → path (the §3 bridge)
    PromptKey(char),
}

/// What changed, so the UI can re-render minimally (R2: diff-not-snapshot).
pub struct Update {
    pub rows_dirty: bool,          // visible_rows() should be re-pulled
    pub status: Option<String>,
    pub error: Option<String>,
    pub quit: bool,
    pub external_edit: Option<String>,   // host should call Host::edit_text, then re-dispatch
}

pub struct ViewRow {
    pub path: Path,
    pub depth: usize,
    pub is_branch: bool,
    pub key: String,
    pub value: Option<String>,
    pub scalar_type: Option<ScalarType>,
    pub format: Format,
    pub trailing_comment: Option<String>,
    pub read_only: bool,
    pub selected: bool,
    pub is_cursor: bool,
}
```

The TUI's `tui/mod.rs` event loop becomes a thin **key → Intent** translator + a `Host` impl +
ratatui rendering of `visible_rows()`. The Web UI does the same in TypeScript over the WASM
boundary. No editor logic lives in either UI.

---

## 7. Stage-1 exit gates (verifiable)

1. **Dependency assertion:** the §2 CI grep over `confy-core` is clean.
2. **Parity:** the full pre-refactor suite (`roundtrip.rs`, `convert_cli.rs`, projection/golden,
   `app.rs` behavior tests) passes against the TUI-on-`Session` build; round-trips stay byte-identical.
3. **Serde round-trip:** `ViewRow`, `Intent`, `Mutation`, `Update` survive `serde_json` round-trip
   in a native test (rehearses the WASM contract before WASM exists).
4. **State-machine parity:** a scripted `Intent` sequence (navigation + selection + edit + undo)
   driven through a headless `Session` asserts the resulting `visible_rows()` — proving the
   machine survived the lift out of `app.rs`.
5. **Fake `Host`:** the `$EDITOR` path is exercised via a fake `Host::edit_text` (no real spawn),
   proving the multi-line edit flow is host-agnostic.

---

## 8. Open items for the next session

- Confirm `Node`/`NodeTree`/`ScalarType`/`Format` derive (or can derive) `Serialize` for `ViewRow`.
- Decide `Host::edit_text` shape: sync for TUI vs. `async`/future for web — likely an associated
  outcome type so the core stays runtime-agnostic.
- Decide whether `Update` carries a structured row **diff** (R2) now or starts with `rows_dirty`
  + full re-pull and adds diffing at Gateway G2 once latency is measured.
