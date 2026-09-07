# Trailing blank lines are an explicit, undoable node operation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user add, reduce, and fully remove the **blank lines after any node**, in all
three formats, from every host, as an ordinary undoable document mutation — with a confirmation
when the change would silently re-parent a following comment.

**Architecture:** A new `Mutation::SetTrailingBlankLines { path, n }`. The blank-line surgery
itself is **one shared, format-neutral text splice** (`model/blank_lines.rs`) operating on the
serialized document; each backend contributes only the one thing it alone knows — the byte
offset just past the node's contiguous extent — reusing the very span logic its `Delete`
already uses. The user-facing surface is **two Action-menu items** (ADR 0009: core owns the
menu, so all four hosts render them with no host code) plus a `Blank after: N` line in the
Detail popup.

**Rejected alternative (the original request's literal form):** pulling the trailing blank
lines into the `$EDITOR`/pop-editor buffer. It fails on three counts — `$EDITOR` only ever
opens for containers and multiline scalars, so the *most common* case (a single-line scalar,
which uses the inline editor) could never reach it; a user leaving blank lines in the buffer
would silently re-parent a following comment with no confirmation possible; and the
"fragment" the clipboard, `serialize_fragment`, copy/paste, and the three backends' splices all
share would have to start carrying trailing whitespace, which would leak blank lines into every
paste. An explicit mutation is uniform across node kinds and formats, independently undoable,
and leaves `serialize_fragment` alone.

**Tech Stack:** Rust (`confy-core` model + session), `web/types.ts` (wire contract),
`i18n/*.json`.

**Spec:** none — this plan is the design record.

## Global Constraints

- `confy-core` stays **filesystem-free** (`tests/no_fs_gate.rs`).
- Every mutation is **atomic**: `apply` edits a copy and commits only on success, returning
  `(SyntaxNode, String)` so the caller commits both without re-serializing (CLAUDE.md
  *Lossless CST*). This mutation follows `SetTrailingComment`'s shape exactly: text splice →
  `reparse_document` → the existing `validate_semantics` backstop.
- An untouched file must still **round-trip byte-identically** — this mutation must be the only
  thing that ever changes blank-line counts.
- `Mutation` is `Serialize`/`Deserialize` and crosses the wasm boundary: a new variant is a
  **wire-contract change** and must be mirrored in `web/types.ts`. Same for the new `Intent`.
- Every user-visible string goes through `tr`/`tr_args` with keys in **both** `i18n/en.json`
  and `i18n/zh-TW.json`.
- A **web/touch/VS Code change reaches the browser only after a wasm rebuild**
  (`cd crates/confy-ffi && wasm-pack build --target web`); `npm run build` only *copies*
  `pkg/`. Any host-visible step here must rebuild the wasm before claiming a visual check.

---

### Task 1: the shared splice + `SetTrailingBlankLines` for TOML

**Files:**
- Create: `crates/confy-core/src/model/blank_lines.rs`
- Modify: `crates/confy-core/src/model/mod.rs` (add `pub(crate) mod blank_lines;`)
- Modify: `crates/confy-core/src/model/document.rs:296-300` (new `Mutation` variant),
  and the `ConfigDocument` trait (new `trailing_blank_lines` method with a default)
- Modify: `crates/confy-core/src/model/cst_edit/mod.rs:128-132` (dispatch arm),
  `crates/confy-core/src/model/cst_edit/replace_delete.rs` (the TOML extent-end helper + splice)
- Modify: `crates/confy-core/src/model/cst_doc.rs` (`trailing_blank_lines` impl)
- Modify: `crates/confy-core/src/model/any_doc.rs` (delegate)
- Test: `crates/confy-core/src/model/blank_lines.rs` inline tests +
  `crates/confy-core/src/model/cst_edit/tests.rs`

**Interfaces:**
- Produces:
  - `blank_lines::count_after(text: &str, end: usize) -> usize`
  - `blank_lines::splice(text: &str, end: usize, n: usize) -> String`
  - `Mutation::SetTrailingBlankLines { path: Path, n: usize }`
  - `ConfigDocument::trailing_blank_lines(&self, path: &[Seg]) -> Option<usize>`
  - `cst_edit::replace_delete::extent_end_offset(tree: &SyntaxNode, path: &[Seg]) -> Result<usize, MutateError>`

- [ ] **Step 1: write the failing tests for the shared splice**

Create `crates/confy-core/src/model/blank_lines.rs` with only the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // `end` is the offset just past a node's extent, i.e. just past its
    // terminating newline — the anchor every backend hands us.
    const SRC: &str = "a = 1\n\n\nb = 2\n";

    #[test]
    fn counts_the_blank_run_after_the_anchor() {
        let end = SRC.find("\n\n").unwrap() + 1; // just past `a = 1\n`
        assert_eq!(count_after(SRC, end), 2);
        assert_eq!(count_after(SRC, SRC.len()), 0, "EOF has no blank run");
        assert_eq!(count_after("a = 1\nb = 2\n", 6), 0);
    }

    #[test]
    fn counts_a_whitespace_only_line_as_blank() {
        assert_eq!(count_after("a = 1\n   \n\t\nb = 2\n", 6), 2);
    }

    #[test]
    fn splice_sets_grows_and_clears_the_run() {
        let end = 6;
        assert_eq!(splice(SRC, end, 2), SRC, "no-op is byte-identical");
        assert_eq!(splice(SRC, end, 0), "a = 1\nb = 2\n");
        assert_eq!(splice(SRC, end, 1), "a = 1\n\nb = 2\n");
        assert_eq!(splice(SRC, end, 4), "a = 1\n\n\n\n\nb = 2\n");
    }

    #[test]
    fn splice_replaces_whitespace_only_lines_with_clean_blanks() {
        assert_eq!(splice("a = 1\n   \nb = 2\n", 6, 1), "a = 1\n\nb = 2\n");
    }

    #[test]
    fn splice_at_eof_terminates_the_last_line_first() {
        // No trailing newline: adding a blank line means terminating `a = 1`
        // and then emitting the blanks.
        assert_eq!(splice("a = 1", 5, 1), "a = 1\n\n");
        assert_eq!(splice("a = 1", 5, 0), "a = 1");
    }
}
```

- [ ] **Step 2: run to verify failure**

Run: `cargo test -p confy-core --lib model::blank_lines`
Expected: compile error — `cannot find function count_after`.

- [ ] **Step 3: implement the shared splice**

Prepend to `crates/confy-core/src/model/blank_lines.rs`:

```rust
//! The one blank-line rule shared by all three backends. A node's trailing
//! blank lines are pure inter-node trivia in every format, so the surgery is
//! format-neutral text work on the serialized document; the only per-backend
//! knowledge is the anchor — the byte offset just past the node's contiguous
//! extent — which each backend derives from the same span logic its `Delete`
//! uses. `Mutation::SetTrailingBlankLines`.

/// How many blank lines follow the anchor `end` (a maximal run of lines that
/// are empty or whitespace-only). `end` must be a line boundary — the offset
/// just past a node's terminating newline.
pub(crate) fn count_after(text: &str, end: usize) -> usize {
    let (n, _) = measure(text, end);
    n
}

/// `(blank line count, byte length of the run)` starting at `end`.
fn measure(text: &str, end: usize) -> (usize, usize) {
    let rest = &text[end.min(text.len())..];
    let mut n = 0usize;
    let mut consumed = 0usize;
    for line in rest.split_inclusive('\n') {
        // A final fragment with no `\n` is the file's last, unterminated line;
        // it is content-or-nothing, never a blank *line*.
        if !line.ends_with('\n') || !line.trim().is_empty() {
            break;
        }
        n += 1;
        consumed += line.len();
    }
    (n, consumed)
}

/// Rewrite the blank run after `end` to exactly `n` clean blank lines.
/// Whitespace-only lines are normalized to empty ones. When `end` is EOF on an
/// unterminated last line, a terminating newline is emitted first so `n`
/// really is a count of *blank lines* and not of line terminators.
pub(crate) fn splice(text: &str, end: usize, n: usize) -> String {
    let end = end.min(text.len());
    let (_, consumed) = measure(text, end);
    let head = &text[..end];
    let tail = &text[end + consumed..];
    let mut out = String::with_capacity(text.len() + n);
    out.push_str(head);
    if n > 0 && !head.is_empty() && !head.ends_with('\n') {
        out.push('\n');
    }
    for _ in 0..n {
        out.push('\n');
    }
    out.push_str(tail);
    out
}
```

Register in `crates/confy-core/src/model/mod.rs` alongside the other `mod` lines:

```rust
pub(crate) mod blank_lines;
```

- [ ] **Step 4: run to verify the splice tests pass**

Run: `cargo test -p confy-core --lib model::blank_lines`
Expected: 5 passed.

- [ ] **Step 5: add the `Mutation` variant and the trait query**

In `crates/confy-core/src/model/document.rs`, append to `enum Mutation` after
`SetTrailingComment` (which ends at line 299):

```rust
    /// Set the number of **blank lines immediately after** the node at `path`
    /// to exactly `n` (0 removes them entirely). Pure inter-node trivia in
    /// every format, so it is a text splice on the serialized document
    /// (`model::blank_lines`) anchored at the byte offset just past the node's
    /// contiguous extent — the same extent the node's `Delete` covers, so a
    /// `[table]`'s blank lines land after its last member, not after its
    /// header. Independent of every other mutation: `Replace`, `nudge`, paste
    /// and the `$EDITOR` round-trip all leave the run alone.
    SetTrailingBlankLines {
        path: Path,
        n: usize,
    },
```

In the same file, add to the `ConfigDocument` trait (next to `comment_prefix`/`kind_options`):

```rust
    /// How many blank lines currently follow the node at `path` — the value
    /// `Mutation::SetTrailingBlankLines` sets, read back for display (the
    /// Detail popup's `Blank after:` line) and for turning a relative
    /// `Intent::SetTrailingBlank(±1)` into an absolute `n`. `None` when `path`
    /// does not resolve or the backend cannot anchor it.
    fn trailing_blank_lines(&self, path: &[Seg]) -> Option<usize> {
        let _ = path;
        None
    }
```

- [ ] **Step 6: write the failing TOML behavior tests**

Append to `crates/confy-core/src/model/cst_edit/tests.rs`:

```rust
#[test]
fn set_trailing_blank_lines_on_a_scalar_entry() {
    let set = |src: &str, n: usize| {
        apply_str(
            src,
            Mutation::SetTrailingBlankLines {
                path: vec![Seg::Key("a".into())],
                n,
            },
        )
    };
    assert_eq!(set("a = 1\nb = 2\n", 1).unwrap(), "a = 1\n\nb = 2\n");
    assert_eq!(set("a = 1\n\n\nb = 2\n", 0).unwrap(), "a = 1\nb = 2\n");
    assert_eq!(set("a = 1\n\n\nb = 2\n", 3).unwrap(), "a = 1\n\n\n\nb = 2\n");
    // A no-op is byte-identical.
    assert_eq!(set("a = 1\n\nb = 2\n", 1).unwrap(), "a = 1\n\nb = 2\n");
}

#[test]
fn set_trailing_blank_lines_anchors_a_table_after_its_last_member() {
    // Not after the `[t]` header: the blank run belongs after the section's
    // last line, exactly where `Delete` on `[t]` would stop.
    let out = apply_str(
        "[t]\nx = 1\ny = 2\n[u]\nz = 3\n",
        Mutation::SetTrailingBlankLines {
            path: vec![Seg::Key("t".into())],
            n: 1,
        },
    )
    .unwrap();
    assert_eq!(out, "[t]\nx = 1\ny = 2\n\n[u]\nz = 3\n");
}

#[test]
fn set_trailing_blank_lines_anchors_a_table_with_a_subtable_after_the_subtable() {
    let out = apply_str(
        "[t]\nx = 1\n[t.sub]\ny = 2\n[u]\nz = 3\n",
        Mutation::SetTrailingBlankLines {
            path: vec![Seg::Key("t".into())],
            n: 1,
        },
    )
    .unwrap();
    assert_eq!(out, "[t]\nx = 1\n[t.sub]\ny = 2\n\n[u]\nz = 3\n");
}

#[test]
fn set_trailing_blank_lines_on_an_aot_entry() {
    let out = apply_str(
        "[[s]]\na = 1\n[[s]]\nb = 2\n",
        Mutation::SetTrailingBlankLines {
            path: vec![Seg::Key("s".into()), Seg::Index(0)],
            n: 1,
        },
    )
    .unwrap();
    assert_eq!(out, "[[s]]\na = 1\n\n[[s]]\nb = 2\n");
}

#[test]
fn set_trailing_blank_lines_on_a_comment_node() {
    let out = apply_str(
        "# note\na = 1\n",
        Mutation::SetTrailingBlankLines {
            path: vec![Seg::Index(0)],
            n: 1,
        },
    )
    .unwrap();
    assert_eq!(out, "# note\n\na = 1\n");
}

#[test]
fn set_trailing_blank_lines_at_eof_terminates_the_file() {
    let out = apply_str(
        "a = 1\n",
        Mutation::SetTrailingBlankLines {
            path: vec![Seg::Key("a".into())],
            n: 2,
        },
    )
    .unwrap();
    assert_eq!(out, "a = 1\n\n\n");
}

#[test]
fn set_trailing_blank_lines_rejects_an_unknown_path() {
    assert!(apply_str(
        "a = 1\n",
        Mutation::SetTrailingBlankLines {
            path: vec![Seg::Key("nope".into())],
            n: 1,
        },
    )
    .is_err());
}
```

Read the top of `crates/confy-core/src/model/cst_edit/tests.rs` first and use its existing
`apply_str` helper signature verbatim (the tests above assume `apply_str(src, mutation) ->
Result<String, _>`); if the real helper differs, adapt the calls, not the helper.

- [ ] **Step 7: run to verify failure**

Run: `cargo test -p confy-core --lib cst_edit::tests::set_trailing_blank`
Expected: compile error — no `SetTrailingBlankLines` arm in the `cst_edit` dispatch match
(the match is exhaustive, so this also proves every backend must be updated).

- [ ] **Step 8: implement the TOML extent anchor + dispatch**

In `crates/confy-core/src/model/cst_edit/replace_delete.rs`, add next to the existing
`section_end_from`/`aot_entry_end_from` helpers:

```rust
/// The byte offset just past the node at `path`'s **contiguous extent** — the
/// anchor `Mutation::SetTrailingBlankLines` splices at. Deliberately the same
/// extent `delete` covers (`section_end_from` for a `[table]`, so nested
/// sub-tables are inside it; `aot_entry_end_from` for one `[[aot]]` entry), so
/// "the blank lines after this node" means the same thing as "the lines a
/// delete of this node would remove".
pub(crate) fn extent_end_offset(
    tree: &SyntaxNode,
    path: &[Seg],
) -> Result<usize, MutateError> {
    let (_proj, idx) = walk(tree, "");
    let target = idx
        .iter()
        .find(|(p, _)| p == path)
        .map(|(_, t)| t.clone())
        .ok_or(MutateError::NotFound)?;
    let els: Vec<_> = tree.children_with_tokens().collect();
    // For the header-anchored targets, the extent is expressed as an index into
    // the root's own child list; convert it to a byte offset (the start of the
    // first element *after* the extent, or EOF).
    let idx_to_offset = |i: usize| -> usize {
        els.get(i)
            .map(|e| usize::from(e.text_range().start()))
            .unwrap_or_else(|| usize::from(tree.text_range().end()))
    };
    let end = match &target {
        Target::Header(header) => idx_to_offset(section_end_from(header, path)),
        Target::AotEntry(header) => {
            idx_to_offset(aot_entry_end_from(header, &header_path(header)))
        }
        Target::AotGroup => {
            let (_, end) = aot_group_span(tree, path).ok_or(MutateError::NotFound)?;
            idx_to_offset(end)
        }
        // A keyed entry, an array element, or a comment: its own span, then
        // forward past its terminating newline.
        Target::Entry(n) | Target::ArrayElement(n) => {
            past_newline(tree, usize::from(n.text_range().end()))
        }
        Target::Comment(t) => past_newline(tree, usize::from(t.text_range().end())),
        _ => return Err(MutateError::Unsupported),
    };
    Ok(end)
}

/// Advance `at` past the rest of its line (through the next `\n`), so the
/// returned offset is a line boundary — `blank_lines`' anchor contract.
fn past_newline(tree: &SyntaxNode, at: usize) -> usize {
    let full = tree.to_string();
    match full[at.min(full.len())..].find('\n') {
        Some(i) => at + i + 1,
        None => full.len(),
    }
}
```

The exact `Target` variant names, and whether `section_end_from`/`aot_entry_end_from` take a
`&[Seg]` or a `&SyntaxNode`, must be read from the file before writing this — `Target` is
defined in `cst_project.rs` and the two span helpers are at
`replace_delete.rs:748` and `:711`. Adapt the match arms to the real variants (`Target::Comment`
in particular may carry a node rather than a token). Reuse `header_path`/`aot_group_span` from
their existing modules (`cst_edit::aot_group`, `cst_project`) via the file's existing `use`
block; do not re-derive a header path by hand.

Then in `crates/confy-core/src/model/cst_edit/mod.rs`, add the dispatch arm after the
`SetTrailingComment` arm (line 129-131):

```rust
        Mutation::SetTrailingBlankLines { path, n } => {
            let end = extent_end_offset(&tree, &path)?;
            let text = tree.to_string();
            reparse_document(&crate::model::blank_lines::splice(&text, end, n))?
        }
```

Import `extent_end_offset` alongside the existing `set_trailing_comment` import.

In `crates/confy-core/src/model/cst_doc.rs`, implement the trait query:

```rust
    fn trailing_blank_lines(&self, path: &[Seg]) -> Option<usize> {
        let end = crate::model::cst_edit::extent_end_offset(self.syntax(), path).ok()?;
        Some(crate::model::blank_lines::count_after(&self.serialize(), end))
    }
```

Use whatever accessor `CstDocument` already exposes for its root `SyntaxNode` (the same one
`apply` uses); if it is a private field, read it directly since this impl is in the same file.
Re-export `extent_end_offset` from `cst_edit/mod.rs` if it is not already `pub(crate)` at that
path.

In `crates/confy-core/src/model/any_doc.rs`, add the delegation beside the other
match-delegated methods, following the file's existing `delegate!` macro pattern:

```rust
    fn trailing_blank_lines(&self, path: &[crate::model::node::Seg]) -> Option<usize> {
        delegate!(self, d => d.trailing_blank_lines(path))
    }
```

- [ ] **Step 9: run to verify the TOML tests pass**

Run: `cargo test -p confy-core --lib cst_edit`
Expected: all pass, including the 7 new ones. Then `cargo test -p confy-core` — the JSON and
YAML `apply` matches will fail to compile until Task 2; if so, add a temporary
`Mutation::SetTrailingBlankLines { .. } => return Err(MutateError::Unsupported),` arm to each
so this task's commit builds, and delete those arms in Task 2.

- [ ] **Step 10: commit**

```bash
git add crates/confy-core/src/model
git commit -m "feat(core): Mutation::SetTrailingBlankLines, shared splice + TOML anchor

Blank lines after a node are pure inter-node trivia in every format, so the
surgery is one format-neutral text splice (model/blank_lines.rs) anchored at
the byte offset just past the node's contiguous extent. TOML derives that
anchor from the same section_end_from/aot_entry_end_from spans its Delete
uses, so a table's blank run sits after its last member (sub-tables included),
not after its header. JSON/YAML reject for now."
```

---

### Task 2: JSON and YAML anchors

**Files:**
- Modify: `crates/confy-core/src/model/json/edit.rs` (dispatch arm + anchor),
  `crates/confy-core/src/model/json/doc.rs` (`trailing_blank_lines`)
- Modify: `crates/confy-core/src/model/yaml/edit/mod.rs` (dispatch + the mutation's path list
  at lines 81-83), `crates/confy-core/src/model/yaml/edit/resolve.rs` (anchor),
  `crates/confy-core/src/model/yaml/doc.rs` (`trailing_blank_lines`)
- Test: `crates/confy-core/src/model/json/edit.rs` inline tests,
  `crates/confy-core/src/model/yaml/edit/tests.rs`

**Interfaces:**
- Consumes: Task 1's `blank_lines::{count_after, splice}` and the
  `Mutation::SetTrailingBlankLines` variant.
- Produces: per-backend `extent_end_offset` equivalents (private to each backend).

- [ ] **Step 1: write the failing tests**

Append to `crates/confy-core/src/model/json/edit.rs`'s test module:

```rust
    #[test]
    fn set_trailing_blank_lines_on_a_member_and_an_object() {
        let set = |src: &str, path: Vec<Seg>, n: usize| {
            apply_str(src, Mutation::SetTrailingBlankLines { path, n })
        };
        assert_eq!(
            set("{\n  \"a\": 1,\n  \"b\": 2\n}\n", vec![Seg::Key("a".into())], 1).unwrap(),
            "{\n  \"a\": 1,\n\n  \"b\": 2\n}\n"
        );
        // Clearing an existing run.
        assert_eq!(
            set("{\n  \"a\": 1,\n\n\n  \"b\": 2\n}\n", vec![Seg::Key("a".into())], 0).unwrap(),
            "{\n  \"a\": 1,\n  \"b\": 2\n}\n"
        );
        // A nested object's run sits after its closing brace.
        assert_eq!(
            set(
                "{\n  \"o\": {\n    \"x\": 1\n  },\n  \"b\": 2\n}\n",
                vec![Seg::Key("o".into())],
                1
            )
            .unwrap(),
            "{\n  \"o\": {\n    \"x\": 1\n  },\n\n  \"b\": 2\n}\n"
        );
    }

    #[test]
    fn set_trailing_blank_lines_keeps_the_separator_comma() {
        // The comma belongs to the member's line, so it must stay *before* the
        // blank run — a comma stranded after a blank line is still legal JSON
        // but is not what the user asked for.
        let out = apply_str(
            "{\n  \"a\": 1,\n  \"b\": 2\n}\n",
            Mutation::SetTrailingBlankLines {
                path: vec![Seg::Key("a".into())],
                n: 1,
            },
        )
        .unwrap();
        assert!(out.contains("\"a\": 1,\n\n"), "{out}");
    }
```

Append to `crates/confy-core/src/model/yaml/edit/tests.rs`:

```rust
#[test]
fn set_trailing_blank_lines_on_a_map_entry() {
    let set = |src: &str, path: Vec<Seg>, n: usize| {
        apply_str(src, Mutation::SetTrailingBlankLines { path, n })
    };
    assert_eq!(
        set("a: 1\nb: 2\n", vec![Seg::Key("a".into())], 1).unwrap(),
        "a: 1\n\nb: 2\n"
    );
    assert_eq!(
        set("a: 1\n\n\nb: 2\n", vec![Seg::Key("a".into())], 0).unwrap(),
        "a: 1\nb: 2\n"
    );
}

#[test]
fn set_trailing_blank_lines_on_a_nested_block_map_spans_its_children() {
    let out = apply_str(
        "o:\n  x: 1\n  y: 2\nb: 3\n",
        Mutation::SetTrailingBlankLines {
            path: vec![Seg::Key("o".into())],
            n: 1,
        },
    )
    .unwrap();
    assert_eq!(out, "o:\n  x: 1\n  y: 2\n\nb: 3\n");
}

#[test]
fn set_trailing_blank_lines_on_a_block_seq_element() {
    let out = apply_str(
        "xs:\n  - a\n  - b\n",
        Mutation::SetTrailingBlankLines {
            path: vec![Seg::Key("xs".into()), Seg::Index(0)],
            n: 1,
        },
    )
    .unwrap();
    assert_eq!(out, "xs:\n  - a\n\n  - b\n");
}

#[test]
fn set_trailing_blank_lines_rejects_an_opaque_node() {
    // Every mutation on or into an opaque span returns Unsupported and leaves
    // the document untouched (CLAUDE.md, YAML subset backend).
    let r = apply_str(
        "base: &b\n  x: 1\nuse: *b\n",
        Mutation::SetTrailingBlankLines {
            path: vec![Seg::Key("use".into())],
            n: 1,
        },
    );
    assert!(r.is_err(), "an opaque value must reject: {r:?}");
}

#[test]
fn set_trailing_blank_lines_rejects_a_flow_member() {
    // A flow member has no line of its own to trail.
    let r = apply_str(
        "m: {a: 1, b: 2}\n",
        Mutation::SetTrailingBlankLines {
            path: vec![Seg::Key("m".into()), Seg::Key("a".into())],
            n: 1,
        },
    );
    assert!(r.is_err(), "a flow member must reject: {r:?}");
}
```

- [ ] **Step 2: run to verify failure**

Run: `cargo test -p confy-core --lib model::json::edit::tests::set_trailing_blank model::yaml`
Expected: FAIL with `MutateError::Unsupported` from the placeholder arms added in Task 1.

- [ ] **Step 3: implement the JSON anchor**

In `crates/confy-core/src/model/json/edit.rs`, replace the placeholder arm with:

```rust
        Mutation::SetTrailingBlankLines { path, n } => {
            let end = extent_end_offset(&tree, &path)?;
            let text = tree.to_string();
            reparse(&crate::model::blank_lines::splice(&text, end, n))?
        }
```

and add the anchor beside `set_trailing_comment` (line 1655), reusing the same
"member end, past an optional comma, to the line's newline" walk that function already
performs — factor that walk into a shared private helper rather than duplicating it:

```rust
/// The byte offset just past the member at `path`'s line — the anchor
/// `Mutation::SetTrailingBlankLines` splices at. A member's separator comma
/// belongs to its own line, so it stays *before* the blank run; anything else
/// on the line (a `//` trailing comment) does too.
fn extent_end_offset(tree: &SyntaxNode, path: &[Seg]) -> Result<usize, MutateError> {
    let member_end = member_line_end(tree, path)?;
    let full = tree.to_string();
    Ok(match full[member_end..].find('\n') {
        Some(i) => member_end + i + 1,
        None => full.len(),
    })
}
```

`member_line_end` is the comma-aware offset `set_trailing_comment` computes at
`json/edit.rs:1655-1700`; extract it as a private helper there and call it from both, so the
comma rule has one implementation. Use the file's own `reparse` function name (read it — it
may be `reparse_document`).

Add to `crates/confy-core/src/model/json/doc.rs`:

```rust
    fn trailing_blank_lines(&self, path: &[Seg]) -> Option<usize> {
        let end = super::edit::extent_end_offset_pub(&self.tree, path).ok()?;
        Some(crate::model::blank_lines::count_after(&self.serialize(), end))
    }
```

adjusting the field/accessor name to the real one and exporting the anchor as `pub(crate)`
(name it consistently — `extent_end_offset` — and drop the `_pub` suffix).

- [ ] **Step 4: implement the YAML anchor**

In `crates/confy-core/src/model/yaml/edit/resolve.rs`, add:

```rust
/// The byte offset just past the node at `path`'s **block extent** — its own
/// line plus every more-indented line beneath it — which is the anchor
/// `Mutation::SetTrailingBlankLines` splices at. Rejects an opaque node (every
/// mutation on or into one is `Unsupported`) and a flow member, which has no
/// line of its own to trail.
pub(crate) fn extent_end_offset(
    tree: &SyntaxNode,
    idx: &YamlIndex,
    path: &[Seg],
) -> Result<usize, MutateError> {
    if is_opaque(idx, path) {
        return Err(MutateError::Unsupported);
    }
    let target = resolve(idx, path)?;
    let node = match &target {
        Target::MapEntry(n) | Target::Element(n) => n.clone(),
        Target::Comment(n) => n.clone(),
        Target::Opaque(_) => return Err(MutateError::Unsupported),
    };
    // A flow member's node lives inside a `{…}`/`[…]` on one line.
    if node
        .ancestors()
        .any(|a| matches!(a.kind(), SyntaxKind::FLOW_MAP | SyntaxKind::FLOW_SEQ))
    {
        return Err(MutateError::Unsupported);
    }
    let full = tree.to_string();
    let at = usize::from(node.text_range().end());
    Ok(match full[at.min(full.len())..].find('\n') {
        Some(i) => at + i + 1,
        None => full.len(),
    })
}
```

The `Target` variants, `resolve`, `is_opaque` and `YamlIndex` all already exist in this module
(CLAUDE.md, *YAML subset backend*) — read their real signatures and match them. A block map
entry's `MAP_ENTRY` node already spans its nested children, so `text_range().end()` is the
extent end; verify that with the
`set_trailing_blank_lines_on_a_nested_block_map_spans_its_children` test rather than assuming.

Then in `crates/confy-core/src/model/yaml/edit/mod.rs`: replace the placeholder arm with the
splice (mirroring TOML's), and add the variant to the affected-paths list at lines 81-83:

```rust
        Mutation::SetTrailingBlankLines { path, .. } => vec![path],
```

Add the `trailing_blank_lines` impl to `crates/confy-core/src/model/yaml/doc.rs`, mirroring
JSON's.

- [ ] **Step 5: run to verify all three backends pass**

Run: `cargo test -p confy-core`
Expected: all green, with the JSON and YAML placeholder arms now gone
(`grep -rn "SetTrailingBlankLines" crates/confy-core/src/model` must show no
`=> return Err(MutateError::Unsupported)` placeholder left).

- [ ] **Step 6: commit**

```bash
git add crates/confy-core/src/model
git commit -m "feat(core): SetTrailingBlankLines anchors for JSON and YAML

JSON reuses set_trailing_comment's comma-aware member-line-end walk (now one
shared helper, so the comma rule has a single implementation); YAML uses the
MAP_ENTRY/element block extent and rejects opaque nodes and flow members,
which have no line of their own to trail."
```

---

### Task 3: the Session surface — Action-menu items, Intent, Detail line

**Files:**
- Modify: `crates/confy-core/src/session/intent.rs` (new variant),
  `crates/confy-core/src/session/dispatch.rs` (routing),
  `crates/confy-core/src/session/action_menu.rs` (two items),
  `crates/confy-core/src/session/session.rs` (the method + the Detail info)
- Modify: `web/types.ts` (mirror `Intent` and `Mutation`)
- Modify: `i18n/en.json`, `i18n/zh-TW.json`
- Test: `crates/confy-core/tests/session_headless.rs`

**Interfaces:**
- Consumes: `ConfigDocument::trailing_blank_lines`, `Mutation::SetTrailingBlankLines`.
- Produces:
  - `Intent::SetTrailingBlank(i32)` — a **relative** step, clamped at 0
  - `Session::set_trailing_blank(&mut self, delta: i32)`
  - `Session::trailing_blank_lines(&self) -> Option<usize>` for the cursor node (Detail popup)

- [ ] **Step 1: i18n keys**

`i18n/en.json`:
```json
  "core.action.blank-add": "Add a blank line after",
  "core.action.blank-remove": "Remove a blank line after",
  "core.blank.set": "blank lines after `{0}`: {1}",
  "core.blank.unsupported": "this node cannot carry trailing blank lines",
  "tui.detail.blank-after": "Blank after",
```
`i18n/zh-TW.json`:
```json
  "core.action.blank-add": "在後方增加一個空行",
  "core.action.blank-remove": "移除後方一個空行",
  "core.blank.set": "`{0}` 後方空行數:{1}",
  "core.blank.unsupported": "此節點無法設定後方空行",
  "tui.detail.blank-after": "後方空行",
```

- [ ] **Step 2: write the failing tests**

Append to `crates/confy-core/tests/session_headless.rs`:

```rust
/// `Intent::SetTrailingBlank` is a *relative* step clamped at zero, so one
/// keypress/menu item both grows and removes the run.
#[test]
fn set_trailing_blank_steps_and_clamps_at_zero() {
    let mut s = toml_session("a = 1\nb = 2\n");
    s.dispatch(Intent::CursorDown);
    s.dispatch(Intent::SetTrailingBlank(1));
    assert_eq!(s.serialize().unwrap(), "a = 1\n\nb = 2\n");
    s.dispatch(Intent::SetTrailingBlank(1));
    assert_eq!(s.serialize().unwrap(), "a = 1\n\n\nb = 2\n");
    s.dispatch(Intent::SetTrailingBlank(-1));
    s.dispatch(Intent::SetTrailingBlank(-1));
    assert_eq!(s.serialize().unwrap(), "a = 1\nb = 2\n");
    // Already zero: a further decrement is a no-op, not an error.
    s.dispatch(Intent::SetTrailingBlank(-1));
    assert_eq!(s.serialize().unwrap(), "a = 1\nb = 2\n");
}

#[test]
fn set_trailing_blank_is_undoable_on_its_own() {
    let mut s = toml_session("a = 1\nb = 2\n");
    s.dispatch(Intent::CursorDown);
    s.dispatch(Intent::SetTrailingBlank(1));
    s.dispatch(Intent::Undo);
    assert_eq!(s.serialize().unwrap(), "a = 1\nb = 2\n");
    s.dispatch(Intent::Redo);
    assert_eq!(s.serialize().unwrap(), "a = 1\n\nb = 2\n");
}

/// It never disturbs the node's value or trailing comment.
#[test]
fn set_trailing_blank_preserves_the_value_and_trailing_comment() {
    let mut s = toml_session("a = 1  # keep\nb = 2\n");
    s.dispatch(Intent::CursorDown);
    s.dispatch(Intent::SetTrailingBlank(1));
    assert_eq!(s.serialize().unwrap(), "a = 1  # keep\n\nb = 2\n");
}

/// The Action menu carries both directions on every host (ADR 0009).
#[test]
fn action_menu_offers_both_blank_line_items() {
    let mut s = toml_session("a = 1\nb = 2\n");
    s.dispatch(Intent::CursorDown);
    s.dispatch(Intent::OpenActionMenu);
    let ModeView::ActionMenu { items, .. } = s.mode_view() else {
        panic!("expected the action menu, got {:?}", s.mode_view());
    };
    let joined = items.iter().map(|i| i.label.clone()).collect::<Vec<_>>().join(" | ");
    assert!(joined.contains("Add a blank line after"), "{joined}");
    assert!(joined.contains("Remove a blank line after"), "{joined}");
}

/// A YAML flow member cannot carry a trailing blank line: the intent reports
/// and leaves the document untouched.
#[test]
fn set_trailing_blank_on_a_flow_member_reports_and_changes_nothing() {
    let mut s = yaml_session("m: {a: 1, b: 2}\n");
    s.dispatch(Intent::CursorDown);
    s.dispatch(Intent::ExpandNode);
    s.dispatch(Intent::CursorDown);
    let before = s.serialize().unwrap();
    s.dispatch(Intent::SetTrailingBlank(1));
    assert_eq!(s.serialize().unwrap(), before);
    assert!(s.notice_text().is_some(), "must report why nothing happened");
}
```

Confirm the real names of `Intent::OpenActionMenu`/`ExpandNode`, `ModeView::ActionMenu`'s field
shape, and the notice accessor (`notice_text` may be named differently — grep
`session_headless.rs` for the existing convention) and match them.

- [ ] **Step 3: run to verify failure**

Run: `cargo test -p confy-core --test session_headless trailing_blank`
Expected: compile error — no `Intent::SetTrailingBlank`.

- [ ] **Step 4: implement**

`crates/confy-core/src/session/intent.rs`, in the `// ---- Mutations ----` group beside
`Nudge(i64)`:

```rust
    /// Step the number of blank lines **after** the cursor node by `delta`,
    /// clamped at 0 (Action-menu items; relative so one item both grows and
    /// removes the run).
    SetTrailingBlank(i32),
```

`crates/confy-core/src/session/dispatch.rs`, beside `Intent::Nudge(d) => self.nudge(d),`:

```rust
            Intent::SetTrailingBlank(d) => self.set_trailing_blank(d),
```

`crates/confy-core/src/session/session.rs`, as a new method on `impl Session`:

```rust
    /// Step the cursor node's trailing blank-line count by `delta`, clamped at
    /// 0. Relative rather than absolute so a single Action-menu item covers
    /// both directions and reaching 0 removes the run entirely.
    pub fn set_trailing_blank(&mut self, delta: i32) {
        if self.guard_clipboard_locked() {
            return;
        }
        let Some(path) = self.cursor_row().map(|r| r.path.clone()) else {
            return;
        };
        let Some(doc) = &self.doc else { return };
        let Some(current) = doc.trailing_blank_lines(&path) else {
            self.set_notice(Notice::core(self.lang, "core.blank.unsupported", &[]));
            return;
        };
        let n = (current as i64 + delta as i64).max(0) as usize;
        if n == current {
            return;
        }
        let Some(doc) = self.doc.as_mut() else { return };
        match doc.apply(Mutation::SetTrailingBlankLines { path: path.clone(), n }) {
            Ok(text) => {
                self.on_mutation_success(None, text);
                let label = self.human_path(&path);
                self.set_notice(Notice::core(
                    self.lang,
                    "core.blank.set",
                    &[&label, &n.to_string()],
                ));
            }
            Err(e) => self.set_notice(Notice::from(e)),
        }
    }

    /// The cursor node's current trailing blank-line count, for the Detail
    /// popup's `Blank after:` line. `None` when the node cannot carry one.
    pub fn trailing_blank_lines(&self) -> Option<usize> {
        let path = self.cursor_row().map(|r| r.path.clone())?;
        self.doc.as_ref()?.trailing_blank_lines(&path)
    }
```

Match the real helper names in `session.rs` for the cursor row (`cursor_row()`), the notice
constructors (`Notice::core`, and whatever converts a `MutateError` — grep an existing
`doc.apply` call site such as `kind_switch_commit` at `session.rs:1164` and copy its
error-handling shape verbatim), and `on_mutation_success`'s real signature.

`crates/confy-core/src/session/action_menu.rs`: add the two items to the item list, using the
file's existing item struct and gating convention (an item that cannot apply to the current
node should be filtered out the same way the existing items are; if the file has no such
gating, append them unconditionally and let `set_trailing_blank`'s `core.blank.unsupported`
notice handle the rejection):

```rust
        out.push(item("core.action.blank-add", Intent::SetTrailingBlank(1)));
        out.push(item("core.action.blank-remove", Intent::SetTrailingBlank(-1)));
```

`web/types.ts`: add the mirrored variants to the hand-written `Intent` and `Mutation` unions,
following the file's existing serde-tag style for a newtype variant (`Nudge` is the model to
copy for `SetTrailingBlank`; `SetTrailingComment` for `SetTrailingBlankLines`).

- [ ] **Step 5: run to verify the tests pass**

```bash
cargo test -p confy-core
cargo clippy --all-targets 2>&1 | grep -E "^(error|warning)"
cd web && npm run typecheck
```
Expected: all green.

- [ ] **Step 6: add the Detail popup line**

`crates/confy-tui/src/tui/overlay_detail.rs`: after the existing `Children:` row, add a
`Blank after: N` row driven by `app.session.trailing_blank_lines()`, omitted entirely when it
returns `None` (label from `tui.detail.blank-after`). Follow the file's existing row-building
helper rather than pushing a raw `Line`.

`web/panel.ts`: the shared panel's field order is **locked**
(Key/Value/Trailing comment/Kind/Path/Children/Sign — CLAUDE.md). Add `Blank after` as a
read-only field immediately after `Children`, and update that locked-order note in both
`CLAUDE.md` and `docs/reference/WEBUI.md` in the same commit so the list and the code cannot
drift.

- [ ] **Step 7: verify**

```bash
cargo test && cargo fmt --check && cargo clippy --all-targets 2>&1 | grep -E "^(error|warning)"
cd web && npm run typecheck && npm test
```

- [ ] **Step 8: commit**

```bash
git add crates web i18n CLAUDE.md docs/reference/WEBUI.md
git commit -m "feat(session): step a node's trailing blank lines from the Action menu

Intent::SetTrailingBlank(delta) is relative and clamped at 0, so one pair of
core-owned Action-menu items (ADR 0009 — every host renders them for free)
grows, shrinks, and fully removes the run, each step independently undoable.
The current count shows as 'Blank after' in the TUI Detail popup and the shared
web panel."
```

---

### Task 4: confirm before a blank-line change re-parents a following comment

**Files:**
- Modify: `crates/confy-core/src/session/state.rs:203-216` (new `PromptKind` variant),
  `crates/confy-core/src/session/session.rs` (`prompt_view`/`prompt_question`/
  `handle_prompt_key` arms + `set_trailing_blank`'s guard)
- Modify: `web/types.ts` (`PromptView`)
- Modify: `i18n/en.json`, `i18n/zh-TW.json`
- Test: `crates/confy-core/tests/session_headless.rs`

**Rationale:** `docs/reference/CONTEXT.md:136-142` — in TOML, a comment sitting between a
table's last entry and the next `[header]` belongs to the **preceding** scope when separated by
a blank line, and to the following header when hugging it. So changing the blank run across the
0↔1 boundary silently moves that comment between scopes. JSON and YAML have explicit
delimiters/indentation and need no such rule, so the guard is TOML-only.

- [ ] **Step 1: i18n keys**

`i18n/en.json`:
```json
  "core.prompt.blank-reparent": "this moves the comment below into another scope — continue?",
```
`i18n/zh-TW.json`:
```json
  "core.prompt.blank-reparent": "這會讓下方的註解改變所屬範圍,要繼續嗎?",
```

- [ ] **Step 2: write the failing tests**

```rust
/// TOML only: crossing the 0<->1 blank boundary when a comment follows would
/// re-parent that comment (CONTEXT.md blank-line rule), so it confirms first.
#[test]
fn blank_line_change_confirms_when_it_would_reparent_a_comment() {
    let mut s = toml_session("[t]\nx = 1\n# note\n[u]\ny = 2\n");
    s.dispatch(Intent::CursorDown);
    s.dispatch(Intent::ExpandNode);
    s.dispatch(Intent::CursorDown); // on `x`
    s.dispatch(Intent::SetTrailingBlank(1));
    assert!(
        matches!(s.mode_view(), ModeView::Prompt { .. }),
        "expected the re-parent confirm, got {:?}",
        s.mode_view()
    );
    // Declining leaves the document byte-identical.
    s.dispatch(Intent::PromptKey('n'));
    assert_eq!(s.serialize().unwrap(), "[t]\nx = 1\n# note\n[u]\ny = 2\n");
    // Accepting applies it.
    s.dispatch(Intent::SetTrailingBlank(1));
    s.dispatch(Intent::PromptKey('y'));
    assert_eq!(s.serialize().unwrap(), "[t]\nx = 1\n\n# note\n[u]\ny = 2\n");
}

/// No comment follows -> no prompt, and growing an already-nonzero run does
/// not cross the boundary, so it does not prompt either.
#[test]
fn blank_line_change_does_not_confirm_without_a_following_comment() {
    let mut s = toml_session("a = 1\nb = 2\n");
    s.dispatch(Intent::CursorDown);
    s.dispatch(Intent::SetTrailingBlank(1));
    assert_eq!(s.serialize().unwrap(), "a = 1\n\nb = 2\n");
    s.dispatch(Intent::SetTrailingBlank(1));
    assert_eq!(s.serialize().unwrap(), "a = 1\n\n\nb = 2\n");
    assert!(matches!(s.mode_view(), ModeView::Normal));
}

/// JSON and YAML have explicit delimiters/indentation: the rule does not apply
/// and must not prompt.
#[test]
fn blank_line_change_never_confirms_in_yaml() {
    let mut s = yaml_session("a: 1\n# note\nb: 2\n");
    s.dispatch(Intent::CursorDown);
    s.dispatch(Intent::SetTrailingBlank(1));
    assert_eq!(s.serialize().unwrap(), "a: 1\n\n# note\nb: 2\n");
    assert!(matches!(s.mode_view(), ModeView::Normal));
}
```

- [ ] **Step 3: run to verify failure**

Run: `cargo test -p confy-core --test session_headless blank_line_change`
Expected: FAIL — `blank_line_change_confirms_when_it_would_reparent_a_comment` finds
`ModeView::Normal` (the change applies with no prompt).

- [ ] **Step 4: implement**

`state.rs`, add to `enum PromptKind`:

```rust
    /// A `SetTrailingBlankLines` that crosses the 0<->1 boundary while a
    /// comment follows: in TOML that comment changes scope
    /// (CONTEXT.md blank-line rule), so it is confirmed first.
    BlankReparent {
        path: Path,
        n: usize,
    },
```

In `session.rs`, split `set_trailing_blank` so the computed `n` first goes through a guard, and
add the actual application as a private `apply_trailing_blank(&mut self, path: Path, n: usize)`
that both the guard's accept-branch and the no-prompt path call:

```rust
    /// True when setting `path`'s trailing blank run to `n` would move a
    /// following comment between scopes: TOML only, and only when the change
    /// crosses the 0<->1 boundary (that is the boundary the ownership rule
    /// turns on — 1 blank and 3 blanks parent the comment identically).
    fn blank_change_reparents_comment(&self, path: &[Seg], current: usize, n: usize) -> bool {
        let Some(doc) = &self.doc else { return false };
        if doc.format() != DocFormat::Toml {
            return false;
        }
        if (current == 0) == (n == 0) {
            return false;
        }
        let text = doc.serialize();
        let Some(end) = doc.trailing_blank_lines(path).and_then(|_| self.blank_anchor(path))
        else {
            return false;
        };
        // The first non-blank line after the run.
        text[end..]
            .lines()
            .find(|l| !l.trim().is_empty())
            .is_some_and(|l| l.trim_start().starts_with('#'))
    }
```

`blank_anchor` needs the same offset the mutation uses. Rather than re-deriving it in the
session layer, add one more small trait method beside `trailing_blank_lines` in Task 1's
`ConfigDocument` — `fn trailing_blank_anchor(&self, path: &[Seg]) -> Option<usize>` returning
the backend's `extent_end_offset` — and have `trailing_blank_lines` be defined in terms of it.
That keeps a single source for the anchor and removes `blank_anchor` from the session entirely;
adjust Task 1 and Task 2's impls accordingly when you get here (each becomes a one-line
`Some(extent_end_offset(...).ok()?)`).

Then wire `PromptKind::BlankReparent` into the three prompt functions the way
`PromptKind::TypeChange` is wired (`prompt_view`, `prompt_question` — key
`core.prompt.blank-reparent` — and `handle_prompt_key`, whose `'y'` arm calls
`apply_trailing_blank(path, n)`), and add the mirrored `PromptView` variant to `web/types.ts`.
The y/n key legend is host-side and generic, so no host change is needed.

- [ ] **Step 5: run to verify the tests pass**

```bash
cargo test -p confy-core
cd web && npm run typecheck
```

- [ ] **Step 6: commit**

```bash
git add crates web i18n
git commit -m "feat(session): confirm a blank-line change that would re-parent a comment

TOML's comment-ownership rule turns on the 0<->1 blank boundary (CONTEXT.md),
so crossing it while a comment follows silently moves that comment between
scopes. PromptKind::BlankReparent gates exactly that case; JSON and YAML have
explicit delimiters and never prompt. The anchor is now a single trait method
(trailing_blank_anchor) that both the count query and the guard read."
```

---

### Task 5: docs, KEYMAP/help, and the real-binary check

**Files:**
- Modify: `CLAUDE.md` (the *`Mutation` enum* list, the `ConfigDocument` trait facet list,
  the `blank_lines.rs` module-map entry, the locked panel field order)
- Modify: `docs/reference/CONTEXT.md` (*Mutation mechanics* — a `SetTrailingBlankLines`
  paragraph; and a cross-reference from the blank-line rule at lines 136-142)
- Modify: `docs/reference/TUI.md` (Detail popup's new row; the Action menu's two new items),
  `docs/reference/WEBUI.md` (panel field), `docs/reference/MESSAGES.md` (the three new
  notices/prompt), `docs/reference/KEYMAP.md` if it enumerates Action-menu items
- Modify: `CHANGELOG.md`

- [ ] **Step 1: write the docs**

`CONTEXT.md` *Mutation mechanics* must state: the anchor is the node's **contiguous extent
end** (the same extent `Delete` covers, so a `[table]`'s run sits after its last member and a
sub-table is inside it); whitespace-only lines are normalized to empty; `n = 0` removes the run;
at EOF a terminating newline is emitted first; YAML rejects opaque nodes and flow members; JSON
keeps the separator comma before the run.

- [ ] **Step 2: CHANGELOG**

New `### Unreleased Update - <date>` block, **Added**: the mutation, the two Action-menu items,
the Detail/panel readout, and the TOML re-parent confirmation — plus one line recording that
the `$EDITOR`-buffer alternative was rejected and why (so the decision travels with the code).

- [ ] **Step 3: full verification**

```bash
cargo fmt && cargo fmt --check
cargo clippy --all-targets 2>&1 | grep -E "^(error|warning)"
cargo test
cd crates/confy-ffi && wasm-pack build --target web && node functional_smoke.mjs
cd ../../web && npm run typecheck && npm test && npm run build
```
The `wasm-pack` step is **not optional**: `web/build.mjs` only copies `pkg/`, so without it the
web host would still be running the pre-mutation core and any browser check would be
meaningless.

- [ ] **Step 4: real-binary check (mandatory)**

```bash
printf '[t]\nx = 1\n# note\n[u]\ny = 2\n' > docs/tmp/claude-scratch/blank.toml
cargo build -p confy-tui
```
Drive the real TUI via `hub` (`op:"start"`, `--lang en`) and confirm from the rendered frames:
1. cursor on `x`, open the Action menu → both blank-line items are listed.
2. "Add a blank line after" → the re-parent confirm appears; `n` → file unchanged; repeat and
   `y` → the file gains one blank line before `# note`.
3. `i` (Detail) → `Blank after: 1`.
4. "Remove a blank line after" → confirm again (crossing 1→0), `y` → back to the original.
   Verify byte-identity: `git diff --stat docs/tmp/claude-scratch/blank.toml` after a save is
   empty (the fixture is gitignored under `docs/tmp/`, so compare against a copy taken before
   the edits instead: `cmp blank.toml blank.toml.orig`).
5. `z` (undo) after step 2 restores the file too — the mutation is on the history stack.
6. Repeat 1–3 on a `.yaml` and a `.json` fixture: no prompt in either, and a YAML flow member
   (`m: {a: 1}` → `a`) reports `this node cannot carry trailing blank lines` with the document
   unchanged.

- [ ] **Step 5: commit**

```bash
git add docs CHANGELOG.md CLAUDE.md
git commit -m "docs: trailing blank lines — CONTEXT mechanics, TUI/WEBUI/MESSAGES, changelog"
```

---

## Self-review notes

- **Coverage of the original ask.** "把後方 trailing blank lines 拉進編輯器一併修改,可修改數量甚至完全移除":
  quantity is `SetTrailingBlank(±1)` repeated (Task 3), complete removal is reaching `n = 0`
  (asserted in `set_trailing_blank_steps_and_clamps_at_zero`), and it works on **any** node
  kind in **all three** formats (Tasks 1–2) rather than only the container/multiline nodes
  `$EDITOR` opens for. The rejected `$EDITOR` route and its three failure modes are recorded in
  the Architecture section and in the CHANGELOG (Task 5 Step 2).
- **Why no ADR.** Unlike the datetime plan, nothing here reverses or narrows an existing
  documented invariant — it adds a `Mutation` variant in the ordinary way. The design record
  lives in this plan plus CONTEXT.md's *Mutation mechanics*.
- **Type consistency.** `Intent::SetTrailingBlank(i32)` (relative, session-level) vs.
  `Mutation::SetTrailingBlankLines { n: usize }` (absolute, document-level) are deliberately
  different names and types; the conversion happens in exactly one place,
  `Session::set_trailing_blank`.
- **Anchor is defined once.** Task 4 collapses the count query and the re-parent guard onto a
  single `ConfigDocument::trailing_blank_anchor`; if Tasks 1–2 are implemented first, expect to
  refactor those two impls when Task 4 lands (called out in Task 4 Step 4).
- **Deliberate non-goal.** No absolute numeric entry field ("set to 4") and no blank-line
  handling anywhere in `serialize_fragment`/clipboard/paste. Blank runs stay a property of the
  document at a position, never of a copied fragment.
