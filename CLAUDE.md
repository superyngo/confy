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
Move, Remark, EditComment). `apply` dispatches each variant to the corresponding `toml_edit` manipulation and
rebuilds the Node tree projection afterward. `Rename` is position- and decor-preserving (re-inserts
the table in order, swapping only the target key) — there is no separate user-facing rename action;
it is driven from the inline editor (see below). `Replace` with an **empty path** targets the whole
document (external `E` on the root/file node): it reparses the edited text as a full `DocumentMut`,
rejecting invalid TOML as `Fragment` (doc untouched) rather than the old `Unsupported`.

**Projection of dotted tables.** Both dotted *keys* (`a.b.c = 1`) and dotted *headers* (`[x.a]` with
no `[x]`) produce an implicit parent in `toml_edit`, but only dotted keys are also `is_dotted()`.
`project_table`/`flatten_dotted` flatten on `is_implicit() && is_dotted()` — so `a.b.c` collapses to
one node, while a header-implied parent (`is_dotted() == false`) projects as a real nested branch.

**Editing.** `e` edits a single-line scalar in an in-TUI **inline editor** (`Mode::Edit`) — a direct
child of a Table/Root, a scalar **member of an inline table** (`pt = { x = 1 }`), a scalar **member of
an array-of-tables entry** (`product[0].sku` — its path carries an `Index`, but the AoT entry is itself
a table, reached by the `Key→Index` AoT descent in `parent_table_mut`/`concrete_table_mut`), or a scalar
**element of an array** addressed by a `Key+ Index*` path (including array-of-arrays nesting; written
back via `Replace` on the trailing `Index`, routed by `replace_array_element` → `Array::replace`, with
`array_at_mut` descending nested arrays). The keyed-scalar inline rule keys on the **absence of an
`Array` ancestor** (an AoT ancestor is addressable; an array element such as `x = [{ a = 1 }]` is not, so
it stays `$EDITOR`). The inline editor edits one field at a time: **`Tab` toggles
between Value (default) and Name**; committing a changed Name applies a `Mutation::Rename` first, then
the value `Replace` (Tab is disabled for array elements, which have no key). `Rename` dispatches on the
parent container — `rename_in_table` for a standard `[table]`, `rename_in_inline_table` (via
`inline_table_mut`) for an inline table — both order- and decor-preserving. Both columns share one
horizontal-scroll/overflow treatment (`edit_field_spans`, also reused to render the `/` filter input as
an inline field with a caret). The editor and the filter input are both caret-based text fields:
`←/→/Home/End` move the caret, `Backspace`/`Del` erase before/at it. Multiline strings, structured
nodes, and `E` open `$EDITOR`. `edit_node` truncates the path only at the first `Index` whose container
is a real `Array` (editing the whole array there); array-of-tables-entry indices and the keys below them
are kept and addressed directly — so `E` on an AoT **entry** (`product[0]`) serializes just that single
`[[product]]` block (`serialize_node_fragment_opts` emits a one-entry `ArrayOfTables`; the immutable
`walk_tablelike` mirrors `parent_table_mut`'s AoT descent) and writes back through `replace`'s
`replace_aot_entry` branch (rewrites only that entry, sibling entries and between-entry comments intact).
For a **structured** node (table/inline table/array/AoT) the
editor fragment carries the node's adjacent leading comment(s) (`serialize_node_fragment_opts` copies
the key's `leaf_decor` prefix; tables already carry theirs in the item decor), and `replace` syncs that
key decor back from the edited fragment so comment edits round-trip — scalars never carry comments. The
fragment's **leading blank separator** is trimmed from the editor view (`split_leading_blank_lines`, so
`E` opens at the comment/header, not an empty line) but **re-attached on write-back** — `replace`
(table item decor / array key leaf_decor) and `replace_aot_entry` (entry decor) prepend the original
node's leading blanks to the trimmed fragment decor, so file spacing round-trips byte-identically. Inline commit and the `←/→` value-nudge write back through `Mutation::Replace` (the nudge
re-applies underscore digit grouping when the original had it). A scalar's **Format** (writing style:
hex/oct/bin, basic/literal/multiline string, …) is derived read-only during projection and is
orthogonal to its `ScalarType`. TOML has no null, so there is no clear-value operation; `a` seeds a
new node with the empty string `""` — a key/value under a Table/Root, or a bare element when the
target is an array (`insert_fragment` → `array_at_mut`).

**Comments.** Consecutive standalone `#` lines project as a *single* multi-line Comment node
(`comment_blocks`; a blank or non-`#` line breaks the group). A comment node carries its text as its
`value`, so the VALUE column and detail popup show it. Multi-line cell values (merged comments,
multiline strings, multiline-array elements whose repr carries leading newline/indent decor) are
collapsed to a one-line preview (first line + ` …`) by `cell_preview` in `ui.rs`; the full text stays
in the detail popup. `e` on a **single-line** comment edits inline (`Mode::Edit` with `is_comment`: the
raw `#`-prefixed text is the sole field — no name, `Tab` is a no-op — and `edit_commit` routes straight
to `Mutation::EditComment`, staying in the editor on a non-`#` validation error). A comment edits inline
whenever it is single-line and **decor-addressable** — no `Array` ancestor, checked by the shared
`no_array_ancestor` (an AoT-entry ancestor is fine even though it puts an `Index` in the path). `E`, a
merged multi-line comment, or one with an `Array` ancestor instead open `$EDITOR` with the raw text.
Either way the edit writes back via `Mutation::EditComment` (`edit_comment` → `transform_comment_in_decor`).
Deleting a comment node (`d`) routes through the same locator: `remove_at` detects the synthetic
`#comment:N` key and calls `remove_comment_from_decor` rather than `Table::remove` (which would fail with
`NotFound`). **The locator sweeps**, it does not guess a single slot: `transform_comment_in_decor` runs
`sweep_table_comment_slots` over the container — every key's `leaf_decor` prefix, every `[table]` header
decor, every `[[aot]]` entry prefix (`transform_aot_entry_prefixes`) — plus the document trailing for the
root, stopping at the first slot the text-matching transform changes. This reaches a comment before **any**
item (not just the first), an AoT parent's between-entry comments, and comments inside an AoT entry alike.
(A comment text that is a substring of an earlier-swept comment is the one edge the sweep can mis-target.)

**Navigation.** Expand/collapse state is an `App.expanded: HashSet<Path>` of open branch paths. The
**root/file node has the empty path** and is collapsible like any branch — `flatten` treats it
uniformly; the App seeds `[]` into `expanded` so it starts open, and `collapse_all` (`0`) re-inserts
`[]` so it keeps the file node open (only an explicit toggle on the root row hides everything).

**Filter.** `/` is a three-state flow: `Mode::Filter` (the inline `/` input field) → **Enter** →
`Mode::FilterResults` (browse/select/edit the locked-in filtered list, status shows `[filter: …]`),
or **Esc** clears the filter back to `Mode::Normal`. `App.last_filter` remembers the last committed
query so `/` (`enter_filter`) prefills it and re-applies the live filter. `FilterResults` reuses the
Normal key dispatch (no early-return block); its only differences are mode-aware `escape`
(`exit_filter_results`, keeps `last_filter`) and `/` (`enter_filter`, to refine). Esc fully unfilters
(`filtered_paths = None`) — `last_filter` is pure memory, never a persisted filter. While a filter is
active the matched chars are highlighted in the NAME/VALUE cells (`search::fuzzy_indices` →
`ui::highlight_spans`, run per-field against each cell's own text; gated on a non-empty query, not the
mode, so the highlight survives an inline edit / detail popup). Transient overlays (detail popup,
inline editor) close back into the filtered selection via `App::resting_mode` (`FilterResults` when
`filtered_paths.is_some()`, else `Normal`) — `exit_detail`/`edit_cancel`/`edit_commit` use it.

**Multi-select.** `Selection` holds `committed` (finalized rows + `s` toggles) and an in-progress
`round` (`anchor..=cursor`); the live set is their union. A Shift+Arrow run extends `round`; the next
Shift+Arrow after any non-shift key (tracked by `App.last_action_was_shift_select`, reset in the event
loop) starts a fresh round, folding the old one into `committed` — so runs union (separate or
overlapping) rather than re-extending the first anchor. `Esc` in `Mode::Normal` clears the selection.

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
