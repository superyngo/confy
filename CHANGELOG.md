# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Filter — `/` is now a three-state flow. Typing in the input filters live; **Enter** locks in the filtered set and enters a filtered-result selection mode (navigate/select/edit on the filtered nodes while the status bar shows `[filter: …]`); **Esc** clears the filter back to the full list; **`/`** reopens the input (prefilled) to refine. The last committed query is remembered, so `/` restores the previous search and its live results. (2026-06-07)
- TUI — the root/file node (`▾ test.toml`) is now collapsible like any branch (Enter/Space toggles `▾`/`▸`); it starts expanded. `0` (collapse all) keeps the file node open; an explicit toggle on its row hides the whole document. (2026-06-07)
- Filter — while a filter is active, the fuzzy-matched characters are highlighted (bold/underlined) in the NAME cell (`search::fuzzy_indices` + `ui::highlight_spans`), so it's clear why each row matched. The highlight persists through an inline edit or detail popup opened from the filtered list (gated on the active query, not the mode), and closing the editor/popup returns to the filtered-result selection (`App::resting_mode`) instead of dropping to plain Normal. (2026-06-07)
- Clipboard — pressing `c`/`x` while a clipboard is already loaded now **toggles** its mode (copy ↔ cut) instead of re-capturing the selection, so a mis-pressed `x` can be corrected to `c` without re-selecting. The status bar reflects the change. (2026-06-08)

### Changed
- Editing — opening a **scalar** in `$EDITOR` (`E`, or a multiline string) now carries its adjacent leading comment into the editor and writes edits/deletes to that comment back to the file, matching the existing behaviour for tables/arrays. Inline edits (commit, `←→` nudge, type-change confirm) are unaffected and never disturb the comment — `Mutation::Replace` gained a `sync_decor` flag that the `$EDITOR` path sets and the inline path clears. (2026-06-08)
- Filter — the fuzzy query now matches a node's **key/path** (plus a **Comment node's own text**, so comments stay searchable as standalone nodes), but **no longer a scalar's value** (and the synthetic `#comment:N` key is excluded). A loose query like `array` previously fuzzy-matched unrelated values (e.g. `…color = "gray"`) and the value+comment duplicate in the haystack dragged in unrelated section comments, which also made them look "scattered"; now `array` surfaces the `array_*` keys and the array-related comment only. (2026-06-07)
- Multi-select — each Shift+Arrow run now starts a fresh range anchored at the cursor and **unions** onto previous selections (separate runs stay separate, overlapping runs merge), instead of every new run extending from the first run's anchor. `Esc` in normal mode now clears the active selection. (2026-06-07)
- Clipboard — selection mode and clipboard mode are now cleanly separated. While a clipboard is active, `s` and Shift+Arrow are locked (no selection changes); the cursor row is shown green (paste-ready) and the copy/cut source rows are shown blue (distinct from the grey of multi-select). `Esc` peels back one layer at a time: if a selection was live when `c`/`x` was pressed, the first `Esc` clears the clipboard (keeping the selection) and a second `Esc` clears the selection. The earlier per-row "valid/invalid target" colouring (green/red cursor + dimmed rows) was removed as noise — an incompatible paste simply reports `paste error: …` in the status bar. (2026-06-08)

### Fixed
- Parsing — dotted table **headers** without an explicit parent (`[product_table2.a]` / `[product_table2.b]` with no `[product_table2]`) now nest under an implicit `product_table2` branch, matching `[product_table]`. Projection only flattens implicit tables created by dotted *keys* (`a.b.c = 1`, which toml_edit marks `is_dotted()`); a dotted header is implicit but not dotted, so it projects as a real branch. (2026-06-07)
- Editing — `E` (external `$EDITOR`) on the root/file node no longer fails on save with `operation not supported by this format`. A `Replace` with an empty path now reparses the edited text as the whole document (invalid TOML is rejected and leaves the document untouched). (2026-06-07)
- Editing — opening `$EDITOR` on a structured node (`[table]`, array, inline table, array-of-tables entry) no longer starts with an empty first line: the node's leading blank separator is trimmed from the editor view. The blank line is re-attached on save (`split_leading_blank_lines` in `toml_doc.rs`), so file spacing round-trips unchanged; leading comments are still shown and editable. (2026-06-07)
- Clipboard — a failed paste no longer discards the clipboard. Previously only a key collision preserved it; any other error (e.g. pasting a bare value into a table) silently emptied the clipboard, forcing a re-copy. `do_paste` now restores the remaining fragments on every failure path, so you can move the cursor to a valid target and retry. (2026-06-08)
- Clipboard/Filter — pasting into a key collision and choosing overwrite/rename (`o`/`r`) while a filter is active now inserts at the correct position. The retry path resolved its insertion index from the *visible* (filtered) row list, so it could disagree with the initial paste; it now uses the full-tree `true_sibling_index`, matching paste/add. (2026-06-08)

## [v0.3.0] - 2026-06-07

### Changed
- Editing — `E` on an **array-of-tables entry** (`product[0]`) now opens `$EDITOR` with just that single `[[product]]` block (was: the whole array-of-tables). Write-back goes through a new AoT-entry `Replace` branch (`replace_aot_entry`) that rewrites only that entry, preserving the others and the between-entries comments; `edit_node` now truncates the path only at a real `Array` index, keeping AoT-entry indices addressable. (2026-06-07)
- Editing — `e` on a **scalar member of an array-of-tables entry** (`product[0].sku`) now edits inline (and `←/→` nudges, `Tab`→Name renames) instead of opening `$EDITOR` on the whole AoT. `parent_table_mut`/`concrete_table_mut` now descend a `Key→Index` AoT entry; the inline rule keys on the absence of an `Array` ancestor, so array-of-inline-table members (`x = [{ a = 1 }]`) still open `$EDITOR`. (2026-06-07)
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
- CI — `.github/workflows/release.yml`: on a `v*.*.*` tag, cross-compiles `confy` for Linux x86_64 (gnu + musl), macOS (arm64 + Intel), and Windows x86_64 + i686 (MSVC), packages tar.gz (Unix) / `.exe` (Windows), emits `SHA256SUMS`, and publishes a GitHub Release (annotated-tag message + auto-generated notes).

### Fixed
- Comments — editing or deleting a standalone comment now works **wherever it sits**, not just before the first item of a container. The shared decor locator (`transform_comment_in_decor`) used to inspect only the first key, so a comment before any *non-first* item — e.g. a section-separator above `[[products]]` when an earlier section precedes it — silently failed to save or delete. It now sweeps every comment-bearing slot (`sweep_table_comment_slots`: each key's `leaf_decor`, each `[table]` header decor, each array-of-tables entry prefix, and the document trailing), stopping at the first slot that matches. This also covers comments **between** AoT entries and **inside** an AoT entry. (2026-06-07)
- Comments — a comment **inside an array-of-tables entry** (`[[product]]` / `#123` / `name = …`) now edits inline like any other single-line comment, and `E` opens `$EDITOR` with its text instead of a blank buffer. Its path carries an `Index` (the entry), but it is decor-addressable, so editing keys on the absence of an `Array` ancestor (shared `no_array_ancestor`) rather than the mere presence of an `Index`. (2026-06-07)
- Editing — deleting a standalone comment node no longer fails with `delete error: path not found`; `Delete` now strips the comment from its decor slot (like `uncomment`) instead of trying to remove a non-existent `#comment:N` table key. (2026-06-07)
- TUI — multi-line cell values (merged comments, multiline strings, and elements of a multiline-formatted array) now render a single-line preview (first line + ` …`) in the VALUE column. Previously a multiline-array element showed nothing because its repr carries leading newline+indent decor; the full text remains available in the detail popup. (2026-06-07)
- TUI — the main tree viewport now persists its scroll offset across frames, so the cursor moves within the visible window instead of staying pinned to the bottom edge and scrolling on every key. (2026-06-06)
- Editing — replacing a value no longer drops a standalone `#` comment sitting above its key; `Replace`/`Insert` overwrite now updates the value in place (preserving key decor) instead of re-inserting the key. (2026-06-06)
- Editing — `e` on a node nested inside an array/AoT (or on an element of a multiline array) no longer opens an empty editor; it edits the nearest addressable container, and multiline-array string elements edit inline with their indentation preserved. (2026-06-06)
- Move — moving a node into a table no longer drops the leading comments and blank lines above it; the move now carries the key's `leaf_decor` (capturing `(Key, Item)` and re-inserting via `entry_format`) instead of re-serializing through a fresh document. Array destinations are unchanged. (2026-06-06)

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
