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

**`Mutation` enum** is the closed set of document operations (Insert, Delete, Replace, Rename,
Move, Remark). `apply` dispatches each variant to the corresponding `toml_edit` manipulation and
rebuilds the Node tree projection afterward. `Rename` is position- and decor-preserving (re-inserts
the table in order, swapping only the target key) — there is no separate user-facing rename action;
it is driven from the inline editor (see below).

**Editing.** `e` edits a single-line scalar in an in-TUI **inline editor** (`Mode::Edit`) — a direct
child of a Table/Root, or a scalar **element of an array** addressed by a `Key+ Index*` path
(including array-of-arrays nesting; written back via `Replace` on the trailing `Index`, routed by
`replace_array_element` → `Array::replace`, with `array_at_mut` descending nested arrays). The inline
editor edits one field at a time: **`Tab` toggles between Value (default) and Name**; committing a
changed Name applies a `Mutation::Rename` first, then the value `Replace` (Tab is disabled for array
elements, which have no key). Both columns share one horizontal-scroll/overflow treatment
(`edit_field_spans`). A node nested inside an AoT, multiline strings, and `E` open `$EDITOR` —
`edit_node` truncates the path at the first `Index` so the edit targets the nearest addressable
container. Inline commit and the `←/→` value-nudge write back through `Mutation::Replace` (the nudge
re-applies underscore digit grouping when the original had it). A scalar's **Format** (writing style:
hex/oct/bin, basic/literal/multiline string, …) is derived read-only during projection and is
orthogonal to its `ScalarType`. TOML has no null, so there is no clear-value operation; `a` seeds a
new node with the empty string `""` — a key/value under a Table/Root, or a bare element when the
target is an array (`insert_fragment` → `array_at_mut`).

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
