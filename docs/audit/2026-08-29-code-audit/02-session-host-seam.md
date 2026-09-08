# Session & Host Seam Audit

## Verdict
The core/host boundary established in Stage 1/2 refactoring and refined in ADR 0003/0005 is structurally sound: `confy-core` is genuinely filesystem-free, state transitions are centralized in Rust, and the Web/WASM host operates cleanly through `dispatch(Intent) -> SessionSnapshot`. However, the boundary suffers from localized leaks in the TUI event loop, redundant presentation reconstructions on the TypeScript side (notably KIND badges and notation formatters), bifurcated help/legend catalogs, and unindexed full-tree re-serialization across the WASM boundary on pure-navigation keystrokes. These issues can be resolved through incremental, non-breaking refinements to the `Intent` vocabulary, `ViewRow` projection payloads, and internal `Session` struct grouping without disturbing the atomic CST mutation model.

---

## Findings

### F1. `tui/app.rs` (4447 LOC) Line Count is 80% Unit Tests; Production Code Follows ADR 0003 with Specific Edit-Routing Leaks
- **What**: `crates/confy-tui/src/tui/app.rs` is 4447 lines, but lines 920–4447 (3528 lines, ~80%) are unit tests (`#[cfg(test)] mod tests`). The production code is only 846 lines of `impl App` (lines 79–865) plus 54 lines of `type_tag` (lines 866–919). Of those 846 lines:
  - ~600 lines are thin delegate wrappers calling `self.session.apply(Intent::...)` and optionally `self.rebuild_rows()` (e.g. `cursor_down` at `app.rs:191`, `nudge` at `app.rs:746`, `add_node` at `app.rs:750`).
  - ~100 lines are legitimate terminal host concerns: `source_path`, scroll offsets (`detail_scroll`, `help_scroll`, `table_offset` at `app.rs:21-28`), `$EDITOR` terminal raw mode suspension / process spawning (`app.rs:619-645`), `std::fs::write` save (`app.rs:807`), and config file persistence (`app.rs:543`).
  - ~150 lines are application logic that duplicates or bypasses core:
    1. `edit_node` (`app.rs:589-675`, 86 lines): manually re-implements comment vs value branching, `no_array_ancestor` check, `serialize_fragment`, `external_edit_path`, and `wrap_element` handling. Core already computes this exact resolution in `Session::begin_edit` / `dispatch.rs` and returns `ExternalEdit` for WASM (`crates/confy-core/src/session/view.rs:232-243`), but TUI hand-rolls the same orchestration.
    2. `convert_toggle_jsonc_ext` (`app.rs:385-402`, 18 lines): ad-hoc file extension manipulation on Tab for the Convert dialog.
    3. `lang_picker` state machine (`app.rs:505-562`, 58 lines): host-owned mini-mode with custom cursor navigation (`LangPickerState`) rather than leveraging `Session` modal modes.
- **Why it matters**: The 4447 LOC figure gives a misleading impression of a bloated host. In reality, `App` is a relatively thin facade. However, the duplicated `edit_node` logic is a concrete drift hazard: if a new node kind or comment rule is added to core's edit router, `app.rs`'s manual branching can fall out of sync with WASM's `ExternalEdit` pipeline.
- **Proposal**:
  1. Move the 3528 lines of tests in `crates/confy-tui/src/tui/app.rs` into `crates/confy-tui/tests/app_tests.rs` or a submodule `app/tests.rs` to reflect actual production code size (~900 LOC).
  2. Unify TUI's `$EDITOR` spawn with core's `Intent::BeginEdit`: have `App::edit_node` dispatch `Intent::BeginEdit`, inspect `ApplyOutcome::external_edit` (or `SessionSnapshot.external_edit`), spawn `$EDITOR` on `initial`, and dispatch `Intent::ApplyReplace` / `Intent::ApplyEditComment`. This deletes ~60 LOC of duplicate routing logic from `app.rs`.
- **Effort / Risk**: S effort, Low risk. Purely internal to `confy-tui`.
- **Verdict**: Recommend.

---

### F2. KIND Badge and Notation Formatting are Duplicated in TypeScript (`tui/app.rs` vs `web/kind-labels.ts`)
- **What**:
  - In `confy-core`: `crates/confy-core/src/session/type_filter.rs:46-120` defines `TypeToken` and `classify(kind, format, doc, read_only)`. `crates/confy-core/src/session/status_fmt.rs:8-34` defines `node_type_label` and `node_type_label_str`.
  - In `confy-tui`: `crates/confy-tui/src/tui/app.rs:874-919` maps `TypeToken` to an 8-character fixed-pitch tag like `[S:str ]`, `[T/D]`, `[A/M]`.
  - In `web`: `web/kind-labels.ts:1-134` re-implements notation formatting from raw `ViewRow` fields (`type_label`, `format`, `scalar_type`, `is_branch`) via `NOTATION_SHORT`, `CONTAINER_NOTE`, `KIND_SHORT`, `notationGlyph()`, and `kindLabelParts()`, including fallback heuristics like `if (r.scalar_type === "Float" && r.format === "Plain") return "dec"` (`web/kind-labels.ts:46`).
- **Why it matters**:
  - ~134 lines of TypeScript in `web/kind-labels.ts` exist solely to reconstruct display tokens that core's `classify()` already knows.
  - Adding a new scalar format or container style (e.g. YAML folded block, TOML multiline literal) requires modifying both `crates/confy-core/src/session/type_filter.rs` and `web/kind-labels.ts`.
  - Any drift between Rust's `classify()` and TypeScript's `kindLabelParts()` causes visual discrepancy between TUI and Web UI.
- **Proposal**:
  - Add structured badge metadata to `ViewRow` (`crates/confy-core/src/session/view.rs:41-94`):
    ```rust
    pub struct ViewRow {
        // ... existing fields ...
        pub badge_label: String, // e.g. "table", "str", "int", "AoT"
        pub badge_note: Option<String>, // e.g. "scope", "dotted", "0x", "1e", "dec"
    }
    ```
  - Compute `badge_label` and `badge_note` in `Session::visible_rows()` using core's existing `classify()` and `status_fmt.rs`.
  - `web/render.ts:93-97`, `web/touch/render.ts`, and `web/panel.ts` can consume `r.badge_label` and `r.badge_note` directly, deprecating `web/kind-labels.ts`.
- **Effort / Risk**: S effort, Low risk. `ViewRow` already carries optional display fields (`path_display`, `key_sign`); adding pre-formatted badge parts simplifies all host renderers.
- **Verdict**: Recommend.

---

### F3. Modal Key-Handling State Machines are Duplicated Across TUI Event Loop and Web Key-Intent Resolver
- **What**:
  - In `crates/confy-tui/src/tui/mod.rs:140-450`: Modal keyboard interactions (Filter, TypeFilter, KindSwitch, SchemaEnum, Convert, Prompt, Help) are handled in separate mode-checking branches that manually translate keys into individual `app.*` or `session.*` calls.
  - In `web/key-intent.ts:35-120`: The exact same mode-precedence ladder (Edit -> Prompt -> Convert -> TypeFilter -> KindSwitch -> SchemaEnum -> Help) is hand-written in TypeScript, mapping DOM `KeyboardEvent` keys to `Intent` objects.
  - Example (`SchemaEnum`):
    - TUI (`mod.rs:385-404`): Up/Down (`session.schema_enum_move`), Home/End/PgUp/PgDn (`session.schema_enum_jump`), Enter (`session.schema_enum_commit`), Esc (`app.escape`).
    - Web (`key-intent.ts:98-112`): Up/Down (`SchemaEnumMove`), Home/End/PgUp/PgDn (`SchemaEnumJump`), Enter (`SchemaEnumCommit`), Esc (unhandled in enum branch, handled globally).
- **Why it matters**:
  - ~300 LOC in `tui/mod.rs` and ~90 LOC in `web/key-intent.ts` encode identical state-machine transitions for modal modes.
  - Fixes and enhancements to modal navigation (such as wrap-around rules, page size clamping, or Tab focus cycling) must be implemented and tested twice.
  - Several TUI modal handlers bypass `Intent::apply` entirely (e.g. `tui/mod.rs:386` calling `session.schema_enum_move` directly instead of dispatching `Intent::SchemaEnumMove`).
- **Proposal**:
  - Make `Intent` variants the universal language for modal navigation on both platforms.
  - On TUI, `tui/keys.rs` should map raw crossterm `KeyEvent` into `KeyAction`/`Intent` in one pass given the active `Mode`, reducing `run_event_loop` to a single `app.session.apply(intent)` dispatch.
- **Effort / Risk**: M effort, Low risk. Keeps key definitions declarative and host-specific while unifying transition semantics.
- **Verdict**: Worth considering.

---

### F4. Help and KIND-Legend Content is Bifurcated Between Core i18n Catalogs and Web TypeScript Constants
- **What**:
  - In `crates/confy-tui/src/tui/keys.rs:75-87`: Keybinding help and format-specific KIND column legends are loaded via `tr(lang, "tui.help.toml" | "tui.help.json" | "tui.help.yaml")` from `i18n/en.json` (lines 65-80) and `i18n/zh-TW.json`.
  - In `web/help-content.ts`: Keybinding help (`HELP_TEXT`, `HELP_TEXT_ZH_TW`, `HELP_TEXT_VSCODE`, `HELP_TEXT_VSCODE_ZH_TW`, lines 4-98) and per-format KIND legends (`KIND_LEGEND`, `KIND_LEGEND_ZH_TW`, lines 160-258, ~100 LOC) are hardcoded as multiline TypeScript string constants.
  - In contrast, `about_text` was already unified: `crates/confy-core/src/session/state.rs:about_text()` is the single source of truth, exposed via `ConfySession.about_text()` to both TUI (`app.rs:486`) and Web (`help-content.ts:133`).
- **Why it matters**:
  - ~180 lines of duplicate documentation and translations in `web/help-content.ts`.
  - Updating backend descriptions or translating to a new language requires editing both `i18n/*.json` and `web/help-content.ts`.
  - TUI help text and Web help text explain the exact same KIND tokens (e.g. `table·scope`, `inline`, `[A/I]`, `[T/D]`, `int·0x`), but are formatted and stored in completely separate systems.
- **Proposal**:
  - Move `KIND_LEGEND` strings into `i18n/en.json` and `i18n/zh-TW.json` under keys `help.legend.toml`, `help.legend.json`, `help.legend.yaml`.
  - Since `web/i18n.ts` already imports `i18n/en.json` and `i18n/zh-TW.json` directly (`web/i18n.ts:7-13`), `web/help-content.ts` can retrieve the localized legend via `t("help.legend." + docFormat)` instead of maintaining separate TS constants.
- **Effort / Risk**: S effort, Low risk. Leverages existing JSON import pipeline in `esbuild`.
- **Verdict**: Recommend.

---

### F5. `Session` Has 26 Fields, but Cohesion is Real; Grouping Sub-States is Preferable to an Object Split
- **What**:
  - `crates/confy-core/src/session/session.rs:20-56` defines `pub struct Session` with 26 fields.
  - Methods on `Session` are spread across:
    - `session.rs` (2314 lines, ~80 `pub fn` methods)
    - `clipboard.rs` (609 lines, ~20 `pub fn` methods)
    - `inline_edit.rs` (1125 lines, ~35 `pub fn` methods)
    - `undo_redo.rs` (116 lines, ~5 `pub fn` methods)
    - `dispatch.rs` (605 lines, `apply` and `dispatch`)
- **Why it matters**:
  - At first glance, 26 fields and ~140 methods look like a "god object" anti-pattern.
  - However, analysis of method usage demonstrates high transactional cohesion: almost every mutating operation requires simultaneous mutable access to `doc` (lossless CST), `tree` (projected syntax graph), `history` (undo ring), `expanded` (visibility map), `cursor` (focus path), `mode` (modal state), and `notice` (diagnostic output).
  - Attempting to split `Session` into separate independent structs (e.g. `DocumentManager`, `ViewportState`, `SelectionManager`) would introduce severe borrow checker friction or require ubiquitous `Rc<RefCell<...>>` / index passing.
- **Proposal**:
  - **Do not split `Session` into separate objects.**
  - Perform an internal structural grouping of flat fields on `Session` into cohesive sub-structs:
    1. `FilterState`: groups `filter`, `filter_cursor`, `last_filter`, `filtered_paths`, `type_filter`, `last_filter_applied` (6 fields -> 1 field).
    2. `PendingEditContext`: groups `pending_edit`, `pending_trailing`, `pending_external_edit`, `prompt_from_commit_edit` (4 fields -> 1 field).
  - This reduces `Session` field count from 26 to 18 while preserving the single-struct atomic transaction boundary.
- **Effort / Risk**: S effort, Low risk. Internal to `confy-core`.
- **Verdict**: Worth considering.

---

### F6. Incomplete `Intent` Coverage in TUI Causes State Leaks and Hinders Record/Replay
- **What**:
  - The WASM host (`crates/confy-ffi/src/lib.rs:56-60`) exclusively drives mutations through `ConfySession::dispatch(Intent)`.
  - The TUI host has multiple direct mutation leaks bypassing `Intent::apply`:
    - `crates/confy-tui/src/tui/mod.rs:280`: `app.session.toggle_help_tab()` (should be `Intent::ToggleHelpTab`)
    - `crates/confy-tui/src/tui/mod.rs:386-403`: `app.session.schema_enum_move`, `schema_enum_jump`, `schema_enum_commit` (should be `Intent::SchemaEnumMove/Jump/Commit`)
    - `crates/confy-tui/src/tui/mod.rs:462`: `app.session.last_action_was_shift_select = false;` (direct field mutation)
    - `crates/confy-tui/src/tui/mod.rs:477`: `app.session.paste_slot = Some(...)` (direct field mutation)
    - `crates/confy-tui/src/tui/app.rs:420, 508`: direct assignment `self.session.mode = Mode::...` in `convert_write` and `open_lang_picker`
- **Why it matters**:
  - Direct mutations bypass the `ApplyOutcome` event log and the diagnostic ring (`Session.diag`).
  - Making `Intent` 100% exhaustive across all hosts enables deterministic session record-and-replay: any user session (TUI or Web) can be recorded as a `Vec<Intent>` stream and replayed headlessly for automated bug reproduction, golden test generation, or macro scripting.
- **Proposal**:
  - Complete the Intent routing initiated in ADR 0003: route `ToggleHelpTab`, `SchemaEnumMove`, `SchemaEnumJump`, `SchemaEnumCommit`, and language/paste-slot updates through `Session::apply(Intent)`.
  - Prohibit direct `pub` mutation of `session.mode`, `session.paste_slot`, and `session.last_action_was_shift_select` from outside `confy-core`.
- **Effort / Risk**: S effort, Low risk. All necessary `Intent` variants already exist in `crates/confy-core/src/session/intent.rs`.
- **Verdict**: Recommend.

---

### F7. WASM Full-State Transport Recomputes and Serializes Full `ViewRow` Array on Pure Navigation Keystrokes
- **What**:
  - In `crates/confy-ffi/src/lib.rs:56-60`, `ConfySession::dispatch` calls `Session::dispatch(intent)`, which unconditionally calls `compute_rows()` (`crates/confy-core/src/session/dispatch.rs:596-604`), creating a full `Vec<ViewRow>` of all visible rows and packing it into `SessionSnapshot`.
  - Every arrow key press (e.g. `CursorDown`, `CursorUp`, `Nudge`) serializes the full snapshot via `serde_wasm_bindgen::to_value(&snap)` into JavaScript.
  - In `web/ui.ts:355-385`, the Web UI receives the full snapshot and re-renders all rows in the DOM.
- **Why it matters**:
  - For small config files (<100 rows), this is unnoticeable (<2ms). For larger files (500–2000 visible rows), allocating, serializing across the WASM boundary, and reconstructing hundreds of JS objects per arrow key press generates avoidable memory churn.
  - In the TUI, ADR 0003 specifically split `dispatch` into `apply` + `rebuild_rows` so pure navigation (`CursorDown`/`Up`/`Home`/`End`) could skip row reconstruction.
  - In WASM, PORTING §8.3 deferred structured diffs ("full-state transport, no diff (for now)"), noting `rows_dirty` as the natural hook.
- **Proposal**:
  - Add an incremental snapshot mode or lightweight cursor update channel to `confy-ffi`:
    ```rust
    pub struct NavigationUpdate {
        pub cursor: Path,
        pub cursor_index: Option<usize>,
        pub notice: Option<Notice>,
    }
    ```
  - For pure cursor movement (`CursorDown`, `CursorUp`, `CursorHome`, `CursorEnd`), `dispatch_nav` can return just the cursor path, allowing `web/ui.ts` to toggle CSS class `.cursor` on the affected `<div class="row">` elements in O(1) time without re-rendering the DOM tree or serializing all `ViewRow`s.
- **Effort / Risk**: M effort, Low risk. Additive API; full `dispatch` remains available for structural mutations.
- **Verdict**: Worth considering.

---

## Left Alone Deliberately

- **Lossless CST as Single Source of Truth with Projection on Mutation**:
  - Invariant: The lossless CST is the single source of truth; `NodeTree` is a derived projection rebuilt on mutation (`CLAUDE.md`, `docs/reference/glossary.md`).
  - While rebuilding `NodeTree` after every edit has an O(N) cost, config files are bounded in size and the guarantees of byte-identical round-tripping and zero comment drift far outweigh the complexity of incremental CST patching.
- **Host Ownership of Filesystem I/O and Terminal/DOM Viewport Scrolling**:
  - `confy-core` remains strictly headless and filesystem-free (`docs/spec/2026-06-17-headless-core-port.md` §2, §4).
  - TUI terminal resizing, crossterm raw mode, and table vertical scroll offsets (`table_offset`, `detail_scroll`, `help_scroll`) belong in `confy-tui`.
  - DOM scroll clamping and File System Access API / download fallbacks belong in `web/fs.ts`.
- **Sync `$EDITOR` Process Spawning (TUI) vs Async Modal Promise (Web)**:
  - TUI synchronously halts the terminal event loop to spawn `$EDITOR` (`crates/confy-tui/src/tui/editor.rs`), which is the standard Unix terminal contract.
  - Web UI uses the async signal `SessionSnapshot.external_edit` (`docs/spec/2026-06-17-headless-core-port.md` §8.2) to open a Promise-based multi-line modal.
  - Keeping this divergence at the host boundary preserves platform-native ergonomics without forcing async runtimes into the TUI binary.
- **Single Translation JSON Source Bundled via `include_str!` and `import`**:
  - Both Rust (`crates/confy-core/src/session/i18n.rs:50-58`) and TypeScript (`web/i18n.ts:7-13`) read the identical repository-root JSON catalogs (`i18n/en.json`, `i18n/zh-TW.json`).
  - This design is already clean, type-safe, and avoids dual-catalog translation drift for core and UI strings.
