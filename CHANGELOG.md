# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

MVP: single-file TOML editor with CST-projection architecture, tree navigation/selection/editing,
byte-identical round-trip preservation, Remark toggle, undo/redo, fuzzy filter, and `$EDITOR`
integration.

### Added
- 2026-05-27: Project scaffold (git init, Cargo skeleton, README, CHANGELOG, .gitignore).
- 2026-05-27: MVP design spec (`docs/superpowers/specs/`), `CONTEXT.md` glossary (Node/Root/Branch/Leaf), and implementation plan (`docs/superpowers/plans/`) — single-file TOML editor, CST-projection architecture, reviewed via grill + external spec-review (0 blockers).
- 2026-05-28: Replace (e) and Remark (r) mutations — comment-out/uncomment toggle for live keys, Replace = delete + insert fragment, non-TOML comment rejection (§7, §8).
- 2026-05-28: Review fixes for Replace + Remark (3 blockers + 1 major): preserve key position in Replace, canonical TOML serialization in comment_out via single-key DocumentMut, recursive tree search for nested comment nodes in uncomment, block-replace for multi-line comment removal, correct Table decor slot for [table] siblings.
- 2026-05-28: TUI skeleton — App state (cursor, expanded set, row snapshot), headless navigation tests, ratatui/crossterm render loop with tree indentation + expand/collapse markers, key mapping (j/k/arrow/PgUp/PgDn/Home/End/Enter/Space/0/9/q), crossterm raw-mode + alternate-screen setup/teardown (§6).
- 2026-05-28: Scalar value in filter haystack + type/value/comment in detail popup — Node.value stores scalar text during projection; RowSnapshot gains value/scalar_type/trailing_comment; open_detail formats per §6; rebuild_rows at enter_filter; debug_assert replaces dead branch guard.
- 2026-05-28: Docs — README with real usage, keybinding table, and scope; CLAUDE.md with build/test commands, architecture summary, and module map.
- 2026-06-03: wenv-style header + columnar tree — title bar (`confy — <file> ──── v<version>`), a `NAME / TYPE / VALUE` column header, and the tree list switched from `List` to a ratatui `Table` so TYPE (kind/scalar-type label) and VALUE align in fixed columns; `RowSnapshot` gains `type_label`; headless `TestBackend` render tests for the title bar and columns.
- 2026-06-06: Inline editor + scalar Format + value nudge — `Node` gains a read-only `Format` (writing style: hex/oct/bin integers, basic/literal/multiline strings) derived during projection; `ScalarType::Datetime` split into the four TOML datetime types (offset/local-datetime/local-date/local-time). New `Mode::Edit` inline editor: `e` edits a plain scalar in-place (type-check with a confirm-on-change prompt, not enforced) and falls back to `$EDITOR` for nested arrays/tables; `E` forces `$EDITOR` on any node; `←`/`→` toggle a bool or step an integer/float by ±1 preserving its base/precision. `n` (New node, `$EDITOR`) replaced by `a` (Add node) which inserts `new_field = ""` below the cursor and opens the inline editor. Clear-to-null and `Del`-clear deliberately omitted (TOML has no null). Unit tests for format/datetime detection, edit-target classification, `nudge_scalar`, inline commit (same-type / type-change / invalid), `add_node`, and inline-editor rendering.
