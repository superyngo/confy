# CLAUDE.md — confy developer guide

## Build & test commands

```bash
cargo build                   # compile
cargo test                    # unit + integration tests
cargo clippy -- -D warnings   # lint (must be clean before commit)
cargo fmt                     # format
cargo fmt --check             # check formatting without modifying
cargo run -- <file.toml>      # run against a TOML file
```

## Architecture

**CST projection.** `toml_edit::DocumentMut` is the single source of truth. The Node tree is a
*projection* rebuilt after every mutation — it is never mutated directly. All edits go through
`toml_edit` APIs or by re-parsing a TOML fragment string.

**`ConfigDocument` trait** abstracts the storage backend so YAML/JSON can be added later; the
MVP ships only the TOML backend (`TomlDocument`). The trait exposes `load`, `serialize`, and
`apply(Mutation)`.

**`Mutation` enum** is the closed set of document operations (Insert, Delete, Replace, Move,
Remark). `apply` dispatches each variant to the corresponding `toml_edit` manipulation and
rebuilds the Node tree projection afterward.

**Editing.** `e` edits a plain scalar (direct child of a Table/Root) in an in-TUI **inline
editor** (`Mode::Edit`); nested arrays/tables and `E` always open `$EDITOR`. Inline commit and
the `←/→` value-nudge both write back through `Mutation::Replace`. A scalar's **Format** (writing
style: hex/oct/bin, basic/literal/multiline string, …) is derived read-only during projection and
is orthogonal to its `ScalarType`. TOML has no null, so there is no clear-value operation; `a`
seeds a new node with the empty string `""`.

## Module map

```
src/
  main.rs          CLI entry: parse args, load TomlDocument, run TUI
  lib.rs           module declarations + re-exports (enables integration tests)
  cli.rs           clap args; confy <file> [--format toml]; format detection
  model/
    mod.rs         re-exports
    node.rs        Seg, ScalarType, Format, NodeKind, Node, NodeTree
    document.rs    ConfigDocument trait, Mutation, Target, OnCollision, errors
    toml_doc.rs    TomlDocument wrapping toml_edit::DocumentMut: load/serialize/apply
    project.rs     DocumentMut → NodeTree projection (§7.1 comment mapping)
    fragment.rs    parse/validate a TOML fragment string
  tui/
    mod.rs         re-exports; run() entry point + event loop (run_event_loop)
    app.rs         App state + operation handlers (the event loop dispatches keys to these)
    state.rs       Mode (incl. Edit), Clipboard, EditState, undo/redo stacks
    keys.rs        KeyAction mapping + help text
    insertion.rs   §6.1 insertion-target resolution from cursor
    selection.rs   multi-select + range select + §6.2 normalization
    search.rs      fuzzy filter state + haystack builder
    editor.rs      $EDITOR integration (external edit for nested array/table)
    ui.rs          ratatui rendering: title bar + NAME/TYPE/VALUE column header + tree Table, detail popup, help, prompts
tests/
  roundtrip.rs     integration: open/edit/save, diff fixture
  fixtures/        sample .toml files
```

`model/` is pure (no TUI deps) and fully unit-testable in isolation.

## Terminology

See **`CONTEXT.md`** for the canonical glossary. Key rule: use **Node** (not "Entry"). Subtypes
are **Root**, **Branch node**, **Leaf node**, **Scalar**, and **Comment**. The operation that
toggles a live Node to/from a Comment is **Remark** (key `r`).
