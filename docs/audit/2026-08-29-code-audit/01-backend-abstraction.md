# Model Layer Backend Abstraction & Structural Duplication Audit

## Verdict
The three-backend model layer (`crates/confy-core/src/model/`) is architecturally sound and correctly centered around the `ConfigDocument` trait and `Mutation` enum as the polymorphic boundary. A unified "generic CST splice engine" across TOML, JSON, and YAML is strongly **rejected** because ~93% (~8.4k LOC) of the edit codebase represents irreducible format-specific grammar and container mechanics (TOML's scattered section spans and external `taplo` rowan types vs. JSON's comma-separated monolithic objects vs. YAML's whitespace/indentation arithmetic). However, high-value, low-risk improvements exist in three areas: (1) eliminating silent behavioral drift on edge cases (such as insert on empty JSON documents and rename quoting), (2) providing default implementations on `ConfigDocument` to eliminate boilerplate delegation, and (3) sharing ~300–600 LOC of pure micro-algorithms (key collision loops, multi-source move coordination, and comment line manipulation).

---

## Duplication & Codebase Breakdown

Across `crates/confy-core/src/model/` (21,175 total LOC):

| Subsystem | Total LOC | Production LOC | Test LOC | Architecture / AST Model |
|---|---|---|---|---|
| **TOML (`cst_doc`, `cst_project`, `cst_edit/*`)** | 9,837 | 5,758 | 4,079 | External `taplo` rowan green tree; scattered section / dotted spans |
| **JSON (`json/parse`, `json/doc`, `json/project`, `json/edit`)** | 4,198 | 2,752 | 1,446 | Hand-rolled rowan green tree; monolithic `{}` / `[]` containers |
| **YAML (`yaml/parse`, `yaml/doc`, `yaml/project`, `yaml/edit/*`)** | 5,064 | 2,907 | 2,157 | Hand-rolled rowan green tree; dual block-indent & flow containers |
| **Shared Seams (`document`, `any_doc`, `node`, `value`, `convert`)** | 2,076 | 1,787 | 289 | Trait boundary, enum dispatch, neutral value lowering |
| **Total Model Layer** | **21,175** | **13,204** | **7,971** | |

### Shareability Analysis
- **Irreducible format semantics (~8,400 LOC / ~93% of edit production code):** Grammar parsers, AST tokens, table-capture / partition rules, dotted-key prefixing, AoT header nesting, JSON comma formatting, YAML block-indentation re-anchoring, block scalar (`|`, `>`) preservation, and opaque node guards.
- **Plausibly shareable / deduplicable logic (~600 LOC / ~7% of edit production code):** Multi-source move scheduling (~150 LOC), collision rename loops (~60 LOC across 5 sites), comment line-prefixing/uncommenting (~60 LOC), and `ConfigDocument` trait default methods (~40 LOC).

---

## Findings

### F1. Reject Shared "Generic CST Splice" Layer — Formats are Fundamentally Asymmetric
- **What**: Evaluating whether the ~8.5k LOC of mutation code across `cst_edit/` (`crates/confy-core/src/model/cst_edit/`), `json/edit.rs` (`crates/confy-core/src/model/json/edit.rs`), and `yaml/edit/` (`crates/confy-core/src/model/yaml/edit/`) can be replaced by a parameterized generic CST splice engine (e.g. `CstSpliceEngine<P: SplicePolicy>`).
- **Why it matters**: A naive abstraction attempt would introduce massive generic complexity without reducing maintenance burden. The formats diverge along three irreconcilable axes:
  1. **Tree Representation & Types**: TOML relies on external `taplo::syntax::{SyntaxNode, SyntaxKind}` (`cst_doc.rs:15`), while JSON and YAML use independent internal rowan definitions (`json/syntax.rs:3`, `yaml/syntax.rs:3`). Rowan syntax kinds are distinct statically-typed enums.
  2. **Container Model**: TOML tables are non-monolithic open sets of member spans (`cst_edit/replace_delete.rs:14`, `docs/reference/MUTATIONS.md § Member spans`) where keys and `[section]` headers are scattered throughout the document. In contrast, JSON objects are strictly bounded by `{}` and separated by commas (`json/edit.rs:454`), while YAML block mappings have no punctuation delimiters and rely entirely on 2D column indentation math (`yaml/edit/block.rs:861`, `reindent`).
  3. **Mutation Technique**: TOML mutates green trees in-place via rowan `splice_children` (`cst_edit/move_paste.rs:366,430`), JSON performs item string extraction followed by container replacement (`json/edit.rs:520-528`), and YAML performs indentation slicing and line re-anchoring (`yaml/edit/block.rs:926`).
- **Proposal**: Explicitly retain the existing design documented in `BEHAVIOR_MATRIX.md §7`:
  > *"The per-backend splice engines share a contract (the `Mutation` enum), not a mechanism... The `Mutation` enum is the abstraction; a shared splice core would add complexity for no behavior gain."*
- **Effort / Risk**: N/A (Keep as-is).
- **Verdict**: **Rejected-and-why**.

---

### F2. Backend Behavioral Drift on Edge Cases: Empty Document Insert, Rename Quoting, and Array Element Remark
- **What**: Three concrete behavioral divergences exist across backends without documented reasons:
  1. **Insert into Empty Document (`""` or comment-only)**:
     - TOML (`cst_edit/move_paste.rs:49-50`): Root is projected as a Table; insert splices at index 0 into the root -> **Succeeds**.
     - YAML (`yaml/edit/block.rs:839-844`): Catches `Err(MutateError::NotFound) if target.parent.is_empty()` and calls `insert_into_empty_document` (`block.rs:806`) -> **Succeeds** (synthesizes `{}` or `[]`).
     - JSON (`json/edit.rs:452,535-544`): `find_container` strictly requires `ROOT -> VALUE -> OBJECT/ARRAY`. On an empty or comment-only document, `tree.children().find(|n| n.kind() == SyntaxKind::VALUE)` returns `None`, causing `Mutation::Insert` at root (`parent: []`) to fail with `MutateError::NotFound` -> **Fails unexpectedly** on `a` (Add) in TUI.
  2. **Key Rename Probing and Escapes**:
     - TOML (`cst_edit/rename.rs:26`): Parses `{new_key} = 0\n` verbatim, supporting quoted, bare, or dotted keys.
     - YAML (`yaml/edit/mutations.rs:28-33`): Parses `{new_key}: 0\n`, extracts `entry_key_name(&new_entry)` (decoded), and compares decoded sibling names (`mutations.rs:44`).
     - JSON (`json/edit.rs:971`): Wraps input in quotes: `format!("{{\"{new_key}\": 0}}")`. If the user passes a key that already carries quotes or escapes (per `Node::key_literal`), JSON constructs malformed syntax or double quotes (`"\"key\""`), and its collision check (`json/edit.rs:951`) compares raw literal `new_key` against decoded `key_name_of(&key_node)`.
  3. **Array Element Remark (`Mutation::Remark`)**:
     - YAML (`yaml/edit/mutations.rs:122`): Supports commenting out both sequence elements (`Target::Element`) and map entries (`Target::MapEntry`) -> **Succeeds**.
     - TOML (`cst_edit/replace_delete.rs:1159`): Matches `Target::ArrayElement` in fallback `_ => Err(MutateError::Unsupported)` -> **Fails with `Unsupported`**.
     - JSON (`json/edit.rs:1089-1091`): `Target::Element(_)` explicitly returns `Err(MutateError::Illegal("cannot remark an array element".into()))` -> **Fails with `Illegal`**.
- **Why it matters**: Inconsistent user experience across formats in the TUI/Web hosts (e.g. Add failing on empty JSON files, Rename failing on quoted JSON keys, different error toasts for Remark).
- **Proposal**:
  1. Add `insert_into_empty_document` fallback to `json/edit.rs:find_container` (mirroring `yaml/edit/block.rs:841-844`) to synthesize a root `{}` or `[]`.
  2. Normalize JSON `rename` in `json/edit.rs:938-986` to decode `new_key` and probe with bare/quoted parsing matching YAML's `parse_map_entry_fragment`.
  3. Align error return types on un-remark unsupported targets to return `MutateError::Unsupported` uniformly.
- **Effort / Risk**: S effort, Low risk (isolated bug fixes with targeted regression tests).
- **Verdict**: **Recommend**.

---

### F3. Streamline `ConfigDocument` Trait Seam and Eliminate Boilerplate Delegation
- **What**: The `ConfigDocument` trait (`crates/confy-core/src/model/document.rs:4-183`) defines 22 methods. Several methods are implemented identically across backends or represent host-specific query leaks:
  1. `to_value(&self)`: Implemented with 100% identical delegation in all three backends:
     - `cst_doc.rs:254`: `crate::model::convert::tree_to_value(&self.project(), DocFormat::Toml)`
     - `json/doc.rs:124`: `crate::model::convert::tree_to_value(&self.project(), DocFormat::Json)`
     - `yaml/doc.rs:119`: `crate::model::convert::tree_to_value(&self.project(), DocFormat::Yaml)`
  2. `serialize_fragment_relative(&self, path)`: Implemented identically in JSON (`json/doc.rs:46`) and YAML (`yaml/doc.rs:50`) as `self.serialize_fragment(path)`.
  3. `value_kind(&self, value)`: All three backends repeat the synthetic wrapper parse-and-project pattern (`cst_doc.rs:216`, `json/doc.rs:96`, `yaml/doc.rs:88`).
  4. Format-specific query leakage: `had_comments_at_open(&self)` (`document.rs:33`) defaults to `false` and is overridden only by `JsonDocument` (`json/doc.rs:69`) purely to drive a one-shot toast in `crates/confy-tui/src/tui/app.rs`.
- **Why it matters**: Clutters the central abstraction with repetitive boilerplate and leaks single-backend UI concerns into the core trait.
- **Proposal**:
  1. Provide a default implementation for `to_value` on `ConfigDocument`:
     ```rust
     fn to_value(&self) -> Result<(crate::model::value::Value, Vec<String>), ConvertAbort> {
         crate::model::convert::tree_to_value(&self.project(), self.format())
     }
     ```
  2. Provide a default implementation for `serialize_fragment_relative`:
     ```rust
     fn serialize_fragment_relative(&self, path: &[crate::model::node::Seg]) -> String {
         self.serialize_fragment(path)
     }
     ```
  3. Keep `had_comments_at_open` as a default method (`false`) or migrate it to a format facet struct.
- **Effort / Risk**: S effort, Low risk (purely additive default methods; zero breaking changes).
- **Verdict**: **Recommend**.

---

### F4. Deduplicate Common Cross-Backend Micro-Algorithms (~300 LOC)
- **What**: Multiple identical utility algorithms are independently written and maintained in 3 to 5 places across the backends:
  1. **Key Collision Suffix Loop (`key_2`, `key_3`, ...)**:
     - `json/edit.rs:487-495`
     - `yaml/edit/block.rs:889-913`
     - `yaml/edit/flow.rs:221-229`
     - `cst_edit/move_paste.rs:372-382`
     Each implements a `loop { format!("{base}_{n}"); ... n += 1; }` search against existing keys.
  2. **Multi-Source Move Coordination (`move_nodes`)**:
     - `json/edit.rs:1167-1258` (92 LOC) and `yaml/edit/mutations.rs:281-385` (105 LOC) share an identical multi-stage move workflow:
       (a) Pre-capture fragments via `serialize_fragment`
       (b) Calculate pre-deletion shift: `parent.children.iter().position(...).is_some_and(|ord| ord < target.index)`
       (c) Sort and delete sources back-to-front by index
       (d) Re-insert loop using `array_element_suggested_key(path)` at shifted index.
  3. **Comment Line Manipulation**:
     - Indentation-preserving `# ` / `// ` prefixing and stripping in `yaml/edit/mutations.rs:87-118`, `json/edit.rs:1011,1050`, and `cst_edit/replace_delete.rs:1124-1128`.
- **Why it matters**: Fixes or improvements to collision renaming, move shift calculations, or comment line handling must currently be manually synchronized across multiple backend files (as evidenced by code comments like *"mirrors the same fix already in the JSON/YAML move_nodes"* in `cst_edit/move_paste.rs:1051`).
- **Proposal**:
  - Introduce `crate::model::node::next_available_key(base: &str, is_taken: impl Fn(&str) -> bool) -> String`.
  - Extract a shared `move_orchestrator` helper in `crates/confy-core/src/model/node.rs` or `document.rs` for backends where container mutation is slot-indexed.
  - Add `comment_lines(text, prefix)` and `uncomment_lines(text, prefix)` helpers.
- **Effort / Risk**: M effort, Low risk.
- **Verdict**: **Worth considering**.

---

### F5. Modularize `json/edit.rs` (2.6k LOC) and Decouple Test Bloat in `cst_edit/mod.rs` (3.1k LOC tests)
- **What**:
  - `crates/confy-core/src/model/cst_edit/mod.rs` is 3,383 lines, but only lines 1–279 (279 LOC) are production code (`apply`, `validate_dom`, `serialize_fragment_impl`). The remaining **3,103 lines** (lines 280–3383) are unit tests left behind when Task 15 (2026-08-11 audit remediation) moved production code into submodules (`replace_delete.rs`, `move_paste.rs`, etc.).
  - `crates/confy-core/src/model/yaml/edit/mod.rs` has a similar structure: lines 1–136 (136 LOC) are production code, while lines 137–2034 (1,897 LOC) are unit tests.
  - `crates/confy-core/src/model/json/edit.rs` is a single monolithic file of 2,612 lines combining all JSON mutation implementations (`delete`, `insert`, `rename`, `remark`, `edit_comment`, `move_nodes`, `convert_kind`) and container helpers.
- **Why it matters**:
  - Navigating `cst_edit/mod.rs` and `yaml/edit/mod.rs` gives a misleading impression of massive module complexity when >90% of both files is test suites.
  - `json/edit.rs` is harder to audit and maintain compared to `yaml/edit/` (which cleanly separates `block.rs`, `flow.rs`, `mutations.rs`, `convert.rs`, `resolve.rs`).
- **Proposal**:
  1. Move the 3.1k LOC test suite in `cst_edit/mod.rs` into `cst_edit/tests.rs` or separate integration test files.
  2. Split `json/edit.rs` into a submodule directory `crates/confy-core/src/model/json/edit/` matching YAML's architecture:
     - `mod.rs`: `apply`, `serialize_fragment`
     - `container.rs`: `find_container`, `collect_items`, `rebuild_multiline`, `rebuild_inline`
     - `mutations.rs`: `delete`, `insert`, `rename`, `remark`, `move_nodes`
     - `convert.rs`: `convert_kind`
     - `tests.rs`: unit tests
- **Effort / Risk**: M effort, Very Low risk (mechanical file moves with zero runtime or API changes).
- **Verdict**: **Worth considering**.

---

## Left Alone Deliberately

- **The Lossless CST Single-Source-of-Truth & Atomic Commit Invariant**:
  - The architectural pattern where `SyntaxNode` is the single source of truth, `NodeTree` is a transient projection rebuilt after every mutation, and `apply` operates on a `clone_for_update` copy with semantic validation before commit (`cst_doc.rs:64-73`, `json/doc.rs:50-58`, `yaml/doc.rs:54-63`) is rock-solid. It guarantees byte-identical round-tripping and rollback on failure with 1055 passing tests.
- **Independent Syntax Trees and Projection Walkers**:
  - `cst_project.rs` (1,604 LOC), `json/project.rs` (696 LOC), and `yaml/project.rs` (1,360 LOC) each implement a single-pass `walk()` that builds the `NodeTree` and the resolver index simultaneously. Their AST traversal patterns are tightly coupled to their respective format grammars and must remain separate.
- **Neutral Value Lowering (`model/convert.rs`)**:
  - `model/convert.rs` (1,489 LOC) already provides the correct high-level abstraction for document format conversions (`to_value()` -> neutral `Value` tree -> target syntax rendering).
