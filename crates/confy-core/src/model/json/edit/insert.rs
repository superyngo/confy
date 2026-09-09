//! `Mutation::Insert` and `Mutation::Move` — split out of `json/edit.rs`
//! (F6, 2026-09-09).

use super::*;

pub(super) fn insert(
    tree: &SyntaxNode,
    target: &MutTarget,
    fragment: &str,
    on_collision: OnCollision,
    suggested_key: Option<&str>,
) -> Result<(), MutateError> {
    // ── 1. Locate the container OBJECT or ARRAY node ────────────────────────
    let container = match find_container(tree, &target.parent) {
        Ok(c) => c,
        Err(MutateError::NotFound) if target.parent.is_empty() => {
            return insert_into_empty_document(tree, fragment, suggested_key);
        }
        Err(e) => return Err(e),
    };

    let is_object = container.kind() == SyntaxKind::OBJECT;
    let is_multiline = container.text().to_string().contains('\n');

    // ── 2. Collect existing items as verbatim strings ───────────────────────
    // Items are MEMBER nodes (objects) or VALUE nodes (arrays), plus standalone
    // comment tokens (LINE_COMMENT / BLOCK_COMMENT). We capture them in
    // projected order — same slot ordering the TUI uses.
    let mut items: Vec<String> = collect_items(&container);

    // ── 3. Adapt the fragment to the destination ────────────────────────────
    let (new_item_text, new_key) = adapt_fragment(fragment, is_object, suggested_key)?;

    // ── 4. Collision check (objects only) ───────────────────────────────────
    let mut final_key = new_key.clone();
    if is_object {
        if let Some(key) = &final_key {
            let existing_keys: Vec<String> = existing_object_keys(&container);
            if existing_keys.iter().any(|k| k == key) {
                match on_collision {
                    OnCollision::Cancel => {
                        return Err(MutateError::Collision(key.clone()));
                    }
                    OnCollision::Overwrite => {
                        // Remove the colliding member from items list; we'll insert the new one.
                        let collision_idx = items
                            .iter()
                            .position(|it| member_key_of_text(it).as_deref() == Some(key.as_str()));
                        if let Some(ci) = collision_idx {
                            items.remove(ci);
                        }
                    }
                    OnCollision::Rename => {
                        final_key = Some(crate::model::node::next_available_key(key, |c| {
                            existing_keys.iter().any(|k| k == c)
                        }));
                    }
                }
            }
        }
    }

    // Re-render the item text with the (possibly renamed) key.
    let item_text = if let Some(key) = &final_key {
        if new_key.as_deref() != Some(key.as_str()) {
            // Key was renamed — rebuild from fragment value part.
            let value_part = bare_value_of_fragment(fragment).unwrap_or(fragment);
            format!("\"{key}\": {value_part}")
        } else {
            new_item_text
        }
    } else {
        new_item_text
    };

    // ── 5. Insert at the projected index ────────────────────────────────────
    let idx = target.index.min(items.len());
    items.insert(idx, item_text);

    // ── 6. Rebuild the container as a string and splice ─────────────────────
    let new_container_text = if is_multiline {
        rebuild_multiline(&container, &items)
    } else {
        rebuild_inline(&container, &items)
    };

    // Parse the rebuilt container as a full document to get a mutable node.
    let new_container = parse_container_text(&new_container_text, is_object)?;
    replace_node(&container, new_container);
    Ok(())
}

// ── Helpers for insert ───────────────────────────────────────────────────────

/// Fallback for a root-level Insert when the document has no top-level VALUE
/// node yet (empty or comment-only file) — `find_container` has nothing to
/// walk into, so a root Insert always failed with `NotFound` even though
/// appending the first key/element is exactly what "Add" on an empty
/// document should do. Mirrors YAML's `insert_into_empty_document`
/// (`yaml/edit/block.rs`). Defaults to an object root (`{}`), matching
/// TOML's root-is-always-Table convention — a brand-new JSON config is
/// overwhelmingly the common case; a bare-array fragment still adapts
/// correctly via `adapt_fragment`'s `is_object` branch.
pub(super) fn insert_into_empty_document(
    tree: &SyntaxNode,
    fragment: &str,
    suggested_key: Option<&str>,
) -> Result<(), MutateError> {
    let (item_text, _) = adapt_fragment(fragment, true, suggested_key)?;
    let new_text = format!("{{ {item_text} }}");
    let green = crate::model::json::parse::parse(&new_text).map_err(MutateError::Fragment)?;
    let new_root = SyntaxNode::new_root(green).clone_for_update();
    let n = tree.children_with_tokens().count();
    let new_children: Vec<_> = new_root.children_with_tokens().collect();
    tree.splice_children(0..n, new_children);
    Ok(())
}

pub(super) fn move_nodes(
    tree: &SyntaxNode,
    sources: &[Vec<Seg>],
    target: &MutTarget,
    on_collision: OnCollision,
) -> Result<(), MutateError> {
    if sources.is_empty() {
        return Ok(());
    }

    // ── 1. Capture fragments BEFORE any deletion ────────────────────────────
    let captured: Vec<(Vec<Seg>, String)> = sources
        .iter()
        .map(|path| {
            let frag = serialize_fragment(tree, path);
            if frag.is_empty() {
                Err(MutateError::NotFound)
            } else {
                Ok((path.clone(), frag))
            }
        })
        .collect::<Result<_, _>>()?;

    // ── 2a. Pre-deletion shift: count same-container sources before target ───
    // `target.index` is a pre-deletion ordinal in the parent's *full* child
    // sequence (comments included — same space the TUI's `true_sibling_index`
    // uses). Every source sitting in that same container at a lower ordinal will
    // shift the surviving slots up by one when deleted, so the insert index must
    // drop by that many. This covers keyed *and* positional sources alike (a
    // keyed node moved down past a trailing comment was previously not adjusted).
    let shift = {
        let proj = crate::model::json::project::project(tree, "");
        crate::model::node::NodeTree::node_at(&proj, &target.parent)
            .map(|parent| {
                sources
                    .iter()
                    .filter(|s| {
                        parent
                            .children
                            .iter()
                            .position(|c| &c.path == *s)
                            .is_some_and(|ord| ord < target.index)
                    })
                    .count()
            })
            .unwrap_or(0)
    };

    // ── 2. Delete sources back-to-front ─────────────────────────────────────
    // Delete in reverse order: last source first, so earlier sources' indices
    // remain valid. For sources in the same container we use index order
    // (highest index first); for other orderings reverse the input order.
    let mut delete_indices: Vec<usize> = (0..sources.len()).collect();
    // Sort descending by last path segment index when applicable.
    delete_indices.sort_by(|&a, &b| {
        let pa = &sources[a];
        let pb = &sources[b];
        // Compare last segments: Index > Index by value descending;
        // otherwise just reverse input order (b cmp a by position).
        match (pa.last(), pb.last()) {
            (Some(Seg::Index(ia)), Some(Seg::Index(ib))) => ib.cmp(ia),
            _ => b.cmp(&a), // reverse input order
        }
    });
    for i in delete_indices {
        delete(tree, &sources[i])?;
    }

    // ── 3. Apply the pre-computed shift ──────────────────────────────────────
    let effective_index = target.index - shift.min(target.index);

    // ── 4. Insert each captured fragment at the effective target ─────────────
    for (i, (path, frag)) in captured.iter().enumerate() {
        // A bare scalar lifted out of a keyed array needs a synthesized member
        // key on the object side; prefer `<arrayKey>_<index>` over the generic
        // "placeholder". Non-array sources (and unkeyed/nested arrays) get None
        // and keep the old placeholder fallback.
        let suggested_key = crate::model::node::array_element_suggested_key(path);
        let insert_target = MutTarget {
            parent: target.parent.clone(),
            index: effective_index + i,
        };
        insert(
            tree,
            &insert_target,
            frag,
            on_collision,
            suggested_key.as_deref(),
        )?;
    }

    Ok(())
}
