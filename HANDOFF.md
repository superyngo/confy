# HANDOFF — confy headless-core port

Compact context-recovery note. Full design: **`PORTING.md`**. This file is the "where we are /
what's next" pointer; delete or rewrite it when the port is done.

## Where we are (2026-06-17)

- Branch **`port/slice-1-workspace-split`**, commit `0e56a1a`. Tree clean. Not yet merged/pushed.
- **Slice 1 DONE:** PORTING.md §1 (workspace split) + §2 **A1** (`from_str`) + **A3** (tempfile-free
  conversion reparse-net). 415 core + 186 tui tests pass; clippy `-D warnings` + `fmt --check` clean.
- Layout: `crates/confy-core/` (pure model) + `crates/confy-tui/` (ratatui TUI + CLI, binary `confy`).
  `confy-tui/src/lib.rs` does `pub use confy_core::model;` so UI modules keep `crate::model::…` paths.

## Next task: PORTING.md §2 **A2 / A4 / A5** + the §7 no-fs gate

Goal: make `confy-core` contain **no runtime `std::fs`** (CI grep gate), by moving file I/O to the host.

- **A4:** drop the `path: PathBuf` field from `CstDocument` / `JsonDocument` / `YamlDocument`.
- **A2:** remove `save()` (the `std::fs::write(&self.path, …)`) from the three backends and
  `AnyDocument::save`. The host writes `doc.serialize()` itself. The TUI already knows the path
  (`App.source_path`), so `App::save` becomes "serialize → host writes path".
- **A5:** `load(path)` (the `fs::read`) leaves the core too — relocate the read to the host
  (confy-tui / cli) which then calls `AnyDocument::from_str_as(text, format)` (already exists).
  Keep `from_str` / `from_str_as` as the core entry points.
- **Gate (§7):** add a check (CI or a test) that greps `crates/confy-core/src` for `std::fs` /
  `std::process` / `std::env` / `tempfile` / `crossterm` / `ratatui` and fails on a hit.

### Expected blast radius (why this is its own slice)
- ~40 test call sites do `CstDocument::load(path)` / `YamlDocument::load(path)` / `AnyDocument::load`
  directly (unit tests in `cst_doc.rs`, `json/doc.rs`, `yaml/doc.rs`, `any_doc.rs`; integration
  `roundtrip*.rs`, `yaml_scratch.rs`). Each must switch to read-then-`from_str`, OR keep a
  test-only `load` helper. Decide: a `#[cfg(test)]` load shim in core vs. rewriting call sites.
- TUI: `App::save`, `convert_write`, `tui/mod.rs run()`, `cli.rs` load/convert paths.
- `convert.rs` already uses `from_str_as` (no fs there anymore) ✓.

## Gotchas / don't re-derive these

- `from_str` is named per PORTING.md and carries `#[allow(clippy::should_implement_trait)]` — a real
  `FromStr` impl is a poor fit (anyhow error; JSON derives `comments_enabled` from content).
- JSON `from_str` keys `comments_enabled` off **content** only; the `.jsonc`-extension enable lives
  in `load` (`doc.comments_enabled |= is_jsonc_ext`). Preserve this when relocating `load`.
- Cross-crate visibility: the split already surfaced `cst_edit::joinable_entry` (now `pub`). Removing
  `save`/`load` may surface more `pub(crate)` items the TUI used — widen to `pub` as they appear.
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
