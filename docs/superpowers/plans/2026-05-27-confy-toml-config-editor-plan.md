# confy MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a single-file TUI editor for TOML config files that renders the file as a navigable Node tree, supports wenv-style navigation/selection/editing keybindings, and preserves comments/order/formatting on save.

**Architecture:** CST projection. `toml_edit::DocumentMut` is the single source of truth; the Node tree is a projection rebuilt after every mutation; all edits go through `toml_edit` or by re-parsing TOML fragments. A `ConfigDocument` trait abstracts the backend so YAML/JSON can be added later, but the MVP ships only the TOML backend.

**Tech Stack:** Rust, `toml_edit` (round-trip CST), `ratatui` + `crossterm` (TUI), `fuzzy-matcher` (filter), `tempfile` (`$EDITOR` temp files), `dirs` (editor fallback), `clap` (CLI), `anyhow`/`thiserror` (errors).

**Source of truth:** the spec at `docs/superpowers/specs/2026-05-27-confy-toml-config-editor-design.md`. Section refs (§N) below point into it. Terminology follows `CONTEXT.md` (Node/Root/Branch/Leaf — never "Entry").

**Build-order rationale:** Tasks 1–4 stand up the round-trip foundation and de-risk the §7.1 decor⇄Comment mapping *first* (the spec's named principal risk) before any TUI work. Model mutations (5–11) are unit-tested in isolation. The TUI (12–20) wires already-tested operations to keys.

---

## File Structure

```
src/
  main.rs            entry point: parse CLI, load doc, run TUI
  lib.rs             module declarations + re-exports (enables integration tests)
  cli.rs             clap args; confy <file> [--format toml]; format detection
  model/
    mod.rs           re-exports
    node.rs          Seg, ScalarType, NodeKind, Node, NodeTree
    document.rs      ConfigDocument trait, Mutation, Target, OnCollision, errors
    toml_doc.rs      TomlDocument (wraps toml_edit::DocumentMut): load/serialize/apply
    project.rs       Document -> NodeTree projection (incl. §7.1 comment mapping)
    fragment.rs      parse/validate a TOML fragment string
  tui/
    mod.rs           re-exports; run() entry
    app.rs           App state + event loop + key dispatch
    state.rs         Mode, Clipboard, undo/redo stacks
    keys.rs          KeyAction mapping + help text
    insertion.rs     §6.1 insertion-target resolution from cursor
    selection.rs     multi-select + range select + §6.2 normalization
    search.rs        fuzzy filter state + haystack builder
    editor.rs        $EDITOR integration
    ui.rs            ratatui rendering (tree rows, detail popup, help, prompts)
tests/
  roundtrip.rs       integration: real binary opens/edits/saves, diff fixture
  fixtures/          sample .toml files
```

Each module has one responsibility; `model/` is pure (no TUI deps) and fully unit-testable.

---

## Task 1: Dependencies + library crate skeleton

**Files:**
- Modify: `Cargo.toml`
- Create: `src/lib.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Pin dependencies in `Cargo.toml`**

Replace the `[dependencies]` block with:

```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
anyhow = "1"
thiserror = "1"
toml_edit = "0.22"
ratatui = "0.28"
crossterm = "0.28"
fuzzy-matcher = "0.3"
tempfile = "3"
dirs = "5"

[dev-dependencies]
assert_cmd = "2"
predicates = "3"
tempfile = "3"
```

- [ ] **Step 2: Create the library crate root `src/lib.rs`**

```rust
pub mod cli;
pub mod model;
pub mod tui;
```

- [ ] **Step 3: Point `src/main.rs` at the library (placeholder wiring)**

```rust
use anyhow::Result;

fn main() -> Result<()> {
    confy::cli::run()
}
```

(`cli::run` is implemented in Task 12; until then this won't compile, so create a temporary stub now and delete it in Task 12.)

Create `src/cli.rs` with a temporary stub:

```rust
use anyhow::Result;

pub fn run() -> Result<()> {
    Ok(())
}
```

Create empty module roots so the crate compiles:

`src/model/mod.rs`:
```rust
pub mod node;
```

`src/tui/mod.rs`:
```rust
// populated in Task 12+
```

- [ ] **Step 4: Verify it builds**

Run: `cargo build`
Expected: compiles (warnings about unused are fine). If `cargo` reports a missing `model::node`, create `src/model/node.rs` empty for now — Task 2 fills it.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/
git commit -m "chore: pin deps, add library crate skeleton"
```

---

## Task 2: Node model

**Files:**
- Create/replace: `src/model/node.rs`
- Test: same file (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing test**

In `src/model/node.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_and_leaf_classification() {
        let leaf = Node::leaf("port", NodeKind::Scalar(ScalarType::Integer));
        let branch = Node::branch("server", NodeKind::Table);
        assert!(leaf.is_leaf());
        assert!(!leaf.is_branch());
        assert!(branch.is_branch());
        assert!(!branch.is_leaf());
    }

    #[test]
    fn comment_is_leaf() {
        let c = Node::leaf("# note", NodeKind::Comment("# note".into()));
        assert!(c.is_leaf());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib node::tests`
Expected: FAIL — `Node`, `NodeKind`, `ScalarType` not defined.

- [ ] **Step 3: Write the model**

Above the test module in `src/model/node.rs`:

```rust
/// One segment of a path from Root to a Node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Seg {
    Key(String),
    Index(usize),
}

pub type Path = Vec<Seg>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarType {
    String,
    Integer,
    Float,
    Bool,
    Datetime,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    Root,
    Table,
    ArrayOfTables,
    Array,
    InlineTable,
    Scalar(ScalarType),
    Comment(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub key: String,
    pub path: Path,
    pub kind: NodeKind,
    pub children: Vec<Node>,
}

impl Node {
    pub fn branch(key: impl Into<String>, kind: NodeKind) -> Self {
        Node { key: key.into(), path: Vec::new(), kind, children: Vec::new() }
    }

    pub fn leaf(key: impl Into<String>, kind: NodeKind) -> Self {
        Node { key: key.into(), path: Vec::new(), kind, children: Vec::new() }
    }

    pub fn is_branch(&self) -> bool {
        matches!(
            self.kind,
            NodeKind::Root | NodeKind::Table | NodeKind::ArrayOfTables
                | NodeKind::Array | NodeKind::InlineTable
        )
    }

    pub fn is_leaf(&self) -> bool {
        !self.is_branch()
    }
}

/// The projected tree, rooted at the filename Node.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeTree {
    pub root: Node,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib node::tests`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/model/node.rs
git commit -m "feat(model): Node/NodeKind/Seg tree types"
```

---

## Task 3: TomlDocument load + serialize (round-trip foundation — §2, §10)

**Files:**
- Modify: `src/model/mod.rs` (add `pub mod document; pub mod toml_doc;`)
- Create: `src/model/document.rs`
- Create: `src/model/toml_doc.rs`
- Test: `src/model/toml_doc.rs`

- [ ] **Step 1: Define the trait (minimal slice) in `src/model/document.rs`**

```rust
use crate::model::node::NodeTree;
use std::path::Path;

pub trait ConfigDocument: Sized {
    fn load(path: &Path) -> anyhow::Result<Self>;
    fn project(&self) -> NodeTree;
    fn serialize(&self) -> String;
    fn is_dirty(&self) -> bool;
    // `apply` (Mutation) added in Task 6.
}
```

- [ ] **Step 2: Write the failing round-trip test in `src/model/toml_doc.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn doc_from_str(s: &str) -> TomlDocument {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(s.as_bytes()).unwrap();
        TomlDocument::load(f.path()).unwrap()
    }

    #[test]
    fn roundtrip_byte_identical_with_comments_and_blanks() {
        let src = "# header comment\n\n[server]\nhost = \"0.0.0.0\"  # bind\nport = 8080\n";
        let doc = doc_from_str(src);
        assert_eq!(doc.serialize(), src, "untouched file must serialize byte-identically");
        assert!(!doc.is_dirty());
    }

    #[test]
    fn load_rejects_invalid_toml() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"this is = = not toml").unwrap();
        assert!(TomlDocument::load(f.path()).is_err());
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib toml_doc::tests`
Expected: FAIL — `TomlDocument` not defined.

- [ ] **Step 4: Implement `TomlDocument` (load/serialize/is_dirty)**

Above the test module in `src/model/toml_doc.rs`:

```rust
use crate::model::document::ConfigDocument;
use crate::model::node::{Node, NodeKind, NodeTree};
use anyhow::Context;
use std::path::{Path, PathBuf};
use toml_edit::DocumentMut;

pub struct TomlDocument {
    pub(crate) doc: DocumentMut,
    pub(crate) path: PathBuf,
    pub(crate) original: String,
    pub(crate) filename: String,
}

impl ConfigDocument for TomlDocument {
    fn load(path: &Path) -> anyhow::Result<Self> {
        let original = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let doc = original
            .parse::<DocumentMut>()
            .with_context(|| format!("parsing {} as TOML", path.display()))?;
        let filename = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        Ok(TomlDocument { doc, path: path.to_path_buf(), original, filename })
    }

    fn project(&self) -> NodeTree {
        // Real projection lands in Task 4/5; placeholder Root keeps the trait whole.
        NodeTree { root: Node::branch(self.filename.clone(), NodeKind::Root) }
    }

    fn serialize(&self) -> String {
        self.doc.to_string()
    }

    fn is_dirty(&self) -> bool {
        self.serialize() != self.original
    }
}
```

Add to `src/model/mod.rs`:
```rust
pub mod node;
pub mod document;
pub mod toml_doc;
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib toml_doc::tests`
Expected: PASS (2 tests). If `roundtrip` fails, inspect the diff — `toml_edit` round-trips verbatim, so a mismatch means a load/serialize bug.

- [ ] **Step 6: Commit**

```bash
git add src/model/
git commit -m "feat(model): TomlDocument load/serialize with byte-identical round-trip"
```

---

## Task 4: Projection of live keys (no comments yet) — §4

**Files:**
- Create: `src/model/project.rs`
- Modify: `src/model/mod.rs` (`pub mod project;`)
- Modify: `src/model/toml_doc.rs` (`project()` calls into `project.rs`)
- Test: `src/model/project.rs`

- [ ] **Step 1: Write failing projection tests**

In `src/model/project.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::{NodeKind, ScalarType, Seg};
    use toml_edit::DocumentMut;

    fn tree(src: &str) -> crate::model::node::NodeTree {
        let doc = src.parse::<DocumentMut>().unwrap();
        project(&doc, "f.toml")
    }

    #[test]
    fn scalars_and_tables() {
        let t = tree("title = \"x\"\n[server]\nport = 8080\n");
        let root = &t.root;
        assert_eq!(root.kind, NodeKind::Root);
        // children: title (scalar), server (table)
        assert_eq!(root.children.len(), 2);
        assert_eq!(root.children[0].key, "title");
        assert_eq!(root.children[0].kind, NodeKind::Scalar(ScalarType::String));
        let server = &root.children[1];
        assert_eq!(server.kind, NodeKind::Table);
        assert_eq!(server.children[0].key, "port");
        assert_eq!(server.children[0].kind, NodeKind::Scalar(ScalarType::Integer));
        assert_eq!(server.children[0].path, vec![Seg::Key("server".into()), Seg::Key("port".into())]);
    }

    #[test]
    fn arrays_and_inline_tables_and_aot() {
        let t = tree("nums = [1, 2]\npt = { x = 1 }\n[[item]]\nn = 1\n[[item]]\nn = 2\n");
        let root = &t.root;
        let nums = root.children.iter().find(|n| n.key == "nums").unwrap();
        assert_eq!(nums.kind, NodeKind::Array);
        assert_eq!(nums.children.len(), 2);
        assert_eq!(nums.children[0].key, "[0]");
        assert_eq!(nums.children[0].path, vec![Seg::Key("nums".into()), Seg::Index(0)]);

        let pt = root.children.iter().find(|n| n.key == "pt").unwrap();
        assert_eq!(pt.kind, NodeKind::InlineTable);
        assert_eq!(pt.children[0].key, "x");

        let item = root.children.iter().find(|n| n.key == "item").unwrap();
        assert_eq!(item.kind, NodeKind::ArrayOfTables);
        assert_eq!(item.children.len(), 2);
        assert_eq!(item.children[0].path, vec![Seg::Key("item".into()), Seg::Index(0)]);
    }

    #[test]
    fn dotted_key_is_single_leaf() {
        let t = tree("a.b.c = 1\n");
        // §4: dotted keys project as a single literal Leaf, not nested branches.
        let root = &t.root;
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].key, "a.b.c");
        assert_eq!(root.children[0].kind, NodeKind::Scalar(ScalarType::Integer));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib project::tests`
Expected: FAIL — `project` not defined.

- [ ] **Step 3: Implement live-key projection**

In `src/model/project.rs` (above tests):

```rust
use crate::model::node::{Node, NodeKind, NodeTree, ScalarType, Seg};
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value};

pub fn project(doc: &DocumentMut, filename: &str) -> NodeTree {
    let mut root = Node::branch(filename.to_string(), NodeKind::Root);
    root.children = project_table(doc.as_table(), &[]);
    NodeTree { root }
}

fn scalar_type(v: &Value) -> ScalarType {
    match v {
        Value::String(_) => ScalarType::String,
        Value::Integer(_) => ScalarType::Integer,
        Value::Float(_) => ScalarType::Float,
        Value::Boolean(_) => ScalarType::Bool,
        Value::Datetime(_) => ScalarType::Datetime,
        Value::Array(_) | Value::InlineTable(_) => unreachable!("handled by item dispatch"),
    }
}

/// Project the key/value entries of a table-like node. `base` is the path of the table itself.
fn project_table(table: &Table, base: &[Seg]) -> Vec<Node> {
    let mut out = Vec::new();
    for (key, item) in table.iter() {
        let mut path = base.to_vec();
        path.push(Seg::Key(key.to_string()));
        out.push(project_item(key, item, path));
    }
    out
}

fn project_item(key: &str, item: &Item, path: Vec<Seg>) -> Node {
    match item {
        Item::Value(Value::Array(arr)) => project_array(key, arr, path),
        Item::Value(Value::InlineTable(it)) => project_inline(key, it, path),
        Item::Value(v) => Node { key: key.to_string(), path, kind: NodeKind::Scalar(scalar_type(v)), children: Vec::new() },
        Item::Table(t) => {
            let mut n = Node::branch(key.to_string(), NodeKind::Table);
            n.path = path.clone();
            n.children = project_table(t, &path);
            n
        }
        Item::ArrayOfTables(aot) => project_aot(key, aot, path),
        Item::None => Node { key: key.to_string(), path, kind: NodeKind::Scalar(ScalarType::String), children: Vec::new() },
    }
}

fn project_array(key: &str, arr: &Array, path: Vec<Seg>) -> Node {
    let mut n = Node::branch(key.to_string(), NodeKind::Array);
    n.path = path.clone();
    for (i, v) in arr.iter().enumerate() {
        let mut p = path.clone();
        p.push(Seg::Index(i));
        n.children.push(project_value(&format!("[{i}]"), v, p));
    }
    n
}

fn project_inline(key: &str, it: &InlineTable, path: Vec<Seg>) -> Node {
    let mut n = Node::branch(key.to_string(), NodeKind::InlineTable);
    n.path = path.clone();
    for (k, v) in it.iter() {
        let mut p = path.clone();
        p.push(Seg::Key(k.to_string()));
        n.children.push(project_value(k, v, p));
    }
    n
}

fn project_aot(key: &str, aot: &ArrayOfTables, path: Vec<Seg>) -> Node {
    let mut n = Node::branch(key.to_string(), NodeKind::ArrayOfTables);
    n.path = path.clone();
    for (i, t) in aot.iter().enumerate() {
        let mut p = path.clone();
        p.push(Seg::Index(i));
        let mut child = Node::branch(format!("[{i}]"), NodeKind::Table);
        child.path = p.clone();
        child.children = project_table(t, &p);
        n.children.push(child);
    }
    n
}

fn project_value(key: &str, v: &Value, path: Vec<Seg>) -> Node {
    match v {
        Value::Array(a) => project_array(key, a, path),
        Value::InlineTable(it) => project_inline(key, it, path),
        other => Node { key: key.to_string(), path, kind: NodeKind::Scalar(scalar_type(other)), children: Vec::new() },
    }
}
```

Note on dotted keys: `toml_edit` exposes `a.b.c = 1` as a single key `"a.b.c"` in the parent table iterator only when written dotted at top level; the `project_table` loop emits it as one leaf, satisfying the §4 literal-leaf rule. (If a future `toml_edit` version splits dotted keys, add a guard that re-joins them — covered by the `dotted_key_is_single_leaf` test.)

- [ ] **Step 4: Wire `TomlDocument::project` to it**

In `src/model/toml_doc.rs`, replace the placeholder body:

```rust
fn project(&self) -> NodeTree {
    crate::model::project::project(&self.doc, &self.filename)
}
```

- [ ] **Step 5: Run to verify pass**

Run: `cargo test --lib project::tests`
Expected: PASS (3 tests). If `dotted_key_is_single_leaf` fails because `toml_edit` split the key, implement the re-join guard mentioned above.

- [ ] **Step 6: Commit**

```bash
git add src/model/
git commit -m "feat(model): project live TOML keys into Node tree (§4)"
```

---

## Task 5: Comment projection — the §7.1 decor⇄Comment mapping (PRINCIPAL RISK)

**Files:**
- Modify: `src/model/project.rs`
- Test: `src/model/project.rs`

This is the spec's named highest-risk piece. Build it test-first against every §7.1 row.

- [ ] **Step 1: Write failing tests, one per §7.1 mapping row + edge cases**

Add to `project::tests`:

```rust
fn keys_of(t: &crate::model::node::NodeTree) -> Vec<String> {
    t.root.children.iter().map(|n| n.key.clone()).collect()
}

#[test]
fn comment_before_item() {
    // row 1: comment before an item -> sibling immediately before it
    let t = tree("# lead\nport = 8080\n");
    assert_eq!(keys_of(&t), vec!["# lead".to_string(), "port".to_string()]);
    assert_eq!(t.root.children[0].kind, NodeKind::Comment("# lead".into()));
}

#[test]
fn comment_between_items() {
    let t = tree("a = 1\n# mid\nb = 2\n");
    assert_eq!(keys_of(&t), vec!["a".to_string(), "# mid".to_string(), "b".to_string()]);
}

#[test]
fn comment_at_end_of_document() {
    // row 4: trailing comment with no following item -> last top-level sibling
    let t = tree("a = 1\n# tail\n");
    assert_eq!(keys_of(&t), vec!["a".to_string(), "# tail".to_string()]);
}

#[test]
fn comment_at_start_of_document() {
    let t = tree("# top\na = 1\n");
    assert_eq!(keys_of(&t), vec!["# top".to_string(), "a".to_string()]);
}

#[test]
fn multiple_comments_in_one_prefix() {
    // edge case: each standalone comment line becomes its own Comment node
    let t = tree("# one\n# two\na = 1\n");
    assert_eq!(keys_of(&t), vec!["# one".to_string(), "# two".to_string(), "a".to_string()]);
}

#[test]
fn comment_only_file() {
    let t = tree("# just\n# comments\n");
    assert_eq!(keys_of(&t), vec!["# just".to_string(), "# comments".to_string()]);
}

#[test]
fn comment_inside_table_before_key() {
    let t = tree("[server]\n# explain\nport = 8080\n");
    let server = &t.root.children[0];
    assert_eq!(server.children.iter().map(|n| n.key.clone()).collect::<Vec<_>>(),
        vec!["# explain".to_string(), "port".to_string()]);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib project::tests`
Expected: the seven new tests FAIL (comments are not yet surfaced).

- [ ] **Step 3: Implement comment extraction from decor**

Add helpers to `src/model/project.rs` and call them while projecting each table scope. The key function parses standalone comment lines out of a decor prefix string:

```rust
/// Pull standalone comment lines out of a decor prefix/trailing string.
/// A standalone comment line is one whose first non-whitespace char is `#`.
/// Blank lines are dropped here (they remain decor on disk); only `#` lines
/// become Comment nodes. Returns the comment texts in order, trimmed of the
/// leading indentation but keeping the `#`.
fn comments_in(decor_text: &str) -> Vec<String> {
    decor_text
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('#'))
        .map(|l| l.to_string())
        .collect()
}

fn comment_node(text: &str, parent: &[Seg], ordinal: usize) -> Node {
    // Comment path: parent path + a synthetic comment marker so it is addressable.
    let mut path = parent.to_vec();
    path.push(Seg::Key(format!("#comment:{ordinal}")));
    Node { key: text.to_string(), path, kind: NodeKind::Comment(text.to_string()), children: Vec::new() }
}
```

Then rewrite `project_table` to interleave comments. For each `(key, item)` the leading comments come from that item's `decor().prefix()`; after the loop, append any comments found in `DocumentMut::trailing()` (top level) or the table's own trailing decor (nested). To read an item's prefix decor, use the key's decor:

```rust
fn project_table(table: &Table, base: &[Seg]) -> Vec<Node> {
    let mut out = Vec::new();
    let mut ordinal = 0usize;
    for (key, item) in table.iter() {
        if let Some(decor) = table.key(key).map(|k| k.leaf_decor()) {
            if let Some(prefix) = decor.prefix().and_then(|r| r.as_str()) {
                for c in comments_in(prefix) {
                    out.push(comment_node(&c, base, ordinal));
                    ordinal += 1;
                }
            }
        }
        let mut path = base.to_vec();
        path.push(Seg::Key(key.to_string()));
        out.push(project_item(key, item, path));
    }
    out
}
```

For the **document-level trailing** comment (row 4) and **comment-only files** (edge case), handle them in `project()` rather than `project_table`, because `DocumentMut::trailing()` is document-scoped:

```rust
pub fn project(doc: &DocumentMut, filename: &str) -> NodeTree {
    let mut root = Node::branch(filename.to_string(), NodeKind::Root);
    root.children = project_table(doc.as_table(), &[]);
    if let Some(trailing) = doc.trailing().as_str() {
        let base_ordinal = root.children.len();
        for (i, c) in comments_in(trailing).into_iter().enumerate() {
            root.children.push(comment_node(&c, &[], base_ordinal + i));
        }
    }
    NodeTree { root }
}
```

**Implementation note for the executor:** the exact `toml_edit` accessor names (`leaf_decor`, `key`, `trailing`, `RawString::as_str`) must be confirmed against `toml_edit 0.22` docs — run `cargo doc --open -p toml_edit` and verify each. The *behavior* (which decor slot holds which comment) is fixed by the §7.1 table; adapt accessor names if the API differs, but keep the tests green. Nested-table trailing comments (row 3: comment after last key before a later `[table]`) are carried by the **following** table's key prefix decor, so they are already handled by the loop above.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib project::tests`
Expected: all projection tests PASS. If `comment_only_file` fails, the comments are in document leading decor rather than `trailing()` — read `doc.as_table().key(..)` is empty, so also scan the document's leading decor; add that branch and re-run.

- [ ] **Step 5: Confirm round-trip still byte-identical**

Run: `cargo test --lib toml_doc::tests`
Expected: PASS — projection is read-only and must not have perturbed serialization.

- [ ] **Step 6: Commit**

```bash
git add src/model/project.rs
git commit -m "feat(model): surface standalone comments as Comment nodes (§7.1 decor mapping)"
```

---

## Task 6: Flatten NodeTree to visible rows — §4 rendering

**Files:**
- Modify: `src/model/node.rs` (add `VisibleRow`, `NodeTree::flatten`)
- Test: `src/model/node.rs`

- [ ] **Step 1: Failing test**

Add to `node::tests`:

```rust
#[test]
fn flatten_respects_expanded_set() {
    // root > server(branch) > port(leaf)
    let mut port = Node::leaf("port", NodeKind::Scalar(ScalarType::Integer));
    port.path = vec![Seg::Key("server".into()), Seg::Key("port".into())];
    let mut server = Node::branch("server", NodeKind::Table);
    server.path = vec![Seg::Key("server".into())];
    server.children = vec![port];
    let mut root = Node::branch("f.toml", NodeKind::Root);
    root.children = vec![server];
    let tree = NodeTree { root };

    // collapsed: only root + server visible (root always shown, expanded)
    let collapsed = tree.flatten(&|_p| false);
    assert_eq!(collapsed.iter().map(|r| r.node.key.clone()).collect::<Vec<_>>(),
        vec!["f.toml".to_string(), "server".to_string()]);

    // expand server -> port appears, depth 2
    let expanded = tree.flatten(&|p| p == &vec![Seg::Key("server".into())]);
    assert_eq!(expanded.len(), 3);
    assert_eq!(expanded[2].node.key, "port");
    assert_eq!(expanded[2].depth, 2);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib node::tests::flatten_respects_expanded_set`
Expected: FAIL — `flatten`/`VisibleRow` missing.

- [ ] **Step 3: Implement flatten**

Add to `src/model/node.rs`:

```rust
#[derive(Clone, Debug)]
pub struct VisibleRow<'a> {
    pub node: &'a Node,
    pub depth: usize,
}

impl NodeTree {
    /// Flatten honoring expanded state. `is_expanded(path)` decides whether a
    /// Branch node's children are shown. The Root is always shown and always
    /// treated as expanded.
    pub fn flatten<'a>(&'a self, is_expanded: &dyn Fn(&Path) -> bool) -> Vec<VisibleRow<'a>> {
        let mut rows = Vec::new();
        fn walk<'a>(n: &'a Node, depth: usize, is_root: bool,
                    is_expanded: &dyn Fn(&Path) -> bool, rows: &mut Vec<VisibleRow<'a>>) {
            rows.push(VisibleRow { node: n, depth });
            let expand = is_root || (n.is_branch() && is_expanded(&n.path));
            if expand {
                for c in &n.children {
                    walk(c, depth + 1, false, is_expanded, rows);
                }
            }
        }
        walk(&self.root, 0, true, is_expanded, &mut rows);
        rows
    }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib node::tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/model/node.rs
git commit -m "feat(model): flatten NodeTree into visible rows by expanded state"
```

---

## Task 7: Mutation enum + Delete — §6 `d`

**Files:**
- Modify: `src/model/document.rs` (add `Mutation`, `Target`, `OnCollision`, errors, `apply` on trait)
- Modify: `src/model/toml_doc.rs` (implement `apply` for `Delete`)
- Test: `src/model/toml_doc.rs`

- [ ] **Step 1: Define mutation types in `src/model/document.rs`**

```rust
use crate::model::node::{Path, Seg};

/// Where an insert/move lands: insert as a child of `parent` at `index`.
#[derive(Clone, Debug)]
pub struct Target {
    pub parent: Path,
    pub index: usize,
}

#[derive(Clone, Copy, Debug)]
pub enum OnCollision {
    Overwrite,
    Rename, // append _2, _3, ...
    Cancel,
}

#[derive(Clone, Debug)]
pub enum Mutation {
    Delete { path: Path },
    Insert { target: Target, toml: String, on_collision: OnCollision },
    Replace { path: Path, toml: String },     // `e`
    Remark { path: Path },                     // toggle
    Move { sources: Vec<Path>, target: Target, on_collision: OnCollision },
}

#[derive(Debug, thiserror::Error)]
pub enum MutateError {
    #[error("path not found")]
    NotFound,
    #[error("key collision: {0}")]
    Collision(String),
    #[error("invalid TOML fragment: {0}")]
    Fragment(String),
    #[error("operation not supported by this format")]
    Unsupported,
}
```

Add `apply` to the trait:

```rust
fn apply(&mut self, m: Mutation) -> Result<(), MutateError>;
```

- [ ] **Step 2: Failing delete test in `src/model/toml_doc.rs`**

```rust
#[test]
fn delete_leaf_and_branch() {
    use crate::model::document::{Mutation};
    use crate::model::node::Seg;
    let mut doc = doc_from_str("a = 1\n[server]\nport = 8080\nhost = \"x\"\n");
    doc.apply(Mutation::Delete { path: vec![Seg::Key("a".into())] }).unwrap();
    assert!(!doc.serialize().contains("a = 1"));
    // delete a whole table (branch) removes its subtree
    doc.apply(Mutation::Delete { path: vec![Seg::Key("server".into())] }).unwrap();
    assert_eq!(doc.serialize().trim(), "");
    assert!(doc.is_dirty());
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test --lib toml_doc::tests::delete_leaf_and_branch`
Expected: FAIL — `apply` unimplemented.

- [ ] **Step 4: Implement a path resolver + `apply` for Delete**

In `src/model/toml_doc.rs`:

```rust
use crate::model::document::{Mutation, MutateError};
use crate::model::node::Seg;
use toml_edit::{Item};

impl TomlDocument {
    /// Remove the item addressed by `path`. Only top-level and nested *table* keys
    /// and array indices are supported (covers MVP node kinds).
    fn remove_at(&mut self, path: &[Seg]) -> Result<(), MutateError> {
        let (parent, last) = path.split_at(path.len().saturating_sub(1));
        let last = last.first().ok_or(MutateError::NotFound)?;
        let table = self.parent_table_mut(parent)?;
        match last {
            Seg::Key(k) => { table.remove(k).ok_or(MutateError::NotFound)?; Ok(()) }
            Seg::Index(_) => Err(MutateError::Unsupported), // array-element delete: Task 9 extends
        }
    }

    /// Walk to the mutable table that directly contains the final segment.
    fn parent_table_mut(&mut self, parent: &[Seg]) -> Result<&mut toml_edit::Table, MutateError> {
        let mut tbl = self.doc.as_table_mut();
        for seg in parent {
            match seg {
                Seg::Key(k) => {
                    tbl = tbl.get_mut(k)
                        .and_then(Item::as_table_mut)
                        .ok_or(MutateError::NotFound)?;
                }
                Seg::Index(_) => return Err(MutateError::Unsupported),
            }
        }
        Ok(tbl)
    }
}

impl crate::model::document::ConfigDocument for TomlDocument {
    // ... existing load/project/serialize/is_dirty stay ...
    // Move the trait methods that already exist; ADD apply:
}
```

Then add `apply` into the existing `impl ConfigDocument for TomlDocument` block:

```rust
    fn apply(&mut self, m: Mutation) -> Result<(), MutateError> {
        match m {
            Mutation::Delete { path } => self.remove_at(&path),
            _ => Err(MutateError::Unsupported), // later tasks fill these in
        }
    }
```

(Array-element delete and the remaining variants are added in Tasks 8–11.)

- [ ] **Step 5: Run to verify pass**

Run: `cargo test --lib toml_doc::tests`
Expected: PASS (delete + earlier round-trip tests).

- [ ] **Step 6: Commit**

```bash
git add src/model/
git commit -m "feat(model): Mutation enum + Delete with path resolver"
```

---

## Task 8: Fragment parse/validate — §8

**Files:**
- Create: `src/model/fragment.rs`
- Modify: `src/model/mod.rs` (`pub mod fragment;`)
- Test: `src/model/fragment.rs`

- [ ] **Step 1: Failing tests**

In `src/model/fragment.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_fragment() {
        let f = parse_fragment("port = 8080\n").unwrap();
        assert_eq!(f.len(), 1);
        assert_eq!(f.keys().next().unwrap(), "port");
    }

    #[test]
    fn parses_table_fragment() {
        let f = parse_fragment("[server]\nport = 8080\n").unwrap();
        assert!(f.contains_key("server"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_fragment("= = nope").is_err());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib fragment::tests`
Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
use crate::model::document::MutateError;
use toml_edit::{DocumentMut, Table};

/// Parse a user-edited TOML fragment into a detached table whose entries can be
/// merged into the document. The fragment is parsed as a standalone TOML doc.
pub fn parse_fragment(src: &str) -> Result<Table, MutateError> {
    let doc = src.parse::<DocumentMut>().map_err(|e| MutateError::Fragment(e.to_string()))?;
    Ok(doc.as_table().clone())
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib fragment::tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/model/fragment.rs src/model/mod.rs
git commit -m "feat(model): TOML fragment parse/validate for e/n (§8)"
```

---

## Task 9: Insert mutation + key-collision — §6.1, §6 collision

**Files:**
- Modify: `src/model/toml_doc.rs`
- Test: `src/model/toml_doc.rs`

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn insert_into_table_and_collision() {
    use crate::model::document::{Mutation, Target, OnCollision};
    use crate::model::node::Seg;
    let mut doc = doc_from_str("[server]\nport = 8080\n");
    let target = Target { parent: vec![Seg::Key("server".into())], index: 1 };
    doc.apply(Mutation::Insert { target: target.clone(), toml: "host = \"x\"\n".into(), on_collision: OnCollision::Cancel }).unwrap();
    assert!(doc.serialize().contains("host = \"x\""));

    // collision: inserting `port` again with Cancel errors
    let err = doc.apply(Mutation::Insert { target: target.clone(), toml: "port = 1\n".into(), on_collision: OnCollision::Cancel });
    assert!(matches!(err, Err(crate::model::document::MutateError::Collision(_))));

    // Overwrite replaces
    doc.apply(Mutation::Insert { target: target.clone(), toml: "port = 1\n".into(), on_collision: OnCollision::Overwrite }).unwrap();
    assert!(doc.serialize().contains("port = 1"));

    // Rename keeps both
    doc.apply(Mutation::Insert { target, toml: "port = 2\n".into(), on_collision: OnCollision::Rename }).unwrap();
    assert!(doc.serialize().contains("port_2 = 2"));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib toml_doc::tests::insert_into_table_and_collision`
Expected: FAIL.

- [ ] **Step 3: Implement insert in `apply`**

Add a method and wire the `Insert` arm:

```rust
impl TomlDocument {
    fn insert_fragment(&mut self, target: &crate::model::document::Target, toml: &str,
                       oc: crate::model::document::OnCollision) -> Result<(), MutateError> {
        use crate::model::document::OnCollision::*;
        let frag = crate::model::fragment::parse_fragment(toml)?;
        let dest = self.parent_table_mut(&target.parent)?;
        for (k, item) in frag.iter() {
            let mut key = k.to_string();
            if dest.contains_key(&key) {
                match oc {
                    Cancel => return Err(MutateError::Collision(key)),
                    Overwrite => { dest.remove(&key); }
                    Rename => {
                        let mut n = 2;
                        while dest.contains_key(&format!("{key}_{n}")) { n += 1; }
                        key = format!("{key}_{n}");
                    }
                }
            }
            dest.insert(&key, item.clone());
        }
        // NOTE: `dest.insert` appends at the end; honoring `target.index` for exact
        // positioning uses Table::position-aware insertion. For MVP, append-then-sort
        // is acceptable because TOML key order within a table is the on-disk order and
        // the insertion-target row index maps to "after cursor"; if exact ordering is
        // required, use `Table::insert` + `Table::sort_values_by` or shift positions.
        Ok(())
    }
}
```

Wire arm: `Mutation::Insert { target, toml, on_collision } => self.insert_fragment(&target, &toml, on_collision),`

**Executor note:** verify whether `toml_edit::Table` preserves insertion order (it does — it is an ordered map). Exact mid-table positioning at `target.index` may need `Table` position APIs; the test above only checks presence, not position. Add a positioning test only if §6.1 ordering proves user-visible in the TUI (Task 16).

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib toml_doc::tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/model/toml_doc.rs
git commit -m "feat(model): Insert mutation with overwrite/rename/cancel collision (§6.1)"
```

---

## Task 10: Replace (`e`) + Remark (`r`) mutations — §7, §8

**Files:**
- Modify: `src/model/toml_doc.rs`
- Test: `src/model/toml_doc.rs`

- [ ] **Step 1: Failing tests (replace + remark round-trip + reject)**

```rust
#[test]
fn replace_node_fragment() {
    use crate::model::document::Mutation;
    use crate::model::node::Seg;
    let mut doc = doc_from_str("port = 8080\n");
    doc.apply(Mutation::Replace { path: vec![Seg::Key("port".into())], toml: "port = 9090\n".into() }).unwrap();
    assert!(doc.serialize().contains("port = 9090"));
}

#[test]
fn remark_toggles_leaf() {
    use crate::model::document::Mutation;
    use crate::model::node::Seg;
    let mut doc = doc_from_str("port = 8080\n");
    // live -> comment
    doc.apply(Mutation::Remark { path: vec![Seg::Key("port".into())] }).unwrap();
    let s = doc.serialize();
    assert!(s.contains("# port = 8080"));
    // comment -> live again (re-parse). Address the comment node by its synthetic path.
    // (Executor: use the path the projector assigns to the comment; see Task 5.)
}

#[test]
fn remark_rejects_non_toml_comment() {
    // a pre-existing prose comment cannot be uncommented into a live node
    use crate::model::document::{Mutation, MutateError};
    let mut doc = doc_from_str("# just prose\n");
    let cpath = doc.project().root.children[0].path.clone();
    let err = doc.apply(Mutation::Remark { path: cpath });
    assert!(matches!(err, Err(MutateError::Fragment(_))));
    // document unchanged
    assert_eq!(doc.serialize(), "# just prose\n");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib toml_doc::tests::replace_node_fragment toml_doc::tests::remark_rejects_non_toml_comment`
Expected: FAIL.

- [ ] **Step 3: Implement Replace and Remark**

Replace = delete at path then insert fragment at the same position (reuse `remove_at` + `insert_fragment` with a Target derived from the path's parent). Remark logic:

- If the node at `path` is a **live** key: read its rendered TOML text (the key + value, plus any nested subtree for a Branch), prefix every line with `# `, delete the live key, and write the commented text into the parent table's decor at that position. (Use the same decor slot the projector reads — Task 5.)
- If the node at `path` is a **Comment**: take its text, strip `# ` from each line, `parse_fragment`; on success delete the comment from decor and insert the parsed item(s) at that position; on failure return `MutateError::Fragment` and leave the document untouched.

```rust
impl TomlDocument {
    fn remark(&mut self, path: &[Seg]) -> Result<(), MutateError> {
        // Determine if `path` addresses a live key or a synthetic comment marker.
        let is_comment = matches!(path.last(), Some(Seg::Key(k)) if k.starts_with("#comment:"));
        if is_comment {
            self.uncomment(path)
        } else {
            self.comment_out(path)
        }
    }
    // comment_out: render node text, prefix "# ", move into decor.
    // uncomment: locate comment text, strip "# ", parse_fragment, insert; reject on parse error.
}
```

**Executor note — this is the §7.1 risk surface.** Rendering a live node's exact text for `comment_out` is done by serializing just that item: clone the `Item`, build a throwaway `Table`/`DocumentMut` containing only it, `to_string()`, then prefix lines. Writing the commented text back as decor uses the same accessor as Task 5's reader (`leaf_decor().set_prefix(...)` or the table-header decor for the only-item case). Keep every Task 5 projection test green after implementing, and add the round-trip assertion for the `remark_toggles_leaf` comment→live half once the synthetic comment path addressing is wired.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib toml_doc::tests`
Expected: PASS. Also run `cargo test --lib project::tests` — comment projection must still pass.

- [ ] **Step 5: Commit**

```bash
git add src/model/toml_doc.rs
git commit -m "feat(model): Replace (e) and Remark (r) mutations (§7, §8)"
```

---

## Task 11: Move mutation + undo/redo snapshots — §6 `m`, `z`/`y`, §6.2

**Files:**
- Modify: `src/model/toml_doc.rs` (Move arm)
- Create: `src/tui/state.rs` (undo/redo via document snapshots)
- Modify: `src/tui/mod.rs` (`pub mod state;`)
- Test: both files

- [ ] **Step 1: Failing Move test (`src/model/toml_doc.rs`)**

```rust
#[test]
fn move_reparents_node() {
    use crate::model::document::{Mutation, Target, OnCollision};
    use crate::model::node::Seg;
    let mut doc = doc_from_str("a = 1\n[dest]\n");
    doc.apply(Mutation::Move {
        sources: vec![vec![Seg::Key("a".into())]],
        target: Target { parent: vec![Seg::Key("dest".into())], index: 0 },
        on_collision: OnCollision::Cancel,
    }).unwrap();
    let s = doc.serialize();
    assert!(s.contains("[dest]"));
    assert!(s.contains("a = 1"));
    // `a` no longer at top level (only under dest)
    assert_eq!(s.matches("a = 1").count(), 1);
}
```

- [ ] **Step 2: Implement Move = capture source items, delete sources, insert at target**

Move arm: for each source path, serialize its item to a TOML fragment, collect, then delete sources (reverse order to keep paths valid), then insert the collected fragments at `target` honoring `on_collision`. This composes Tasks 7+9.

Run: `cargo test --lib toml_doc::tests::move_reparents_node`
Expected: PASS.

- [ ] **Step 3: Failing undo/redo test (`src/tui/state.rs`)**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn undo_redo_restores_snapshots() {
        let mut h = History::new("v0".to_string());
        h.push("v1".to_string()); // snapshot AFTER an action
        h.push("v2".to_string());
        assert_eq!(h.undo(), Some("v1".to_string()));
        assert_eq!(h.undo(), Some("v0".to_string()));
        assert_eq!(h.undo(), None);
        assert_eq!(h.redo(), Some("v1".to_string()));
    }
}
```

- [ ] **Step 4: Implement `History` (string snapshots of serialized document)**

```rust
/// Multi-step undo/redo over full serialized-document snapshots.
/// One snapshot per user action (§6 z/y). UI-state changes never push.
pub struct History {
    past: Vec<String>,
    current: String,
    future: Vec<String>,
}

impl History {
    pub fn new(initial: String) -> Self {
        History { past: Vec::new(), current: initial, future: Vec::new() }
    }
    pub fn push(&mut self, snapshot: String) {
        self.past.push(std::mem::replace(&mut self.current, snapshot));
        self.future.clear();
    }
    pub fn undo(&mut self) -> Option<String> {
        let prev = self.past.pop()?;
        self.future.push(std::mem::replace(&mut self.current, prev.clone()));
        Some(prev)
    }
    pub fn redo(&mut self) -> Option<String> {
        let next = self.future.pop()?;
        self.past.push(std::mem::replace(&mut self.current, next.clone()));
        Some(next)
    }
}
```

Restoring a snapshot reloads the document from the string via `src.parse::<DocumentMut>()`. Add a `TomlDocument::replace_from_str(&mut self, s)` helper for the app to call on undo/redo.

Run: `cargo test --lib state::tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/model/toml_doc.rs src/tui/state.rs src/tui/mod.rs
git commit -m "feat: Move mutation + undo/redo snapshot history (§6, §6.2)"
```

---

## Task 12: CLI args + format detection — §6 CLI

**Files:**
- Replace: `src/cli.rs` (remove the Task 1 stub)
- Test: `src/cli.rs`

- [ ] **Step 1: Failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_non_toml() {
        assert!(detect_format(std::path::Path::new("a.yaml")).is_err());
        assert!(detect_format(std::path::Path::new("a.toml")).is_ok());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib cli::tests`
Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
use anyhow::{bail, Result};
use clap::Parser;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "confy", about = "TUI editor for structured config files")]
struct Args {
    /// Path to the config file to edit
    file: PathBuf,
    /// Override format detection (only `toml` supported in MVP)
    #[arg(long)]
    format: Option<String>,
}

pub enum Format { Toml }

pub fn detect_format(path: &Path) -> Result<Format> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("toml") => Ok(Format::Toml),
        other => bail!("format not yet supported: {:?} (MVP supports .toml only)", other),
    }
}

pub fn run() -> Result<()> {
    let args = Args::parse();
    let fmt = match args.format.as_deref() {
        Some("toml") => Format::Toml,
        Some(other) => anyhow::bail!("format not yet supported: {other}"),
        None => detect_format(&args.file)?,
    };
    let Format::Toml = fmt;
    crate::tui::run(&args.file)
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib cli::tests`
Expected: PASS. (`cargo build` will fail until `tui::run` exists — Task 13.)

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): confy <file> with .toml format detection"
```

---

## Task 13: TUI skeleton — render tree + navigation + expand/collapse — §6 nav, 0/9

**Files:**
- Create: `src/tui/app.rs`, `src/tui/ui.rs`, `src/tui/keys.rs`
- Modify: `src/tui/mod.rs` (add `run`, modules)
- Test: `src/tui/app.rs` (logic tests, headless)

This is where wenv's `src/tui/{app,ui,list,keys}.rs` are the reference implementation for ratatui/crossterm boilerplate (terminal setup/teardown, event loop, draw loop). Mirror its structure.

- [ ] **Step 1: Failing navigation logic test (headless — no terminal)**

In `src/tui/app.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::*;

    fn sample() -> App {
        // build a tree: root > [a(branch: x), b(leaf)]
        let mut x = Node::leaf("x", NodeKind::Scalar(ScalarType::Integer));
        x.path = vec![Seg::Key("a".into()), Seg::Key("x".into())];
        let mut a = Node::branch("a", NodeKind::Table);
        a.path = vec![Seg::Key("a".into())];
        a.children = vec![x];
        let mut b = Node::leaf("b", NodeKind::Scalar(ScalarType::Integer));
        b.path = vec![Seg::Key("b".into())];
        let mut root = Node::branch("f.toml", NodeKind::Root);
        root.children = vec![a, b];
        App::from_tree(NodeTree { root })
    }

    #[test]
    fn cursor_moves_and_expand_reveals_children() {
        let mut app = sample();
        app.rebuild_rows();
        // collapsed: root, a, b
        assert_eq!(app.visible_keys(), vec!["f.toml", "a", "b"]);
        app.cursor_down(); // on `a`
        app.toggle_expand(); // expand a
        app.rebuild_rows();
        assert_eq!(app.visible_keys(), vec!["f.toml", "a", "x", "b"]);
        app.collapse_all();
        app.rebuild_rows();
        assert_eq!(app.visible_keys(), vec!["f.toml", "a", "b"]);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib tui::app::tests`
Expected: FAIL.

- [ ] **Step 3: Implement App state + navigation (no rendering yet)**

`src/tui/app.rs` core (rendering split into `ui.rs`):

```rust
use crate::model::node::{NodeTree, Path, Seg};
use std::collections::HashSet;

pub struct App {
    pub tree: NodeTree,
    pub expanded: HashSet<Path>,
    pub cursor: usize,
    pub rows: Vec<RowSnapshot>, // owned snapshot to avoid borrow issues during input
}

#[derive(Clone)]
pub struct RowSnapshot {
    pub key: String,
    pub path: Path,
    pub depth: usize,
    pub is_branch: bool,
}

impl App {
    pub fn from_tree(tree: NodeTree) -> Self {
        App { tree, expanded: HashSet::new(), cursor: 0, rows: Vec::new() }
    }
    pub fn rebuild_rows(&mut self) {
        let expanded = &self.expanded;
        self.rows = self.tree
            .flatten(&|p| expanded.contains(p))
            .into_iter()
            .map(|r| RowSnapshot {
                key: r.node.key.clone(), path: r.node.path.clone(),
                depth: r.depth, is_branch: r.node.is_branch(),
            })
            .collect();
        if self.cursor >= self.rows.len() { self.cursor = self.rows.len().saturating_sub(1); }
    }
    pub fn visible_keys(&self) -> Vec<String> { self.rows.iter().map(|r| r.key.clone()).collect() }
    pub fn cursor_down(&mut self) { if self.cursor + 1 < self.rows.len() { self.cursor += 1; } }
    pub fn cursor_up(&mut self) { self.cursor = self.cursor.saturating_sub(1); }
    pub fn toggle_expand(&mut self) {
        if let Some(r) = self.rows.get(self.cursor) {
            if r.is_branch {
                if !self.expanded.remove(&r.path) { self.expanded.insert(r.path.clone()); }
            }
        }
    }
    pub fn collapse_all(&mut self) { self.expanded.clear(); }
    pub fn expand_all(&mut self) {
        // expand every branch path
        let mut all = HashSet::new();
        fn walk(n: &crate::model::node::Node, all: &mut HashSet<Path>) {
            if n.is_branch() { all.insert(n.path.clone()); for c in &n.children { walk(c, all); } }
        }
        walk(&self.tree.root, &mut all);
        self.expanded = all;
    }
}
```

- [ ] **Step 4: Implement the render + event loop**

`src/tui/mod.rs` `run(path)`: load `TomlDocument`, `App::from_tree(doc.project())`, set up crossterm raw mode + alternate screen (copy wenv's setup/teardown), loop: `ui::draw(frame, &app)`, read key event, dispatch via `keys::map`. Implement only navigation + expand keys in this task (`j/k/↑/↓/PgUp/PgDn/Home/End/Enter/Space/0/9/q`). `ui::draw` renders `app.rows` with indentation `"  ".repeat(depth)`, a `▸/▾` marker for branches, highlight at `cursor`.

**Verification (manual, real binary):** create `tests/fixtures/sample.toml`, run `cargo run -- tests/fixtures/sample.toml`, confirm: arrows move highlight, Enter expands/collapses a table, `0`/`9` collapse/expand all, `q` quits.

- [ ] **Step 5: Run logic tests + build**

Run: `cargo test --lib tui::app::tests && cargo build`
Expected: PASS + builds. `cargo run -- tests/fixtures/sample.toml` launches.

- [ ] **Step 6: Commit**

```bash
git add src/tui/ tests/fixtures/
git commit -m "feat(tui): tree render + navigation + expand/collapse (§6)"
```

---

## Task 14: Selection + §6.2 normalization — `s`, `Shift+↑/↓`

**Files:**
- Create: `src/tui/selection.rs`
- Modify: `src/tui/app.rs` (hold `Selection`, key handlers), `src/tui/ui.rs` (selection marker)
- Test: `src/tui/selection.rs`

- [ ] **Step 1: Failing test for normalization**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::Seg;
    #[test]
    fn normalize_drops_selected_descendants() {
        // selected: [server], [server.port]  -> port dropped (carried by server)
        let server = vec![Seg::Key("server".into())];
        let port = vec![Seg::Key("server".into()), Seg::Key("port".into())];
        let normalized = normalize(vec![server.clone(), port]);
        assert_eq!(normalized, vec![server]);
    }
}
```

- [ ] **Step 2: Run to verify failure → Step 3: Implement**

```rust
use crate::model::node::Path;

/// Drop any selected path that is a descendant of another selected path (§6.2).
pub fn normalize(mut paths: Vec<Path>) -> Vec<Path> {
    paths.sort_by_key(|p| p.len());
    let mut kept: Vec<Path> = Vec::new();
    for p in paths {
        let is_descendant = kept.iter().any(|anc| p.len() > anc.len() && p.starts_with(anc));
        if !is_descendant { kept.push(p); }
    }
    kept
}
```

Add a `Selection { indices: HashSet<usize>, anchor: Option<usize> }` for `s` toggle and `Shift+↑/↓` range over `app.rows`; map row indices → paths, then `normalize` before any clipboard/move op.

Run: `cargo test --lib tui::selection::tests`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/tui/selection.rs src/tui/app.rs src/tui/ui.rs
git commit -m "feat(tui): multi-select + range select + §6.2 normalization"
```

---

## Task 15: Insertion-target resolution — §6.1

**Files:**
- Create: `src/tui/insertion.rs`
- Test: `src/tui/insertion.rs`

- [ ] **Step 1: Failing tests covering every §6.1 row**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::{Seg};
    use crate::tui::app::RowSnapshot;

    fn row(key: &str, path: Vec<Seg>, branch: bool, depth: usize) -> RowSnapshot {
        RowSnapshot { key: key.into(), path, depth, is_branch: branch }
    }

    #[test]
    fn leaf_inserts_after_in_parent() {
        // cursor on server.port (leaf) -> parent=server, index=after port
        let cursor = row("port", vec![Seg::Key("server".into()), Seg::Key("port".into())], false, 2);
        let t = resolve_target(&cursor, /*expanded*/ false, /*sibling_index*/ 1);
        assert_eq!(t.parent, vec![Seg::Key("server".into())]);
        assert_eq!(t.index, 2);
    }

    #[test]
    fn expanded_branch_inserts_as_first_child() {
        let cursor = row("server", vec![Seg::Key("server".into())], true, 1);
        let t = resolve_target(&cursor, true, 0);
        assert_eq!(t.parent, vec![Seg::Key("server".into())]);
        assert_eq!(t.index, 0);
    }

    #[test]
    fn collapsed_branch_inserts_after_sibling() {
        let cursor = row("server", vec![Seg::Key("server".into())], true, 1);
        let t = resolve_target(&cursor, false, 3);
        assert_eq!(t.parent, Vec::<Seg>::new());
        assert_eq!(t.index, 4);
    }

    #[test]
    fn root_inserts_as_first_top_level() {
        let cursor = row("f.toml", vec![], true, 0);
        let t = resolve_target(&cursor, true, 0);
        assert_eq!(t.parent, Vec::<Seg>::new());
        assert_eq!(t.index, 0);
    }
}
```

- [ ] **Step 2: Run → fail → Step 3: Implement**

```rust
use crate::model::document::Target;
use crate::model::node::Seg;
use crate::tui::app::RowSnapshot;

/// Resolve where `n`/`v`/`m` land, per spec §6.1.
/// `expanded` = whether the cursor branch is expanded.
/// `sibling_index` = index of the cursor node within its parent's children.
pub fn resolve_target(cursor: &RowSnapshot, expanded: bool, sibling_index: usize) -> Target {
    let is_root = cursor.path.is_empty();
    if is_root || (cursor.is_branch && expanded) {
        // into the branch as first child
        Target { parent: cursor.path.clone(), index: 0 }
    } else {
        // sibling immediately after cursor, in cursor's parent
        let parent = cursor.path[..cursor.path.len().saturating_sub(1)].to_vec();
        Target { parent, index: sibling_index + 1 }
    }
}
```

Run: `cargo test --lib tui::insertion::tests`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/tui/insertion.rs
git commit -m "feat(tui): §6.1 insertion-target resolution"
```

---

## Task 16: $EDITOR integration — `e`, `n` (§8)

**Files:**
- Create: `src/tui/editor.rs`
- Test: `src/tui/editor.rs` (use a fake editor via `$EDITOR=cat`-style script)

- [ ] **Step 1: Implement editor round-trip helper (TDD with a scripted editor)**

```rust
use anyhow::{Context, Result};
use std::io::Write;
use std::process::Command;

/// Open `initial` in $EDITOR (fallback $VISUAL, then nano/vi/notepad), return edited text.
pub fn edit_text(initial: &str) -> Result<String> {
    let mut tmp = tempfile::Builder::new().suffix(".toml").tempfile()?;
    tmp.write_all(initial.as_bytes())?;
    let path = tmp.path().to_path_buf();
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| default_editor());
    let status = Command::new(&editor).arg(&path).status()
        .with_context(|| format!("launching editor: {editor}"))?;
    anyhow::ensure!(status.success(), "editor exited non-zero");
    Ok(std::fs::read_to_string(&path)?)
}

fn default_editor() -> String {
    if cfg!(windows) { "notepad".into() } else { "nano".into() }
}
```

Test (uses a tiny shell script that appends a line, set via `$EDITOR`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edit_text_reads_back_editor_output() {
        // editor = `sh -c 'echo overwrite > "$1"' --` is awkward cross-platform;
        // instead point EDITOR at a script that truncates+writes a fixed value.
        let script = tempfile::Builder::new().suffix(".sh").tempfile().unwrap();
        std::fs::write(script.path(), "#!/bin/sh\necho 'port = 9090' > \"$1\"\n").unwrap();
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(script.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
            std::env::set_var("EDITOR", script.path());
            let out = edit_text("port = 8080\n").unwrap();
            assert_eq!(out.trim(), "port = 9090");
        }
    }
}
```

Run: `cargo test --lib tui::editor::tests`
Expected: PASS on unix (gate the test with `#[cfg(unix)]`).

- [ ] **Step 2: Wire `e` and `n` in `app.rs`**

- `e`: get cursor node's TOML fragment text (serialize just that item — reuse the helper from Task 10), `edit_text`, then `doc.apply(Replace { path, toml })`; on `MutateError::Fragment` show the error in the status line, leave doc unchanged.
- `n`: `edit_text("")`, resolve `Target` via Task 15, `doc.apply(Insert { target, toml, on_collision: Cancel })`; on `Collision` trigger the prompt (Task 17).
- After any successful mutation: push undo snapshot (`history.push(doc.serialize())`), `app.tree = doc.project()`, `app.rebuild_rows()`.

**Verification (real binary):** `EDITOR=nano cargo run -- tests/fixtures/sample.toml`; press `e` on a scalar, change the value, save+exit nano, confirm the tree updates and the value changed; press `n`, type `newkey = 1`, confirm it appears.

- [ ] **Step 3: Commit**

```bash
git add src/tui/editor.rs src/tui/app.rs
git commit -m "feat(tui): $EDITOR integration for e/n with validation (§8)"
```

---

## Task 17: Operations wiring — `d`/`x`/`c`/`v`/`m`/`r` + collision & move-pending prompts

**Files:**
- Modify: `src/tui/app.rs`, `src/tui/state.rs` (Clipboard, Mode), `src/tui/ui.rs` (prompt overlay)
- Test: `src/tui/app.rs` (logic tests for clipboard/move state machine, headless)

- [ ] **Step 1: Failing logic test for cut/paste flatten + move-pending**

```rust
#[test]
fn cut_then_paste_moves_node() {
    // headless: drive app methods, assert on doc.serialize via app.doc
    // (App owns the TomlDocument in the full build.)
    // 1) select `a`, cut -> clipboard has a, doc unchanged until paste
    // 2) move cursor into [dest] expanded, paste -> a now under dest
    // Assert app.doc.serialize() contains dest with a.
}
```

(Executor: flesh out using `App`'s real methods; assert via the owned document's `serialize()`.)

- [ ] **Step 2: Implement**

State additions:

```rust
pub enum Mode { Normal, MovePending { sources: Vec<crate::model::node::Path> }, Prompt(PromptKind) }
pub enum PromptKind { Collision { key: String }, ConfirmQuit }
pub struct Clipboard { pub fragments: Vec<String>, pub cut: bool }
```

Key handlers (all push an undo snapshot on success, re-project, rebuild rows):
- `d`: normalize selection (or cursor), `Delete` each (reverse path order).
- `c`: serialize selected nodes' fragments into `Clipboard { cut: false }`.
- `x`: same as `c` but `cut: true`; defer deletion until paste (wenv-style) OR delete now and hold — choose delete-on-paste to match wenv; document the choice.
- `v`: resolve `Target` (Task 15), `Insert` each clipboard fragment with `OnCollision::Cancel`; if it returns `Collision`, switch to `Mode::Prompt(Collision{key})`; on user choice re-issue with Overwrite/Rename/Cancel. If clipboard was a cut, delete sources after successful paste.
- `m`: first press → `Mode::MovePending { sources: normalized }`; second `m` → resolve target, `doc.apply(Move{..})`; `Esc` → back to Normal.
- `r`: `doc.apply(Remark { path: cursor.path })`; on `Fragment` error show "not valid TOML, kept as comment".
- `z`/`y`: `history.undo()/redo()` → `doc.replace_from_str(snapshot)` → re-project + rebuild.

`ui.rs`: render a prompt overlay for `Mode::Prompt` (Collision: keys `o`verwrite / `r`ename / `c`ancel; ConfirmQuit: `y`/`n`).

**Verification (real binary):** on `sample.toml`: select two scalars in different tables, `x`, navigate into a third table (expanded), `v` → both appear there (flatten §6.2); trigger a paste collision → prompt appears, choose overwrite/rename; `r` on a key → it becomes `# key = ...`; `r` again → returns; `z` undoes the last op, `y` redoes.

- [ ] **Step 3: Run logic tests + manual verify → Commit**

```bash
git add src/tui/
git commit -m "feat(tui): wire d/x/c/v/m/r/z/y with collision + move-pending prompts (§6)"
```

---

## Task 18: Detail view (Leaf) + Filter (`/`) + Help (`?`)

**Files:**
- Create: `src/tui/search.rs`
- Modify: `src/tui/app.rs`, `src/tui/ui.rs`, `src/tui/keys.rs`
- Test: `src/tui/search.rs`

- [ ] **Step 1: Failing filter test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn haystack_includes_path_and_value() {
        // leaf server.port = 8080 -> haystack contains "server.port" and "8080"
        let h = haystack(&["server", "port"], Some("8080"), None);
        assert!(h.contains("server.port"));
        assert!(h.contains("8080"));
    }
    #[test]
    fn matches_filter() {
        assert!(fuzzy_match("server.port 8080", "srvport"));
        assert!(!fuzzy_match("server.host", "zzz"));
    }
}
```

- [ ] **Step 2: Implement haystack + fuzzy match (uses `fuzzy-matcher`)**

```rust
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

pub fn haystack(path_keys: &[&str], leaf_value: Option<&str>, comment: Option<&str>) -> String {
    let mut s = path_keys.join(".");
    if let Some(v) = leaf_value { s.push(' '); s.push_str(v); }
    if let Some(c) = comment { s.push(' '); s.push_str(c); }
    s
}

pub fn fuzzy_match(haystack: &str, needle: &str) -> bool {
    SkimMatcherV2::default().fuzzy_match(haystack, needle).is_some()
}
```

Filter behavior in `app.rs`: `/` opens an input line; on each keystroke, recompute which node paths match; a node is visible if it matches OR is an ancestor of a match (keep context). `rebuild_rows` filters accordingly. `Esc` clears.

- [ ] **Step 3: Detail view + Help**

- Detail (Leaf only): `Enter`/`Space` on a Leaf opens a read-only popup showing dotted path, `ScalarType`, value, and trailing comment if present. Branch `Enter` only toggles expand (Task 13).
- Help (`?`): overlay listing all keybindings (copy the §6 table). Mirror wenv's help popup.

**Verification (real binary):** `/` then type a value substring → tree filters to matches + ancestors; `Esc` restores; `Enter` on a leaf shows detail; `?` shows help.

- [ ] **Step 4: Run tests + Commit**

```bash
cargo test --lib tui::search::tests
git add src/tui/
git commit -m "feat(tui): detail view, fuzzy filter (/), help (?)"
```

---

## Task 19: Save (`w`/`Ctrl+s`) + quit confirm (`q`) — §9

**Files:**
- Modify: `src/tui/app.rs`, `src/tui/ui.rs`

- [ ] **Step 1: Implement save + quit**

- `w` / `Ctrl+s`: if `doc.is_dirty()`, write `doc.serialize()` to the original path; on I/O error show the error and keep dirty; on success show "Saved" and update the in-memory `original` baseline so `is_dirty()` resets.
- `q`: if `doc.is_dirty()`, enter `Mode::Prompt(ConfirmQuit)` (`y` quit without saving / `n` cancel); else quit.

Add `TomlDocument::mark_saved(&mut self)` to reset the dirty baseline (`self.original = self.serialize()`).

- [ ] **Step 2: Verification (real binary)**

Edit a value, `w`, quit, re-open the file → change persisted and other bytes unchanged (eyeball or `git diff` the fixture). Edit again and `q` → confirm prompt appears.

- [ ] **Step 3: Commit**

```bash
git add src/tui/
git commit -m "feat(tui): save + dirty-aware quit confirm (§9)"
```

---

## Task 20: Integration test — real binary round-trip — §10

**Files:**
- Create: `tests/roundtrip.rs`, `tests/fixtures/sample.toml`, `tests/fixtures/expected_after_edit.toml`

Because the TUI is interactive, drive the *model* end-to-end through the public API in an integration test (no PTY needed), then assert the serialized output matches a committed fixture. This validates the spec's §2 round-trip on the real crate.

- [ ] **Step 1: Write the integration test**

```rust
use confy::model::document::{ConfigDocument, Mutation, Target, OnCollision};
use confy::model::node::Seg;
use confy::model::toml_doc::TomlDocument;

#[test]
fn untouched_file_roundtrips_byte_identical() {
    let src = include_str!("fixtures/sample.toml");
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("sample.toml");
    std::fs::write(&p, src).unwrap();
    let doc = TomlDocument::load(&p).unwrap();
    assert_eq!(doc.serialize(), src);
}

#[test]
fn edit_one_value_leaves_other_bytes_untouched() {
    let src = include_str!("fixtures/sample.toml");
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("sample.toml");
    std::fs::write(&p, src).unwrap();
    let mut doc = TomlDocument::load(&p).unwrap();
    doc.apply(Mutation::Replace {
        path: vec![Seg::Key("server".into()), Seg::Key("port".into())],
        toml: "port = 9090\n".into(),
    }).unwrap();
    let expected = include_str!("fixtures/expected_after_edit.toml");
    assert_eq!(doc.serialize(), expected);
}
```

- [ ] **Step 2: Create fixtures**

`tests/fixtures/sample.toml`:
```toml
# confy sample
title = "demo"

[server]
host = "0.0.0.0"  # bind address
port = 8080

[[plugin]]
name = "a"
```

`tests/fixtures/expected_after_edit.toml`: same file with `port = 8080` → `port = 9090`, everything else byte-identical (comments, blank lines, trailing comment preserved).

- [ ] **Step 3: Run**

Run: `cargo test --test roundtrip`
Expected: PASS. If `edit_one_value...` fails, diff the output — any change outside the `port` line is a round-trip regression to fix in `toml_doc`.

- [ ] **Step 4: Commit**

```bash
git add tests/
git commit -m "test: integration round-trip on real crate (§10)"
```

---

## Task 21: Docs — README, CHANGELOG, CLAUDE.md

**Files:**
- Modify: `README.md`, `CHANGELOG.md`
- Create: `CLAUDE.md`

- [ ] **Step 1: Update README** — replace the "early planning" status with usage (`confy <file.toml>`), the §6 keybinding table, and the round-trip/single-file/TOML-first scope.

- [ ] **Step 2: Append CHANGELOG `Unreleased`** — summarize the MVP (single-file TOML editor; tree nav/selection/editing; round-trip; remark; undo/redo).

- [ ] **Step 3: Create CLAUDE.md** — build/test commands (`cargo build/test/clippy/fmt/run`), the architecture summary (CST projection, `ConfigDocument` trait, module map), and a pointer to `CONTEXT.md` for terminology.

- [ ] **Step 4: Commit**

```bash
git add README.md CHANGELOG.md CLAUDE.md
git commit -m "docs: README usage, CHANGELOG, CLAUDE.md for confy MVP"
```

---

## Final Verification Checklist

- [ ] `cargo fmt --check && cargo clippy -- -D warnings` clean
- [ ] `cargo test` all green (unit + integration)
- [ ] `cargo run -- tests/fixtures/sample.toml` exercises every §6 key by hand
- [ ] Round-trip: edit one value, save, `git diff` the fixture shows only that line changed
- [ ] Remark a key, save, reopen → it is a comment; remark back → live again
- [ ] Spec §2 success criteria 1–6 each demonstrably met

---

## Self-Review (author's notes)

**Spec coverage:** §3 architecture → Tasks 3–5; §3.1 trait → Task 3/7; §4 model → Tasks 2,4,5,6; §6 keys → Tasks 13,14,16,17,18,19; §6.1 → Task 15; §6.2 → Task 14/17; §7+§7.1 → Tasks 5,10; §8 → Tasks 8,16; §9 → Task 19; §10 → every task's tests + Task 20. No spec section is unmapped.

**Known soft spots flagged inline for the executor (not placeholders — real API-confirmation points):** (a) exact `toml_edit 0.22` decor accessor names in Tasks 5/10; (b) exact mid-table insertion positioning in Task 9. Both have green-test definitions of done; only accessor *names* may differ.

**Type consistency:** `Node`/`NodeKind`/`Seg`/`Path`/`NodeTree` (Task 2), `Mutation`/`Target`/`OnCollision`/`MutateError` (Task 7), `RowSnapshot` (Task 13) are used consistently in every later task.
