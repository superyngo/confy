//! `Mutation::Insert`/`Move` — fragment parsing/adaptation and the
//! insert/move splice logic — split out of `cst_edit.rs` (Task 15,
//! 2026-08-11 audit remediation).

use super::aot_group::{aot_entry_member_fragments, aot_entry_section_body, aot_group_insert};
use super::convert::struct_node;
use super::dotted_table::{
    dotted_ancestor_prefix_len, inline_ancestor_len, inline_member_entries, is_headerless_table,
    strip_key_prefix,
};
use super::joinable_entry;
use super::rename::{is_key_seg, key_seg_token};
use super::replace_delete::MemberSpan;
use super::replace_delete::{
    delete, path_key_display, section_text, table_fragment, table_member_spans,
};
use super::tree_nav::{
    extend_over_newline, fragment_key_segs, inline_raw_member_index, node_at, resolve_insert_at,
};
use crate::model::cst_project::{walk, CstIndex, Target};
use crate::model::document::{MutateError, OnCollision, Target as InsTarget};
use crate::model::node::{Format, Node, NodeKind, NodeTree, Seg};
use std::collections::HashSet;
use taplo::rowan::NodeOrToken;
use taplo::syntax::{SyntaxKind, SyntaxNode};

/// Insert a keyed node fragment (`key = val`, `[table]…`) at the projected
/// `target`. The fragment's first key is collision-checked against the parent
/// scope's existing keys. (Overwrite/Rename collision modes and bare array-element
/// inserts are deferred; `Cancel` and the no-collision path are handled.)
///
/// `suggested_key` is the preferred synthesized key for a **bare** fragment that
/// needs one (`<arrayKey>_<index>` for a scalar moved out of a keyed array);
/// `None` keeps the generic `PLACEHOLDER_KEY`. Ignored when the fragment
/// already carries its own key.
pub(crate) fn insert(
    tree: &SyntaxNode,
    target: &InsTarget,
    toml: &str,
    on_collision: OnCollision,
    suggested_key: Option<&str>,
) -> Result<(), MutateError> {
    let (proj, idx) = walk(tree, "");
    insert_with(tree, &proj, &idx, target, toml, on_collision, suggested_key)
}

fn insert_with(
    tree: &SyntaxNode,
    proj: &NodeTree,
    idx: &CstIndex,
    target: &InsTarget,
    toml: &str,
    on_collision: OnCollision,
    suggested_key: Option<&str>,
) -> Result<(), MutateError> {
    let frag_text = if toml.ends_with('\n') {
        toml.to_string()
    } else {
        format!("{toml}\n")
    };

    let parent = node_at(&proj.root, &target.parent).ok_or(MutateError::NotFound)?;
    let parent_is_array = matches!(parent.kind, crate::model::node::NodeKind::Array);
    let parent_is_inline = matches!(parent.kind, crate::model::node::NodeKind::InlineTable);

    // Member spans of a *table* parent (empty for root / arrays / inline tables):
    // they drive the headerless-table insert rules below.
    let parent_spans = if matches!(parent.kind, NodeKind::Table)
        && matches!(target.parent.last(), Some(Seg::Key(_)))
    {
        table_member_spans(tree, idx, &target.parent)
    } else {
        Vec::new()
    };
    let parent_entry_members = parent_spans
        .iter()
        .any(|s| matches!(s, MemberSpan::Entry(_)));
    let parent_section_members = parent_spans
        .iter()
        .any(|s| matches!(s, MemberSpan::Section(_)));
    let parent_headerless =
        !target.parent.is_empty() && is_headerless_table(idx, &proj.root, &target.parent);
    // An *implicit* scope table (only `[a.sub]` sections were written, no dotted
    // members): an entry child gets the table's own `[a]` section synthesized at
    // its first definition instead of a dotted prefix.
    let implicit_scope_parent =
        parent_headerless && parent_section_members && !parent_entry_members;

    // Inserting into a headerless table (`[T/D]` dotted, or the dotted side of a
    // mixed table): the new entry has no header to live under, so it is written as
    // a dotted entry whose key carries the ancestor prefix (`x = v` into `a.b`
    // becomes `a.b.x = v`), placed next to its dotted siblings. The prefix is the
    // trailing run of headerless-table ancestors of the parent (down from the
    // nearest real scope / root); empty for a normal table.
    let dotted_prefix: Vec<String> = if implicit_scope_parent {
        Vec::new()
    } else {
        let mut segs = Vec::new();
        for i in (0..target.parent.len()).rev() {
            let anc_path = &target.parent[..=i];
            node_at(&proj.root, anc_path).ok_or(MutateError::NotFound)?;
            if !is_headerless_table(idx, &proj.root, anc_path) {
                break;
            }
            if let Seg::Key(k) = &target.parent[i] {
                segs.push(k.clone());
            }
        }
        segs.reverse();
        segs
    };

    // D1 simple adaptation across container types:
    //  - into an ARRAY: a *keyless* bare value becomes the element as-is; a *keyed*
    //    fragment is wrapped as a `{ key = value }` inline-table element so the key is
    //    preserved (`key→{}`, below); a `[table]`/`[[aot]]` fragment is rejected.
    //  - into a TABLE/root we need a keyed entry: a bare element gets a synthesized
    //    `placeholder` key (`key+`).
    let (frag, synthesized_key) =
        parse_fragment_adapted(&frag_text, parent_is_array, suggested_key)?;

    // A keyless `{ … }` element copied out of an array **unpacks** into its
    // member entries for a table/root/[A/T] destination — matching the cut path
    // in `move_nodes` (and packing into ONE `[[…]]` entry for an `[A/T]` group).
    // A bare scalar keeps the synthesized `placeholder` key; into a plain array
    // the element form is kept.
    if synthesized_key && !parent_is_array {
        if let Some(entries) = unpack_inline_table(frag_text.trim()) {
            return insert_with(
                tree,
                proj,
                idx,
                target,
                &entries.concat(),
                on_collision,
                None,
            );
        }
    }

    // A `[table]`/`[[aot]]` **section** fragment is legal only into a real scope/root.
    // It cannot live inside an inline table, nor be nested under a synthetic `[T/D]`
    // dotted table (which opens no scope) — both surface a clear `Illegal`. Into a real
    // sub-scope its headers are re-prefixed with the destination path, so a `[T/S]`
    // table moved into another scope nests: `[a]` into `[b]` → `[b.a]`.
    let has_header = frag.descendants().any(|n| {
        matches!(
            n.kind(),
            SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER
        )
    });
    if has_header {
        if parent_is_inline {
            return Err(MutateError::Illegal(
                "a table cannot be inserted into an inline table".into(),
            ));
        }
        // A *pure* dotted table opens no scope a section could live in. A mixed
        // table (dotted members + existing sub-sections) does accept further
        // sub-sections — the TOML-spec `[fruit.apple.texture]` pattern.
        if parent_entry_members && !parent_section_members {
            return Err(MutateError::Illegal(
                "a scope table cannot be nested under a dotted table".into(),
            ));
        }
        if !target.parent.is_empty() {
            let prefix: Vec<String> = target
                .parent
                .iter()
                .filter_map(|s| match s {
                    Seg::Key(k) => Some(k.clone()),
                    _ => None,
                })
                .collect();
            prefix_section_headers(&frag, &prefix)?;
        }
    }

    if parent_is_array {
        // Into an array (no collision). A *keyless* bare value keeps its element form;
        // a *keyed* node is wrapped as a `{ key = value }` inline-table element so the
        // key is preserved (a keyed inline table becomes a nested inline table); a
        // multi-entry fragment (several pasted nodes, or a `[T/D]` table's members)
        // packs into ONE `{ a = 1, b = 2 }` element.
        // `[T/S]`/`[A/T]` headers are already rejected by `parse_fragment_adapted`.
        let element = if synthesized_key {
            frag
        } else {
            wrap_keyed_as_inline_element(&frag_text)?
        };
        return array_insert(idx, &target.parent, target.index, &element);
    }

    if matches!(parent.kind, NodeKind::ArrayOfTables) {
        // Into an `[A/T]` group: keyed entries land in a **new `[[…]]` entry**
        // synthesized at the target slot (multiple pasted nodes are joined into one
        // fragment by the caller, so they pack into the same entry). A
        // `[table]`/`[[aot]]` section cannot become an entry — `Illegal`.
        if has_header {
            return Err(MutateError::Illegal(
                "a table section cannot be inserted into an array of tables".into(),
            ));
        }
        return aot_group_insert(
            tree,
            idx,
            parent,
            &target.parent,
            target.index,
            &frag,
            on_collision,
        );
    }

    // A header-less **multi-entry** fragment (a copied `[T/D]` table block, whose
    // members are several flat dotted entries) is inserted one entry at a time, so the
    // dotted prefix and the per-leaf collision check apply to each member — a single
    // splice would only re-key the first (and an inline-table destination would drop
    // every member but the first). A `[table]`/`[[aot]]` section keeps its entries
    // together (they belong under the header) and is never split. The landing slot is
    // held by a stable **anchor path** (the first non-comment child at/after the
    // target index): inserted dotted members can merge into one projected child, so a
    // plain `index + k` would drift past later siblings after the first insert.
    let top_entries: Vec<SyntaxNode> = frag
        .children()
        .filter(|n| n.kind() == SyntaxKind::ENTRY)
        .collect();
    if !has_header && top_entries.len() > 1 {
        let anchor_path: Option<Vec<Seg>> = parent
            .children
            .iter()
            .skip(target.index)
            .find(|c| !matches!(c.kind, NodeKind::Comment(_)))
            .map(|c| c.path.clone());
        for e in &top_entries {
            let entry_text = format!("{}\n", e.to_string().trim());
            let (proj2, idx2) = walk(tree, "");
            let index = {
                let parent2 = node_at(&proj2.root, &target.parent).ok_or(MutateError::NotFound)?;
                match &anchor_path {
                    Some(ap) => parent2
                        .children
                        .iter()
                        .position(|c| &c.path == ap)
                        .unwrap_or(parent2.children.len()),
                    None => parent2.children.len(),
                }
            };
            insert_with(
                tree,
                &proj2,
                &idx2,
                &InsTarget {
                    parent: target.parent.clone(),
                    index,
                },
                &entry_text,
                on_collision,
                None,
            )?;
        }
        return Ok(());
    }

    // An inline-table destination: the parent is the `{ … }` itself, or a synthetic
    // `[T/D]` table nested inside one (its members are `x.y = 1` dotted keys). The
    // flat-ROOT splice machinery below must not reach through a `{ … }`, so both
    // route to `inline_table_insert` — the synthetic case with the key re-prefixed
    // scope-relative (`q = 9` into `t.x` becomes the member `x.q = 9`). Collision is
    // exact full path (like the flat path below): a dotted member sharing only a
    // prefix with an existing `[T/D]` chain merges instead of colliding.
    let inline_len = if parent_is_inline {
        Some(target.parent.len())
    } else if matches!(parent.kind, NodeKind::Table) {
        inline_ancestor_len(&proj.root, &target.parent)
    } else {
        None
    };
    if let Some(inline_len) = inline_len {
        if has_header {
            return Err(MutateError::Illegal(
                "a table cannot be inserted into an inline table".into(),
            ));
        }
        let prefix: Vec<String> = target.parent[inline_len..]
            .iter()
            .filter_map(|s| match s {
                Seg::Key(k) => Some(k.clone()),
                Seg::Index(_) => None,
            })
            .collect();
        if !prefix.is_empty() {
            prefix_entry_key(&frag, &prefix)?;
        }
        let new_segs = fragment_key_segs(&frag);
        if new_segs.is_empty() {
            return Err(MutateError::Fragment("fragment has no key".into()));
        }
        let mut full = target.parent[..inline_len].to_vec();
        full.extend(new_segs.iter().cloned().map(Seg::Key));
        if node_at(&proj.root, &full).is_some() {
            return Err(MutateError::Collision(new_segs.join(".")));
        }
        let raw_index = inline_raw_member_index(idx, parent, target.index);
        return inline_table_insert(idx, &target.parent[..inline_len], raw_index, &frag);
    }

    let frag_segs = fragment_key_segs(&frag);
    if frag_segs.is_empty() {
        return Err(MutateError::Fragment("fragment has no key".into()));
    }
    // A synthesized `placeholder` key is auto-renamed on collision — the user never
    // chose it, so a clash shouldn't surface as a prompt/error.
    let on_collision = if synthesized_key {
        OnCollision::Rename
    } else {
        on_collision
    };
    // Within a table the entry run and the sub-section run partition the layout
    // (D5). Targeting *this table* means the position can always be honored at the
    // nearest legal slot, so clamp instead of rejecting: an entry-like fragment
    // lands no further than the partition split (the end of the entry run — for a
    // headerless table, after the last dotted member; never inside a section), a
    // header-like one no earlier than it. The Root keeps explicit-position
    // semantics (out-of-partition inserts there still surface `Illegal`).
    let split = parent
        .children
        .iter()
        .position(|c| {
            matches!(c.kind, NodeKind::Table | NodeKind::ArrayOfTables)
                && c.format != Format::Dotted
        })
        .unwrap_or(parent.children.len());
    let parent_is_table =
        matches!(parent.kind, NodeKind::Table) && matches!(target.parent.last(), Some(Seg::Key(_)));
    let eff_index = if parent_is_table && !has_header {
        target.index.min(split)
    } else if parent_is_table && has_header {
        target.index.max(split)
    } else {
        target.index
    };
    // Carry the dotted ancestor prefix onto the key *before* the collision check, so
    // an Overwrite/splice keeps the destination prefix. Collision is decided on
    // `frag_segs` (the key relative to the parent), which equals the leaf's projected
    // path tail regardless of how the key is written.
    check_partition(parent, &frag, eff_index)?;
    if !dotted_prefix.is_empty() {
        prefix_entry_key(&frag, &dotted_prefix)?;
    }
    // Collision is **exact full path** (`target.parent ++ frag_segs`): dotted siblings
    // that merely share a prefix (`a.x` beside `a.y`) merge into one `[T/D]` table
    // instead of colliding — only an identical full key clashes. A header fragment
    // bound for a sub-scope was already re-prefixed with the destination path
    // (`prefix_section_headers`), so its key segments are absolute from the root —
    // prepending `target.parent` again would check a phantom `b.b.a` path and let a
    // duplicate `[b.a]` through.
    let header_is_absolute = has_header && !target.parent.is_empty();
    let full_path = |segs: &[String]| -> Vec<Seg> {
        let mut p = if header_is_absolute {
            Vec::new()
        } else {
            target.parent.clone()
        };
        p.extend(segs.iter().cloned().map(Seg::Key));
        p
    };
    if node_at(&proj.root, &full_path(&frag_segs)).is_some() {
        match on_collision {
            OnCollision::Cancel => return Err(MutateError::Collision(frag_segs.join("."))),
            OnCollision::Overwrite => {
                // Replace the colliding leaf's element in place (keeps position).
                let victim_path = full_path(&frag_segs);
                let velem = match idx.iter().find(|(p, _)| p == &victim_path).map(|(_, t)| t) {
                    Some(Target::Entry(n)) => n.clone(),
                    _ => return Err(MutateError::Unsupported),
                };
                let vparent = velem.parent().ok_or(MutateError::NotFound)?;
                let mut new_els: Vec<_> = frag.children_with_tokens().collect();
                while matches!(new_els.last(), Some(NodeOrToken::Token(t)) if t.kind() == SyntaxKind::NEWLINE)
                {
                    new_els.pop();
                }
                for e in &new_els {
                    e.detach();
                }
                let i = velem.index();
                vparent.splice_children(i..i + 1, new_els);
                return Ok(());
            }
            OnCollision::Rename => {
                // Append _2, _3, … to the **last** segment until the full path is free.
                let base = frag_segs.last().cloned().unwrap_or_default();
                let mut segs = frag_segs.clone();
                let last = segs.len() - 1;
                let candidate = crate::model::node::next_available_key(&base, |c| {
                    let mut cand = segs.clone();
                    cand[last] = c.to_string();
                    node_at(&proj.root, &full_path(&cand)).is_some()
                });
                segs[last] = candidate;
                rewrite_last_key(&frag, segs.last().unwrap())?;
            }
        }
    }

    let at = if implicit_scope_parent && !has_header {
        // Synthesize the table's own `[…]` section at its first definition and
        // put the entry right under the new header.
        let parsed = taplo::parser::parse(&format!("[{}]\n", path_key_display(&target.parent)));
        if let Some(e) = parsed.errors.first() {
            return Err(MutateError::Fragment(e.to_string()));
        }
        let header = parsed.into_syntax().clone_for_update();
        let hdr_els: Vec<_> = header.children_with_tokens().collect();
        for e in &hdr_els {
            e.detach();
        }
        let n = hdr_els.len();
        let at = parent_spans.first().map(|s| s.start()).unwrap_or(0);
        tree.splice_children(at..at, hdr_els);
        at + n
    } else if parent_headerless && !has_header && parent_section_members && eff_index >= split {
        // Mixed table, append: the entry stays with the dotted-member run (after
        // its last line), never inside a member section.
        let last = parent_spans
            .iter()
            .filter_map(|s| match s {
                MemberSpan::Entry(e) => Some(e.clone()),
                MemberSpan::Section(_) => None,
            })
            .next_back()
            .ok_or(MutateError::Unsupported)?;
        extend_over_newline(tree, last.index() + 1)
    } else {
        resolve_insert_at(
            tree,
            &proj.root,
            idx,
            &InsTarget {
                parent: target.parent.clone(),
                index: eff_index,
            },
        )?
    };
    let els: Vec<_> = frag.children_with_tokens().collect();
    for e in &els {
        e.detach();
    }
    tree.splice_children(at..at, els);
    Ok(())
}

/// The synthesized key for a bare element pasted into a table (`key+`, D1).
pub(crate) const PLACEHOLDER_KEY: &str = "placeholder";

/// Parse a fragment for insertion into a table (`into_array == false`) or an array
/// (`true`), adapting across container types (D1 simple adaptation). Returns the
/// parsed fragment and whether a synthesized key was added for it.
///
/// A fragment that parses as a TOML document is used as-is (a keyed entry, or a
/// `[table]`/`[[aot]]` section). A fragment that does not (a **bare array-element
/// value** like `42` or `{ a = 1 }`) is wrapped as `<key> = <value>` (the
/// caller's `suggested_key`, else `PLACEHOLDER_KEY`) so it
/// becomes a keyed entry — for a table dest the key is kept (`key+`); for an array
/// dest the synthesized key marks the value as keyless, so it stays a bare element
/// (a *real* keyed fragment is instead wrapped as `{ key = value }` by the caller to
/// preserve its key). A `[table]`/`[[aot]]` section cannot become an array element
/// (a hard coerce), so it is rejected for an array.
pub(crate) fn parse_fragment_adapted(
    frag_text: &str,
    into_array: bool,
    suggested_key: Option<&str>,
) -> Result<(SyntaxNode, bool), MutateError> {
    let parse = taplo::parser::parse(frag_text);
    if parse.errors.is_empty() {
        let node = parse.into_syntax().clone_for_update();
        if into_array
            && node.descendants().any(|n| {
                matches!(
                    n.kind(),
                    SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER
                )
            })
        {
            return Err(MutateError::Illegal(
                "a table cannot be pasted as an array element".into(),
            ));
        }
        return Ok((node, false));
    }
    // Not a standalone document — try treating it as a bare value with a key.
    let wrapped = format!(
        "{} = {}\n",
        suggested_key.unwrap_or(PLACEHOLDER_KEY),
        frag_text.trim_end()
    );
    let parse2 = taplo::parser::parse(&wrapped);
    match parse2.errors.first() {
        Some(e) => Err(MutateError::Fragment(e.to_string())),
        None => Ok((parse2.into_syntax().clone_for_update(), true)),
    }
}

/// Wrap a keyed fragment as a bare inline-table value (`__w = { k = v }`) so
/// inserting it into an array preserves the key as a `{ k = v }` element (a keyed
/// inline-table value becomes a nested inline table). A **multi-entry** fragment
/// packs all entries into ONE element (`{ a = 1, b = 2 }`, `, `-joined).
/// `array_insert` extracts the first VALUE descendant, which is the wrapping
/// inline table. A multi-line value (multiline string/array) can't live on the
/// inline table's single line, so it surfaces as a `Fragment` error.
pub(crate) fn wrap_keyed_as_inline_element(frag_text: &str) -> Result<SyntaxNode, MutateError> {
    let pre = taplo::parser::parse(frag_text);
    let inner = if pre.errors.is_empty() {
        let entries: Vec<String> = pre
            .into_syntax()
            .children()
            .filter(|n| n.kind() == SyntaxKind::ENTRY)
            .map(|e| e.to_string().trim().to_string())
            .collect();
        if entries.is_empty() {
            frag_text.trim().to_string()
        } else {
            entries.join(", ")
        }
    } else {
        frag_text.trim().to_string()
    };
    let parse = taplo::parser::parse(&format!("__w = {{ {inner} }}\n"));
    match parse.errors.first() {
        Some(e) => Err(MutateError::Fragment(e.to_string())),
        None => Ok(parse.into_syntax().clone_for_update()),
    }
}

/// If `value_text` is an inline table (`{ k = v, … }`), return its member entries
/// (`k = v`, one per element). The inverse of `wrap_keyed_as_inline_element`:
/// moving such an element out of an array into a table **unpacks** it back into
/// keyed entries (each insert runs the per-leaf collision check). An empty `{}`
/// or any other value returns `None` and gets a synthesized key instead.
pub(crate) fn unpack_inline_table(value_text: &str) -> Option<Vec<String>> {
    let parse = taplo::parser::parse(&format!("__w = {}\n", value_text.trim()));
    if !parse.errors.is_empty() {
        return None;
    }
    let it = parse
        .into_syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::INLINE_TABLE)?;
    let entries: Vec<String> = it
        .children()
        .filter(|c| c.kind() == SyntaxKind::ENTRY)
        .map(|e| format!("{}\n", e.to_string().trim()))
        .collect();
    if entries.is_empty() {
        return None;
    }
    Some(entries)
}

/// D5 (TOML table-capture): within a table/root the legal layout is partitioned —
/// a leading region (scalars / arrays / inline tables) then a header region
/// (sub-`[table]` / `[[aot]]`). A `[table]`/`[[aot]]` header before the keys above
/// it would capture them; a plain key after a header would be re-keyed into that
/// section. So a header-like fragment may only land at index `>= split`, a leaf-like
/// one only at index `<= split`, where `split` is the parent's first sub-table/AoT
/// child index (or `len` when it has none).
pub(crate) fn check_partition(
    parent: &Node,
    frag: &SyntaxNode,
    index: usize,
) -> Result<(), MutateError> {
    use crate::model::node::NodeKind;
    let len = parent.children.len();
    // Clamp the append sentinel (callers pass an out-of-range index to mean "end").
    let index = index.min(len);
    // A `[T/D]` dotted table is not a capturing header (it opens no scope), so it
    // is not a partition boundary — a scalar may sit after it.
    let split = parent
        .children
        .iter()
        .position(|c| {
            matches!(c.kind, NodeKind::Table | NodeKind::ArrayOfTables)
                && c.format != Format::Dotted
        })
        .unwrap_or(len);
    let header_like = frag.descendants().any(|n| {
        matches!(
            n.kind(),
            SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER
        )
    });
    if header_like {
        if index < split {
            return Err(MutateError::Illegal(
                "a table here would capture the keys above it".into(),
            ));
        }
    } else if index > split {
        return Err(MutateError::Illegal(
            "a key here would be captured by the table above it".into(),
        ));
    }
    Ok(())
}

/// Insert a bare value into the array at `array_path`, at element `index` (or
/// appended). Uses single-line `, ` separators; multiline-array spacing is rough.
pub(crate) fn array_insert(
    idx: &CstIndex,
    array_path: &[Seg],
    index: usize,
    frag: &SyntaxNode,
) -> Result<(), MutateError> {
    let arr = match idx.iter().find(|(p, _)| p == array_path).map(|(_, t)| t) {
        Some(Target::Entry(entry)) => entry
            .children()
            .find(|c| c.kind() == SyntaxKind::VALUE)
            .and_then(|v| struct_node(&v))
            .filter(|n| n.kind() == SyntaxKind::ARRAY)
            .ok_or(MutateError::Unsupported)?,
        _ => return Err(MutateError::Unsupported),
    };
    let new_val = frag
        .descendants()
        .find(|n| n.kind() == SyntaxKind::VALUE)
        .ok_or_else(|| MutateError::Fragment("fragment has no value".into()))?;
    new_val.detach();

    let els: Vec<_> = arr.children_with_tokens().collect();
    let value_pos: Vec<usize> = els
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e, NodeOrToken::Node(n) if n.kind() == SyntaxKind::VALUE))
        .map(|(i, _)| i)
        .collect();

    if index < value_pos.len() {
        let at = value_pos[index];
        let (comma, space) = array_sep();
        arr.splice_children(at..at, vec![NodeOrToken::Node(new_val), comma, space]);
    } else if let Some(&last) = value_pos.last() {
        let (comma, space) = array_sep();
        arr.splice_children(
            last + 1..last + 1,
            vec![comma, space, NodeOrToken::Node(new_val)],
        );
    } else {
        // Empty array: insert before the closing bracket.
        let be = els
            .iter()
            .position(|e| matches!(e, NodeOrToken::Token(t) if t.kind() == SyntaxKind::BRACKET_END))
            .ok_or(MutateError::Unsupported)?;
        arr.splice_children(be..be, vec![NodeOrToken::Node(new_val)]);
    }
    Ok(())
}

/// Insert a keyed `ENTRY` into the inline table at `table_path`, at member `index`
/// (or appended). taplo bakes the closing `}`'s leading whitespace into the last
/// entry, so token surgery is brittle — instead the table is rebuilt from its
/// members' verbatim source with normalized `, ` separators (`{ … }` padding), each
/// existing member kept byte-for-byte. An empty `{}` becomes `{ k = v }`.
pub(crate) fn inline_table_insert(
    idx: &CstIndex,
    table_path: &[Seg],
    index: usize,
    frag: &SyntaxNode,
) -> Result<(), MutateError> {
    // The inline table is either a keyed entry's value (`t = { … }`) or an array
    // **element** (`x = [{ … }]`, a `Target::ArrayElement` whose own node is the
    // `VALUE`) — both reach their `INLINE_TABLE` through `struct_node`.
    let it = match idx.iter().find(|(p, _)| p == table_path).map(|(_, t)| t) {
        Some(Target::Entry(entry)) => entry
            .children()
            .find(|c| c.kind() == SyntaxKind::VALUE)
            .and_then(|v| struct_node(&v))
            .filter(|n| n.kind() == SyntaxKind::INLINE_TABLE)
            .ok_or(MutateError::Unsupported)?,
        Some(Target::ArrayElement(value)) => struct_node(value)
            .filter(|n| n.kind() == SyntaxKind::INLINE_TABLE)
            .ok_or(MutateError::Unsupported)?,
        _ => return Err(MutateError::Unsupported),
    };
    let new_entry = frag
        .descendants()
        .find(|n| n.kind() == SyntaxKind::ENTRY)
        .ok_or_else(|| MutateError::Fragment("fragment has no entry".into()))?;

    let mut texts: Vec<String> = it
        .children()
        .filter(|c| c.kind() == SyntaxKind::ENTRY)
        .map(|e| e.to_string().trim().to_string())
        .collect();
    let new_text = new_entry.to_string().trim().to_string();
    if index < texts.len() {
        texts.insert(index, new_text);
    } else {
        texts.push(new_text);
    }
    let built = format!("__v__ = {{ {} }}\n", texts.join(", "));
    let parse = taplo::parser::parse(&built);
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    let new_it = parse
        .into_syntax()
        .clone_for_update()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::INLINE_TABLE)
        .ok_or(MutateError::Unsupported)?;
    new_it.detach();
    let value = it.parent().ok_or(MutateError::Unsupported)?;
    let i = it.index();
    value.splice_children(i..i + 1, vec![NodeOrToken::Node(new_it)]);
    Ok(())
}

/// A fresh detached `,` + ` ` pair for array separators (parsed from a sample).
pub(crate) fn array_sep() -> (taplo::syntax::SyntaxElement, taplo::syntax::SyntaxElement) {
    let frag = taplo::parser::parse("x = [0, 0]\n")
        .into_syntax()
        .clone_for_update();
    let arr = frag
        .descendants()
        .find(|n| n.kind() == SyntaxKind::ARRAY)
        .expect("sample array");
    let comma = arr
        .children_with_tokens()
        .find(|c| matches!(c, NodeOrToken::Token(t) if t.kind() == SyntaxKind::COMMA))
        .expect("comma");
    let space = arr
        .children_with_tokens()
        .find(|c| matches!(c, NodeOrToken::Token(t) if t.kind() == SyntaxKind::WHITESPACE))
        .expect("space");
    comma.detach();
    space.detach();
    (comma, space)
}

/// Swap the **last** key-segment token of a node fragment to `new_seg` (`a.b` →
/// `a.b_2`), used to de-collide an entry on `OnCollision::Rename` (for a bare key the
/// last segment is the only one).
pub(crate) fn rewrite_last_key(frag: &SyntaxNode, new_seg: &str) -> Result<(), MutateError> {
    let key = frag
        .descendants()
        .find(|n| n.kind() == SyntaxKind::KEY)
        .ok_or_else(|| MutateError::Fragment("fragment has no key".into()))?;
    let last = key
        .children_with_tokens()
        .filter_map(key_seg_token)
        .last()
        .ok_or_else(|| MutateError::Fragment("fragment key has no segment".into()))?;
    let parse = taplo::parser::parse(&format!("{new_seg} = 0\n"));
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    let nk = parse
        .into_syntax()
        .clone_for_update()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::KEY)
        .and_then(|k| k.children_with_tokens().find_map(key_seg_token))
        .ok_or_else(|| MutateError::Fragment("invalid key".into()))?;
    nk.detach();
    let i = last.index();
    key.splice_children(i..i + 1, vec![NodeOrToken::Token(nk)]);
    Ok(())
}

/// Prefix the fragment's (single-segment) key with a dotted ancestor path, so an
/// insert into a synthetic `[T/D]` table is written as a dotted entry: `x = v`
/// with prefix `[a, b]` becomes `a.b.x = v`. Replaces the whole `KEY` node,
/// preserving the original final segment's source (quoting intact); non-bare
/// prefix segments are re-quoted.
pub(crate) fn prefix_entry_key(frag: &SyntaxNode, prefix: &[String]) -> Result<(), MutateError> {
    let key = frag
        .descendants()
        .find(|n| n.kind() == SyntaxKind::KEY)
        .ok_or_else(|| MutateError::Fragment("fragment has no key".into()))?;
    let joined = prefix
        .iter()
        .map(|s| quote_key_seg(s))
        .collect::<Vec<_>>()
        .join(".");
    // Borrow correctly-tokenized `<prefix>.` segments (idents + dots) from a
    // throwaway parse, then splice them in front of the original key — preserving
    // the original final segment's tokens (and the entry's spacing) verbatim.
    let parsed = taplo::parser::parse(&format!("{joined}.__seg__ = 0\n"));
    if let Some(e) = parsed.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    let pkey = parsed
        .into_syntax()
        .clone_for_update()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::KEY)
        .ok_or_else(|| MutateError::Fragment("invalid key".into()))?;
    let toks: Vec<_> = pkey.children_with_tokens().collect();
    let last = toks
        .iter()
        .rposition(|c| matches!(c, NodeOrToken::Token(t) if is_key_seg(t.kind())))
        .ok_or_else(|| MutateError::Fragment("invalid key".into()))?;
    let prefix_tokens = &toks[..last];
    for e in prefix_tokens {
        e.detach();
    }
    key.splice_children(0..0, prefix_tokens.to_vec());
    Ok(())
}

/// Prefix every `[table]`/`[[aot]]` header in a section fragment with `prefix`, so a
/// `[T/S]` scope table moved into another scope nests: `[a]` (with a nested `[a.sub]`)
/// dropped under `[b]` becomes `[b.a]` (and `[b.a.sub]`). Mirrors `prefix_entry_key`'s
/// front-splice, applied to each header's `KEY` (a fresh token copy per header, since a
/// token can only live in one tree).
pub(crate) fn prefix_section_headers(
    frag: &SyntaxNode,
    prefix: &[String],
) -> Result<(), MutateError> {
    if prefix.is_empty() {
        return Ok(());
    }
    let joined = prefix
        .iter()
        .map(|s| quote_key_seg(s))
        .collect::<Vec<_>>()
        .join(".");
    let headers: Vec<SyntaxNode> = frag
        .descendants()
        .filter(|n| {
            matches!(
                n.kind(),
                SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER
            )
        })
        .collect();
    for h in headers {
        let key = h
            .children()
            .find(|c| c.kind() == SyntaxKind::KEY)
            .ok_or_else(|| MutateError::Fragment("header has no key".into()))?;
        let parsed = taplo::parser::parse(&format!("{joined}.__seg__ = 0\n"));
        if let Some(e) = parsed.errors.first() {
            return Err(MutateError::Fragment(e.to_string()));
        }
        let pkey = parsed
            .into_syntax()
            .clone_for_update()
            .descendants()
            .find(|n| n.kind() == SyntaxKind::KEY)
            .ok_or_else(|| MutateError::Fragment("invalid key".into()))?;
        let toks: Vec<_> = pkey.children_with_tokens().collect();
        let last = toks
            .iter()
            .rposition(|c| matches!(c, NodeOrToken::Token(t) if is_key_seg(t.kind())))
            .ok_or_else(|| MutateError::Fragment("invalid key".into()))?;
        let prefix_tokens: Vec<_> = toks[..last].to_vec();
        for e in &prefix_tokens {
            e.detach();
        }
        key.splice_children(0..0, prefix_tokens);
    }
    Ok(())
}

/// A key segment as written in source: bare if it is a legal bare key, else a
/// basic-quoted string.
pub(crate) fn quote_key_seg(s: &str) -> String {
    let bare = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if bare {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// Move `sources` to `target`, atomically (the caller commits the clone only on
/// success). Comments are independent CST nodes, so a move repositions only the
/// named nodes — adjacent comments stay put with no special handling. Entry,
/// `[table]` and **array-element** sources are supported. An AoT-entry source
/// (`Target::AotEntry`) splits into dotted member fragments for a
/// table/root/plain-array destination; into another `[A/T]` group it moves
/// atomically (nested sections preserved) — a nested `[[…]]` sub-group has
/// neither form (`Unsupported`, ADR 0004 §3).
pub(crate) fn move_nodes(
    tree: &SyntaxNode,
    sources: &[Vec<Seg>],
    target: &InsTarget,
    on_collision: OnCollision,
) -> Result<(), MutateError> {
    let (proj, idx) = walk(tree, "");

    // Destination kind, computed up front (before the capture loop) since
    // `Target::AotEntry` sources need to know it to decide between flattening
    // and the atomic reconstruction below (ADR 0004 §3) — `target.parent`'s
    // kind doesn't depend on `frags`. Only a real `[A/T]` group can host the
    // atomic form: a plain array's elements are inline values and cannot
    // carry `[table]` headers, so a plain-array destination keeps the flatten
    // + pack-into-`{ … }` path below.
    let dest_kind = node_at(&proj.root, &target.parent).map(|n| &n.kind);
    let dest_is_aot = matches!(dest_kind, Some(NodeKind::ArrayOfTables));
    let dest_packs = matches!(dest_kind, Some(NodeKind::ArrayOfTables | NodeKind::Array));

    // Capture each source's source text before any removal. Each fragment
    // carries its *suggested* synthesized key alongside the text (`Some` only
    // for a bare scalar pulled out of a keyed array — `<arrayKey>_<index>`;
    // every other source already carries its own key, so the generic
    // placeholder remains the fallback).
    let mut frags: Vec<(String, Option<String>)> = Vec::new();
    // Indices into `frags` whose text is a composite AoT-entry body carrying
    // its own nested `[table]` headers (ADR 0004 §3) — these splice via
    // `aot_group_insert` directly, bypassing `insert`'s generic
    // `has_header -> Illegal` gate (correct for a *bare* section paste, wrong
    // here: this is an entry's *own* body, whose sub-section headers belong
    // inside the destination group). Stays valid across the `dest_packs`-join
    // below: a composite always contains a header, so it always fails
    // `joinable_entry` and is never touched by that join.
    let mut aot_composite_idxs: HashSet<usize> = HashSet::new();
    for p in sources {
        // A table — `[T/D]`, `[T/S]` (scattered or not), implicit, or mixed — is an
        // open set of member spans: capture them all, scope-relative (entry keys
        // drop the headerless-ancestor prefix, headers drop the ancestor path), so
        // the re-insert re-prefixes only for the destination. A pure `[T/D]` table
        // fans out to one fragment per member line so the per-leaf collision check
        // applies; a sectioned capture stays one fragment (its entries belong under
        // their headers). The source side is removed by `delete`, which fans out
        // over the same spans.
        if node_at(&proj.root, p).is_some_and(|n| matches!(n.kind, NodeKind::Table))
            && matches!(p.last(), Some(Seg::Key(_)))
        {
            let spans = table_member_spans(tree, &idx, p);
            if spans.iter().any(|s| matches!(s, MemberSpan::Section(_))) {
                if let Some(text) = table_fragment(tree, &idx, &proj.root, p, true) {
                    frags.push((text, None));
                    continue;
                }
            } else if !spans.is_empty() {
                let strip = dotted_ancestor_prefix_len(&idx, &proj.root, p);
                for s in &spans {
                    if let MemberSpan::Entry(m) = s {
                        frags.push((strip_key_prefix(m, strip), None));
                    }
                }
                continue;
            } else if let Some(inline_len) = inline_ancestor_len(&proj.root, p) {
                // A synthetic `[T/D]` table *inside an inline table* fans out to
                // its `{ … }` member entries, captured scope-relative (drop the
                // segments between the inline table and the node, keep its own
                // key) — the source side is removed by `delete`'s inline fan-out.
                let members = inline_member_entries(&idx, p);
                if !members.is_empty() {
                    let strip = p.len() - 1 - inline_len;
                    for m in &members {
                        frags.push((format!("{}\n", strip_key_prefix(m, strip).trim()), None));
                    }
                    continue;
                }
            }
        }
        let t = match idx.iter().find(|(ip, _)| ip == p).map(|(_, t)| t.clone()) {
            Some(t) => t,
            None => return Err(MutateError::NotFound),
        };
        match t {
            // Scope-relative capture: drop the source's dotted-ancestor prefix so the
            // re-insert re-prefixes only for the destination (matching copy/paste).
            Target::Entry(n) => {
                let strip = dotted_ancestor_prefix_len(&idx, &proj.root, p);
                frags.push((strip_key_prefix(&n, strip), None));
            }
            Target::Header(h) => frags.push((section_text(tree, p, h.index(), false), None)),
            // Moving an array element out: into another array it stays a bare element;
            // into a table/root an inline table `{ k = v, … }` **unpacks** into its
            // member entries (keys preserved, one node each — the per-leaf collision
            // check applies), anything else gets a synthesized key on insert —
            // `<arrayKey>_<index>` for a keyed source array (below), else the
            // generic `placeholder`. The destination format is then applied by
            // `insert` (dotted prefix, inline-table splice, …).
            Target::ArrayElement(value) => {
                let text = value.to_string();
                let dest_is_array = node_at(&proj.root, &target.parent)
                    .map(|n| matches!(n.kind, crate::model::node::NodeKind::Array))
                    .unwrap_or(false);
                let suggested = crate::model::node::array_element_suggested_key(p);
                match (dest_is_array, unpack_inline_table(&text)) {
                    // Unpacked `{ k = v }` members carry their own keys.
                    (false, Some(entries)) => frags.extend(entries.into_iter().map(|e| (e, None))),
                    _ => frags.push((format!("{}\n", text.trim()), suggested)),
                }
            }
            // Moving a `[[…]]` entry out of its array: into a table/root/plain
            // array it still splits into member nodes (unchanged, `[T/D]`-parity
            // — one fragment per line, sub-sections flattened to dotted; a plain
            // array then packs them into ONE `{ … }` element below, the only
            // lossless form an inline element can take). Into another `[A/T]`
            // group it now moves *atomically* instead (ADR 0004 §3): the body
            // keeps its nested `[table]` sub-sections as relative headers
            // (`aot_entry_section_body`) rather than flattening them, and the
            // composite branch below re-qualifies each header against the
            // destination before splicing it in as a new entry.
            Target::AotEntry(h) => {
                if dest_is_aot {
                    aot_composite_idxs.insert(frags.len());
                    frags.push((aot_entry_section_body(tree, &h)?, None));
                } else {
                    frags.extend(
                        aot_entry_member_fragments(tree, &h)?
                            .into_iter()
                            .map(|f| (f, None)),
                    );
                }
            }
            _ => return Err(MutateError::Unsupported),
        }
    }

    // Destination `[A/T]` group or plain array: several moved nodes pack into ONE
    // new `[[…]]` entry / `{ … }` element, so join the fragments when every one is
    // a header-less keyed entry (bare values / sections keep the per-fragment path
    // and its own handling). A composite AoT-entry body (above) always contains a
    // header, so it always fails `joinable_entry` and is never swept into this join
    // — it keeps its own slot, indices in `aot_composite_idxs` stay valid.
    let frags = if dest_packs && frags.len() > 1 && frags.iter().all(|f| joinable_entry(&f.0)) {
        // Packing only fires for array/AoT destinations, where no key is
        // synthesized at all — the joined fragment carries no suggested key.
        vec![(
            frags
                .iter()
                .map(|f| format!("{}\n", f.0.trim_end()))
                .collect::<String>(),
            None,
        )]
    } else {
        frags
    };

    // Resolve a stable anchor — the first child at/after the target index that is
    // not itself a source *and not a comment* (a comment's positional path is not
    // stable across the source removals, so it can't be relocated by path) — to
    // insert before; its keyed path is stable. `None` means append.
    //
    // Because the anchor skips comment slots, comments sitting between
    // `target.index` and the anchor would otherwise be jumped over (the insert
    // landing *after* a trailing comment instead of at the requested slot). Count
    // those non-source comment slots as `gap` and subtract it from the relocated
    // anchor position so the insert lands at the intended ordinal.
    let parent = node_at(&proj.root, &target.parent).ok_or(MutateError::NotFound)?;
    let anchor_orig = parent
        .children
        .iter()
        .enumerate()
        .skip(target.index)
        .find(|(_, c)| {
            !matches!(c.kind, crate::model::node::NodeKind::Comment(_))
                && !sources.contains(&c.path)
        });
    let anchor_path: Option<Vec<Seg>> = anchor_orig.map(|(_, c)| c.path.clone());
    let anchor_end = anchor_orig.map_or(parent.children.len(), |(i, _)| i);
    let gap = parent.children[target.index.min(parent.children.len())..anchor_end]
        .iter()
        .filter(|c| !sources.contains(&c.path))
        .count();

    // Delete sources (longest path first keeps shallower paths valid; among
    // same-length array-index siblings, highest index first — otherwise
    // deleting a lower index shifts the not-yet-deleted higher indices out
    // from under their still-stale paths, e.g. `NotFound` once the array's
    // last element is part of the selection — mirrors the same fix already
    // in the JSON/YAML `move_nodes`).
    let mut ordered: Vec<&Vec<Seg>> = sources.iter().collect();
    ordered.sort_by(|a, b| {
        if a.len() == b.len() {
            match (a.last(), b.last()) {
                (Some(Seg::Index(ia)), Some(Seg::Index(ib))) => ib.cmp(ia),
                _ => std::cmp::Ordering::Equal,
            }
        } else {
            b.len().cmp(&a.len())
        }
    });
    for p in ordered {
        delete(tree, p)?;
    }

    // Re-insert before the anchor's current position (or append), in order.
    for (i, (frag, frag_key)) in frags.into_iter().enumerate() {
        let (proj2, idx2) = walk(tree, "");
        let parent2 = node_at(&proj2.root, &target.parent).ok_or(MutateError::NotFound)?;
        let index = {
            let base = match &anchor_path {
                Some(ap) => parent2
                    .children
                    .iter()
                    .position(|c| &c.path == ap)
                    .unwrap_or(parent2.children.len()),
                None => parent2.children.len(),
            };
            base - gap.min(base)
        };
        if aot_composite_idxs.contains(&i) {
            let parsed = taplo::parser::parse(&frag);
            if let Some(e) = parsed.errors.first() {
                return Err(MutateError::Fragment(e.to_string()));
            }
            let node = parsed.into_syntax().clone_for_update();
            // Re-qualify the body's *relative* sub-section headers against the
            // destination (`[physical]` → `[items.physical]`): TOML headers are
            // absolute, so splicing them verbatim after the new `[[…]]` header
            // would strand each sub-section at the top level instead of nesting
            // it inside the moved entry.
            let prefix: Vec<String> = target
                .parent
                .iter()
                .filter_map(|s| match s {
                    Seg::Key(k) => Some(k.clone()),
                    _ => None,
                })
                .collect();
            prefix_section_headers(&node, &prefix)?;
            aot_group_insert(
                tree,
                &idx2,
                parent2,
                &target.parent,
                index,
                &node,
                on_collision,
            )?;
        } else {
            insert_with(
                tree,
                &proj2,
                &idx2,
                &InsTarget {
                    parent: target.parent.clone(),
                    index,
                },
                &frag,
                on_collision,
                frag_key.as_deref(),
            )?;
        }
    }
    Ok(())
}
