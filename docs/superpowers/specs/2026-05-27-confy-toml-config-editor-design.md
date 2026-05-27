# confy — Single-File TUI Config Editor (Design Spec)

**Date:** 2026-05-27
**Status:** Draft for review
**Scope of this spec:** MVP — TOML only, single file. YAML/JSON are explicitly out of scope here (the architecture is chosen so they can be added later).

## 1. Purpose

confy is a cross-platform Terminal User Interface (TUI) for editing a **single** structured
config file. It reproduces [wenv](https://github.com/superyngo/wenv)'s
Navigation / Selection / Editing UX and keybindings, but targets **markup config formats**
(TOML first) instead of shell rc files.

Where wenv is line-based, flat (File → Entry, 2 levels), and multi-file, confy is:

- **Single-file** — no multi-file tree, no cross-file operations, no cross-format clipboard.
- **Recursive tree** — objects/tables/arrays become expandable, multi-level toggle nodes;
  scalars are leaves.
- **Round-trip preserving** — comments, key order, and formatting survive editing.

## 2. Success Criteria

1. Open a `.toml` file; render it as a navigable Node tree rooted at the filename.
2. All keybindings in §6 work.
3. **Round-trip fidelity:** loading then saving an untouched file produces a byte-identical
   file. Editing one Node leaves all other bytes untouched.
4. Editing/creating Nodes via `$EDITOR` validates before applying; invalid input shows an
   error and never corrupts the document.
5. `r` toggles a Node between live and commented ("disabled") state, wenv-style.
6. Undo/redo restores prior document state across multiple steps.

## 3. Architecture

**Chosen approach: CST projection (the format-native round-trip document is the single
source of truth).**

The backing `toml_edit::DocumentMut` *is* the document. The Node tree is a **projection**
rebuilt from it after every mutation. All edits go through `toml_edit`'s API or by
re-parsing a TOML fragment. Round-trip fidelity comes for free because `toml_edit` preserves
comments, ordering, and whitespace.

Two approaches were rejected:

- **Unified IR + adapters** — would force us to re-implement comment/order/format
  preservation per format; that is the hardest part and `toml_edit` already solves it.
- **Line/range overlay (wenv-style)** — nested structures don't map to clean contiguous line
  ranges; reparent/move across the tree is awkward.

### 3.1 Multi-format extension (future, not this spec)

A `ConfigDocument` trait abstracts the backing document so YAML/JSON can plug in later, each
wrapping its own format-native round-trip CST. The Node tree and entire TUI layer are
format-agnostic and shared.

```rust
trait ConfigDocument {
    fn load(path: &Path) -> Result<Self> where Self: Sized;
    fn project(&self) -> NodeTree;
    fn apply(&mut self, op: Mutation) -> Result<()>;
    fn serialize(&self) -> String;
    fn is_dirty(&self) -> bool;
}
```

For the MVP only the TOML backend (`TomlDocument` wrapping `toml_edit::DocumentMut`) exists.

### 3.2 Module structure (mirrors wenv)

```
src/
  cli/            clap args: `confy <file>` [--format toml]
  model/
    document.rs   ConfigDocument trait + Mutation enum
    toml_doc.rs   TomlDocument (wraps toml_edit::DocumentMut)
    node.rs       Node + NodeKind
    tree.rs       projection: Document -> NodeTree -> flattened visible-rows
  tui/
    app.rs        main state + event loop
    state.rs      modes, clipboard, undo/redo stacks
    keys.rs       keybindings + help text
    ui.rs         ratatui rendering
    list.rs       tree/visible-row rendering
    selection.rs  multi-select + range select
    search.rs     fuzzy filter state
    operations.rs Node mutations (delete/cut/paste/move/remark/undo)
    editor.rs     $EDITOR integration + fragment parse/validate
```

## 4. Data Model

Terminology follows `CONTEXT.md`: the tree is made of **Nodes** (umbrella term — confy never
says "Entry"). A **Branch node** has children; a **Leaf node** does not. The **Root** is the
single top-of-tree Node whose key is the filename.

**Single source of truth:** `toml_edit::DocumentMut`.

**Node tree (projection):**

```rust
enum Seg { Key(String), Index(usize) }     // addresses a Node back into the Document

enum ScalarType { String, Integer, Float, Bool, Datetime }

enum NodeKind {
    Root,            // the filename header (exactly one)
    Table,           // [table] / nested table          \
    ArrayOfTables,   // [[array.of.tables]]              | Branch nodes
    Array,           // [1, 2, 3]                        | (have children)
    InlineTable,     // { a = 1, b = 2 }                 /
    Scalar(ScalarType),  // typed value                  \ Leaf nodes
    Comment(String),     // standalone comment (see §7)  / (no children)
}

struct Node {
    key: String,         // key name, array index label, or comment text for display
    path: Vec<Seg>,      // full path from Root
    kind: NodeKind,
    children: Vec<Node>, // empty for Scalar / Comment
}
```

- **Root** is the filename header Node; top-level keys are its children. (This is why
  `NodeKind` has a dedicated `Root` variant — the Root is not a Table or Scalar.)
- **Arrays** are Branch nodes with index-labelled children; **array-of-tables** is a Branch
  node with repeated table children; **inline tables** are Branch nodes.
- **Rendering:** the tree is flattened into an ordered list of *visible rows* honoring each
  Branch node's expanded/collapsed state. Navigation and selection operate on this flat
  sequence (same flattening model as wenv).

## 5. Data Flow

```
load    : read file -> toml_edit parse -> project -> flatten -> render
keypress: action -> operation -> mutate Document -> push undo snapshot -> re-project -> render
save    : Document.serialize() -> write to disk (round-trip preserved)
```

## 6. Keybindings → Operations

| Key | confy behavior |
|-----|----------------|
| `j`/`k`, `↑`/`↓`, PgUp/PgDn, Home/End | Move on visible-rows |
| `Enter`/`Space` | Branch node: expand/collapse; Leaf node: open read-only **Detail** view (path / type / value / attached comments) |
| `0` / `9` | Collapse / expand all Branch nodes |
| `s` | Toggle select Node |
| `Shift+↑`/`↓` | Range select (over the flat visible sequence) |
| `e` | Edit: hand the Node's raw TOML fragment to `$EDITOR`, re-parse on return, validate, apply (see §8) |
| `n` | New: empty `$EDITOR` buffer; write arbitrary TOML; insert into cursor's parent scope as a sibling after the cursor; validate before insert — on parse/type error, show error and do **not** save |
| `d` | Delete Node (a Branch node deletes its whole subtree) |
| `x` / `c` / `v` | Cut / copy / paste; unit = Node (a Branch node carries its subtree); paste lands in cursor's parent scope as a sibling |
| `m` | Move: reorder among siblings / reparent; applies the key-collision rule |
| key collision (paste/move into a table already holding that key) | Prompt: **overwrite / rename / cancel** |
| `r` | Toggle remark: comment ⇄ uncomment the whole Node, wenv-style (see §7) |
| `z` / `y` | Undo / redo (multi-step, full-document snapshots) |
| `/` | Fuzzy filter over dotted key-paths; matching Nodes and their ancestors stay visible |
| `w` / `Ctrl+s` | Save all changes (only writes when dirty) |
| `q` | Quit (confirm if dirty) |
| `?` | Show help |

CLI surface for the MVP is just `confy <file>`. A non-`.toml` path produces a clear
"format not yet supported" error. `--format` may override extension-based detection.

## 7. Remark (`r`) — Comment-as-Node Model

confy adopts wenv's model (adapted to a tree): **a standalone comment line is a first-class
Leaf node** (`NodeKind::Comment`). This removes any need for a sentinel marker or a separate
"disabled-node" registry — a disabled setting is simply *a comment that happens to be valid
TOML*, and it round-trips across sessions naturally because it is stored as an ordinary
comment.

`r` behavior:

- **Live Node → comment:** prefix each line of the Node's text with `# `; the Node becomes a
  `NodeKind::Comment` Leaf node.
- **Comment → live Node:** strip the leading `# `, parse the remainder as a TOML fragment.
  - If it parses as valid TOML → replace the Comment node with the resulting live Node(s).
  - If it does **not** parse (e.g. `# just prose`) → show "not valid TOML, kept as comment"
    and leave it commented.

**Honest limitation vs wenv:** shell is lenient — uncommenting any text yields at least a
valid `Code` entry, so the toggle never fails. TOML is strict — there is no "raw code line"
node, so uncommenting non-TOML prose cannot produce a live Node. This asymmetry is expected
and surfaced to the user.

**Implementation cost / main risk:** `toml_edit` stores comments as node *decor* (trivia
strings on the prefix/suffix of items), not as iterable nodes. The projector must extract
leading-comment lines from each item's `decor().prefix()` and emit them as sibling `Comment`
entries; mutations must write comment entries back into the appropriate decor. This
"decor ⇄ comment-node" mapping is the principal implementation risk of the chosen approach.
It is bounded but must be covered thoroughly by tests (§10).

## 8. Editing & Write-Back Validation (`e` and `n`)

Both `e` and `n` route through `$EDITOR` and share one validation gate:

1. Write the relevant TOML fragment (existing Node for `e`; empty for `n`) to a temp file.
2. Launch `$EDITOR` (fallback to `$VISUAL`, then a platform default).
3. On return, parse the fragment with `toml_edit`.
4. **Validate:** syntax parses + structurally fits the insertion point + no unresolved key
   collision.
5. If valid → apply to the Document. If invalid → show the error, discard the edit, leave the
   document unchanged.

Types are determined by the TOML syntax the user writes (e.g. quoting makes a string); there
is no separate type picker.

## 9. Error Handling

- **Load failure** (file is not valid TOML): show the parse error and refuse to open. Never
  open a partially-parsed document (avoids corruption on save).
- **Write-back failure** (`e`/`n`): validation gate above; document unchanged.
- **Save failure** (I/O error): show the error, keep the document marked dirty.
- **Quit with unsaved changes:** confirm before exiting.

## 10. Testing

- **Unit (model):**
  - Round-trip: `load → serialize` is byte-identical for untouched files; editing one Node
    leaves all other bytes untouched.
  - Projection: Document → NodeTree for tables, nested tables, arrays, array-of-tables,
    inline tables, scalars of each type, and standalone comments.
  - Fragment parse/validate for `e`/`n`, including rejection paths.
  - Remark toggle round-trip, including the non-TOML-comment rejection path and the
    decor ⇄ comment-node mapping.
  - Key-collision resolution (overwrite / rename / cancel).
- **TUI logic tests** (mirroring wenv's `tui_logic_tests`): effect of each operation on the
  tree and selection.
- **Integration (real binary):** open a sample `.toml`, perform a sequence of operations,
  save, and diff against an expected fixture.

## 11. Out of Scope (recorded for future evaluation)

- **YAML / JSON formats** — enabled later via the `ConfigDocument` trait; YAML comment
  preservation is a known weak point in the Rust ecosystem and will need its own design pass.
- **Multi-file management & cross-file/cross-format clipboard** — confy is single-file by
  design.
- **wenv's "entries combine" merging** — wenv merges a preceding comment / blank line into the
  following structured entry as one entry. Deliberately excluded from this MVP; recorded here
  to evaluate adopting an equivalent later for confy.
