# HANDOFF — confy headless-core port

Compact context-recovery note. Full design: **`PORTING.md`**. This file is the "where we are /
what's next" pointer; delete or rewrite it when the port is done.

## Where we are (2026-06-18)

- Branch **`port/slice-4-session-lift`** (off `port/slice-3-path-cursor`). Tree clean. Not pushed.
  Latest commit: `afd1c6c` (Slice 5 Phase D).
- **Slice 1 DONE:** PORTING.md §1 (workspace split) + §2 **A1** (`from_str`) + **A3** (tempfile-free
  conversion reparse-net).
- **Slice 2 DONE:** PORTING.md §2 **A2/A4/A5** + the §7 gate. `confy-core` is now **fully
  filesystem-free at runtime**: no `load`/`save`, no `path` field, no `tempfile` dep. `from_str` /
  `from_str_as` are the sole constructors; the host owns I/O via `confy_tui::load_document` (read →
  `from_str_as` → `set_filename` → `.jsonc` enable) and `App::save` (serialize → `fs::write` to
  `App::source_path`). Enforced by `crates/confy-core/tests/no_fs_gate.rs`.
- **Slice 3 DONE:** PORTING.md §3 identity reshape. `App.cursor: Path` (was a row `usize`);
  `Selection`/`PasteSlot` re-keyed to `Path`; nav/selection/paste read `App::visible_paths()` +
  `cursor_row()` instead of indexing `rows`. The **only** index↔path bridge is `cursor_row_index()`
  (ratatui highlight/viewport + footer). `insertion::resolve_target` now takes `(path, is_branch, …)`,
  not a `&RowSnapshot`.
- **Slice 4 DONE:** PORTING.md §5 Phases A–C. `confy-core/session/` now contains the complete
  `Session` struct with all CORE fields and every CORE operation. New types: `Intent` enum, `Host`
  trait, `Update` struct, `PendingCommit`, `EditKind`. `Session::visible_rows() -> Vec<ViewRow>` is
  a pure on-demand computation. `crates/confy-core/tests/session_headless.rs` (13 tests, §7 exit
  gate #4) passes across TOML/JSON/YAML.
- **Slice 5 Phase D DONE:** `App` rewritten as thin Host wrapper. `App` holds `pub session: Session`
  + 5 HOST-only fields (`rows: Vec<RowSnapshot>`, `source_path`, `detail_scroll`, `help_scroll`,
  `table_offset`). Every CORE method is a 1-line delegate to `self.session.*`. `RowSnapshot` (HOST
  view model for ratatui) adds `type_label`/`type_tag`/`scalar_type` on top of `ViewRow`.
  `rebuild_rows()` calls `session.compute_rows()` then maps `ViewRow→RowSnapshot` by looking up
  `NodeKind` from `session.tree`. HOST-split methods (`edit_node`, `save`, `convert_write`) stay on
  `App` and do all filesystem I/O. All ~444 test field accesses updated (`app.cursor` →
  `app.session.cursor`, etc.). `selection.clear()` removed from `compute_rows()` (selection is
  path-keyed and survives structural changes). Free functions cleaned: removed `char_byte_idx`,
  `unique_key`, `project_first_label`; marked `clamp_scroll`/`nudge_scalar`/etc. `#[cfg(test)]`.
  Full suite: 438 core-unit + 167 tui + 26 integration + 13 session-headless; clippy/fmt clean.
- Layout: `crates/confy-core/` (pure model + session) + `crates/confy-tui/` (ratatui TUI + CLI,
  binary `confy`). `confy-tui/src/lib.rs` does `pub use confy_core::model;` so UI modules keep
  `crate::model::…` paths.

## Next task: Slice 5 Phase E — serde + fake-Host tests

**Phase E (§7 exit gates #3 and #5):**
- **Serde round-trip tests** for `Intent`/`ViewRow`/`Update`/`Mutation` (§7 exit gate #3). Add
  `#[derive(Serialize, Deserialize)]` to these types in `confy-core` and write round-trip tests
  that serialize → deserialize → assert equality. This is preparation for the WASM/web-UI port
  (serde for JS interop via `serde-wasm-bindgen` or similar). The types are in:
  - `Intent` enum — `crates/confy-core/src/session/intent.rs`
  - `ViewRow` struct — `crates/confy-core/src/session/view.rs`
  - `Update` struct — `crates/confy-core/src/session/view.rs`
  - `Mutation` enum — `crates/confy-core/src/model/document.rs`
- **Fake-Host `$EDITOR` integration test** (§7 exit gate #5). Write a test that uses a fake/mock
  `Host` implementation (implementing the `Host` trait's `edit_text` callback) to exercise the
  `$EDITOR`/multiline path headlessly — no real editor process spawned. Likely goes in
  `crates/confy-core/tests/` alongside `session_headless.rs`.

## Gotchas / don't re-derive these

- `from_str` is named per PORTING.md and carries `#[allow(clippy::should_implement_trait)]` — a real
  `FromStr` impl is a poor fit (anyhow error; JSON derives `comments_enabled` from content).
- JSON `from_str` keys `comments_enabled` off **content** only; the `.jsonc`-extension enable lives
  in the host `load_document` (`doc.enable_comments()` for a `.jsonc` path). Preserve this if the host
  load path moves.
- The §7 gate (`no_fs_gate.rs`) scans each core `src/**.rs` file **up to its `#[cfg(test)]` module**
  (the one-trailing-test-module-per-file convention), so unit tests may still read fixtures while
  runtime code stays fs-free. New core files must follow that convention or the gate won't skip tests.
  Adding `serde::{Serialize, Deserialize}` derives to core types is fine — `serde` is a no-op derive
  with no runtime fs/process/env use.
- Cross-crate visibility: the split already surfaced `cst_edit::joinable_entry` (now `pub`) and
  `Session::{slot_target, no_array_ancestor}` (now `pub`). Further lifts may surface more
  `pub(crate)` items the TUI used — widen to `pub` as they appear.
- **Never drive the TUI via pty / long-lived background bash** (it needs a terminal); the user tests
  the TUI manually. Verify the *binary* via the `confy convert` subcommand and the `convert_cli`
  integration test, and the *model* via unit tests.
- `Session::convert_pick_format` takes `default_stem: Option<String>` (not `source_path`) because
  Session is fs-free; the host passes the stem derived from `source_path`.
- `Session::convert_run` / `convert_confirm` return `Update` with `convert_write: Some((path, text))`
  — the host performs the actual `fs::write`, not Session.
- `handle_prompt_key` returns `bool` (`true` = quit) — the host event loop exits when `true`.
- `git mv` keeps history as renames — use it for any further moves.

## Phase D implementation specifics (reference for Phase E)

- **`App` struct:** `pub session: Session`, `pub rows: Vec<RowSnapshot>`, `pub source_path:
  Option<PathBuf>`, `pub detail_scroll: u16`, `pub help_scroll: u16`, `pub table_offset: Cell<usize>`.
- **`rebuild_rows()`:** calls `self.session.compute_rows()` (stateful — snaps cursor, clears
  paste_slot/selection), maps each `ViewRow` to `RowSnapshot` by looking up `NodeKind` from
  `self.session.tree.node_at(&vr.path)` for `type_tag`/`type_label`.
- **Delegates vs delegates+rebuild:** Methods that were calling `on_mutation_success` in the old
  code → need `self.rebuild_rows()` after delegating (filter methods, mutations, expand/collapse
  level, kind_switch_commit, edit_commit, escape, undo, redo). Simple navigation (cursor up/down,
  toggle_expand, etc.) → plain delegates.
- **`edit_node` is SPLIT:** App keeps it as a HOST method — calls `session.edit_target_kind()`,
  `session.no_array_ancestor()`, `session.external_edit_path()`, spawns `editor::edit_text()`, then
  calls `session.apply_replace()` or `session.apply_edit_comment()`.
- **`convert_write` resting mode:** Inlined — `if self.session.filtered_paths.is_some() {
  Mode::FilterResults } else { Mode::Normal }`.
- **`RowSnapshot` fields:** `key`, `path`, `depth`, `is_branch`, `value: Option<String>`,
  `scalar_type: Option<String>`, `type_label`, `type_tag`, `format: Format`,
  `trailing_comment: Option<String>`. Note: `scalar_type` and `format` are stored on the HOST
  `RowSnapshot`, NOT on the CORE `ViewRow` (which has `scalar_type: Option<ScalarType>` only).

## Verify (run before committing the next slice)

```bash
cargo test                                # all crates, all suites
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo run -q -- convert <in> <out> --yes  # real-binary smoke test
```

## Doc-update checklist after the slice (repo convention)

`CHANGELOG.md` `[Unreleased]`, the `PORTING.md` status banner, and `CLAUDE.md` if the module map
or the "no runtime fs" note changes.
