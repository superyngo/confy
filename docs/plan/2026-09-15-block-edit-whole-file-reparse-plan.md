# Block Edit via Whole-File Reparse — Implementation Plan
Status: Shipped (2026-09-15)

Post-ship audit: [`../audit/2026-09-15-block-edit-implementation-audit.md`](../audit/2026-09-15-block-edit-implementation-audit.md)

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development`
> (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the multi-line editor's buffer *the text of the byte span(s) a Node owns* —
spliced into the document text and committed through the existing whole-document `Replace`
route — so key renames, N sibling nodes, and Comment↔live conversions all become legal, and
anything illegal is rejected atomically with the user's text preserved.

**Architecture:** A new read-only backend query `node_text_spans(path) -> Vec<(usize, usize)>`
returns the Node's Block. A new session method `Session::apply_block_text(path, text)` splices
the buffer over those spans in the serialized document and commits
`Mutation::Replace { path: [], fragment: new_text }` — the route every backend already
implements atomically, with reparse and semantic validation included. No new `Mutation` variant.

**Tech Stack:** Rust 2021 workspace (`confy-core`, `confy-tui`, `confy-ffi`), `rowan =0.15.18`
green trees, `taplo` for TOML, hand-rolled lossless JSON/YAML parsers; TypeScript + esbuild for
the web/touch hosts with a plain-Node spec harness.

**Spec:** [`../spec/2026-09-15-block-edit-whole-file-reparse-design.md`](../spec/2026-09-15-block-edit-whole-file-reparse-design.md)
— read it before Task 1; every task below argues from a numbered section of it.

## Global Constraints

- `confy-core` is **filesystem-free at runtime**: no `fs`/`process`/`env`/`tempfile`, no
  terminal deps. Enforced by `crates/confy-core/tests/no_fs_gate.rs`. All file I/O is the host's.
- Every mutation is **atomic**: edit a `clone_for_update` copy, commit only on success. Never
  add a path that can leave a half-applied document.
- Vocabulary is fixed by [`../reference/glossary.md`](../reference/glossary.md): **Node**, never
  "Entry". Subtypes **Root / Branch node / Leaf node / Scalar / Comment**. A new term needs its
  glossary entry **in the same commit**.
- The `Mutation` enum stays closed — this feature adds **no** variant.
- The contract sentence, copied verbatim into the reference docs in Task 6:
  > A multi-line edit is accepted if and only if its buffer parses, on its own, as a legal Node
  > sequence at the edited Node's container level. Otherwise the whole commit is rejected and
  > the document is untouched. Backends differ only where the format's grammar differs.
- Span definition (spec §3): Node body **+** trailing EOL comment **+** trailing blank-line run.
  A *preceding* standalone Comment is **excluded**. Inside a flow collection: the Node's own
  `text_range()` **trimmed of surrounding whitespace**, with the separating `,` **outside** the
  span.
- An **empty buffer is rejected** (`core.block.empty`, points the user at `d`). Never deletes.
- YAML buffers are **verbatim, indentation included** — no dedent on open, no reindent on commit.
- Two new notice keys, **both `Severity::Warn`**: `core.block.invalid`, `core.block.empty`.
- Schema revalidation after a Block commit passes `touched: None` — **always full**.
- Rejection **keeps the editor open holding the user's text** on every host. Errors go on the
  notice channel only, **never** injected into the buffer.
- Cursor re-anchor order: surviving path → first Node of the edited span (resolved from the
  span's start offset) → first row.
- Commands: `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check`,
  `cargo bench -p confy-core --bench perf -- --nodes 5000`; web: `cd web && npm run typecheck`,
  `npm test`. A CLI test asserting message text **MUST** pin `--lang en` (MESSAGES.md §5.5).
- Every task ends with a commit containing code **+** its `CHANGELOG.md` entry (under
  `## [Unreleased]` → `### 2026-09-15`) **+** every doc that task's change invalidates.

**Task ordering note.** Spec §9 requires that the core and the TUI land together *so the
behavior is verified on the real binary*. This plan satisfies that by making Tasks 1-5
**inert**: they add a read-only query, a session method nothing calls, and tests. The first
user-visible behavior change is Task 6, which switches the TUI and verifies on the real binary
in the same commit.

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `crates/confy-core/src/model/cst_edit/replace_delete.rs` | TOML `replace_value` key guard (stopgap); TOML span ends | 0, 2 |
| `crates/confy-core/src/model/document.rs` | `ConfigDocument::node_text_spans` declaration + default | 1 |
| `crates/confy-core/src/model/any_doc.rs` | `AnyDocument` delegation for the new method | 1 |
| `crates/confy-core/src/model/toml_doc/spans.rs` *(new)* | TOML `node_text_spans` implementation | 2 |
| `crates/confy-core/src/model/json/edit/spans.rs` *(new)* | JSON/JSONC `node_text_spans` | 3 |
| `crates/confy-core/src/model/yaml/edit/spans.rs` *(new)* | YAML `node_text_spans`, incl. opaque | 4 |
| `crates/confy-core/src/model/block_splice.rs` *(new)* | The one shared `splice_spans` function | 5 |
| `crates/confy-core/src/session/inline_edit.rs` | `Session::apply_block_text` | 5 |
| `crates/confy-core/src/session/intent.rs`, `dispatch.rs` | `Intent::ApplyBlockText` | 5 |
| `crates/confy-core/src/session/notice.rs`, `i18n/{en,zh-TW}.json` | two `Warn` keys + both message tables | 5 |
| `crates/confy-core/tests/block_edit_parity.rs` *(new)* | the {shape × format × outcome} matrix | 5 |
| `crates/confy-core/tests/block_edit_proptest.rs` *(new)* | unmodified buffer ⇒ byte-identical | 5 |
| `crates/confy-tui/src/tui/app.rs` | TUI switchover + `$EDITOR` re-spawn on rejection | 6 |
| `web/ui.ts`, `web/touch/app.ts`, `web/types.ts` | web/touch switchover | 7 |

New backend files are separate because span computation is a self-contained read-only concern
with its own test surface; the existing `*_edit` modules are already large and mutation-shaped.

---

## Task 0: TOML leaf `Replace` rejects a key mismatch

Independent of everything below; ships first. Spec §1 defect (1) and §9 step 0.

**Why now:** "I renamed the key in `$EDITOR` and nothing happened, with no message" is silent
data loss users hit today, and `docs/reference/MUTATIONS.md`'s `Replace` row promises the
opposite of what the TOML backend does. Task 5 deletes this guard again; it is still worth the
~15 lines for the interim.

**Files:**
- Modify: `crates/confy-core/src/model/cst_edit/replace_delete.rs:449-494` (`replace_value`)
- Modify: `docs/reference/MUTATIONS.md` (the `Replace` row, the F14 sentence)
- Test: `crates/confy-core/src/model/cst_edit/tests.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: nothing later tasks depend on (Task 5 removes it).

- [ ] **Step 1: Write the failing test**

In `crates/confy-core/src/model/cst_edit/tests.rs`:

```rust
#[test]
fn leaf_replace_rejects_a_fragment_whose_key_differs() {
    let mut doc = TomlDocument::from_str("[s]\nk1 = 1\n").unwrap();
    let err = doc
        .apply(Mutation::Replace {
            path: vec![Seg::Key("s".into()), Seg::Key("k1".into())],
            fragment: "renamed = 1\n".to_string(),
        })
        .unwrap_err();
    assert!(matches!(err, MutateError::Fragment(_)), "got {err:?}");
    // Atomicity: the document is untouched.
    assert_eq!(doc.serialize(), "[s]\nk1 = 1\n");
}

#[test]
fn leaf_replace_accepts_a_fragment_repeating_the_same_key() {
    let mut doc = TomlDocument::from_str("[s]\nk1 = 1\n").unwrap();
    doc.apply(Mutation::Replace {
        path: vec![Seg::Key("s".into()), Seg::Key("k1".into())],
        fragment: "k1 = 2\n".to_string(),
    })
    .unwrap();
    assert_eq!(doc.serialize(), "[s]\nk1 = 2\n");
}
```

- [ ] **Step 2: Run the tests to verify the first fails**

Run: `cargo test -p confy-core leaf_replace_rejects_a_fragment_whose_key_differs`
Expected: FAIL — `apply` returns `Ok`, so `unwrap_err()` panics.

- [ ] **Step 3: Add the guard**

In `replace_value`, immediately after the fragment parses cleanly (after the
`MutateError::Fragment(e.to_string())` early return on `parse.errors.first()`) and before the
VALUE is located, compare the fragment's key against the path's last key segment. Only do this
for `Target::Entry` — an `ArrayElement` fragment is legitimately keyless:

```rust
    // The fragment is the node's *complete* representation, so a key it spells
    // out must be the node's own: `replace_value` swaps only the VALUE's
    // content, so a differing key would be dropped silently (MUTATIONS.md
    // `Replace`, F14). A keyless fragment (`1`, an array element, or a bare
    // value typed into the leaf's editor) is still accepted.
    if let Target::Entry(_) = target {
        if let (Some(frag_key), Some(Seg::Key(want))) = (
            walk(&frag, "").0.root.children.first().and_then(|n| n.key.clone()),
            path.last(),
        ) {
            if &frag_key != want {
                return Err(MutateError::Fragment(format!(
                    "fragment key `{frag_key}` does not match `{want}`"
                )));
            }
        }
    }
```

Place it after `let frag = parse.into_syntax().clone_for_update();` so `frag` exists. If `walk`'s
node key is the *decoded* key while `Seg::Key` holds the decoded key too (check
`glossary.md` § "key literal vs decoded key" and the `Seg::Key` construction in
`toml_doc`'s `project`), compare them directly as above; if they differ in quoting, decode the
fragment key with the same helper `project` uses before comparing — do not hand-strip quotes.

- [ ] **Step 4: Run the tests to verify both pass**

Run: `cargo test -p confy-core replace` — Expected: PASS, and no pre-existing `replace_*` or
`external_edit_*` test regresses. In particular
`crates/confy-core/tests/external_edit_clears_trailing_comment.rs` must still pass: that test
round-trips the node's *own* key, so the guard is inert for it.

- [ ] **Step 5: Correct the reference doc**

In `docs/reference/MUTATIONS.md`'s `Replace` row, the sentence "The fragment must resolve to
**exactly one** value node in every backend; surplus text is rejected as `Fragment`, never
silently dropped (F14 …)" is currently false for TOML in a second way — surplus *nodes*. State
what is actually true after this task:

```markdown
The fragment must resolve to **exactly one** value node in every backend; JSON and YAML reject
surplus text as `Fragment` (F14 — it mattered for YAML, whose lenient grammar happily parses a
two-node fragment). TOML rejects a fragment whose **key** differs from the path's key, but still
drops surplus *nodes* after the first silently; that remaining gap is closed when the block
editor moves to the whole-file route (see `docs/spec/2026-09-15-block-edit-whole-file-reparse-design.md`).
```

- [ ] **Step 6: Changelog + commit**

Add under `## [Unreleased]` → `### 2026-09-15` in `CHANGELOG.md`:

```markdown
### Fixed
- TOML: a value `Replace` whose fragment spells a **different key** is now rejected as
  `Fragment` instead of applying the value and dropping the key silently — a `$EDITOR` key
  rename used to report success and change nothing.
```

```bash
cargo fmt && cargo clippy -p confy-core -- -D warnings && cargo test -p confy-core
git add crates/confy-core/src/model/cst_edit/ docs/reference/MUTATIONS.md CHANGELOG.md
git commit -m "fix(toml): reject a value Replace whose fragment renames the key"
```

- [ ] **Step 7: Verify on the real binary**

`cargo test` is not the gate (repo conduct: reproduce **and** confirm on the real binary).

```bash
printf '[s]\nk1 = 1\n' > /tmp/blk.toml
EDITOR='sed -i "" s/k1/renamed/' cargo run -q -- --lang en /tmp/blk.toml
```

Navigate to `k1`, press `e`, quit the editor, and confirm a notice now appears and the value is
unchanged — where before the edit was accepted with no message and no effect. Record the
observed notice text in the commit message body or in
`docs/plan/BACKLOG.md` if it reads badly.

---

## Task 1: `ConfigDocument::node_text_spans` — declaration and delegation

Spec §4. Inert: the default returns "unsupported" for all three backends until Tasks 2-4.

**Files:**
- Modify: `crates/confy-core/src/model/document.rs` (after `trailing_blank_anchor`, ~line 57)
- Modify: `crates/confy-core/src/model/any_doc.rs` (~line 100, beside `trailing_blank_anchor`)
- Test: `crates/confy-core/src/model/document.rs` (inline `#[cfg(test)]` is not used in this
  file — assert the default via `crates/confy-core/tests/block_edit_parity.rs` in Task 5 instead;
  this task's gate is `cargo check`)

**Interfaces:**
- Consumes: nothing.
- Produces: `fn node_text_spans(&self, path: &[Seg]) -> Vec<(usize, usize)>` on
  `ConfigDocument` and on `AnyDocument`. Empty `Vec` = the backend cannot express this Node as
  a Block. Spans are **byte offsets into `self.serialize()`**, sorted ascending, non-overlapping,
  each `start <= end`.

- [ ] **Step 1: Declare the method with its contract**

In `crates/confy-core/src/model/document.rs`, directly after `trailing_blank_lines`:

```rust
    /// The byte spans of `path`'s **Block** — the text a multi-line edit owns
    /// (`Session::apply_block_text`). Offsets index [`Self::serialize`]'s
    /// output, sorted ascending and non-overlapping.
    ///
    /// A span covers the Node's body, its trailing EOL comment, and its
    /// trailing blank-line run; a *preceding* standalone Comment is an
    /// independent Node and is never included. Inside a flow collection the
    /// span is the Node's own token range trimmed of surrounding whitespace,
    /// and the separating `,` stays **outside** it — a comma belongs to the
    /// container, not to any member.
    ///
    /// A Node may own several spans (a scattered `[T/S]`, a `[T/D]` dotted
    /// table, a scattered `[A/T]` group). An empty `Vec` means the backend
    /// cannot express this Node as a Block, and the caller must refuse the
    /// edit rather than guess.
    fn node_text_spans(&self, path: &[crate::model::node::Seg]) -> Vec<(usize, usize)> {
        let _ = path;
        Vec::new()
    }
```

- [ ] **Step 2: Delegate it in `AnyDocument`**

In `crates/confy-core/src/model/any_doc.rs`, beside `trailing_blank_anchor`:

```rust
    fn node_text_spans(&self, path: &[Seg]) -> Vec<(usize, usize)> {
        delegate!(self, d => d.node_text_spans(path))
    }
```

- [ ] **Step 3: Verify it compiles and nothing else moved**

Run: `cargo check -p confy-core && cargo clippy -p confy-core -- -D warnings`
Expected: clean. No test change — the method has no caller yet.

- [ ] **Step 4: Commit**

No `CHANGELOG.md` entry: this commit has no user-visible behavior.

```bash
cargo fmt
git add crates/confy-core/src/model/document.rs crates/confy-core/src/model/any_doc.rs
git commit -m "feat(core): declare ConfigDocument::node_text_spans"
```

---

## Task 2: TOML `node_text_spans`

Spec §3 (span boundaries, multi-span Nodes) and §4 (which helpers already exist).

**Files:**
- Create: `crates/confy-core/src/model/toml_doc/spans.rs`
- Modify: the `TomlDocument` `impl ConfigDocument` block (add `fn node_text_spans` delegating
  to the new module) and its parent `mod` declaration
- Test: `crates/confy-core/src/model/toml_doc/spans.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `ConfigDocument::node_text_spans` from Task 1.
- Produces: `pub(crate) fn node_text_spans(root: &SyntaxNode, path: &[Seg]) -> Vec<(usize, usize)>`.

**Existing machinery — reuse, do not reimplement.** Start offsets are free from rowan
(`node.text_range().start()`). Ends: `extent_end_offset` (`cst_edit/replace_delete.rs:826`),
`section_end_from` (`:801`), `aot_group_span` (`cst_edit/aot_group.rs:17`),
`aot_entry_end_from` (`:154`), `comment_block_range` (`cst_edit/tree_nav.rs:260`),
`table_member_spans` (`replace_delete.rs:92`), `dotted_member_entries`
(`cst_edit/dotted_table.rs:18`), `inline_member_entries` (`:55`). The trailing blank run comes
from `blank_lines::measure` (`model/blank_lines.rs:89`).

**Genuinely new work here:** converting `table_member_spans`' / `dotted_member_entries`' **child
indices** into byte ranges, and the flow-level shapes where `extent_end_offset` answers
`Unsupported` — an inline-array element, an inline-table member, a `/* */`-style span — whose
range is the trimmed `text_range()`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::document::ConfigDocument;
    use crate::model::node::Seg;
    use crate::model::toml_doc::TomlDocument;

    fn spans_of(src: &str, path: &[Seg]) -> Vec<String> {
        let doc = TomlDocument::from_str(src).unwrap();
        let text = doc.serialize();
        doc.node_text_spans(path)
            .into_iter()
            .map(|(a, b)| text[a..b].to_string())
            .collect()
    }

    fn key(k: &str) -> Seg {
        Seg::Key(k.to_string())
    }

    #[test]
    fn leaf_span_is_its_own_line() {
        assert_eq!(
            spans_of("[s]\nk1 = 1\nk2 = 2\n", &[key("s"), key("k1")]),
            vec!["k1 = 1\n"]
        );
    }

    #[test]
    fn leaf_span_includes_its_eol_comment_and_trailing_blank_run() {
        assert_eq!(
            spans_of("[s]\nk1 = 1  # note\n\n\nk2 = 2\n", &[key("s"), key("k1")]),
            vec!["k1 = 1  # note\n\n\n"]
        );
    }

    #[test]
    fn leaf_span_excludes_a_preceding_standalone_comment() {
        assert_eq!(
            spans_of("[s]\n# lead\nk1 = 1\n", &[key("s"), key("k1")]),
            vec!["k1 = 1\n"]
        );
    }

    #[test]
    fn section_span_covers_header_members_and_subsections() {
        assert_eq!(
            spans_of("[a]\nk = 1\n[a.sub]\nj = 2\n[b]\nm = 3\n", &[key("a")]),
            vec!["[a]\nk = 1\n[a.sub]\nj = 2\n"]
        );
    }

    #[test]
    fn scattered_table_yields_one_span_per_run_in_document_order() {
        let got = spans_of("[a]\nk = 1\n[b]\nm = 2\n[a]\nj = 3\n", &[key("a")]);
        assert_eq!(got, vec!["[a]\nk = 1\n", "[a]\nj = 3\n"]);
    }

    #[test]
    fn dotted_table_yields_one_span_per_member_line() {
        let got = spans_of("a.x = 1\nb = 9\na.y = 2\n", &[key("a")]);
        assert_eq!(got, vec!["a.x = 1\n", "a.y = 2\n"]);
    }

    #[test]
    fn array_element_span_is_trimmed_and_excludes_the_comma() {
        assert_eq!(
            spans_of("a = [ 1, 22 ]\n", &[key("a"), Seg::Index(1)]),
            vec!["22"]
        );
    }

    #[test]
    fn inline_table_member_span_is_trimmed_and_excludes_the_comma() {
        assert_eq!(
            spans_of("a = { x = 1, y = 2 }\n", &[key("a"), key("x")]),
            vec!["x = 1"]
        );
    }

    #[test]
    fn comment_node_span_is_its_whole_comment_block() {
        let got = spans_of("# one\n# two\nk = 1\n", &[Seg::Index(0)]);
        assert_eq!(got, vec!["# one\n# two\n"]);
    }

    #[test]
    fn unresolvable_path_yields_no_spans() {
        assert!(spans_of("k = 1\n", &[key("nope")]).is_empty());
    }
}
```

**Before implementing, confirm the `Seg` spelling for a Comment node and for an array element**
against `crates/confy-core/src/model/node.rs` and an existing test that addresses one (search
`Seg::Index` in `crates/confy-core/src/model/cst_edit/tests.rs`). If a Comment is addressed
differently in this codebase, fix the test's path — not the production code — to match.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p confy-core toml_doc::spans`
Expected: every test FAILS with an empty `Vec` (Task 1's default) — e.g.
`assertion failed: left: [], right: ["k1 = 1\n"]`.

- [ ] **Step 3: Implement**

```rust
//! TOML `node_text_spans` — the byte ranges a multi-line edit owns
//! (`ConfigDocument::node_text_spans`, design record §3). Read-only: this
//! module never mutates a tree, so it takes `&SyntaxNode`, not
//! `clone_for_update`.

use crate::model::blank_lines;
use crate::model::node::Seg;
use taplo::syntax::{SyntaxKind, SyntaxNode};

/// Every span `path`'s Node owns, ascending. Empty = unsupported/unresolved.
pub(crate) fn node_text_spans(root: &SyntaxNode, path: &[Seg]) -> Vec<(usize, usize)> {
    let text = root.text().to_string();
    let mut out = raw_spans(root, path);
    out.sort_by_key(|(a, _)| *a);
    // Each span absorbs the blank-line run that follows it, but only when the
    // span ends at a line boundary — a flow member has no run to own.
    for (_, end) in out.iter_mut() {
        if text[..*end].ends_with('\n') {
            *end += blank_lines::measure(&text, *end);
        }
    }
    out
}
```

Then a `raw_spans` that dispatches on the resolved target, one arm per shape the tests above
cover, each arm built from the helpers listed in this task's preamble:

- **leaf / keyed entry** — `(entry.text_range().start(), extent_end_offset(entry)?)`.
- **section (`[T/S]`)** — header start → `section_end_from(header, path)`; for a *scattered*
  table iterate `table_member_spans`' runs and emit one span each, converting each run's child
  index pair to `(first.text_range().start(), extent_end_offset(last)?)`.
- **dotted table (`[T/D]`)** — one span per `dotted_member_entries` entry.
- **`[A/T]` group** — `aot_group_span`; a single `[A/T]` entry — `aot_entry_end_from`.
- **array element / inline-table member** — `trimmed(node)` below.
- **comment node** — `comment_block_range`.

```rust
/// A flow-level Node's range, trimmed of the whitespace taplo bakes into the
/// token (`[ 1 ]` ⇒ `VALUE "1 "`, `cst_edit/move_paste.rs:782`). The
/// separating `,` is never inside this range: it is the container's token, so
/// it is not part of any member's Block (design record §3).
fn trimmed(node: &SyntaxNode) -> (usize, usize) {
    let r = node.text_range();
    let s = node.text().to_string();
    let lead = s.len() - s.trim_start().len();
    let trail = s.len() - s.trim_end().len();
    (
        usize::from(r.start()) + lead,
        usize::from(r.end()) - trail,
    )
}
```

Check `blank_lines::measure`'s exact signature before use — if it is
`measure(&str, usize) -> usize` returning the run's **byte length** the code above is right; if
it returns a **line count**, use `blank_lines::count_after` plus the run's byte length the way
`trailing_blank_lines` (`document.rs:65`) does, and add a test asserting the `\n\n\n` case from
Step 1 to pin whichever it is.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p confy-core toml_doc::spans` — Expected: PASS (11/11).
Then `cargo test -p confy-core` — Expected: no regression; this module has no caller.

- [ ] **Step 5: Commit**

```bash
cargo fmt && cargo clippy -p confy-core -- -D warnings
git add crates/confy-core/src/model/toml_doc/
git commit -m "feat(toml): node_text_spans — the byte spans a Node owns"
```

---

## Task 3: JSON/JSONC `node_text_spans`

Same shape as Task 2, JSON's grammar. Spec §3-4.

**Files:**
- Create: `crates/confy-core/src/model/json/edit/spans.rs`
- Modify: `JsonDocument`'s `impl ConfigDocument` + the `edit` module declaration
- Test: inline `#[cfg(test)] mod tests` in the new file

**Interfaces:**
- Consumes: Task 1's trait method.
- Produces: `pub(crate) fn node_text_spans(&JsonSyntax, &[Seg]) -> Vec<(usize, usize)>` — use
  whatever the crate's own root-node type is named in `json/edit/mutations.rs`, matching that
  file's existing `extent_end_offset` signature exactly.

**Reuse:** `extent_end_offset` (`json/edit/mutations.rs:320`), `member_line_end` (`:288`),
`comment_block_end` (`:334`). New: inline-object members and array elements on one line, whose
range is the trimmed `text_range()` with the `,` outside.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn object_member_span_is_its_own_line_with_trailing_comment() {
    assert_eq!(
        spans_of("{\n  \"a\": 1, // note\n  \"b\": 2\n}\n", &[key("a")]),
        vec!["\"a\": 1, // note\n"]
    );
}

#[test]
fn nested_object_span_covers_the_whole_object() {
    assert_eq!(
        spans_of("{\n  \"a\": {\n    \"x\": 1\n  },\n  \"b\": 2\n}\n", &[key("a")]),
        vec!["\"a\": {\n    \"x\": 1\n  },\n"]
    );
}

#[test]
fn inline_object_member_span_is_trimmed_and_excludes_the_comma() {
    assert_eq!(
        spans_of("{ \"a\": { \"x\": 1, \"y\": 2 } }\n", &[key("a"), key("x")]),
        vec!["\"x\": 1"]
    );
}

#[test]
fn array_element_span_is_trimmed_and_excludes_the_comma() {
    assert_eq!(
        spans_of("{ \"a\": [ 1, 22 ] }\n", &[key("a"), Seg::Index(1)]),
        vec!["22"]
    );
}

#[test]
fn line_comment_node_span_is_its_whole_block() {
    assert_eq!(
        spans_of("{\n  // one\n  // two\n  \"a\": 1\n}\n", &[Seg::Index(0)]),
        vec!["// one\n  // two\n"]
    );
}

#[test]
fn unresolvable_path_yields_no_spans() {
    assert!(spans_of("{\"a\": 1}\n", &[key("nope")]).is_empty());
}
```

**A trailing `,` on a full-line member is inside the span** (see the first two tests): it
terminates *that line*, and a buffer replacing the line must be free to re-emit or drop it. That
is the opposite of the *flow* rule, where the comma separates two members on one line. Note this
divergence in the module doc-comment, because it is the single most confusable part of the JSON
implementation.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p confy-core json::edit::spans` — Expected: all FAIL with `[]`.

- [ ] **Step 3: Implement**

Mirror Task 2's structure: `node_text_spans` = `raw_spans` + the line-boundary blank-run
absorption; `raw_spans` arms = object member (`member_line_end`), nested object/array (
`extent_end_offset`), comment block (`comment_block_end`), flow member/element (`trimmed`, copy
the helper from Task 2's module — it is six lines and lives on a different `SyntaxNode` type, so
duplicating it is cheaper than a generic).

JSON has no scattered or dotted shapes, so **every JSON Block is exactly one span**. Assert that
in the module:

```rust
debug_assert!(out.len() <= 1, "JSON Blocks are single-span: {out:?}");
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p confy-core json::edit::spans` then `cargo test -p confy-core`.
Expected: PASS, no regression.

- [ ] **Step 5: Commit**

```bash
cargo fmt && cargo clippy -p confy-core -- -D warnings
git add crates/confy-core/src/model/json/
git commit -m "feat(json): node_text_spans — the byte spans a Node owns"
```

---

## Task 4: YAML `node_text_spans`, including opaque spans

Spec §3-5. This is the task that changes a documented capability (`read_only`), so its docs and
test rewrites belong here.

**Files:**
- Create: `crates/confy-core/src/model/yaml/edit/spans.rs`
- Modify: `YamlDocument`'s `impl ConfigDocument`; `crates/confy-core/src/model/yaml/edit/mod.rs`
  (module declaration only — **do not** touch the `is_opaque` guard at `:108`)
- Modify: `docs/reference/glossary.md:169-175`, `docs/reference/BEHAVIOR_MATRIX.md:214-215`
- Test: inline tests; rewrite `crates/confy-core/src/model/yaml/edit/tests.rs:795` and
  `crates/confy-core/tests/session_headless.rs:3746`

**Interfaces:**
- Consumes: Task 1's trait method.
- Produces: YAML `node_text_spans`, **including a non-empty span for an opaque Node**.

**Reuse:** `extent_end_offset` (`yaml/edit/resolve.rs:72`). YAML opaque fencing is recomputed
from scratch on every parse (`yaml/parse.rs:20-63`), which is why an opaque span is safe to
hand out: nothing persists across the reparse.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn mapping_leaf_span_is_its_own_line_verbatim_with_indentation() {
    assert_eq!(
        spans_of("a:\n  b: 1\n  c: 2\n", &[key("a"), key("b")]),
        vec!["  b: 1\n"]
    );
}

#[test]
fn mapping_branch_span_covers_its_children() {
    assert_eq!(
        spans_of("a:\n  b: 1\nz: 9\n", &[key("a")]),
        vec!["a:\n  b: 1\n"]
    );
}

#[test]
fn sequence_item_span_is_its_own_line() {
    assert_eq!(
        spans_of("a:\n  - one\n  - two\n", &[key("a"), Seg::Index(1)]),
        vec!["  - two\n"]
    );
}

#[test]
fn flow_sequence_element_span_is_trimmed_and_excludes_the_comma() {
    assert_eq!(
        spans_of("a: [ 1, 22 ]\n", &[key("a"), Seg::Index(1)]),
        vec!["22"]
    );
}

#[test]
fn opaque_node_span_is_its_raw_text() {
    // An anchor/alias/tag construct the backend fences as opaque: the Block is
    // still its raw text, because `E` already rewrites it and fencing is
    // recomputed on every parse (`yaml/parse.rs:20-63`).
    let src = "base: &b\n  x: 1\nuse: *b\n";
    let got = spans_of(src, &[key("use")]);
    assert_eq!(got, vec!["use: *b\n"], "opaque nodes must be editable as a Block");
}
```

Before writing the last test, confirm which construct this backend actually fences as opaque —
read `yaml/parse.rs:20-63` and the assertion at `yaml/edit/tests.rs:795` — and use **that**
construct, not a guess.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p confy-core yaml::edit::spans` — Expected: all FAIL with `[]`.

- [ ] **Step 3: Implement**

Mirror Tasks 2-3. The YAML-specific rules:

- The span starts at the **line's indentation**, not at the key token — the buffer is verbatim,
  indentation included (Global Constraints), so the indent must be inside the span or the
  re-splice would lose it.
- No dedent, no reindent, anywhere in this module.
- An opaque Node returns its raw text range instead of an empty `Vec`.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p confy-core yaml::edit::spans` — Expected: PASS.

- [ ] **Step 5: Restate what `read_only` means**

`read_only` narrows from "not writable" to "not **structurally** editable" — rename, `K`, remark
and paste-into stay refused; a raw-text Block edit is allowed.

In `docs/reference/glossary.md:169-175`, replace the opaque-node paragraph's "cannot be edited"
claim with:

```markdown
An **opaque node** is a YAML construct the backend cannot model structurally (anchors, aliases,
tags). It is **read-only** in the narrow sense: no rename, no kind switch, no remark, no
paste-into — every *structural* Mutation refuses it. Its **text** is editable, through the
multi-line Block editor (`e`) and through whole-document editing (`E`), because opaque fencing
is recomputed from scratch on every parse and so never outlives the reparse.
```

In `docs/reference/BEHAVIOR_MATRIX.md:214-215`, update the opaque row's `e` cell from refused to
allowed (raw text, no confirmation prompt) and leave every structural cell unchanged.

- [ ] **Step 6: Rewrite the two tests that pinned the old policy**

`crates/confy-core/src/model/yaml/edit/tests.rs:795` and
`crates/confy-core/tests/session_headless.rs:3746` currently assert an opaque node refuses
editing. Rewrite them to assert the **new** split — structural Mutations still refused, a Block
edit permitted — rather than deleting them. Keep each test's name accurate to what it now pins
(e.g. `opaque_node_refuses_structural_mutations_but_allows_a_block_edit`).

- [ ] **Step 7: Run the full suite and commit**

```bash
cargo fmt && cargo clippy -p confy-core -- -D warnings && cargo test -p confy-core
git add crates/confy-core/src/model/yaml/ crates/confy-core/tests/session_headless.rs \
        docs/reference/glossary.md docs/reference/BEHAVIOR_MATRIX.md
git commit -m "feat(yaml): node_text_spans; opaque nodes are text-editable, not structurally"
```

`CHANGELOG.md` gets no entry yet — no host reaches this behavior until Task 6 — but the
`glossary.md` and `BEHAVIOR_MATRIX.md` edits **must** be in this commit, because they describe
the code it lands.

---

## Task 5: `splice_spans`, `apply_block_text`, the intent, the notices, and the test matrix

Spec §3-§8. Still inert: `Intent::ApplyBlockText` exists and works, but no host dispatches it.

**Files:**
- Create: `crates/confy-core/src/model/block_splice.rs`
- Modify: `crates/confy-core/src/session/inline_edit.rs` (add `apply_block_text`; remove Task 0's
  guard is **not** done here — see Task 8)
- Modify: `crates/confy-core/src/session/intent.rs:213` area, `session/dispatch.rs:246-263`
- Modify: `crates/confy-core/src/session/notice.rs:57` (`severity_of`), `i18n/en.json`,
  `i18n/zh-TW.json`, `docs/reference/MESSAGES.md` §2.2
- Create: `crates/confy-core/tests/block_edit_parity.rs`,
  `crates/confy-core/tests/block_edit_proptest.rs`

**Interfaces:**
- Consumes: `ConfigDocument::node_text_spans` (Tasks 1-4).
- Produces:
  - `pub fn splice_spans(text: &str, spans: &[(usize, usize)], replacement: &str) -> String`
  - `pub fn apply_block_text(&mut self, path: Path, text: String)` on `Session`
  - `Intent::ApplyBlockText { path: Path, text: String }`
  - notice keys `core.block.invalid` (one `{0}` arg: the backend's error) and `core.block.empty`
    (no args)

- [ ] **Step 1: Write the failing splice tests**

In `crates/confy-core/src/model/block_splice.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::splice_spans;

    #[test]
    fn single_span_is_replaced_in_place() {
        assert_eq!(splice_spans("ab_cd", &[(2, 3)], "XY"), "abXYcd");
    }

    #[test]
    fn multi_span_replaces_the_first_and_deletes_the_rest() {
        // Design record §3: later spans are removed, the buffer lands at the
        // first — so a scattered table consolidates where it started.
        assert_eq!(splice_spans("A1-B-A2", &[(0, 2), (5, 7)], "Z"), "Z-B-");
    }

    #[test]
    fn spans_out_of_order_are_handled_by_offset_not_by_argument_order() {
        assert_eq!(splice_spans("A1-B-A2", &[(5, 7), (0, 2)], "Z"), "Z-B-");
    }

    #[test]
    fn an_empty_span_list_returns_the_text_unchanged() {
        assert_eq!(splice_spans("abc", &[], "Z"), "abc");
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p confy-core block_splice`
Expected: FAIL to compile — `splice_spans` not found.

- [ ] **Step 3: Implement `splice_spans`**

```rust
//! The one text splice behind a Block edit (design record §4). Deliberately
//! format-agnostic: everything format-specific already happened in
//! `ConfigDocument::node_text_spans`, and everything semantic happens after,
//! in the backend's whole-document `Replace`.

/// `text` with `replacement` written over the **first** span and every later
/// span removed. Later spans are cut in reverse document order so earlier
/// offsets stay valid. `spans` need not be sorted.
pub fn splice_spans(text: &str, spans: &[(usize, usize)], replacement: &str) -> String {
    if spans.is_empty() {
        return text.to_string();
    }
    let mut ordered: Vec<(usize, usize)> = spans.to_vec();
    ordered.sort_by_key(|(a, _)| *a);
    let mut out = text.to_string();
    for &(a, b) in ordered.iter().skip(1).rev() {
        out.replace_range(a..b, "");
    }
    let (a, b) = ordered[0];
    out.replace_range(a..b, replacement);
    out
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p confy-core block_splice` — Expected: PASS (4/4).

- [ ] **Step 5: Register the two notice keys, both message texts, and the rejection accessor**

In `severity_of` (`session/notice.rs:57`), in the `Severity::Warn` arm — **not** the `Error`
arm — with the reason recorded, since the next reader will ask:

```rust
        // A rejected Block edit: `Warn`, not `Error`, because the host keeps
        // the editor open holding the user's text (design record §5-6) — no
        // typing is lost. Contrast `core.document.apply-failed`, which is
        // `Error` because that route closes its surface.
        | "core.block.invalid"
        | "core.block.empty"
```

`i18n/en.json` (canonical):

```json
  "core.block.invalid": "the edited block was not applied: {0}",
  "core.block.empty": "an empty block is not applied — use d to delete this node",
```

`i18n/zh-TW.json`:

```json
  "core.block.invalid": "編輯的區塊未套用：{0}",
  "core.block.empty": "空白區塊不會套用 —— 請用 d 刪除這個節點",
```

Add both keys to `docs/reference/MESSAGES.md` §2.2's severity table in the `Warn` tier with a
one-line "when" column matching the JSON text.

Finally add the accessor both the parity test (Step 7) and the TUI's re-spawn loop (Task 6)
need — one place answers "was the last commit a rejected Block?", so the hosts never
string-match a message:

```rust
    /// Whether the current notice is a rejected Block commit — the condition
    /// a host's editor loop re-spawns on (design record §5).
    pub fn notice_is_block_rejection(&self) -> bool {
        matches!(
            self.notice.as_ref().map(|n| n.key.as_str()),
            Some("core.block.invalid") | Some("core.block.empty")
        )
    }
```

If `Notice` carries no `key` field today, add one and set it in the `Notice::core`/`host_*`
constructors, which already receive the key. That is load-bearing for the host loop, not a test
convenience.

- [ ] **Step 6: Run the catalog test to verify it now passes**

Run: `cargo test -p confy-core severity_of_covers_the_full_catalog_table`
Expected: PASS. If it fails, the test enumerates the catalog from the JSON — the two keys must
be in **both** `severity_of` and `i18n/en.json`, spelled identically.

- [ ] **Step 7: Write the failing `apply_block_text` tests**

Create `crates/confy-core/tests/block_edit_parity.rs`. Structure it data-driven: one table of
cases, one loop, so a new shape is a row rather than a function. Cover the full
{shape × format × outcome} matrix from spec §8; these are the rows that must exist — write every
one, they are the deliverable:

```rust
//! The Block-edit contract (design record §8): a buffer is accepted iff it
//! parses on its own as a legal Node sequence at the edited Node's container
//! level; otherwise the whole commit is rejected and the document is untouched.

use confy_core::model::node::Seg;
use confy_core::session::{Intent, Session};

struct Case {
    name: &'static str,
    src: &'static str,
    path: &'static [&'static str],
    buffer: &'static str,
    /// `Some(expected document)` = accepted; `None` = rejected, document
    /// untouched and a `core.block.*` notice raised.
    expect: Option<&'static str>,
}

const CASES: &[Case] = &[
    // ---- outcome: unchanged buffer is byte-identical ----
    Case {
        name: "toml/leaf/unchanged",
        src: "[s]\nk1 = 1\n",
        path: &["s", "k1"],
        buffer: "k1 = 1\n",
        expect: Some("[s]\nk1 = 1\n"),
    },
    // ---- outcome: the key rename the old mechanism could not do ----
    Case {
        name: "toml/leaf/rename-key",
        src: "[s]\nk1 = 1\nk2 = 2\n",
        path: &["s", "k1"],
        buffer: "renamed = 1\n",
        expect: Some("[s]\nrenamed = 1\nk2 = 2\n"),
    },
    // ---- outcome: N siblings from one Node ----
    Case {
        name: "toml/leaf/two-siblings",
        src: "[s]\nk1 = 1\n",
        path: &["s", "k1"],
        buffer: "k1 = 1\nk3 = 3\n",
        expect: Some("[s]\nk1 = 1\nk3 = 3\n"),
    },
    // ---- outcome: Comment -> live Node ----
    Case {
        name: "toml/comment/to-live",
        src: "# k = 1\nz = 9\n",
        path: &["#0"],
        buffer: "k = 1\n",
        expect: Some("k = 1\nz = 9\n"),
    },
    // ---- outcome: live Node -> Comment ----
    Case {
        name: "toml/leaf/to-comment",
        src: "k = 1\nz = 9\n",
        path: &["k"],
        buffer: "# k = 1\n",
        expect: Some("# k = 1\nz = 9\n"),
    },
    // ---- outcome: rejected, document untouched ----
    Case {
        name: "toml/leaf/duplicate-key",
        src: "[s]\nk1 = 1\nk2 = 2\n",
        path: &["s", "k1"],
        buffer: "k2 = 7\n",
        expect: None,
    },
    Case {
        name: "toml/leaf/unparsable",
        src: "[s]\nk1 = 1\n",
        path: &["s", "k1"],
        buffer: "k1 = = =\n",
        expect: None,
    },
    // ---- outcome: empty buffer rejected ----
    Case {
        name: "toml/leaf/empty-buffer",
        src: "[s]\nk1 = 1\n",
        path: &["s", "k1"],
        buffer: "",
        expect: None,
    },
    // ---- shape: branch section ----
    Case {
        name: "toml/section/rename-header",
        src: "[a]\nk = 1\n[b]\nm = 2\n",
        path: &["a"],
        buffer: "[c]\nk = 1\n",
        expect: Some("[c]\nk = 1\n[b]\nm = 2\n"),
    },
    // ---- shape: scattered table consolidates at its first span ----
    Case {
        name: "toml/scattered-table/consolidates",
        src: "[a]\nk = 1\n[b]\nm = 2\n[a]\nj = 3\n",
        path: &["a"],
        buffer: "[a]\nk = 1\nj = 3\n",
        expect: Some("[a]\nk = 1\nj = 3\n[b]\nm = 2\n"),
    },
    // ---- shape: array element ----
    Case {
        name: "toml/array-element/edit",
        src: "a = [ 1, 22 ]\n",
        path: &["a", "#1"],
        buffer: "33",
        expect: Some("a = [ 1, 33 ]\n"),
    },
    // ---- shape: flow member ----
    Case {
        name: "toml/inline-member/rename",
        src: "a = { x = 1, y = 2 }\n",
        path: &["a", "x"],
        buffer: "w = 1",
        expect: Some("a = { w = 1, y = 2 }\n"),
    },
    // ---- shape: dotted table ----
    Case {
        name: "toml/dotted-table/consolidates",
        src: "a.x = 1\nb = 9\na.y = 2\n",
        path: &["a"],
        buffer: "a.x = 1\na.y = 2\n",
        expect: Some("a.x = 1\na.y = 2\nb = 9\n"),
    },
    // ---- shape: AoT entry ----
    Case {
        name: "toml/aot-entry/rename-member",
        src: "[[p]]\nn = 1\n[[p]]\nn = 2\n",
        path: &["p", "#0"],
        buffer: "[[p]]\nm = 1\n",
        expect: Some("[[p]]\nm = 1\n[[p]]\nn = 2\n"),
    },
    // ---- format: JSON ----
    Case {
        name: "json/member/rename-key",
        src: "{\n  \"a\": 1,\n  \"b\": 2\n}\n",
        path: &["a"],
        buffer: "\"renamed\": 1,\n",
        expect: Some("{\n  \"renamed\": 1,\n  \"b\": 2\n}\n"),
    },
    Case {
        name: "json/member/duplicate-key",
        src: "{\n  \"a\": 1,\n  \"b\": 2\n}\n",
        path: &["a"],
        buffer: "\"b\": 7,\n",
        expect: None,
    },
    Case {
        name: "json/member/two-siblings",
        src: "{\n  \"a\": 1\n}\n",
        path: &["a"],
        buffer: "\"a\": 1,\n  \"c\": 3\n",
        expect: Some("{\n  \"a\": 1,\n  \"c\": 3\n}\n"),
    },
    Case {
        name: "json/member/empty-buffer",
        src: "{\n  \"a\": 1\n}\n",
        path: &["a"],
        buffer: "",
        expect: None,
    },
    // ---- format: YAML ----
    Case {
        name: "yaml/leaf/rename-key-keeps-indentation",
        src: "a:\n  b: 1\n  c: 2\n",
        path: &["a", "b"],
        buffer: "  renamed: 1\n",
        expect: Some("a:\n  renamed: 1\n  c: 2\n"),
    },
    Case {
        name: "yaml/leaf/two-siblings",
        src: "a:\n  b: 1\n",
        path: &["a", "b"],
        buffer: "  b: 1\n  d: 4\n",
        expect: Some("a:\n  b: 1\n  d: 4\n"),
    },
    Case {
        name: "yaml/leaf/unparsable",
        src: "a:\n  b: 1\n",
        path: &["a", "b"],
        buffer: "  - this: is\n   bad: indentation\n",
        expect: None,
    },
    Case {
        name: "yaml/leaf/empty-buffer",
        src: "a:\n  b: 1\n",
        path: &["a", "b"],
        buffer: "",
        expect: None,
    },
    Case {
        name: "yaml/opaque/text-edit-allowed",
        src: "base: &b\n  x: 1\nuse: *b\n",
        path: &["use"],
        buffer: "use: *b  # keep\n",
        expect: Some("base: &b\n  x: 1\nuse: *b  # keep\n"),
    },
];
```

Plus the runner and the two invariants every rejected case shares:

```rust
/// `"#N"` means `Seg::Index(N)`; anything else is a key.
fn seg(s: &str) -> Seg {
    match s.strip_prefix('#').and_then(|n| n.parse::<usize>().ok()) {
        Some(i) => Seg::Index(i),
        None => Seg::Key(s.to_string()),
    }
}

#[test]
fn block_edit_matrix() {
    let mut failures = Vec::new();
    for c in CASES {
        let fmt = c.name.split('/').next().unwrap();
        let mut s = Session::from_str_as(c.src, fmt).expect(c.name);
        let path: Vec<Seg> = c.path.iter().copied().map(seg).collect();
        s.dispatch(Intent::ApplyBlockText {
            path,
            text: c.buffer.to_string(),
        });
        let got = s.document_text();
        match c.expect {
            Some(want) if got != want => {
                failures.push(format!("{}: accepted wrongly\n got {got:?}\nwant {want:?}", c.name))
            }
            Some(_) => {}
            None => {
                if got != c.src {
                    failures.push(format!("{}: rejected but document changed to {got:?}", c.name));
                }
                if !s.notice_is_block_rejection() {
                    failures.push(format!("{}: no core.block.* notice ({:?})", c.name, s.notice()));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n---\n"));
}
```

**Adapt the three API calls to this codebase before running:** `Session::from_str_as`,
`document_text()`, and `notice_is_block_rejection()` are written above from the shape they need,
not from a verified signature. Read `crates/confy-core/tests/session_headless.rs`'s own setup
helper and `Session`'s public surface in `session/session.rs`, and use exactly what they use for
the first two. `notice_is_block_rejection()` is **new** — add it in Step 5 below, since both this
test and Task 6's re-spawn loop need the same question answered:

```rust
    /// Whether the current notice is a rejected Block commit — the condition
    /// a host's editor loop re-spawns on (design record §5).
    pub fn notice_is_block_rejection(&self) -> bool {
        matches!(
            self.notice.as_ref().map(|n| n.key.as_str()),
            Some("core.block.invalid") | Some("core.block.empty")
        )
    }
```

If `Notice` carries no `key` field today, add one in Step 5 — it is load-bearing for the host
loop, not a test convenience — and set it in the `Notice::core`/`host_*` constructors, which
already receive the key.

Do the same for the `Seg` spelling of a Comment node (`"#0"` above assumes `Seg::Index`) — Task
2 Step 1 already settled this; reuse that answer.

- [ ] **Step 8: Run to verify it fails**

Run: `cargo test -p confy-core --test block_edit_parity`
Expected: FAIL to compile — `Intent::ApplyBlockText` does not exist.

- [ ] **Step 9: Add the intent**

In `crates/confy-core/src/session/intent.rs`, beside `ApplyReplace` (~line 213):

```rust
    /// The host's multi-line editor returned the **Block** text for `path`
    /// (design record §5). Additive: `ApplyReplace`/`ApplyEditComment` keep
    /// working, and `ApplyReplace` at the empty path stays the whole-document
    /// route (VS Code's reparse channel).
    ApplyBlockText { path: Path, text: String },
```

In `session/dispatch.rs`, beside the existing `ApplyReplace` arm:

```rust
            Intent::ApplyBlockText { path, text } => {
                self.pending_external_edit = None;
                self.apply_block_text(path, text);
            }
```

- [ ] **Step 10: Implement `apply_block_text`**

In `crates/confy-core/src/session/inline_edit.rs`, next to `apply_document_text`:

```rust
    /// Block commit (design record §4): `text` is the complete text of the
    /// span(s) the node at `path` owns. It is spliced into the document's own
    /// text and committed at the **empty** path, so legality is exactly
    /// "does the whole file still parse and validate" — which is how a key
    /// rename, N siblings and a Comment↔live conversion all become legal
    /// without a new `Mutation` variant.
    ///
    /// Rejection is atomic (every backend's `apply` commits only on success)
    /// and reports `core.block.invalid` at `Warn`: the host keeps its editor
    /// open holding the user's text, so nothing is lost. An empty buffer is
    /// refused outright (`core.block.empty`) — deletion is `d`.
    pub fn apply_block_text(&mut self, path: Path, text: String) {
        if text.trim().is_empty() {
            self.set_notice(Notice::core(self.lang, "core.block.empty", &[]));
            return;
        }
        let Some(doc) = self.doc.as_ref() else {
            return;
        };
        let spans = doc.node_text_spans(&path);
        if spans.is_empty() {
            self.set_notice(Notice::core(
                self.lang,
                "core.block.invalid",
                &["this node has no editable block"],
            ));
            return;
        }
        let new_text = crate::model::block_splice::splice_spans(&doc.serialize(), &spans, &text);
        let anchor = spans[0].0;
        let doc = self.doc.as_mut().expect("checked above");
        match doc.apply(Mutation::Replace {
            path: Vec::new(),
            fragment: new_text,
        }) {
            // `None`: a Block commit can add or rename arbitrary paths, so the
            // one-path schema fast skip is unsound here (design record §7).
            Ok(new_text) => {
                self.on_mutation_success(None, new_text);
                self.reanchor_cursor_after_block(&path, anchor);
            }
            Err(e) => self.set_notice(Notice::core(
                self.lang,
                "core.block.invalid",
                &[&e.to_string()],
            )),
        }
    }

    /// Cursor re-anchor after a Block commit (design record §5): the pre-edit
    /// path if it still resolves, else the first node whose extent starts at
    /// or after the edited span's start offset, else the first row — which is
    /// what `compute_rows` already snaps to. Span-based, not row-index-based,
    /// so a key rename lands on the renamed node.
    fn reanchor_cursor_after_block(&mut self, path: &Path, anchor: usize) {
        if self.tree.node_at(path).is_some() {
            self.cursor = path.clone();
            return;
        }
        if let Some(p) = self.path_at_offset(anchor) {
            self.cursor = p;
        }
        self.rebuild_rows();
    }
```

`path_at_offset` does not exist yet. Implement it as the smallest thing that works: walk the
projected `NodeTree` in document order and return the first node whose `node_text_spans` start
is `>= anchor`. If that proves awkward against the tree's actual API, an acceptable alternative
is to compare against `trailing_blank_anchor`/span starts of the root's children recursively —
but **do not** fall back to a visible-row index, which is the rule spec §5 explicitly rejects.
Set `self.cursor` only through whatever setter the surrounding code uses (check how
`remap_renamed_path` and `compute_rows` touch the cursor at `session/session.rs:425-427`), so
selection and clipboard locks stay consistent (`ROW_STATE_MODEL.md`).

- [ ] **Step 11: Run the matrix to verify it passes**

Run: `cargo test -p confy-core --test block_edit_parity`
Expected: PASS. Every failure prints its case name; fix the backend span or the expectation,
whichever the spec says is right — and when a case's expectation turns out to contradict the
spec, fix the **spec's** §8 list in the same commit rather than quietly changing the test.

- [ ] **Step 12: Write the byte-identity proptest**

Create `crates/confy-core/tests/block_edit_proptest.rs`. Model it on the existing
`crates/confy-core/tests/roundtrip_proptest.rs` — use the same generator and the same proptest
configuration style, do not invent a second convention:

```rust
//! Design record §8: returning a Block's buffer **unmodified** must leave the
//! document byte-identical. Under the whole-file route that invariant is a
//! pure string equality, which is exactly why it can be swept randomly — and
//! it is the invariant a mis-computed span breaks first.

proptest! {
    #[test]
    fn unmodified_block_buffer_is_byte_identical(doc in arb_document()) {
        let before = doc.text.clone();
        let mut s = Session::from_str_as(&before, doc.fmt).unwrap();
        for path in all_paths(&s) {
            let mut s2 = Session::from_str_as(&before, doc.fmt).unwrap();
            let buffer = s2.multiline_edit_initial(&path);
            prop_assume!(!buffer.is_empty());
            s2.dispatch(Intent::ApplyBlockText { path: path.clone(), text: buffer });
            prop_assert_eq!(
                s2.document_text(), before.clone(),
                "path {:?} was not byte-identical", path
            );
        }
        let _ = &mut s;
    }
}
```

`all_paths` is a helper this file owns: walk the session's projected tree and collect every
node's path. `arb_document` is whatever `roundtrip_proptest.rs` already generates — reuse it via
a shared test module if it is `pub`, otherwise copy the strategy with a comment pointing at the
original.

**Note the dependency:** this test asserts `multiline_edit_initial` returns the Block's exact
text. Today it returns `serialize_fragment` + a packaged blank run, which is **not** the Block
(spec §3 — the Block includes the trailing EOL comment and the blank run in one verbatim slice).
So in this step also change `multiline_edit_initial` to be defined in terms of the spans:

```rust
    pub fn multiline_edit_initial(&self, path: &Path) -> String {
        let Some(doc) = self.doc.as_ref() else {
            return String::new();
        };
        let spans = doc.node_text_spans(path);
        if spans.is_empty() {
            // No Block: fall back to the fragment, so the inline/whole-document
            // callers that never had one keep working until Task 8.
            return doc.serialize_fragment(path);
        }
        let text = doc.serialize();
        spans
            .iter()
            .map(|&(a, b)| &text[a..b])
            .collect::<Vec<_>>()
            .join("")
    }
```

Keep the old doc-comment's explanation of *why* the blank run is in the buffer, updated to say
the run now arrives as part of the span rather than through `with_trailing_run`.

- [ ] **Step 13: Run the proptest**

Run: `cargo test -p confy-core --test block_edit_proptest`
Expected: PASS. A failure prints a minimal shrunk document plus the offending path — that is a
span bug in Tasks 2-4, not a proptest bug. Fix the span and re-run; add the shrunk case as a
literal row in `block_edit_parity.rs`'s `CASES` so it can never silently return.

- [ ] **Step 14: Full suite, perf check, commit**

```bash
cargo fmt && cargo clippy -- -D warnings && cargo test
cargo bench -p confy-core --bench perf -- --nodes 5000
```

The bench is a guard, not a gate: `apply(Replace scalar)` was 305 ms and `project()` 89.0 ms at
5000 nodes before this work. Nothing in this task touches either path, so a >10% move means
something unintended changed — investigate before committing.

`CHANGELOG.md`: still no entry (no host reaches this yet).

```bash
git add crates/confy-core/src/model/block_splice.rs crates/confy-core/src/session/ \
        crates/confy-core/tests/block_edit_parity.rs \
        crates/confy-core/tests/block_edit_proptest.rs \
        i18n/en.json i18n/zh-TW.json docs/reference/MESSAGES.md
git commit -m "feat(core): Session::apply_block_text — Block edits via whole-file reparse"
```

---

## Task 6: TUI switchover — the first user-visible change

Spec §5, §9 step 1. **This commit is where the behavior ships**, so it carries the contract
docs, the `CHANGELOG.md` entry, and the real-binary verification.

**Files:**
- Modify: `crates/confy-tui/src/tui/app.rs:700-760` (`edit_node`'s external branch and
  `spawn_pending_external_edit`)
- Modify: `docs/reference/BEHAVIOR_MATRIX.md` (the contract sentence + the `e` rows),
  `docs/reference/HOST_PARITY.md` (a transitional row), `docs/reference/MUTATIONS.md`
  (retire `replace_table_spans`' two bespoke checks, §84-86),
  `docs/reference/glossary.md` (the **Block** entry), `CONTEXT.md` if its index gains the term
- Test: `crates/confy-tui/src/tui/tests.rs`

**Interfaces:**
- Consumes: `Intent::ApplyBlockText`, `Session::apply_block_text` (Task 5).
- Produces: nothing for later tasks — Tasks 7-8 consume Task 5's API directly.

- [ ] **Step 1: Write the failing TUI test**

In `crates/confy-tui/src/tui/tests.rs`, using whatever fake-editor seam the existing external-
edit tests use (read the tests around `crates/confy-tui/src/tui/tests.rs:404`, which pins the
"a preceding standalone comment is its own node" rule, and reuse their harness):

```rust
#[test]
fn e_on_a_leaf_can_rename_its_key() {
    // The gesture the old 1:1 fragment mechanism silently dropped.
    let mut app = app_with("[s]\nk1 = 1\nk2 = 2\n");
    app.move_cursor_to_key("k1");
    app.edit_node_with_fake_editor(|buf| buf.replace("k1", "renamed"));
    assert_eq!(app.document_text(), "[s]\nrenamed = 1\nk2 = 2\n");
}

#[test]
fn a_rejected_block_keeps_the_users_text_and_reopens_the_editor() {
    let mut app = app_with("[s]\nk1 = 1\nk2 = 2\n");
    app.move_cursor_to_key("k1");
    // First pass writes a duplicate key (rejected); the retry fixes it.
    let mut pass = 0;
    app.edit_node_with_fake_editor(move |buf| {
        pass += 1;
        if pass == 1 {
            assert_eq!(buf, "k1 = 1\n", "first spawn seeds the block");
            "k2 = 7\n".to_string()
        } else {
            assert_eq!(buf, "k2 = 7\n", "the retry is seeded with the user's own text");
            "k1 = 7\n".to_string()
        }
    });
    assert_eq!(app.document_text(), "[s]\nk1 = 7\nk2 = 2\n");
}

#[test]
fn a_rejected_block_never_injects_the_error_into_the_buffer() {
    let mut app = app_with("[s]\nk1 = 1\nk2 = 2\n");
    app.move_cursor_to_key("k1");
    let mut seen = Vec::new();
    app.edit_node_with_fake_editor(|buf| {
        seen.push(buf.to_string());
        if seen.len() == 1 { "k1 = = =\n".to_string() } else { "k1 = 1\n".to_string() }
    });
    assert!(
        seen.iter().all(|b| !b.contains('#') || b.starts_with("k1")),
        "an error comment leaked into the buffer: {seen:?}"
    );
}
```

If no fake-editor seam exists, add the narrowest one: a field on the app holding
`Option<Box<dyn FnMut(&str) -> String>>` that `edit_node` consults instead of
`crate::tui::editor::edit_text` when set. That seam is the only way to test the re-spawn loop
without a terminal, and it is `#[cfg(test)]`-gated.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p confy-tui e_on_a_leaf_can_rename_its_key`
Expected: FAIL — the key rename is dropped (or, after Task 0, rejected), so the document is
unchanged.

- [ ] **Step 3: Switch both `$EDITOR` sites to the Block route with a re-spawn loop**

Replace the `apply_external_replace` calls at `app.rs:718` and `:754` with a shared loop. Both
sites currently differ only in where the path comes from, so factor one helper:

```rust
    /// Spawn `$EDITOR` on the Block at `path` and commit it, re-spawning
    /// seeded with the user's own text while the commit is rejected (design
    /// record §5: a rejected buffer must never cost the user their typing,
    /// and the error travels on the notice channel, never inside the buffer).
    /// HOST SPLIT: spawns $EDITOR.
    fn edit_block_at(&mut self, path: Path) {
        let mut buffer = self.session.multiline_edit_initial(&path);
        if buffer.is_empty() {
            return;
        }
        loop {
            let edited = match self.spawn_editor(&buffer) {
                Ok(t) => t,
                Err(e) => {
                    self.report_editor_error(e);
                    return;
                }
            };
            // Unmodified buffer = quit without saving: cancel rather than
            // splicing identical text back and dirtying the document.
            if edited == buffer {
                return;
            }
            self.session.dispatch(Intent::ApplyBlockText {
                path: path.clone(),
                text: edited.clone(),
            });
            if !self.session.notice_is_block_rejection() {
                break;
            }
            buffer = edited;
        }
        self.rebuild_rows();
    }
```

`spawn_editor` and `report_editor_error` are extracted verbatim from the two existing call
sites (the `crate::tui::editor::edit_text` call and the `tui.host.editor-error` dispatch) — do
not rewrite their behavior. `notice_is_block_rejection()` already exists — Task 5 Step 5 added
it to `Session`, and this loop is the reason it is public rather than test-only.

**Keep the comment branch untouched in this task.** `edit_node`'s `is_comment` path still calls
`apply_edit_comment`; Task 8 retires it. Note that in a comment on the branch so the next reader
does not think it was missed.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p confy-tui` then `cargo test`
Expected: PASS. `crates/confy-tui/src/tui/tests.rs:404` (the preceding-comment rule) must still
pass unchanged — the Block excludes a preceding standalone Comment, which is exactly what that
test pins.

- [ ] **Step 5: Write the contract into the reference docs**

`docs/reference/BEHAVIOR_MATRIX.md` — add the contract sentence from Global Constraints verbatim
as the multi-line-editor section's opening rule, then adjust the per-scope `e` rows to say
"Block (accepted iff the buffer parses as a legal Node sequence at this container level)"
instead of the old per-node fragment wording.

`docs/reference/MUTATIONS.md:84-86` — retire `replace_table_spans`' two bespoke checks:

```markdown
A block edit's legality is whatever the reparse and the DOM validation accept: there is no
"headers must stay inside the subtree" or "the block must start with a `[header]`" check any
more. Those existed because the old mechanism could only consolidate in place, so a header
leaving the subtree was necessarily an accident; with a Block it is an intent.
```

`docs/reference/glossary.md` — add the new term, alphabetically placed:

```markdown
### Block

The byte span(s) a Node owns, as text — what the multi-line editor (`e`, `$EDITOR`) opens and
commits. A Block covers the Node's body, its trailing EOL comment and its trailing blank-line
run; a *preceding* standalone **Comment** is an independent Node and never part of it. Inside a
flow collection a Block is the Node's own token range trimmed of whitespace, with the separating
`,` outside it. One Node may own several spans (a scattered `[T/S]`, a `[T/D]` dotted table, a
scattered `[A/T]` group), in which case a commit consolidates them at the first. Computed by
`ConfigDocument::node_text_spans`, committed by `Session::apply_block_text`.
```

`docs/reference/HOST_PARITY.md` — add the transitional row, **with its removal condition**:

```markdown
| Multi-line editor commit route | TUI: `ApplyBlockText` (Block) | Web/touch: `ApplyReplace` (1:1 fragment) | Transitional — removed when the web hosts migrate (plan Task 7) |
```

- [ ] **Step 6: Changelog**

```markdown
### Changed
- The multi-line editor (`e` / `$EDITOR`) now edits a Node's **Block** — the text of the spans
  it owns — instead of a fragment bound 1:1 to that Node. A buffer may rename the Node's key,
  emit several sibling Nodes, or convert a Comment to live content and back; it is accepted iff
  it parses as a legal Node sequence at the Node's container level, and otherwise the whole
  commit is rejected, the document is untouched, and the editor reopens holding your text.
  (TUI first; the web hosts follow.)
- YAML opaque nodes (anchors, aliases, tags) are now **text**-editable through `e`. They remain
  read-only structurally: no rename, kind switch, remark, or paste-into.
```

- [ ] **Step 7: Commit**

```bash
cargo fmt && cargo clippy -- -D warnings && cargo test
git add crates/confy-tui/ crates/confy-core/src/session/notice.rs docs/reference/ CHANGELOG.md
git commit -m "feat(tui): the multi-line editor edits a Node's Block"
```

- [ ] **Step 8: Verify on the real binary — the gate for this whole plan**

Green unit tests are not the gate. Run each of these against the built binary and confirm the
described outcome before claiming the task done:

```bash
cargo build --release
printf '[s]\nk1 = 1  # note\nk2 = 2\n' > /tmp/blk.toml
```

1. **Key rename** — `EDITOR=vi ./target/release/confy --lang en /tmp/blk.toml`, cursor to `k1`,
   `e`, change `k1` to `renamed`, `:wq`. Expect the row to read `renamed` and the cursor to
   still be on it (spec §5's span re-anchor).
2. **Two siblings** — `e` on `renamed`, add a second line `k3 = 3`, `:wq`. Expect two rows.
3. **Rejected duplicate** — `e` on `k3`, change its key to `k2`, `:wq`. Expect: a `Warn` notice,
   the document unchanged, and **`$EDITOR` reopening with `k2 = ...`, your own text**. Fix it to
   `k3 = 3` and `:wq`; expect it to commit.
4. **Empty buffer** — `e`, delete everything, `:wq`. Expect the `core.block.empty` notice
   naming `d`, the document unchanged, and no deletion.
5. **Trailing comment preserved** — `e` on `k1`/`renamed` (which carries `# note`), change only
   the value, `:wq`. Expect the comment intact.
6. **Comment ↔ live** — `e` on a `# …` row, remove the `#`, `:wq`. Expect a live Node. Re-add
   the `#` and expect a Comment.
7. **YAML opaque** — repeat (1) on a YAML file with an alias node; expect the text edit to
   apply where it used to be refused.
8. Repeat (1) and (3) on a `.json` file.

Record what you observed in the commit message body (amend) or, for anything that reads badly
but works, add a row to `docs/plan/BACKLOG.md`.

---

## Task 7: Web and touch switchover

Spec §5, §9 step 2.

**Files:**
- Modify: `web/ui.ts:1417-1419`, `web/touch/app.ts:1104-1106`, `web/types.ts:245-252,367-369`
- Modify: `docs/reference/WEBUI.md` (the wire-contract intent list),
  `docs/reference/HOST_PARITY.md` (delete Task 6's transitional row)
- Test: a new `web/block-edit.spec.mjs`, modelled on `web/raw-write.spec.mjs`

**Interfaces:**
- Consumes: `Intent::ApplyBlockText` (Task 5) over the wasm channel.
- Produces: nothing downstream.

**Critical build fact:** `npm run build` **copies** `pkg/`, it does not rebuild the wasm. A
`confy-core` change reaches the browser only after
`cd crates/confy-ffi && wasm-pack build --target web`. Do that first in this task or the new
intent will not exist at runtime.

- [ ] **Step 1: Rebuild the wasm and confirm the intent crossed the boundary**

```bash
cd crates/confy-ffi && wasm-pack build --target web && node functional_smoke.mjs
```
Expected: the smoke script passes. If `ApplyBlockText` is absent from the generated glue, the
`Intent` enum's serde representation did not change shape — check `web/types.ts`'s `Intent`
union mirrors it.

- [ ] **Step 2: Add the wire type**

In `web/types.ts` (~line 245, beside the existing external-edit intents):

```typescript
  | { ApplyBlockText: { path: Seg[]; text: string } }
```

Mirror the exact serde shape `Intent` serializes to — check an existing variant's spelling in
the same union rather than assuming.

- [ ] **Step 3: Write the failing web spec**

Create `web/block-edit.spec.mjs`, following `web/raw-write.spec.mjs`'s structure (esbuild-bundle
the module under test, tally `check(name, cond)`):

```javascript
check("a rejected block keeps the popup open with the user's text",
  afterReject.popupOpen === true && afterReject.popupText === "k2 = 7\n");
check("a rejected block leaves the document untouched",
  afterReject.documentText === "[s]\nk1 = 1\nk2 = 2\n");
check("a key rename in the popup applies",
  afterRename.documentText === "[s]\nrenamed = 1\nk2 = 2\n");
check("the popup never shows the error inside the buffer",
  !afterReject.popupText.includes("not applied"));
```

Run: `cd web && npm test` — Expected: the new spec FAILS.

- [ ] **Step 4: Switch both hosts' commit calls**

At `web/ui.ts:1417-1419` and `web/touch/app.ts:1104-1106`, replace the `ApplyReplace` dispatch
with `ApplyBlockText`, keeping the popup open when the resulting snapshot carries a
`core.block.*` notice. The web hosts already have this shape for raw write
(`web/raw-write.spec.mjs:202`, `web/touch-ext-apply.spec.mjs:141`) — reuse that exact pattern,
do not invent a second one.

**Leave `ApplyReplace` at the empty path alone**: it is raw-write mode and the VS Code reparse
channel, and this task does not touch it.

- [ ] **Step 5: Run the web gates**

```bash
cd web && npm run typecheck && npm test && npm run build
```
Expected: typecheck clean, all specs pass.

- [ ] **Step 6: Docs, changelog, commit**

Update `docs/reference/WEBUI.md`'s intent list with `ApplyBlockText` and **delete** the
transitional row Task 6 added to `docs/reference/HOST_PARITY.md` — its removal condition is now
met. `CHANGELOG.md`: extend Task 6's "Changed" bullet's parenthetical from "(TUI first; the web
hosts follow.)" to note that desktop web and touch now match.

```bash
git add web/ docs/reference/ CHANGELOG.md
git commit -m "feat(web): the multi-line popup edits a Node's Block"
```

---

## Task 8: Retire the superseded machinery

Spec §9 step 4. Pure deletion; no behavior change, which is why it is last and separate — a
reviewer can reject this without rejecting Task 6.

**Files:**
- Modify: `crates/confy-core/src/session/inline_edit.rs` (remove `apply_external_replace`'s
  `wrap_element` parameter and `split_packaged_blank`; **keep** `apply_packaged_blank` — the
  *inline* editor still uses it)
- Modify: `crates/confy-core/src/session/state.rs:37` (`wrap_element` field),
  `session/dispatch.rs`, `session/intent.rs` (remove `ApplyEditComment`),
  `session/view.rs:284-299` (`ExternalEditKind::Comment` if now unreachable)
- Modify: `crates/confy-core/src/model/cst_edit/replace_delete.rs` (remove Task 0's guard)
- Modify: `crates/confy-tui/src/tui/app.rs` (the `is_comment` branch → `edit_block_at`)
- Modify: `web/types.ts`, `docs/reference/MUTATIONS.md` (the F14 sentence Task 0 edited),
  `docs/reference/MESSAGES.md` if a key is dropped

- [ ] **Step 1: Point the comment branch at the Block route**

In `crates/confy-tui/src/tui/app.rs`, replace both `apply_edit_comment` calls (in `edit_node`
and `spawn_pending_external_edit`) with `self.edit_block_at(path)`. A Comment's Block is its
whole comment block (Task 2's `comment_node_span_is_its_whole_comment_block`), so the dedicated
comment route has nothing left to do.

- [ ] **Step 2: Run the comment tests to verify they still pass**

Run: `cargo test -p confy-core comment && cargo test -p confy-tui comment`
Expected: PASS. Any test asserting `ApplyEditComment` specifically should be rewritten to assert
the behavior (the comment text changed / a multi-line comment block edited as one) rather than
the intent name.

- [ ] **Step 3: Delete the superseded code**

Remove, in this order so the compiler guides you: `Intent::ApplyEditComment` and its dispatch
arm → `apply_edit_comment` → `ExternalEditKind::Comment` (only if nothing constructs it) →
`split_packaged_blank` → `apply_external_replace`'s `wrap_element` parameter and the
`pending_external_edit.wrap_element` field → `external_edit_path`'s bool return if it becomes
vestigial → Task 0's key guard in `replace_value` → the `web/types.ts` union members.

**Do not remove** `apply_packaged_blank` or `blank_lines::with_trailing_run` without checking
`crates/confy-core/src/session/inline_edit.rs`'s inline-editor callers: the inline editor is a
different feature and is out of scope.

- [ ] **Step 4: Verify nothing else referenced them**

```bash
cargo fmt && cargo clippy -- -D warnings && cargo test
cd web && npm run typecheck && npm test
cd crates/confy-ffi && wasm-pack build --target web && node functional_smoke.mjs
```
Expected: all clean. Also confirm `editors/vscode/src/schemaSessionManager.ts:60` still compiles
against the intent union — it uses `ApplyReplace{path: []}`, which this task keeps.

- [ ] **Step 5: Docs, changelog, commit**

In `docs/reference/MUTATIONS.md`, replace the interim F14 sentence Task 0 wrote with the final
truth: every backend's block edit is governed by the contract sentence, and the per-node
`Replace` surplus rule now applies only to the inline editor's fragments.

```markdown
### Removed
- The `ApplyEditComment` intent and the per-node fragment plumbing (`wrap_element`, the buffer's
  packaged blank-line split) the Block editor replaced. Comments are edited through the same
  Block route as every other Node.
```

```bash
git add crates/ web/ docs/reference/ CHANGELOG.md
git commit -m "refactor(core): retire the per-node fragment block-edit machinery"
```

- [ ] **Step 6: Final real-binary pass**

Re-run Task 6 Step 8's eight scenarios on the release binary, plus one comment-specific case:
`e` on a multi-line `# …` block, add a line, `:wq` — expect all lines to persist as one Comment
Node. Then run `cargo bench -p confy-core --bench perf -- --nodes 5000` one last time and record
the numbers in the commit body; spec §2 predicts block commits are now bounded by
whole-document `Replace` (~360-400 ms at 5000 sections), not by the 4.1-5.2 s trailing-comment
path.

---

## Follow-ups this plan deliberately does not do

Add these to `docs/plan/BACKLOG.md` (the repo's living backlog) as part of
Task 8's commit, since they are discovered-but-not-fixed by definition:

- Translating a rejected commit's document-space parse offset back into a buffer offset, so a
  host could place the caret on the offending line (spec §11).
- The `Block` term's interaction with the **Remark** operation (`r`): remarking a Node whose
  Block spans several lines is unchanged today, but the two concepts now overlap and the
  glossary should say how.
- The trailing-comment double-pass in `cst_edit/mod.rs:81` still costs 14× for any *inline*
  editor commit on a Node with an EOL comment (spec §1 defect 3). The Block route bypasses it;
  the inline route does not.
