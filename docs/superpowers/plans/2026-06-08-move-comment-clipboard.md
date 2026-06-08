# Move-aware cut, exact-position reorder, and comment clipboard — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** (1) Make a failed paste never leak/lose the clipboard; (2) make cut+paste within the same scope reposition the node (no false "Key already exists") and land it at the cursor position; (3) let Comment nodes copy/cut/paste like any other node.

**Architecture:** Route the **cut** path of `do_paste` through the existing-but-unused atomic `Mutation::Move` (snapshot+rollback, deletes-before-reinsert) — this fixes #1 and #2's collision in one move. Add exact-position table insertion via the proven `rename_in_table` rebuild technique. Add comment clipboard support via a serialize fix plus a new `Mutation::InsertComment` that writes into TOML decor (reusing the `comment_out` decor-placement pattern). Comments never collide.

**Tech Stack:** Rust, toml_edit, ratatui — `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check`

---

## Background (verified in code)

- `Mutation::Move { sources, target, on_collision }` exists (`document.rs`) and is implemented (`toml_doc.rs` `r#move`/`move_inner`/`move_inner_array`) with a **snapshot + atomic rollback** wrapper, **captures items before deleting**, then re-inserts — but is **never called by the TUI** (dead code). Its table re-insert currently **appends** (ignores `target.index`); its array path honors index via `arr.insert(idx, v)`.
- `do_paste` (`src/tui/app.rs`) currently: inserts each fragment at `target`, then (if cut) deletes sources in a loop. The cut-delete error arm returns **without restoring the clipboard** (#1). Pasting a cut node into its own parent collides because the source still exists at insert time (#2).
- Table key reorder has no toml_edit positional API; the canonical technique is `rename_in_table`'s rebuild: snapshot key order, `remove_entry` each, `insert_formatted` back — re-inserting the target at the desired slot (`toml_doc.rs:51-74`).
- Comment nodes have synthetic `#comment:N` keys. `serialize_node_fragment` returns `""` for them (so copy/cut capture nothing). `insert_fragment` of `"# text\n"` parses to an empty table → comment lost. Delete of a comment already works via `remove_at` → `remove_comment_from_decor`. `comment_out` shows how to write a comment block into decor (empty table → `doc.trailing()`; else prepend to the next key's `leaf_decor` prefix or the table decor).

---

## File Map

| File | Changes |
|---|---|
| `src/model/document.rs` | add `Mutation::InsertComment { target, text }` variant |
| `src/model/toml_doc.rs` | positional re-insert in `move_inner`; `apply` arm + impl for `InsertComment`; a reusable `insert_comment_in_decor` helper factored from `comment_out` |
| `src/tui/app.rs` | `do_paste` rewrite: cut→Move for nodes, comment entries via InsertComment; `serialize_node_fragment_opts` comment branch; `handle_prompt_key` retry uses Move for cut |

---

## Task 1 — Cut paste goes through atomic `Mutation::Move` (fixes #1 + #2 collision)

**Files:** `src/tui/app.rs` (`do_paste`, `handle_prompt_key` retry)

### Context
Switching the cut branch to a single `Mutation::Move` makes it atomic (no partial-failure clipboard leak — #1) and delete-before-reinsert (same-scope paste no longer collides — #2). Copy stays the insert-loop. **Comment sources are excluded here** and handled in Task 4 (Move can't address `#comment:N` keys).

For this task, partition the clipboard's `(fragment, source)` pairs into **node** vs **comment** by the source path's last segment (`Seg::Key(k)` where `k.starts_with("#comment:")` → comment). In Task 1, assume **no comment entries** (process only nodes; leave a `// comments handled in Task 4` marker). Task 4 completes the partition.

- [ ] **Step 1: Write failing test — same-scope cut/paste repositions without collision**

Add to `#[cfg(test)] mod tests` in `src/tui/app.rs` (use `app_with` which builds a real doc):

```rust
#[test]
fn cut_paste_same_scope_moves_without_collision() {
    // Two top-level keys; cut `a`, move cursor onto `b`, paste — should move
    // `a` after `b` with NO collision prompt and NO "already exists" error.
    let mut app = app_with("a = 1\nb = 2\n");
    app.rebuild_rows();
    // cursor on `a` (row 1: row 0 is the root/file node)
    app.cursor = 1;
    app.cut_selected();
    assert!(app.clipboard.is_some());
    // move cursor onto `b`
    app.cursor = 2;
    app.paste();
    // No collision prompt; document still has exactly one `a` and one `b`.
    assert!(matches!(app.mode, Mode::Normal), "no collision prompt expected");
    let out = app.doc.as_ref().unwrap().serialize();
    assert_eq!(out.matches("a =").count(), 1, "exactly one `a`: {out:?}");
    assert_eq!(out.matches("b =").count(), 1, "exactly one `b`: {out:?}");
    assert!(app.clipboard.is_none(), "clipboard consumed on successful move");
}
```

- [ ] **Step 2: Run it — confirm it fails** (current code raises a collision prompt)

```bash
cd /Volumes/Home/Users/wen/repos/confy
cargo test cut_paste_same_scope_moves_without_collision 2>&1
```

- [ ] **Step 3: Rewrite `do_paste`**

Replace the body of `do_paste` so the **cut** path issues one `Mutation::Move` over the node sources, and the **copy** path keeps the existing per-fragment insert loop. Keep the existing collision-prompt + clipboard-restore behavior. Sketch:

```rust
pub(crate) fn do_paste(
    &mut self,
    clipboard: Clipboard,
    target: Target,
    on_collision: OnCollision,
) {
    let Clipboard { fragments, cut: is_cut, sources } = clipboard;

    // Partition into node vs comment entries by the source path.
    // (Comment handling is added in Task 4; assume none here.)
    let is_comment = |p: &Path| matches!(p.last(), Some(Seg::Key(k)) if k.starts_with("#comment:"));
    // For Task 1, treat all as nodes; Task 4 splits these.
    let node_sources: Vec<Path> = sources.iter().filter(|p| !is_comment(p)).cloned().collect();
    let _ = &is_comment; // silence until Task 4

    if self.doc.is_none() {
        self.clipboard = Some(Clipboard { fragments, cut: is_cut, sources });
        return;
    }

    if is_cut {
        // Atomic move: delete-before-reinsert, so same-parent never collides and
        // any failure rolls the document back (no partial state, no lost clipboard).
        let doc = self.doc.as_mut().unwrap();
        match doc.apply(Mutation::Move {
            sources: node_sources.clone(),
            target: target.clone(),
            on_collision,
        }) {
            Ok(()) => { self.on_mutation_success(); }
            Err(crate::model::document::MutateError::Collision(key)) => {
                self.clipboard = Some(Clipboard { fragments, cut: is_cut, sources });
                self.status = Some(format!("collision on key '{key}' — o/r/c"));
                self.mode = Mode::Prompt(PromptKind::Collision { key });
            }
            Err(e) => {
                self.clipboard = Some(Clipboard { fragments, cut: is_cut, sources });
                self.status = Some(format!("paste error: {e}"));
            }
        }
        return;
    }

    // COPY: existing per-fragment insert loop (restore remaining on failure).
    let doc = self.doc.as_mut().unwrap();
    for (i, frag) in fragments.iter().enumerate() {
        match doc.apply(Mutation::Insert { target: target.clone(), toml: frag.clone(), on_collision }) {
            Ok(()) => {}
            Err(crate::model::document::MutateError::Collision(key)) => {
                self.clipboard = Some(Clipboard { fragments: fragments[i..].to_vec(), cut: is_cut, sources });
                self.status = Some(format!("collision on key '{key}' — o/r/c"));
                self.mode = Mode::Prompt(PromptKind::Collision { key });
                return;
            }
            Err(e) => {
                self.clipboard = Some(Clipboard { fragments: fragments[i..].to_vec(), cut: is_cut, sources });
                self.status = Some(format!("paste error: {e}"));
                return;
            }
        }
    }
    self.on_mutation_success();
}
```

> Note: the cut-delete loop is **removed** — Move performs the deletion atomically. This eliminates the #1 partial-failure gap entirely.

- [ ] **Step 4: Update the `handle_prompt_key` collision retry**

In the `PromptKind::Collision` arm, the retry currently rebuilds a `Clipboard` and calls `do_paste`. Leave it calling `do_paste(Clipboard { .. }, target, oc)` — `do_paste` now internally chooses Move (cut) vs insert (copy), so the retry works for both. Verify it compiles and the existing `multi_fragment_paste_collision_stores_only_remaining_fragments` test (copy path) still passes.

- [ ] **Step 5: Run the new test + collision test**

```bash
cargo test cut_paste_same_scope_moves_without_collision 2>&1
cargo test multi_fragment_paste_collision_stores_only_remaining_fragments 2>&1
cargo test cut_then_paste_moves_node 2>&1
```

- [ ] **Step 6: Full suite + lint**

```bash
cargo test 2>&1 | tail -5
cargo clippy -- -D warnings 2>&1
cargo fmt --check 2>&1
```

- [ ] **Step 7: Commit**

```bash
git add src/tui/app.rs
git commit -m "fix: cut+paste uses atomic Mutation::Move — same-scope no longer collides, clipboard never lost on failure

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2 — Exact-position reorder for table moves

**Files:** `src/model/toml_doc.rs` (`move_inner` table branch + a positional-insert helper)

### Context
After Task 1, a cut node moved within a table lands at the **end** (Move appends). The user wants it at the cursor position. `move_inner_array` already honors `target.index`; only the **table** branch needs positional insert, via the `rename_in_table` rebuild technique.

`target.index` is a **projected** child index (the projection lists comment nodes as children too). Convert it to a **real-key index** by counting non-comment entries among the parent's projected children before `target.index`. A helper on `TomlDocument` can do this against `self.project()`.

- [ ] **Step 1: Write a failing test (model level)**

Add to `#[cfg(test)] mod tests` in `src/model/toml_doc.rs`:

```rust
#[test]
fn move_into_table_honors_target_index() {
    // dest has x, y, z; move `a` (top-level) to index 1 (between x and y).
    let mut doc = doc_from_str("a = 1\n[dest]\nx = 1\ny = 2\nz = 3\n");
    doc.apply(Mutation::Move {
        sources: vec![vec![Seg::Key("a".into())]],
        target: Target { parent: vec![Seg::Key("dest".into())], index: 1 },
        on_collision: OnCollision::Cancel,
    }).unwrap();
    let out = doc.serialize();
    // `a` should sit between x and y inside [dest].
    let xi = out.find("x = 1").unwrap();
    let ai = out.find("a = 1").unwrap();
    let yi = out.find("y = 2").unwrap();
    assert!(xi < ai && ai < yi, "expected x < a < y, got:\n{out}");
}
```

(Use the same `doc_from_str`/`Target`/`Seg` helpers the existing Move tests use.)

Also add a **same-parent shift** test (the case the naive index approach gets wrong):

```rust
#[test]
fn move_within_table_after_anchor_handles_shift() {
    // [a, b, c]; move `a` to just-after `b` (cursor on b → target.index = 2).
    // Because `a` is deleted before re-insert, a naive index would land `a`
    // after `c`. Anchor-based insert must place it between b and c.
    let mut doc = doc_from_str("a = 1\nb = 2\nc = 3\n");
    doc.apply(Mutation::Move {
        sources: vec![vec![Seg::Key("a".into())]],
        target: Target { parent: vec![], index: 2 },
        on_collision: OnCollision::Cancel,
    }).unwrap();
    let out = doc.serialize();
    let bi = out.find("b = 2").unwrap();
    let ai = out.find("a = 1").unwrap();
    let ci = out.find("c = 3").unwrap();
    assert!(bi < ai && ai < ci, "expected b < a < c, got:\n{out}");
}
```

- [ ] **Step 2: Run them — confirm they fail** (currently the moved key appends at the end)

```bash
cargo test move_into_table_honors_target_index move_within_table_after_anchor_handles_shift 2>&1
```

- [ ] **Step 3: Add an anchor-based positional-insert helper for a concrete `Table`**

`target.index` is a **projected** index taken **before** the move's deletions. Since `move_inner` deletes sources before re-inserting, a raw index shifts when a same-parent source precedes the target. Avoid this by resolving an **anchor key name** (the real key the new entry should sit *before*) from the pre-move projection, then insert before that anchor's *current* position at insert time.

Add a free function near `rename_in_table` in `src/model/toml_doc.rs`:

```rust
/// Insert `(key, item)` into `tbl` immediately before the entry named `anchor`
/// (or at the end when `anchor` is `None` or not present). Rebuilds via the
/// order-preserving remove/`insert_formatted` technique used by
/// `rename_in_table`, so existing keys keep their decor and order.
fn insert_before(
    tbl: &mut toml_edit::Table,
    key: toml_edit::Key,
    item: Item,
    anchor: Option<&str>,
) {
    let order: Vec<String> = tbl.iter().map(|(k, _)| k.to_string()).collect();
    let mut new_entry = Some((key, item));
    for k in &order {
        let (ko, it) = tbl.remove_entry(k).expect("key listed from iter");
        if anchor == Some(k.as_str()) {
            if let Some((nk, ni)) = new_entry.take() {
                tbl.insert_formatted(&nk, ni);
            }
        }
        tbl.insert_formatted(&ko, it);
    }
    // anchor was None, or not found, or at end → append.
    if let Some((nk, ni)) = new_entry.take() {
        tbl.insert_formatted(&nk, ni);
    }
}
```

- [ ] **Step 4: Add an "anchor name at projected index" resolver on `TomlDocument`**

```rust
/// Name of the real (non-comment) child key at projected `index` under `parent`,
/// resolved from the current projection. `None` when `index` is at/after the
/// last real key — meaning "append". Used to position a moved/inserted entry
/// stably even though `move_inner` deletes sources before re-inserting.
fn anchor_key_at(&self, parent: &[Seg], index: usize) -> Option<String> {
    let projected = self.project();
    let children = find_node_by_path(&projected.root, parent)
        .map(|n| n.children.as_slice())
        .unwrap_or(&[]);
    children
        .iter()
        .take(index)
        .filter(|c| !matches!(c.kind, crate::model::node::NodeKind::Comment(_)))
        .count(); // (real index — kept for clarity; not used directly)
    // The anchor is the first real key at or after `index`.
    children
        .iter()
        .skip(index)
        .find(|c| !matches!(c.kind, crate::model::node::NodeKind::Comment(_)))
        .and_then(|c| match c.path.last() {
            Some(Seg::Key(k)) => Some(k.clone()),
            _ => None,
        })
}
```

> Simplify the body to just the `skip(index).find(non-comment)` lookup — drop the dead `take().count()` line if clippy flags it. The anchor is the first real key at/after the projected `index`; if none, return `None` (append). `find_node_by_path` already exists in the file (used by `remove_at`).

- [ ] **Step 5: Use anchor-based insert in `move_inner` (standard-table branch)**

In `move_inner`, the captured entries are currently re-inserted with `dest.entry_format(&key).or_insert(item)` (append). Change the **standard-table** destination to position via the anchor:

1. Resolve the anchor **before** taking the `&mut` borrow and **before** the deletes already done above it — i.e. compute it at the top of `move_inner` (right after the array-branch check) while `&self` is free: `let anchor = self.anchor_key_at(&target.parent, target.index);`
2. After capturing + deleting sources, if the destination is a concrete standard table (`self.concrete_table_mut(&target.parent)` is `Some`), run the collision handling as today but for the **fresh insert** call `insert_before(tbl, key, item, anchor.as_deref())`. Insert captured entries in order; since each `insert_before` uses the same anchor, successive entries naturally accumulate in order right before the anchor.
3. If the destination is **not** concrete (inline table), keep the existing `parent_table_mut` + `entry_format` append path (rare; add a one-line comment).

Keep Overwrite/Rename behavior; position only applies to the non-colliding fresh insert. (For Rename, inserting the suffixed key before the anchor is fine too.)

> Borrow ordering: compute `anchor` (needs `&self`) before any `&mut` borrow. `concrete_table_mut` borrows `&mut self`; `insert_before` is a free fn over `&mut Table`, so no nested `self` calls inside the borrow.

- [ ] **Step 6: Run the new test + all Move tests**

```bash
cargo test move_into_table_honors_target_index 2>&1
cargo test move_ 2>&1
cargo test cut_paste_same_scope_moves_without_collision 2>&1
```

The same-scope test from Task 1 should now place `a` at the cursor position (after `b`).

- [ ] **Step 7: Full suite + lint + commit**

```bash
cargo test 2>&1 | tail -5
cargo clippy -- -D warnings 2>&1
cargo fmt --check 2>&1
git add src/model/toml_doc.rs
git commit -m "feat: table moves honor target position (exact-position reorder)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3 — Serialize comments + `Mutation::InsertComment`

**Files:** `src/model/document.rs`, `src/model/toml_doc.rs`, `src/tui/app.rs` (serialize)

### Context
Two pieces: (a) make a comment node serialize to its raw `# …` text; (b) add `Mutation::InsertComment { target, text }` that writes a comment block into the parent's decor at the (real) target position, reusing `comment_out`'s decor-placement logic. Comments never collide.

- [ ] **Step 1: Add the mutation variant**

In `src/model/document.rs` `Mutation` enum, add:

```rust
    /// Insert a standalone comment block (`# …` lines) into `target.parent`'s
    /// decor at the projected `target.index`. Comments live in decor, so there
    /// is no key and no collision.
    InsertComment {
        target: Target,
        text: String,
    },
```

- [ ] **Step 2: Serialize a comment node to raw text**

In `serialize_node_fragment_opts` (`src/tui/app.rs`), before the existing key-lookup that returns `""`, add: if the path's last seg is `Seg::Key(k)` with `k.starts_with("#comment:")`, return the comment node's text. Get it from the projection (the node's `value`/`NodeKind::Comment(text)`), e.g.:

```rust
if let Some(Seg::Key(k)) = path.last() {
    if k.starts_with("#comment:") {
        if let Some(n) = node_at(&doc.project().root, path) {
            if let crate::model::node::NodeKind::Comment(t) = &n.kind {
                return t.clone();
            }
        }
        return String::new();
    }
}
```

(Place this so both `serialize_node_fragment` and the clipboard path get it. The text is the raw `#`-prefixed block, possibly multi-line for merged comments.)

- [ ] **Step 3: Write a failing test for `InsertComment`**

Add to `#[cfg(test)] mod tests` in `src/model/toml_doc.rs`:

```rust
#[test]
fn insert_comment_lands_before_target_key() {
    // keys a, b; insert a comment at projected index 1 (before b) → appears above b.
    let mut doc = doc_from_str("a = 1\nb = 2\n");
    doc.apply(Mutation::InsertComment {
        target: Target { parent: vec![], index: 1 },
        text: "# hello".into(),
    }).unwrap();
    let out = doc.serialize();
    let ci = out.find("# hello").unwrap();
    let bi = out.find("b = 2").unwrap();
    let ai = out.find("a = 1").unwrap();
    assert!(ai < ci && ci < bi, "expected a < #hello < b, got:\n{out}");
}

#[test]
fn insert_comment_at_end_uses_trailing() {
    let mut doc = doc_from_str("a = 1\n");
    doc.apply(Mutation::InsertComment {
        target: Target { parent: vec![], index: 1 },
        text: "# tail".into(),
    }).unwrap();
    assert!(doc.serialize().trim_end().ends_with("# tail"), "{}", doc.serialize());
}

#[test]
fn insert_comment_rejects_non_comment_text() {
    let mut doc = doc_from_str("a = 1\n");
    let err = doc.apply(Mutation::InsertComment {
        target: Target { parent: vec![], index: 0 },
        text: "not a comment".into(),
    });
    assert!(matches!(err, Err(MutateError::Fragment(_))));
}
```

- [ ] **Step 4: Run — confirm failure** (`InsertComment` not yet handled in `apply`)

```bash
cargo test insert_comment 2>&1
```

- [ ] **Step 5: Implement `InsertComment` in `apply` + helper**

In `TomlDocument::apply`, add the match arm dispatching to a new method `insert_comment(&target, &text)`. Implement it:
1. Validate every line of `text` starts with `#` (after trim); else `Err(MutateError::Fragment("comment lines must start with #".into()))`.
2. Compute `real_index = self.real_key_index(&target.parent, target.index)` (from Task 2; if Task 2 not yet merged, inline the same logic).
3. Place the block, mirroring `comment_out`'s decor logic:
   - Resolve the parent table.
   - If `real_index` < number of real keys: prepend `text + "\n"` to the `leaf_decor` **prefix** of the key currently at `real_index` (preserving any existing prefix/comment after it).
   - Else (at/after end): append to the parent's trailing decor — for the document root use `doc.trailing()`; for a `[table]` parent append to the last key's decor or the table's decor, matching how `comment_out` handles the empty/last case.
   - If the table is empty, write to the appropriate trailing/decor slot (same as `comment_out`'s empty-table branch).

> Factor the shared decor-writing out of `comment_out` into a helper like `insert_comment_in_decor(&mut self, parent, real_index, text)` and call it from both, so the two stay consistent. Keep `comment_out` behavior byte-identical (run its existing tests).

- [ ] **Step 6: Run InsertComment + remark/comment tests**

```bash
cargo test insert_comment 2>&1
cargo test comment 2>&1
cargo test remark 2>&1
```

- [ ] **Step 7: Full suite + lint + commit**

```bash
cargo test 2>&1 | tail -5
cargo clippy -- -D warnings 2>&1
cargo fmt --check 2>&1
git add src/model/document.rs src/model/toml_doc.rs src/tui/app.rs
git commit -m "feat: serialize comments + Mutation::InsertComment (decor-aware, positional)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4 — Wire comment entries into copy/cut/paste

**Files:** `src/tui/app.rs` (`do_paste` partition completion)

### Context
Complete the partition started in Task 1: comment entries (source path ends `#comment:N`) are pasted via `Mutation::InsertComment` and never collide. For **cut**, delete the source comment after a successful insert. Process comment entries after the node phase succeeds; if the node phase enters a collision prompt, comment entries remain in the restored clipboard and run on retry.

- [ ] **Step 1: Write failing tests (copy a comment, cut a comment)**

Add to `#[cfg(test)] mod tests` in `src/tui/app.rs`:

```rust
#[test]
fn copy_paste_comment_node() {
    // Doc with a standalone comment then a key; copy the comment, paste onto `b`.
    let mut app = app_with("# note\na = 1\nb = 2\n");
    app.rebuild_rows();
    // find the comment row and the `b` row by path/key
    let cpos = app.rows.iter().position(|r|
        matches!(r.path.last(), Some(Seg::Key(k)) if k.starts_with("#comment:"))).unwrap();
    app.cursor = cpos;
    app.copy_selected();
    assert!(app.clipboard.is_some());
    let bpos = app.rows.iter().position(|r|
        matches!(r.path.last(), Some(Seg::Key(k)) if k == "b")).unwrap();
    app.cursor = bpos;
    app.paste();
    let out = app.doc.as_ref().unwrap().serialize();
    assert_eq!(out.matches("# note").count(), 2, "comment now appears twice:\n{out}");
}

#[test]
fn cut_paste_comment_node_moves_it() {
    let mut app = app_with("# note\na = 1\nb = 2\n");
    app.rebuild_rows();
    let cpos = app.rows.iter().position(|r|
        matches!(r.path.last(), Some(Seg::Key(k)) if k.starts_with("#comment:"))).unwrap();
    app.cursor = cpos;
    app.cut_selected();
    let bpos = app.rows.iter().position(|r|
        matches!(r.path.last(), Some(Seg::Key(k)) if k == "b")).unwrap();
    app.cursor = bpos;
    app.paste();
    let out = app.doc.as_ref().unwrap().serialize();
    assert_eq!(out.matches("# note").count(), 1, "comment moved, not duplicated:\n{out}");
    // moved below b
    assert!(out.find("# note").unwrap() > out.find("b = 2").unwrap(), "{out}");
}
```

- [ ] **Step 2: Run — confirm failure**

```bash
cargo test copy_paste_comment_node cut_paste_comment_node_moves_it 2>&1
```

- [ ] **Step 3: Complete the partition in `do_paste`**

Extend `do_paste` (from Task 1) so after the node phase succeeds (or when there are no node sources), it processes comment entries: for each `(fragment, source)` whose source is a comment, apply `Mutation::InsertComment { target: target.clone(), text: fragment.clone() }`; if `is_cut`, then `Mutation::Delete { path: source.clone() }` (which routes to `remove_comment_from_decor`). Wrap the comment phase so a failure restores the clipboard with the unprocessed comment entries and sets `paste error: …`. Call `on_mutation_success()` once at the end. Ensure the node-collision path restores a clipboard that still includes the comment entries (so retry completes them).

> Keep it readable: build `node_entries` and `comment_entries` as `Vec<(String, Path)>` up front; the node phase consumes `node_entries`, the comment phase consumes `comment_entries`.

- [ ] **Step 4: Run the comment paste tests + regression**

```bash
cargo test copy_paste_comment_node cut_paste_comment_node_moves_it 2>&1
cargo test cut_paste_same_scope_moves_without_collision 2>&1
cargo test paste_error_preserves_clipboard 2>&1
```

- [ ] **Step 5: Full suite + lint + commit**

```bash
cargo test 2>&1 | tail -5
cargo clippy -- -D warnings 2>&1
cargo fmt --check 2>&1
git add src/tui/app.rs
git commit -m "feat: comment nodes participate in copy/cut/paste

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5 — Docs + changelog

**Files:** `CHANGELOG.md`, `CLAUDE.md`

- [ ] **Step 1: CHANGELOG** — add to `## [Unreleased]`:
  - Fixed: cut+paste within the same scope now repositions the node (atomic `Mutation::Move`) instead of failing with "Key already exists"; a failed paste no longer loses the clipboard. (2026-06-08)
  - Added: table moves land at the cursor position (exact-position reorder). (2026-06-08)
  - Added: Comment nodes can now be copied/cut/pasted like any other node. (2026-06-08)

- [ ] **Step 2: CLAUDE.md** — in the **Clipboard / paste mode** and **Comments** paragraphs, note: cut paste routes through atomic `Mutation::Move` (delete-before-reinsert → same-scope reorder, no false collision; honors target position via the `rename_in_table` rebuild technique); comments serialize to raw `#` text and paste via `Mutation::InsertComment` (decor-aware, never collide).

- [ ] **Step 3: Commit**

```bash
git add CHANGELOG.md CLAUDE.md
git commit -m "docs: move-aware cut, exact-position reorder, comment clipboard

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-Review

| Requirement | Task |
|---|---|
| #1 failed paste never loses clipboard | Task 1 (Move atomic; copy restores remaining) |
| #2 same-scope cut/paste doesn't collide | Task 1 (Move deletes-before-reinsert) |
| #2 moved node lands at cursor position | Task 2 (positional table insert) |
| #3 comment copy/cut/paste | Tasks 3 + 4 |

**Risks / watch-items:**
- `move_inner` borrow order: compute `real_key_index` (needs `&self`) before the `&mut` table borrow.
- Inline-table move destinations keep append (no `concrete_table_mut`); documented, rare.
- Comment decor placement must keep `comment_out` byte-identical — factor a shared helper and run the existing comment/remark tests.
- Mixed comment+node selection with a node collision: comments are processed after nodes; the restored clipboard must retain comment entries for the retry.
