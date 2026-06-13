# YAML Subset Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a lossless YAML-subset backend (`src/model/yaml/`) to confy as a third `ConfigDocument`, with out-of-subset constructs degrading to read-only opaque nodes, wired through `AnyDocument` and the TUI exactly as the JSON/JSONC backend already is.

**Architecture:** Mirror the JSON backend's six-file trio (`syntax`/`parse`/`doc`/`project`/`edit` + `mod`). The **parser spike already passed** (spec §3.3 gate — `syntax.rs`+`parse.rs`+11-file corpus landed and green) and is the seed for `parse.rs`. Indentation is in the token stream (`INDENT`), so `serialize()` stays token concatenation; the splice layer's novelty over JSON is an **indent engine** (re-indent a fragment to its destination depth) replacing JSON's comma-normalization. Atomic-commit + `validate_semantics` backstop copied from `json/doc.rs`. Opaque nodes use the existing `Node.read_only` flag (added in Phase 2).

**Tech Stack:** Rust, `rowan = "=0.15.18"` (already a direct dep), `ratatui`/`crossterm` TUI, `thiserror`, `anyhow`. Test with `cargo test`; lint `cargo clippy --all-targets -- -D warnings`; format `cargo fmt`.

**Reference template (read before starting):** the JSON backend is the line-for-line model. Key files and the functions to mirror:
- `src/model/json/doc.rs` — `JsonDocument` load/serialize/apply/facets, `kind_options`.
- `src/model/json/project.rs` — `walk`/`build_value_node`/`walk_container_tokens`, the resolver `Target` enum + `JsonIndex`, comment merging, golden tests.
- `src/model/json/edit.rs` — `resolve`/`apply` dispatcher + one fn per `Mutation` variant (`replace`/`delete`/`insert`/`rename`/`remark`/`edit_comment`/`insert_comment`/`move_nodes`/`convert_kind`), `validate_semantics`, `serialize_fragment`.
- `src/model/any_doc.rs` — enum dispatch (`delegate!` macro, `load_as`, `enable_comments`).
- `src/tui/app.rs:2514` `type_tag`; `src/tui/type_filter.rs` `TypeToken`/`classify`/`Group`/`layout`/`nav_rows`; `src/tui/keys.rs:88` `help_text`; `src/cli.rs` `parse_format`.

**Calibration note:** model/wiring/projection/tag tasks below carry complete code. The `edit.rs` mutation tasks (the largest surface) give the function contract, the **exact JSON analog to copy**, the YAML-specific delta (indent engine), and complete test code — the implementer copies the JSON function and swaps comma/brace normalization for the indent engine rather than transcribing 1500 lines here. This is deliberate: the JSON functions are in-repo and battle-tested.

---

## Hard constraints (from spec §Phase 3 — already decided, do not re-litigate)

- **Subset:** single document (optional leading `---`); block + **single-line** flow map/seq; 5 scalar styles (plain / single / double / `|` literal / `>` folded incl. `+`/`-` chomping); `#` comments (standalone merge like TOML, end-of-line → `trailing_comment`); YAML 1.2 **core schema** typing (`null`/`~`, bool, int dec·`0x`·`0o`, float plain·exp·`.inf`·`.nan`, else string) — **no datetime** (date-looking scalars are strings).
- **Opaque (read-only):** `&anchor`, `*alias`, `<<:` merge, `!tag`/`!!tag`, exotic block headers, **multi-line flow** → whole value span = one `OPAQUE`/read-only node; every mutation touching or into it → `MutateError::Unsupported`, document untouched; **copy allowed** (raw text fragment). Mutations elsewhere splice around it.
- **Multi-document** (`---` ≥2) → **rejected at load**.
- `r` remark uses `#`; comments are `#`. A keyed fragment pasted into a sequence becomes a `- ` block mapping (idiomatic YAML, not flow `{ }`); an element mapping pasted into a mapping unpacks into members. Block nodes captured with full indented extent; paste re-indents to destination.

---

## File structure

```
src/model/yaml/
  mod.rs        re-exports (extend: + doc, project, edit; doc::YamlDocument)
  syntax.rs     SyntaxKind + Language     [EXISTS from spike — keep]
  parse.rs      lexer + parser → green tree [EXISTS from spike — harden in Task 2]
  doc.rs        YamlDocument: load/serialize/apply (atomic) + facets + kind_options  [NEW]
  project.rs    green tree → NodeTree + resolver index + golden tests               [NEW]
  edit.rs       resolve + apply dispatcher + one fn per Mutation (indent engine)     [NEW]
tests/
  fixtures/yaml/*.yaml   [EXISTS from spike — reused as roundtrip corpus]
  roundtrip_yaml.rs      integration: byte-identical roundtrip + mutation lossless   [NEW]
```

Model/TUI files modified (additive arms only): `model/node.rs` (Format variants), `model/document.rs` (KindTarget variants), `model/any_doc.rs`, `model/mod.rs` (already has `pub mod yaml`), `tui/app.rs` (`type_tag` + DocFormat thread), `tui/type_filter.rs` (`TypeToken`/`classify`/`Group`/`layout`/`nav_rows`), `tui/keys.rs` (`YAML_HELP`), `cli.rs` (drop the YAML bail), `CHANGELOG.md`/`CLAUDE.md`/`CONTEXT.md`/`README.md`/`Cargo.toml`.

**Design decision — flow KIND tags need `DocFormat`.** Spec §3.4 says YAML flow collections *reuse* `Format::Inline` (only block adds `Format::Block`). But an `Inline` array must render `[A/I]` for JSON/TOML and `[A/F]` for YAML — `(kind, format)` alone can't tell them apart. **Resolution (recommended): thread `DocFormat` into `type_tag` and `classify`.** Both already live next to a loaded doc whose format is known. This keeps the spec's "reuse Inline" and is the minimal model change; it touches ~4 call sites (the `type_tag` caller in `app.rs`, the `classify` callers in `type_filter.rs`, and the invariant test). Tasks 7–8 implement this.

---

## Task 1: Model atoms — `Format` + `KindTarget` variants

**Files:**
- Modify: `src/model/node.rs:29-56` (`Format` enum)
- Modify: `src/model/document.rs:133-153` (`KindTarget` enum)

- [ ] **Step 1: Add YAML `Format` variants**

In `src/model/node.rs`, extend `Format` (after `Dotted`):

```rust
    // YAML containers / scalar styles (block collections + 4 explicit string
    // styles; flow collections reuse `Inline`, plain scalars stay `Plain`).
    /// YAML block mapping/sequence (`key:\n  …`, `- …`). Rendered [T/B]/[A/B].
    Block,
    /// YAML 'single quoted' scalar.
    SingleQuoted,
    /// YAML "double quoted" scalar.
    DoubleQuoted,
    /// YAML literal block scalar `|` (newlines preserved).
    LiteralBlock,
    /// YAML folded block scalar `>` (newlines folded).
    Folded,
```

- [ ] **Step 2: Add YAML `KindTarget` variants**

In `src/model/document.rs`, extend `KindTarget` (after `TableMultiline`):

```rust
    /// YAML flow collection (single-line `{ }` / `[ ]`).
    Flow,
    /// YAML block collection (`key:\n  …` / `- …`).
    Block,
    StringPlain,
    StringSingle,
    StringDouble,
    StringLiteralBlock,
    StringFolded,
```

- [ ] **Step 3: Compile — find every non-exhaustive match the new variants break**

Run: `cargo build 2>&1 | rg "non-exhaustive|not covered" -A2`
Expected: errors at `tui/app.rs` `type_tag`, `tui/type_filter.rs` `classify`. (Both have catch-all arms for scalars/containers today, so most are unaffected; confirm exactly which need arms — they get them in Tasks 7–8. If the build is clean, the catch-alls already absorb the new variants and Tasks 7–8 add the *distinguishing* arms.)

- [ ] **Step 4: Commit**

```bash
git add src/model/node.rs src/model/document.rs
git commit -m "feat(model): YAML Format + KindTarget variants"
```

---

## Task 2: Harden the spike parser into the production lexer/parser

The spike (`src/model/yaml/parse.rs`) already passes the §3.3 gate (lossless lex, byte-identical roundtrip on 10 files, opaque fencing, multi-doc reject). This task locks in correctness with a broader inline test set and removes the `#[allow(dead_code)]` once `doc.rs` consumes `parse`.

**Files:**
- Modify: `src/model/yaml/parse.rs`
- Test: `src/model/yaml/parse.rs` (existing `#[cfg(test)] mod tests`)

- [ ] **Step 1: Add edge-case roundtrip tests (write first, expect some to fail)**

Append to the test module:

```rust
#[test]
fn roundtrips_edge_cases() {
    for src in [
        "a: b: c\n",                       // colon in plain value
        "url: http://example.com\n",       // colon-not-indicator stays plain
        "tpl: ${{ matrix.os }}\n",         // braces mid-plain-scalar are literal
        "empty:\n",                         // implicit null value
        "nested:\n  - - 1\n  - - 2\n",     // compact nested sequence
        "k: 'a''b'\n",                      // single-quote escape
        "k: \"a\\\"b\"\n",                 // double-quote escape
        "- - a\n- - b\n",                   // sequence of sequences
        "a:\n- x\n- y\n",                   // seq at key indent (block, same column)
        "x: |-\n  no trailing nl\n",        // chomping indicator
        "  # leading-indented comment\nk: 1\n",
    ] {
        let green = parse(src).unwrap_or_else(|e| panic!("parse {src:?}: {e}"));
        assert_eq!(
            crate::model::yaml::syntax::SyntaxNode::new_root(green).to_string(),
            src,
            "roundtrip {src:?}"
        );
    }
}
```

- [ ] **Step 2: Run and fix any roundtrip failures in `lex`/`parse`**

Run: `cargo test --lib yaml::parse 2>&1 | tail -30`
Expected: all green. Fix only the lexer/parser arms a failing case exposes (e.g. a colon-stop edge); do **not** restructure passing logic. Round-trip is the gate — every byte must be bumped exactly once.

- [ ] **Step 3: Commit**

```bash
git add src/model/yaml/parse.rs
git commit -m "test(yaml): harden lexer/parser edge cases"
```

---

## Task 3: `YamlDocument` — load / serialize / facets (project/apply/kind_options stubbed)

Mirror `json/doc.rs` exactly. Stub `project`→empty tree and `apply`→`Unsupported` so the file compiles before Tasks 4–6 fill them (same staging the JSON backend used at commit `d81dcc3`).

**Files:**
- Create: `src/model/yaml/doc.rs`
- Modify: `src/model/yaml/mod.rs` (add `pub mod doc;` + `pub use doc::YamlDocument;`)
- Test: `src/model/yaml/doc.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the doc with stubs + facet tests**

```rust
//! `YamlDocument` — the lossless YAML-subset backend (mirrors `json/doc.rs`).

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::model::document::{ConfigDocument, DocFormat, KindTarget, MutateError, Mutation};
use crate::model::node::{NodeKind, NodeTree, Seg};
use crate::model::yaml::syntax::SyntaxNode;

pub struct YamlDocument {
    pub(crate) syntax: SyntaxNode,
    pub(crate) path: PathBuf,
    pub(crate) original: String,
    pub(crate) filename: String,
}

impl ConfigDocument for YamlDocument {
    fn load(path: &Path) -> anyhow::Result<Self> {
        let original =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let green = crate::model::yaml::parse::parse(&original)
            .map_err(|e| anyhow::anyhow!("parsing {} as YAML: {}", path.display(), e))?;
        let syntax = SyntaxNode::new_root(green);
        let filename = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        Ok(YamlDocument { syntax, path: path.to_path_buf(), original, filename })
    }

    fn project(&self) -> NodeTree {
        crate::model::yaml::project::project(&self.syntax, &self.filename)
    }
    fn serialize(&self) -> String {
        self.syntax.to_string()
    }
    fn is_dirty(&self) -> bool {
        self.serialize() != self.original
    }
    fn serialize_fragment(&self, path: &[Seg]) -> String {
        if path.is_empty() {
            return self.serialize();
        }
        crate::model::yaml::edit::serialize_fragment(&self.syntax, path)
    }
    fn serialize_fragment_relative(&self, path: &[Seg]) -> String {
        // YAML has no dotted scope tables; relative == absolute fragment.
        self.serialize_fragment(path)
    }
    fn apply(&mut self, m: Mutation) -> Result<(), MutateError> {
        let new = crate::model::yaml::edit::apply(&self.syntax, m)?;
        let text = new.to_string();
        let green = crate::model::yaml::parse::parse(&text).map_err(MutateError::Fragment)?;
        self.syntax = SyntaxNode::new_root(green);
        Ok(())
    }
    fn format(&self) -> DocFormat {
        DocFormat::Yaml
    }
    fn comment_prefix(&self) -> &'static str {
        "#"
    }
    fn supports_comments(&self) -> bool {
        true
    }
    fn kind_options(&self, path: &[Seg]) -> Vec<(String, KindTarget)> {
        kind_options(&self.project(), path)
    }
    fn scalar_fragment(&self, key: Option<&str>, value: &str) -> String {
        match key {
            Some(k) => format!("{k}: {value}\n"),
            None => format!("- {value}\n"),
        }
    }
    fn value_kind(&self, value: &str) -> Result<NodeKind, String> {
        // Project the value as the sole member of a mapping and read its kind.
        let green = crate::model::yaml::parse::parse(&format!("__k__: {value}\n"))?;
        crate::model::yaml::project::project(&SyntaxNode::new_root(green), "")
            .root
            .children
            .into_iter()
            .next()
            .map(|n| n.kind)
            .ok_or_else(|| "fragment has no value".into())
    }
}

impl YamlDocument {
    pub fn save(&self) -> std::io::Result<()> {
        std::fs::write(&self.path, self.serialize())
    }
    pub fn mark_saved(&mut self) {
        self.original = self.serialize();
    }
    pub fn replace_from_str(&mut self, s: &str) -> Result<(), MutateError> {
        let green = crate::model::yaml::parse::parse(s).map_err(MutateError::Fragment)?;
        self.syntax = SyntaxNode::new_root(green);
        Ok(())
    }
}

/// Per-node convertible-kind list (current notation excluded). See Task 6 for
/// the body; stubbed empty until then.
pub(crate) fn kind_options(_tree: &NodeTree, _path: &[Seg]) -> Vec<(String, KindTarget)> {
    Vec::new()
}
```

Add facet tests mirroring `json/doc.rs::tests` (`roundtrip_and_facets`, `load_rejects_multi_doc`, `scalar_fragment_uses_yaml_forms`, `value_kind_classifies_yaml_values`). Example:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn yaml_from_str(s: &str) -> YamlDocument {
        let mut f = tempfile::Builder::new().suffix(".yaml").tempfile().unwrap();
        f.write_all(s.as_bytes()).unwrap();
        YamlDocument::load(f.path()).unwrap()
    }
    #[test]
    fn roundtrip_and_facets() {
        let src = "a: 1\nb: two\n";
        let doc = yaml_from_str(src);
        assert_eq!(doc.serialize(), src);
        assert!(!doc.is_dirty());
        assert_eq!(doc.format(), DocFormat::Yaml);
        assert_eq!(doc.comment_prefix(), "#");
        assert!(doc.supports_comments());
    }
    #[test]
    fn load_rejects_multi_doc() {
        let mut f = tempfile::Builder::new().suffix(".yaml").tempfile().unwrap();
        f.write_all(b"---\na: 1\n---\nb: 2\n").unwrap();
        assert!(YamlDocument::load(f.path()).is_err());
    }
    #[test]
    fn scalar_fragment_uses_yaml_forms() {
        let doc = yaml_from_str("a: 1\n");
        assert_eq!(doc.scalar_fragment(Some("k"), "v"), "k: v\n");
        assert_eq!(doc.scalar_fragment(None, "v"), "- v\n");
    }
}
```

- [ ] **Step 2: Wire mod.rs** — set `src/model/yaml/mod.rs` to:

```rust
pub mod doc;
pub mod edit;
pub mod parse;
pub mod project;
pub mod syntax;

pub use doc::YamlDocument;
```

(`edit`/`project` modules are created in Tasks 4–6; create empty stub files now so this compiles: `project.rs` with a `pub fn project(_: &SyntaxNode, filename: &str) -> NodeTree { NodeTree { root: crate::model::node::Node::branch(filename, NodeKind::Root) } }` and `edit.rs` with stubbed `pub fn apply(syntax: &SyntaxNode, _m: Mutation) -> Result<SyntaxNode, MutateError> { Ok(syntax.clone_for_update()) }` + `pub fn serialize_fragment(_: &SyntaxNode, _: &[Seg]) -> String { String::new() }`.)

- [ ] **Step 3: Run facet tests**

Run: `cargo test --lib yaml::doc 2>&1 | tail -20`
Expected: PASS (project/apply still stubbed; facet tests don't exercise them).

- [ ] **Step 4: Commit**

```bash
git add src/model/yaml/
git commit -m "feat(yaml): YamlDocument load/serialize/facets (project/apply stubbed)"
```

---

## Task 4: Projection — green tree → `NodeTree` (+ resolver index, golden tests)

Mirror `json/project.rs`: a `Target` resolver enum, a `YamlIndex = Vec<(Vec<Seg>, Target)>`, a `walk` building tree+index together, comment merging (consecutive `#` lines → one Comment node, blank splits), trailing comments, opaque→read-only nodes.

**Files:**
- Create (replace stub): `src/model/yaml/project.rs`
- Test: `src/model/yaml/project.rs` (golden tests)

**Projection mapping (the YAML analog of json/project.rs §2.2 table):**

| YAML syntax node | NodeKind | Format | KeySign |
| --- | --- | --- | --- |
| `MAPPING` (block) | `Table` | `Block` | own key's sign |
| `MAPPING` value of `FLOW_MAP` | `InlineTable` | `Inline` | — |
| `SEQUENCE` (block) | `Array` | `Block` | — |
| `FLOW_SEQ` | `Array` | `Inline` | — |
| `SCALAR` plain | `Scalar(<core-schema type>)` | `Plain` (or `Decimal`/`Exponent`/`Inf`/`Nan`) | — |
| `SCALAR` SINGLE | `Scalar(String)` | `SingleQuoted` | — |
| `SCALAR` DOUBLE | `Scalar(String)` | `DoubleQuoted` | — |
| `BLOCK_SCALAR` `\|` | `Scalar(String)` | `LiteralBlock` | — |
| `BLOCK_SCALAR` `>` | `Scalar(String)` | `Folded` | — |
| standalone `#` line(s) | `Comment` | `Plain` | `None` |
| end-of-line `#` | owner's `trailing_comment` | — | — |
| `OPAQUE` node | `NodeKind` per top-level shape if cheap, else `Scalar(String)`; **`read_only: true`** | `Plain` | `None`/key sign |

- Key sign: a plain (unquoted) key → `KeySign::Bare`; a `'…'`/`"…"` key → `KeySign::Quoted`; keyless (sequence element, comment, opaque) → `KeySign::None`. (Spec §3.5: `(B)` plain, `(Q)` quoted, `(-)` keyless.)
- **Core-schema scalar typing** (`classify_scalar`): `null`/`~`→`Null`; `true`/`false`→`Bool`; `^[-+]?[0-9]+$`→`Integer`/`Decimal`, `0x…`→`Integer`/`Hex`, `0o…`→`Integer`/`Octal`; `.inf`/`-.inf`→`Float`/`Inf`, `.nan`→`Float`/`Nan`, exponent (`[eE]`)→`Float`/`Exponent`, has `.`→`Float`/`Plain`; **everything else (incl. date-looking) → `String`/`Plain`**.
- Addressing identical to TOML/JSON: keyed nodes `Seg::Key`, positional (elements, comments) `Seg::Index` over the **full child sequence** (comments share slot space).

- [ ] **Step 1: Write golden tests first**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::{Format, KeySign, NodeKind, ScalarType, Seg};
    fn tree(src: &str) -> NodeTree {
        let g = crate::model::yaml::parse::parse(src).unwrap();
        project(&crate::model::yaml::syntax::SyntaxNode::new_root(g), "c.yaml")
    }
    #[test]
    fn scalars_core_schema() {
        let t = tree("s: hello\nq: 'x'\ni: 42\nh: 0x1A\nf: 3.14\ne: 6e2\ninf: .inf\nb: true\nnul: ~\nd: 2026-06-13\n");
        let by = |k: &str| t.root.children.iter().find(|c| c.key == k).unwrap();
        assert_eq!(by("s").kind, NodeKind::Scalar(ScalarType::String));
        assert_eq!(by("s").key_sign, KeySign::Bare);
        assert_eq!(by("q").format, Format::SingleQuoted);
        assert_eq!(by("i").kind, NodeKind::Scalar(ScalarType::Integer));
        assert_eq!(by("h").format, Format::Hex);
        assert_eq!(by("f").kind, NodeKind::Scalar(ScalarType::Float));
        assert_eq!(by("e").format, Format::Exponent);
        assert_eq!(by("inf").format, Format::Inf);
        assert_eq!(by("b").kind, NodeKind::Scalar(ScalarType::Bool));
        assert_eq!(by("nul").kind, NodeKind::Scalar(ScalarType::Null));
        // date-looking is a STRING (no datetime type)
        assert_eq!(by("d").kind, NodeKind::Scalar(ScalarType::String));
    }
    #[test]
    fn block_mapping_and_sequence() {
        let t = tree("srv:\n  host: localhost\n  ports:\n    - 80\n    - 443\n");
        let srv = t.root.children.iter().find(|c| c.key == "srv").unwrap();
        assert_eq!(srv.kind, NodeKind::Table);
        assert_eq!(srv.format, Format::Block);
        let ports = srv.children.iter().find(|c| c.key == "ports").unwrap();
        assert_eq!(ports.kind, NodeKind::Array);
        assert_eq!(ports.children.len(), 2);
        assert_eq!(ports.children[0].path,
            vec![Seg::Key("srv".into()), Seg::Key("ports".into()), Seg::Index(0)]);
    }
    #[test]
    fn flow_collections() {
        let t = tree("pt: {x: 1, y: 2}\nls: [a, b]\n");
        let pt = t.root.children.iter().find(|c| c.key == "pt").unwrap();
        assert_eq!(pt.kind, NodeKind::InlineTable);
        assert_eq!(pt.format, Format::Inline);
        let ls = t.root.children.iter().find(|c| c.key == "ls").unwrap();
        assert_eq!(ls.kind, NodeKind::Array);
        assert_eq!(ls.format, Format::Inline);
    }
    #[test]
    fn block_scalars() {
        let t = tree("lit: |\n  a\n  b\nfold: >\n  c d\n");
        assert_eq!(t.root.children.iter().find(|c| c.key=="lit").unwrap().format, Format::LiteralBlock);
        assert_eq!(t.root.children.iter().find(|c| c.key=="fold").unwrap().format, Format::Folded);
    }
    #[test]
    fn comments_standalone_trailing_merged() {
        let t = tree("# a\n# b\nk: 1 # tail\n");
        assert!(matches!(t.root.children[0].kind, NodeKind::Comment(_)));
        assert_eq!(t.root.children[0].value.as_deref(), Some("# a\n# b"));
        let k = t.root.children.iter().find(|c| c.key == "k").unwrap();
        assert_eq!(k.trailing_comment.as_deref(), Some("# tail"));
    }
    #[test]
    fn opaque_is_read_only() {
        let t = tree("ref: *anchor\ntag: !!str 7\n");
        let r = t.root.children.iter().find(|c| c.key == "ref").unwrap();
        assert!(r.read_only);
        let g = t.root.children.iter().find(|c| c.key == "tag").unwrap();
        assert!(g.read_only);
    }
    #[test]
    fn merge_key_is_read_only_node() {
        let t = tree("base:\n  <<: *d\n  x: 1\n");
        let base = t.root.children.iter().find(|c| c.key == "base").unwrap();
        assert!(base.children.iter().any(|c| c.read_only));
    }
}
```

- [ ] **Step 2: Implement `project` by mirroring `json/project.rs`**

Copy the structure of `json/project.rs`: `Target` enum (`MapEntry(SyntaxNode)`, `Element(SyntaxNode)`, `Comment(SyntaxToken)`, `Opaque(SyntaxNode)`), `YamlIndex`, `walk`/`build_value_node`/`walk_container_tokens`, the consecutive-comment accumulator (split on a blank-line `NEWLINE` run, same algorithm), `classify_scalar` (the core-schema rules above), and key-sign derivation. Differences from JSON: iterate `MAPPING`/`SEQUENCE`/`FLOW_MAP`/`FLOW_SEQ`/`BLOCK_SCALAR`/`OPAQUE` node kinds instead of `OBJECT`/`ARRAY`/`MEMBER`; comment leader is `#`; an `OPAQUE` node projects with `read_only: true` and `value = Some(raw text)`.

- [ ] **Step 3: Run golden tests**

Run: `cargo test --lib yaml::project 2>&1 | tail -25`
Expected: all PASS. Iterate the projection until green.

- [ ] **Step 4: Commit**

```bash
git add src/model/yaml/project.rs
git commit -m "feat(yaml): CST->NodeTree projection (scalars/maps/seqs/flow/opaque, golden tests)"
```

---

## Task 5: The indent engine + `Mutation` splices (`edit.rs`)

This is the largest task and the spec's highest risk (§Risks 1). Build the indent engine first (with its own unit tests), then implement the mutations one sub-task at a time, each mirroring its `json/edit.rs` analog with the indent engine swapped in for JSON's comma/brace normalization. **Opaque guard is shared by all mutations.**

**Files:**
- Create (replace stub): `src/model/yaml/edit.rs`
- Test: `src/model/yaml/edit.rs` (unit tests per mutation, `apply_str` helper like `json/edit.rs:1425`)

### 5a: Indent engine + resolver + opaque guard

- [ ] **Step 1: Write indent-engine unit tests**

```rust
#[test]
fn reindent_shifts_every_line() {
    // re-indent a captured block from depth 0 to 4 spaces
    assert_eq!(reindent("a: 1\nb:\n  c: 2\n", 0, 4), "    a: 1\n    b:\n      c: 2\n");
    // dedent
    assert_eq!(reindent("    x: 1\n", 4, 0), "x: 1\n");
}
#[test]
fn reindent_preserves_block_scalar_body_relative_indent() {
    let frag = "note: |\n  line one\n  line two\n";
    assert_eq!(reindent(frag, 0, 2), "  note: |\n    line one\n    line two\n");
}
```

- [ ] **Step 2: Implement `reindent` + `resolve` + `is_opaque`**

```rust
/// Re-indent every line of `fragment` from `from` leading spaces to `to`.
/// Literal/folded block-scalar bodies shift with their header (uniform shift of
/// all lines preserves their *relative* indentation). Blank lines stay blank.
fn reindent(fragment: &str, from: usize, to: usize) -> String {
    let mut out = String::with_capacity(fragment.len());
    for line in fragment.split_inclusive('\n') {
        let (content, nl) = match line.strip_suffix('\n') {
            Some(c) => (c, "\n"),
            None => (line, ""),
        };
        if content.trim().is_empty() {
            out.push_str(content);
            out.push_str(nl);
            continue;
        }
        let stripped = content.strip_prefix(&" ".repeat(from)).unwrap_or(content);
        out.push_str(&" ".repeat(to));
        out.push_str(stripped);
        out.push_str(nl);
    }
    out
}
```

`resolve(syntax, path) -> Option<Target>` rebuilds the path→element index via a `walk` mirroring `json/edit.rs::resolve` (reuse the projection's index builder if exposed). `is_opaque(target)` returns true for `Target::Opaque(_)` and for any path whose resolved node has an `OPAQUE` ancestor.

- [ ] **Step 3: Run; commit**

Run: `cargo test --lib yaml::edit::tests::reindent 2>&1 | tail`
Expected: PASS.
```bash
git add src/model/yaml/edit.rs && git commit -m "feat(yaml): indent engine + resolver + opaque guard"
```

### 5b: `apply` dispatcher + `serialize_fragment` + opaque rejection

- [ ] **Step 1: Test that any mutation on/into an opaque node is `Unsupported`**

```rust
#[test]
fn mutations_on_opaque_are_unsupported() {
    let src = "ref: *anchor\nk: 1\n";
    let g = crate::model::yaml::parse::parse(src).unwrap();
    let s = crate::model::yaml::syntax::SyntaxNode::new_root(g);
    let m = Mutation::Delete { path: vec![Seg::Key("ref".into())] };
    assert!(matches!(apply(&s, m), Err(MutateError::Unsupported)));
}
```

- [ ] **Step 2: Implement `apply` dispatcher (mirror `json/edit.rs:1366`) with an opaque pre-check**

`apply` clones `clone_for_update`, opaque-guards the target path(s), dispatches to the per-variant fn, runs `validate_semantics`, returns the new tree. `serialize_fragment` mirrors `json/edit.rs:15`.

- [ ] **Step 3: Run; commit** (`feat(yaml): apply dispatcher + opaque rejection + serialize_fragment`)

### 5c–5j: one sub-task per mutation

For **each** mutation below: write the failing unit test (using `apply_str`), implement by copying the named `json/edit.rs` analog and substituting the indent engine, run, commit. Each sub-task is its own commit.

- [ ] **5c `Replace`** (analog `json::replace`): inline scalar value, mapping-entry value, whole-document (empty path). Indent: a `$EDITOR` block fragment for a block node re-indents to the node's depth. Test: replace `k: 1`→`k: 2`; replace a block-mapping value.
- [ ] **5d `Delete`** (analog `json::delete`/`delete_item`): remove a map entry / sequence element / comment at its `Seg::Index`, dropping its **full indented extent** (block children). No comma logic; instead drop the entry's line(s) + deeper-indented continuation. Test: delete middle seq element keeps siblings byte-clean.
- [ ] **5e `Insert`** (analog `json::insert`/`adapt_fragment`): member into mapping, element into sequence. **Pack/unpack rules:** a keyed fragment into a sequence becomes `- key: value` (block mapping under `-`), not flow; a bare value into a mapping gets a `placeholder` key (auto-renamed on collision); fragment re-indented to destination depth via `reindent`. Test: insert `b: 2` into a mapping; insert keyed frag into a sequence → `- b: 2`.
- [ ] **5f `Rename`** (analog `json::rename`): rewrite the key scalar token in place, preserving/adding quoting as the new key requires; collision-checked. Test: rename `a`→`c`.
- [ ] **5g `Remark`** (analog `json::remark`): prefix every line of the node's extent with `# `; unremark re-parses the stripped text as a member. Test: remark a 2-line block entry → both lines `# `-prefixed; unremark restores.
- [ ] **5h `EditComment`** (analog `json::edit_comment`): rewrite a standalone `#` comment block in place; validate every line starts with `#`.
- [ ] **5i `InsertComment`** (analog `json::insert_comment`): splice a `#` block at the target child index, re-indented to the container depth; validate every line starts with `#`.
- [ ] **5j `Move`** (analog `json::move_nodes`): atomic delete-before-reinsert on a scratch tree; cross-container pack/unpack (element↔member) mirroring 5e; captured block re-indented to destination via `reindent`; multiple keyed nodes pasted into a sequence join into one `- ` block mapping. Source/destination opaque → `Unsupported`. Test: move a block entry between two mappings at different depths (verify re-indent).

After 5j:

- [ ] **Step: Run the whole edit suite**

Run: `cargo test --lib yaml::edit 2>&1 | tail -30`
Expected: all PASS.

---

## Task 6: `ConvertKind` (`K` kind switch) + `kind_options`

**Files:**
- Modify: `src/model/yaml/edit.rs` (`convert_kind` + helpers)
- Modify: `src/model/yaml/doc.rs` (`kind_options` body)
- Test: both files.

**Kind options (spec §3.4):**
- mapping/sequence: **Block ↔ Flow** (`Format::Block` ↔ `Format::Inline`). Flow target **rejects** held comments and multi-line members (mirrors TOML array-collapse rule, `json::convert_array`'s collapse check).
- string: **plain ↔ single ↔ double ↔ literal ↔ folded**. Plain target rejected when content needs quoting (leading indicator char, `: `/` #`, etc.); literal/folded targets only for multi-line-capable contexts. Decode-then-re-encode per style (mirror `cst_edit` string conversion).
- int: **dec ↔ hex ↔ oct** (`0x`/`0o`). float: **plain ↔ exponent** (mirror `json::convert_float`).
- opaque / Root / comment / scalar of single notation (bool, null) → no options.

- [ ] **Step 1: `kind_options` body + test** (replace the stub in `doc.rs`)

```rust
pub(crate) fn kind_options(tree: &NodeTree, path: &[Seg]) -> Vec<(String, KindTarget)> {
    use crate::model::node::{Format, NodeKind, ScalarType};
    let Some(node) = tree.node_at(path) else { return Vec::new() };
    if node.read_only { return Vec::new(); }
    match &node.kind {
        NodeKind::Table | NodeKind::InlineTable | NodeKind::Array => {
            if node.format == Format::Inline {
                vec![("block  [_/B]".into(), KindTarget::Block)]
            } else {
                vec![("flow  [_/F]".into(), KindTarget::Flow)]
            }
        }
        NodeKind::Scalar(ScalarType::String) => {
            // offer the four styles other than the current one
            let all = [
                (Format::Plain, "plain", KindTarget::StringPlain),
                (Format::SingleQuoted, "single", KindTarget::StringSingle),
                (Format::DoubleQuoted, "double", KindTarget::StringDouble),
                (Format::LiteralBlock, "literal |", KindTarget::StringLiteralBlock),
                (Format::Folded, "folded >", KindTarget::StringFolded),
            ];
            all.iter().filter(|(f, ..)| *f != node.format)
               .map(|(_, l, t)| (l.to_string(), *t)).collect()
        }
        NodeKind::Scalar(ScalarType::Integer) => {
            let all = [(Format::Decimal, "dec", KindTarget::IntDecimal),
                       (Format::Hex, "hex 0x", KindTarget::IntHex),
                       (Format::Octal, "oct 0o", KindTarget::IntOctal)];
            all.iter().filter(|(f, ..)| *f != node.format)
               .map(|(_, l, t)| (l.to_string(), *t)).collect()
        }
        NodeKind::Scalar(ScalarType::Float) => {
            if node.format == Format::Exponent {
                vec![("plain float".into(), KindTarget::FloatPlain)]
            } else if node.format == Format::Plain {
                vec![("exponent float".into(), KindTarget::FloatExponent)]
            } else { Vec::new() } // inf/nan don't convert
        }
        _ => Vec::new(),
    }
}
```

Test mirrors `json/doc.rs::kind_options_per_node` for YAML nodes.

- [ ] **Step 2: `convert_kind` + test** — mirror `json::convert_kind`/`convert_array`/`convert_object`/`convert_float`; add the YAML block↔flow re-indent and the 5 string-style transcoders. Test each conversion + each rejection (flow with comment → `Illegal`; plain target needing quotes → `Illegal`).

- [ ] **Step 3: Run; commit**

Run: `cargo test --lib yaml:: 2>&1 | tail -20`
```bash
git add src/model/yaml/ && git commit -m "feat(yaml): ConvertKind block<->flow + 5 string styles + radix/exp, kind_options"
```

---

## Task 7: KIND column tags (`type_tag`) — thread `DocFormat`, add YAML tags

**Files:**
- Modify: `src/tui/app.rs:2514` (`type_tag` signature + arms + caller)
- Test: `src/tui/app.rs` (`type_tag_is_fixed_pitch` extended)

- [ ] **Step 1: Thread `DocFormat` and add YAML arms**

Change signature to `fn type_tag(kind: &NodeKind, format: Format, key_sign: KeySign, doc: DocFormat) -> String`. In the `Array` and `Table`/`InlineTable` arms, branch on `doc`:

```rust
        NodeKind::Array => match (doc, format) {
            (DocFormat::Yaml, Format::Block) => "[A/B]",
            (DocFormat::Yaml, _) => "[A/F]",   // Inline flow seq
            (_, Format::Multiline) => "[A/M]",
            _ => "[A/I]",
        },
        NodeKind::Table => match (doc, format) {
            (DocFormat::Yaml, Format::Block) => "[T/B]",
            (DocFormat::Yaml, _) => "[T/F]",
            (_, Format::Dotted) => "[T/D]",
            (_, Format::Multiline) => "[T/M]",
            _ => "[T/S]",
        },
        NodeKind::InlineTable => match doc {
            DocFormat::Yaml => "[T/F]",
            _ => "[T/I]",
        },
```

Add the read-only/opaque tag: a `read_only` YAML node renders `[opaq ]`. Since `type_tag` takes `(kind, format, key_sign, doc)` but not `read_only`, pass it too **or** (simpler) have the caller render `[opaq ]` when `node.read_only && doc == Yaml`. **Recommended:** add a `read_only: bool` param to `type_tag` (one more arg, caller has the node) and a leading guard: `if read_only { return format!("{sign} [opaq ]"); }`. String-style arms: add `(String, SingleQuoted)`/`(String, DoubleQuoted)`/`(String, LiteralBlock)`/`(String, Folded)` → `[S:sq  ]`/`[S:dq  ]`/`[S:lit ]`/`[S:fold]` (12-col fixed pitch; choose tags consistent with the popup labels in Task 8).

Update the **one** caller (search `type_tag(` in `app.rs`) to pass `doc_format` (available from `self.doc.as_ref().map(|d| d.format())`) and `node.read_only`.

- [ ] **Step 2: Extend the fixed-pitch test** (`type_tag_is_fixed_pitch`) to cover the new YAML tags — assert every returned string is the same display width.

- [ ] **Step 3: Run; commit**

Run: `cargo test --lib type_tag 2>&1 | tail`
```bash
git add src/tui/app.rs && git commit -m "feat(tui): YAML KIND tags [T/B]/[T/F]/[A/B]/[A/F]/[opaq] + DocFormat thread"
```

---

## Task 8: Type-filter facets (`classify` thread `DocFormat`, YAML layout, invariant)

**Files:**
- Modify: `src/tui/type_filter.rs` (`TypeToken`, `classify`, `Group`, `layout`, `nav_rows`)
- Test: `src/tui/type_filter.rs` (`classify_covers_every_kind_slot`, the `classify`↔`type_tag` inverse-invariant test)

- [ ] **Step 1: Add YAML `TypeToken`s + thread `DocFormat` into `classify`**

Add tokens: `SeqBlock`, `SeqFlow`, `MapBlock`, `MapFlow`, `StrSingle`, `StrDouble`, `StrLiteralBlock`, `StrFolded`, `Opaque`. Change `classify(kind, format)` → `classify(kind, format, doc, read_only)`; mirror Task 7's arms exactly (the inverse-function invariant). A `read_only` node → `TypeToken::Opaque`.

- [ ] **Step 2: YAML branch in `layout()` + `nav_rows()`**

Add `DocFormat::Yaml => vec![…]` to `layout`: key-sign half `(B)/(Q)/(-)` (no `(D)`); type half = root/comment + **Block/Flow group** (seq + map) + the 5 string styles + int(dec/hex/oct) + float(plain/exp/inf/nan) + bool + null + opaque. No datetime, no AoT, no dotted, no radix bin, no `[A/M]`/`[T/M]`. Add `Group` members if needed (e.g. extend `Group::tokens` with a Yaml-aware set or add `Group::Seq`/`Group::Map`).

- [ ] **Step 3: Extend the inverse-invariant test**

The existing test asserts `classify` is the arm-for-arm inverse of `type_tag`. Parameterize it over `DocFormat::Yaml` so every YAML `(kind, format, read_only)` maps to a token whose label/tag matches `type_tag`'s slot. Add `classify_covers_every_kind_slot` cases for the YAML tokens.

- [ ] **Step 4: Run; commit**

Run: `cargo test --lib type_filter 2>&1 | tail -20`
```bash
git add src/tui/type_filter.rs && git commit -m "feat(tui): YAML type-filter facets + classify<->type_tag invariant"
```

---

## Task 9: Wire `AnyDocument::Yaml` + CLI

**Files:**
- Modify: `src/model/any_doc.rs` (variant, `delegate!`, `load_as`)
- Modify: `src/cli.rs` (drop YAML bail — already maps extension)
- Test: `src/model/any_doc.rs`, `src/cli.rs`

- [ ] **Step 1: Test loading YAML through `AnyDocument`**

```rust
#[test]
fn any_document_loads_yaml() {
    let f = tempfile::Builder::new().suffix(".yaml").tempfile().unwrap();
    std::fs::write(f.path(), "a: 1\n").unwrap();
    let doc = AnyDocument::load(f.path()).unwrap();
    assert_eq!(doc.format(), DocFormat::Yaml);
    assert_eq!(doc.serialize(), "a: 1\n");
}
```

- [ ] **Step 2: Add the variant + dispatch**

In `any_doc.rs`: `Yaml(YamlDocument)` variant; add `AnyDocument::Yaml($d) => $body,` to the `delegate!` macro; `DocFormat::Yaml => Ok(Self::Yaml(YamlDocument::load(path)?))` in `load_as` (remove the bail). Import `use crate::model::yaml::YamlDocument;`. In `cli.rs`, if a `--format yaml` bail exists, remove it (the extension mapping already returns `DocFormat::Yaml`).

- [ ] **Step 3: Run full suite; commit**

Run: `cargo test 2>&1 | tail -15`
```bash
git add src/model/any_doc.rs src/cli.rs && git commit -m "feat: wire AnyDocument::Yaml + enable YAML CLI"
```

---

## Task 10: Help text (`YAML_HELP`)

**Files:**
- Modify: `src/tui/keys.rs:88` (`help_text` — add `DocFormat::Yaml => YAML_HELP`, define `YAML_HELP`)
- Test: `src/tui/keys.rs::tests`

- [ ] **Step 1: Define `YAML_HELP`** — copy `TOML_HELP`, drop dotted/AoT/datetime/radix-bin/multiline-string lines, change the `K` line to list YAML options (block↔flow, 5 string styles, dec/hex/oct, plain↔exp), change the `r`/comment lines to `#`, add a note that anchors/aliases/merge/tags are read-only (`[opaq ]`).

- [ ] **Step 2: Test** — assert `help_text(DocFormat::Yaml) != help_text(DocFormat::Toml)` and contains `[opaq ]` and `block`/`flow`.

- [ ] **Step 3: Run; commit** (`feat(tui): YAML-mode help text`)

---

## Task 11: Integration roundtrip + mutation-lossless tests

**Files:**
- Create: `tests/roundtrip_yaml.rs` (mirror `tests/roundtrip_json.rs`)

- [ ] **Step 1: Write the integration tests**

```rust
use confy::model::any_doc::AnyDocument;
use confy::model::document::ConfigDocument;
// ... mirror tests/roundtrip_json.rs:

#[test]
fn yaml_fixtures_roundtrip_byte_identical() {
    for name in [
        "docker-compose","github-actions","deployment","helm-values","prometheus",
        "simple-config","flow-style","scalars","comments","tags-and-anchors",
    ] {
        let path = format!("tests/fixtures/yaml/{name}.yaml");
        let src = std::fs::read_to_string(&path).unwrap();
        let doc = AnyDocument::load(std::path::Path::new(&path)).unwrap();
        assert_eq!(doc.serialize(), src, "roundtrip {name}");
    }
}

#[test]
fn mutation_then_reparse_is_lossless() {
    // load a simple file, rename a key, serialize, reload — no stray bytes
    // (mirror roundtrip_json.rs::mutation_then_reparse_is_lossless)
}

#[test]
fn opaque_file_mutation_is_unsupported() {
    // load tags-and-anchors.yaml; Delete an opaque path → Err(Unsupported); doc unchanged
}
```

(Confirm the integration crate-path: `roundtrip_json.rs` shows whether it's `confy::model::…`. Match it.)

- [ ] **Step 2: Run; commit**

Run: `cargo test --test roundtrip_yaml 2>&1 | tail`
```bash
git add tests/roundtrip_yaml.rs && git commit -m "test(yaml): byte-identical roundtrip + mutation lossless over fixtures"
```

---

## Task 12: Documentation + Cargo description

**Files:** `CHANGELOG.md`, `CLAUDE.md`, `CONTEXT.md`, `README.md`, `Cargo.toml`.

- [ ] **Step 1:** `CHANGELOG.md` — Unreleased entry (timestamp 2026-06-13) describing the YAML subset backend matching the eventual squash/commit message.
- [ ] **Step 2:** `CLAUDE.md` — add a **YAML subset backend** paragraph after the JSON one (subset scope, opaque/read-only degradation, multi-doc reject, indent engine, the new `Format`/`KindTarget`/tags, `value_kind`/`scalar_fragment` YAML forms); add `model/yaml/` to the module map; note `type_tag`/`classify` now take `DocFormat`.
- [ ] **Step 3:** `CONTEXT.md` — glossary: **Opaque node**, **Indent engine**, **YAML subset**, **core schema typing**.
- [ ] **Step 4:** `README.md` — format-support table: add YAML (subset, read-only degradation).
- [ ] **Step 5:** `Cargo.toml` — description line mentions YAML.
- [ ] **Step 6: Commit** (`docs: YAML subset backend (changelog, architecture, glossary, README)`)

---

## Final verification

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test                       # full suite incl. all yaml:: + roundtrip_yaml
```

**Manual sanity (per the no-pty rule, the user drives the TUI):** hand off these manual checks rather than scripting a TUI session —
1. `cargo run -- tests/fixtures/yaml/simple-config.yaml` — tree renders, KIND tags show `[T/B]`/`[A/B]`/`[S:str ]`, `e`/`a`/`d`/`K`/`f`/`/` behave per matrix.
2. `cargo run -- tests/fixtures/yaml/tags-and-anchors.yaml` — opaque rows show `[opaq ]`; `e`/`d`/`x`/`r` on them report read-only/unsupported; `c` (copy) works; edits to non-opaque rows save and re-load byte-clean.
3. `cargo run -- tests/fixtures/yaml/multi-doc.yaml` — refuses to load with a clear multi-document message.

**Phase boundary:** leave `main` green and shippable (spec §Phase boundaries). Phase 4 (document-level conversion) is a separate plan.

---

## Self-review notes

- **Spec coverage:** §3.1 subset → Tasks 2,4; §3.2 opaque/multi-doc → Tasks 2,4,5b; §3.3 parser+indent engine → Tasks 2,5a (spike gate already passed); §3.4 behavior matrix → Tasks 5,6; §3.4 `KindTarget`/`Format` additions → Task 1; §3.5 KIND column + facets + invariant → Tasks 7,8; §Testing → golden (4), roundtrip (11), mutation (5–6), invariant (8); §Documentation → Task 12. CLI/help/wiring → Tasks 9,10.
- **Type consistency:** `Format::{Block,SingleQuoted,DoubleQuoted,LiteralBlock,Folded}` and `KindTarget::{Flow,Block,String*}` defined in Task 1 and used unchanged thereafter. `type_tag`/`classify` both gain `(doc: DocFormat, read_only: bool)` — applied in lockstep (Tasks 7,8) so the inverse invariant holds.
- **Open implementation choices** (decide during execution, low-risk): exact 12-col text for the 4 YAML string-style tags (`[S:sq  ]` etc.) — pick once in Task 7 and reuse in Task 8's labels; whether `resolve` reuses the projection index (preferred) or rebuilds.
- **Known v1 edges (spec-sanctioned, not bugs):** multi-line flow → opaque; whole-document `E` re-parse must still reject multi-doc; flow members not deeply sub-structured beyond what projection needs.
```
