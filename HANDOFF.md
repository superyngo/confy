# HANDOFF — confy headless-core port

Compact context-recovery note. Full design: **`PORTING.md`**. This file is the "where we are /
what's next" pointer; delete or rewrite it when the port is done.

## Where we are (2026-06-17)

- Branch **`port/slice-2-fs-boundary`** (off `port/slice-1-workspace-split`). Tree clean. Not pushed.
- **Slice 1 DONE:** PORTING.md §1 (workspace split) + §2 **A1** (`from_str`) + **A3** (tempfile-free
  conversion reparse-net).
- **Slice 2 DONE:** PORTING.md §2 **A2/A4/A5** + the §7 gate. `confy-core` is now **fully
  filesystem-free at runtime**: no `load`/`save`, no `path` field, no `tempfile` dep. `from_str` /
  `from_str_as` are the sole constructors; the host owns I/O via `confy_tui::load_document` (read →
  `from_str_as` → `set_filename` → `.jsonc` enable) and `App::save` (serialize → `fs::write` to
  `App::source_path`). Enforced by `crates/confy-core/tests/no_fs_gate.rs`. 415 core-unit + 190 tui
  + 26 integration tests pass; clippy `-D warnings` + `fmt --check` clean; real-binary `confy convert`
  smoke-tested.
- Layout: `crates/confy-core/` (pure model) + `crates/confy-tui/` (ratatui TUI + CLI, binary `confy`).
  `confy-tui/src/lib.rs` does `pub use confy_core::model;` so UI modules keep `crate::model::…` paths.

## Next task: PORTING.md §3 (cursor → `Path` reshape) and §5 (state-machine lift)

The fs boundary is fully severed. The remaining work is the larger §3/§5 reshape — inverting
`App.cursor: usize` (row index) to a `Path`-based selection, then lifting the `app.rs` state machine
into `confy-core` behind a `Host` capability for the `$EDITOR`/multi-line path. See `PORTING.md` §3–§6.
This is a big, invasive slice — scope it on its own.

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
