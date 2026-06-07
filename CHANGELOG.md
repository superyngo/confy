# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- Editing — `e` on a **single-line comment** now edits inline (the raw `#`-prefixed text as the sole field, no `Tab`/name, committed via `Mutation::EditComment`) instead of opening `$EDITOR`. Merged multi-line comments and comments nested in an array-of-tables still open `$EDITOR`. (2026-06-07)
- Editing — `e` on a scalar **member of an inline table** (`pt = { x = 1 }`) now edits inline instead of opening `$EDITOR`; `Tab`→Name renames the key in place (`Mutation::Rename` now handles inline-table keys, preserving order and the other members). (2026-06-07)
- Editing — opening `$EDITOR` on a **structured** node (table/inline table/array/array-of-tables) now carries its adjacent leading comment(s) into the editor, and edits to that comment round-trip on save. Previously only `[table]` headers carried their comment; arrays did not. Scalars (including multiline strings) never carry comments. (2026-06-07)
- Editing — `e` on a multiline string now opens `$EDITOR` instead of the single-line inline editor, matching the existing behavior for nested arrays/tables. Single-line scalars still edit inline. (2026-06-06)
- Editing — `e` on a scalar **element of an array** now edits it inline (was: opened an empty `$EDITOR` and failed to save). Write-back goes through `Replace` on the trailing `Index` path via `Array::replace`, preserving the other elements and their formats. Non-scalar array elements still open `$EDITOR`. (2026-06-06)
- Editing — `←/→` value-nudge now also works on a scalar array element (toggle bool / step int/float in place). (2026-06-06)
- Editing — the value-nudge now re-applies underscore digit grouping when the original value had it (decimal every 3, hex/oct/bin every 4, float fractional every 3), so `1_000_000` stays grouped after a step. (2026-06-06)

### Added
- Editing — the inline editor now supports `Del` (forward-delete the char at the caret, alongside `Backspace`). (2026-06-07)
- Filter — `/` filter is now a full inline text field: a reverse-highlighted caret, `←/→/Home/End` to move it, and `Backspace`/`Del` to edit at the caret (was: append/pop only at the end). (2026-06-07)
- Comments — adjacent comment lines now project as a single multi-line comment node (a blank line, or any non-`#` line, breaks the group), so a comment block is one navigable node. Comment nodes now carry their text as a value, shown in the VALUE column and the detail popup. (2026-06-07)
- Editing — `e`/`E` on a comment now opens `$EDITOR` with the comment's raw `#`-prefixed text and writes the edit back into the decor via a new `Mutation::EditComment` (was: opened an empty editor and could not save). Edited text must remain comment lines, else the document is left untouched. (2026-06-07)
- Editing — `a` on an array now inserts a new element (seeded `""`) and opens it for inline editing, instead of failing with a key-collision/`NotFound`. (2026-06-06)
- Editing — `Tab` in the inline editor toggles between the Value (default) and Name fields; committing a changed Name renames the key via a new position/decor-preserving `Mutation::Rename`. `Tab` is disabled for array elements (no key), and the NAME field gets the same horizontal-overflow scrolling as VALUE. (2026-06-06)
- Editing — scalar elements of nested arrays (array-of-arrays, `Key Index Index…`) now edit inline and nudge in place, addressed via `array_at_mut`. (2026-06-06)

### Fixed
- Editing — deleting a standalone comment node no longer fails with `delete error: path not found`; `Delete` now strips the comment from its decor slot (like `uncomment`) instead of trying to remove a non-existent `#comment:N` table key. (2026-06-07)
- TUI — multi-line cell values (merged comments, multiline strings, and elements of a multiline-formatted array) now render a single-line preview (first line + ` …`) in the VALUE column. Previously a multiline-array element showed nothing because its repr carries leading newline+indent decor; the full text remains available in the detail popup. (2026-06-07)
- TUI — the main tree viewport now persists its scroll offset across frames, so the cursor moves within the visible window instead of staying pinned to the bottom edge and scrolling on every key. (2026-06-06)
- Editing — replacing a value no longer drops a standalone `#` comment sitting above its key; `Replace`/`Insert` overwrite now updates the value in place (preserving key decor) instead of re-inserting the key. (2026-06-06)
- Editing — `e` on a node nested inside an array/AoT (or on an element of a multiline array) no longer opens an empty editor; it edits the nearest addressable container, and multiline-array string elements edit inline with their indentation preserved. (2026-06-06)
- Move — moving a node into a table no longer drops the leading comments and blank lines above it; the move now carries the key's `leaf_decor` (capturing `(Key, Item)` and re-inserting via `entry_format`) instead of re-serializing through a fresh document. Array destinations are unchanged. (2026-06-06)

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
