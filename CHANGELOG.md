# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- 2026-05-27: Project scaffold (git init, Cargo skeleton, README, CHANGELOG, .gitignore).
- 2026-05-27: MVP design spec (`docs/superpowers/specs/`), `CONTEXT.md` glossary (Node/Root/Branch/Leaf), and implementation plan (`docs/superpowers/plans/`) — single-file TOML editor, CST-projection architecture, reviewed via grill + external spec-review (0 blockers).
- 2026-05-28: Replace (e) and Remark (r) mutations — comment-out/uncomment toggle for live keys, Replace = delete + insert fragment, non-TOML comment rejection (§7, §8).
- 2026-05-28: Review fixes for Replace + Remark (3 blockers + 1 major): preserve key position in Replace, canonical TOML serialization in comment_out via single-key DocumentMut, recursive tree search for nested comment nodes in uncomment, block-replace for multi-line comment removal, correct Table decor slot for [table] siblings.
- 2026-05-28: TUI skeleton — App state (cursor, expanded set, row snapshot), headless navigation tests, ratatui/crossterm render loop with tree indentation + expand/collapse markers, key mapping (j/k/arrow/PgUp/PgDn/Home/End/Enter/Space/0/9/q), crossterm raw-mode + alternate-screen setup/teardown (§6).
