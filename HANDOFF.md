# HANDOFF — confy headless-core port

Compact context-recovery note. Full design: **`PORTING.md`**. This file is the "where we are /
what's next" pointer; delete or rewrite it when the port is done.

## Where we are (2026-06-17)

- Branch **`port/slice-4-session-lift`** (off `port/slice-3-path-cursor`). Tree clean. Not pushed.
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
  not a `&RowSnapshot`. Touched methods carry `§5: CORE/HOST/SPLIT` seam comments.
- **Slice 4 DONE:** PORTING.md §5 Phases A–C. `confy-core/session/` now contains the complete
  `Session` struct with all CORE fields and every CORE operation. New types: `Intent` enum, `Host`
  trait, `Update` struct, `PendingCommit`, `EditKind`. `Session::visible_rows() -> Vec<ViewRow>` is
  a pure on-demand computation. `crates/confy-core/tests/session_headless.rs` (13 tests, §7 exit
  gate #4) passes across TOML/JSON/YAML. Full suite: 438 core-unit + 167 tui + 26 integration +
  13 session-headless. `App.rs` is **unchanged** (Phase D deferred to Slice 5).
- Layout: `crates/confy-core/` (pure model + session) + `crates/confy-tui/` (ratatui TUI + CLI,
  binary `confy`). `confy-tui/src/lib.rs` does `pub use confy_core::model;` so UI modules keep
  `crate::model::…` paths.

## Next task: Slice 5 — PORTING.md §5 Phase D + Phase E

**Phase D — thin App wrapper:**
- Rewrite `App` struct to hold `pub session: Session` + HOST-only fields (`rows`, `source_path`,
  `detail_scroll`, `help_scroll`, `table_offset`).
- All App public methods become 1-line wrappers delegating to `self.session.*`.
- Update ~70+ test field accesses from `app.field` to `app.session.field` (cursor, mode, selection,
  clipboard, doc, error, status, filter, expanded, etc.) — Rust doesn't forward field access
  through Deref, so every direct field touch in `app.rs` tests must be updated.
- Implement `App: Host` for the `$EDITOR` path.

**Phase E — serde + fake-Host tests:**
- Serde round-trip tests for `Intent`/`ViewRow`/`Update`/`Mutation` (§7 exit gate #3).
- Fake-Host `$EDITOR` path integration test (§7 exit gate #5).

## Gotchas / don't re-derive these

- `from_str` is named per PORTING.md and carries `#[allow(clippy::should_implement_trait)]` — a real
  `FromStr` impl is a poor fit (anyhow error; JSON derives `comments_enabled` from content).
- JSON `from_str` keys `comments_enabled` off **content** only; the `.jsonc`-extension enable lives
  in the host `load_document` (`doc.enable_comments()` for a `.jsonc` path). Preserve this if the host
  load path moves.
- The §7 gate (`no_fs_gate.rs`) scans each core `src/**.rs` file **up to its `#[cfg(test)]` module**
  (the one-trailing-test-module-per-file convention), so unit tests may still read fixtures while
  runtime code stays fs-free. New core files must follow that convention or the gate won't skip tests.
- Cross-crate visibility: the split already surfaced `cst_edit::joinable_entry` (now `pub`). Further
  lifts may surface more `pub(crate)` items the TUI used — widen to `pub` as they appear.
- **Never drive the TUI via pty / long-lived background bash** (it needs a terminal); the user tests
  the TUI manually. Verify the *binary* via the `confy convert` subcommand and the `convert_cli`
  integration test, and the *model* via unit tests.
- `Session::convert_pick_format` takes `default_stem: Option<String>` (not `source_path`) because
  Session is fs-free; the host passes the stem derived from `source_path`.
- `Session::convert_run` / `convert_confirm` return `Update` with `convert_write: Some((path, text))`
  — the host performs the actual `fs::write`, not Session.
- `handle_prompt_key` returns `bool` (`true` = quit) — the host event loop exits when `true`.
- `git mv` keeps history as renames — use it for any further moves.

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
