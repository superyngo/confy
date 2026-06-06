# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- CI — `.github/workflows/release.yml`: on a `v*.*.*` tag, cross-compiles `confy` for Linux x86_64 (gnu + musl), macOS (arm64 + Intel), and Windows x86_64 + i686 (MSVC), packages tar.gz (Unix) / `.exe` (Windows), emits `SHA256SUMS`, and publishes a GitHub Release (annotated-tag message + auto-generated notes).

## [v0.2.0] - 2026-06-06

Single-file TOML editor with a CST-projection architecture: tree navigation/selection/editing,
byte-identical round-trip preservation, undo/redo, fuzzy filter, an inline value editor, a
read-only scalar/branch Format axis, and a scrollable detail popup.

### Added
- Core MVP — single-file TOML editor on a CST projection (`toml_edit::DocumentMut` as the source of truth), tree navigation/selection, Insert/Delete/Replace/Move/Remark mutations, undo/redo, fuzzy filter, `$EDITOR` integration, byte-identical round-trip. (2026-05-27 … 05-28)
- wenv-style title bar + columnar tree — `confy — <file> ──── v<version>` header and a `NAME / TYPE / VALUE` ratatui `Table` so type and value align in fixed columns. (2026-06-03)
- Scalar `Format` attribute — read-only writing style derived during projection (integers: dec/hex/oct/bin; strings: basic/literal/multiline). `ScalarType::Datetime` split into the four TOML datetime types (offset/local-datetime/local-date/local-time).
- Inline editor (`Mode::Edit`) — `e` edits a plain scalar in place with a not-enforced type check (confirm-on-change prompt) and falls back to `$EDITOR` for nested arrays/tables; `E` forces `$EDITOR` on any node. Cursor shown by reverse-highlight (no glyph drift), `Home`/`End` support, horizontal scroll on overflow with a `⟨start–end/len⟩` hint, and semantic-error feedback in the status line.
- `←`/`→` value nudge — toggle a bool or step an integer/float by ±1, preserving base and decimal precision.
- `a` Add node — inserts `new_field = ""` below the cursor and opens the inline editor. (Clear-to-null / `Del`-clear intentionally omitted — TOML has no null.)
- `i` detail/info popup on any node — content-adaptive height (`[5, 80% of screen]`), scrollable (`↑`/`↓`/`j`/`k`, `PgUp`/`PgDn`, `Home`/`End`), shows the full wrapped value and a `Format` line for every node (branch detail splits Type vs. writing style, e.g. inline table → Type table / Format inline).

### Changed
- `n` (New node via `$EDITOR`) replaced by `a` (Add node, inline).
- `TYPE` tree column became `TYPE/FORMAT`; inline tables read `table/inline`, standard tables stay `table`.
- Inline-editor horizontal viewport is persistent state, clamped minimally per keystroke.

### Fixed
- Inline-commit semantic-check errors are shown in the status line instead of being hidden behind the edit-mode hint.
- Inline-edit viewport no longer pins the cursor to the right edge when moving left after reaching the end.
- Replace/Remark review fixes — preserve key position, canonical serialization (no double-space), nested/AoT comment round-trip.
