//! Synthetic `[T/D]` dotted-table helpers: member enumeration, header
//! detection, and whole-table `$EDITOR` block replace — split out of
//! `cst_edit.rs` (Task 15, 2026-08-11 audit remediation).

use crate::model::cst_project::{walk, CstIndex, Target};
use crate::model::document::MutateError;
use crate::model::node::{Node, NodeKind, Seg};
use taplo::rowan::NodeOrToken;
use taplo::syntax::{SyntaxKind, SyntaxNode};
use super::move_paste::{inline_table_insert};
use super::rename::{entry_seg_idx, is_key_seg, rename_key_seg_at_pos};
use super::replace_delete::{delete_seq_element, detach_entry_line};
use super::tree_nav::{node_at};

/// Replace a scalar value in place (inline value edit). `toml` is a `key = <value>`
/// fragment (array elements use a synthetic `__elem__ = <value>`); only the scalar
/// token is swapped, so a trailing EOL comment and any surrounding array indent are
/// preserved.
/// Every flat-ROOT `ENTRY` element belonging to the dotted table at `path` (paths
/// strictly under it), in document order. Shared by the `[T/D]` block edit, delete
/// fan-out, and fragment serialization.
pub(crate) fn dotted_member_entries(idx: &CstIndex, path: &[Seg]) -> Vec<SyntaxNode> {
    let mut v: Vec<(usize, SyntaxNode)> = idx
        .iter()
        .filter_map(|(p, t)| match t {
            // Only *flat-root* entries: an entry nested inside an inline-table (or
            // array) value belongs to that value, not to the dotted table — skip it
            // so a `new_field = {x=1}` member never has its inner `x=1` pulled out.
            Target::Entry(n)
                if p.len() > path.len()
                    && p[..path.len()] == *path
                    && !n.ancestors().skip(1).any(|a| {
                        matches!(a.kind(), SyntaxKind::INLINE_TABLE | SyntaxKind::ARRAY)
                    }) =>
            {
                Some((n.index(), n.clone()))
            }
            _ => None,
        })
        .collect();
    v.sort_by_key(|(i, _)| *i);
    v.into_iter().map(|(_, n)| n).collect()
}

/// The prefix length of the **nearest inline-table ancestor** of `path` (the
/// largest `i < path.len()` whose node is an `InlineTable`), if any. A synthetic
/// `[T/D]` table whose members are dotted keys *inside* a `{ … }` has one — the
/// flat-ROOT machinery must not reach through it, so such paths route to the
/// inline-table helpers instead.
pub(crate) fn inline_ancestor_len(root: &Node, path: &[Seg]) -> Option<usize> {
    (1..path.len()).rev().find(|&i| {
        node_at(root, &path[..i]).is_some_and(|n| matches!(n.kind, NodeKind::InlineTable))
    })
}

/// The member `ENTRY`s of a synthetic `[T/D]` table nested inside an inline table:
/// every indexed entry strictly under `path`, in source order, skipping entries
/// that live inside another member's *value* (they belong to that member).
pub(crate) fn inline_member_entries(idx: &CstIndex, path: &[Seg]) -> Vec<SyntaxNode> {
    let mut v: Vec<SyntaxNode> = idx
        .iter()
        .filter_map(|(p, t)| match t {
            Target::Entry(n) if p.len() > path.len() && p[..path.len()] == *path => Some(n.clone()),
            _ => None,
        })
        .collect();
    v.sort_by_key(|n| n.text_range().start());
    let mut out: Vec<SyntaxNode> = Vec::new();
    for n in v {
        if !out.iter().any(|m| n.ancestors().skip(1).any(|a| &a == m)) {
            out.push(n);
        }
    }
    out
}

/// Whether the projected node at `path` has its own `[…]` header in the source.
/// A *headerless* table — a `[T/D]` dotted table, an implicit scope (only
/// `[a.sub]` was written), or the dotted side of a mixed table — opens no scope
/// of its own; its key segments live in its member lines instead.
pub(crate) fn has_own_header(idx: &CstIndex, path: &[Seg]) -> bool {
    idx.iter()
        .any(|(p, t)| p == path && matches!(t, Target::Header(_)))
}

/// Whether the node at `path` is a **headerless table**: a real `Table` projection
/// node, keyed (not an AoT entry), with no own `[…]` header. Such a table's key
/// prefix is carried by its member entries, so captures strip it and inserts
/// re-add it.
pub(crate) fn is_headerless_table(idx: &CstIndex, root: &Node, path: &[Seg]) -> bool {
    matches!(path.last(), Some(Seg::Key(_)))
        && node_at(root, path).is_some_and(|n| matches!(n.kind, NodeKind::Table))
        && !has_own_header(idx, path)
}

/// The number of contiguous **headerless-table proper ancestors** above the node
/// at `path` (counted from the deepest up, stopping at the first real scope).
/// This is exactly the count of leading key segments a copied fragment must drop
/// to become scope-relative: a `dotted.test.bool_true` leaf yields `2`, the
/// `test` subtable yields `1`.
pub(crate) fn dotted_ancestor_prefix_len(idx: &CstIndex, root: &Node, path: &[Seg]) -> usize {
    let mut count = 0;
    for k in (1..path.len()).rev() {
        if is_headerless_table(idx, root, &path[..k]) {
            count += 1;
        } else {
            break;
        }
    }
    count
}

/// The source text of `entry` with the first `strip` key segments (and the dots
/// that separate them) dropped from its `KEY` — so `dotted.test.bool_true = true`
/// with `strip = 2` renders `bool_true = true`. `strip == 0` is the entry verbatim.
pub(crate) fn strip_key_prefix(entry: &SyntaxNode, strip: usize) -> String {
    let full = entry.to_string();
    if strip == 0 {
        return full;
    }
    let key = match entry.children().find(|c| c.kind() == SyntaxKind::KEY) {
        Some(k) => k,
        None => return full,
    };
    let old_key = key.to_string();
    // The ENTRY begins with its KEY token text, so the new key text plus the rest of
    // the entry (the ` = value …` tail) reproduces a scope-relative line.
    let mut new_key = String::new();
    let mut seen_segs = 0usize;
    let mut started = false;
    for c in key.children_with_tokens() {
        if let NodeOrToken::Token(t) = &c {
            if is_key_seg(t.kind()) {
                seen_segs += 1;
                if seen_segs > strip {
                    started = true;
                }
            }
            if started {
                new_key.push_str(t.text());
            }
        }
    }
    format!("{new_key}{}", &full[old_key.len()..])
}

/// Block-rewrite a `[T/D]` dotted table (`$EDITOR` on the table): remove all of its
/// member entries and splice the edited block in at the **first** member's position
/// (the consolidation the user opted into; the table projects at its first
/// definition). Scattered members are gathered; any standalone comments between
/// them stay put.
pub(crate) fn replace_dotted_table(
    tree: &SyntaxNode,
    idx: &CstIndex,
    path: &[Seg],
    toml: &str,
) -> Result<(), MutateError> {
    let members = dotted_member_entries(idx, path);
    let first = members.first().ok_or(MutateError::NotFound)?.clone();
    let parse = taplo::parser::parse(toml);
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    let frag = parse.into_syntax().clone_for_update();
    let els: Vec<_> = frag.children_with_tokens().collect();
    for e in &els {
        e.detach();
    }
    // Remove the non-first members (whole lines); `detach` is position-independent.
    for m in &members[1..] {
        detach_entry_line(m);
    }
    // Replace the first member's slot (line) with the edited block.
    let i = first.index();
    let end = match first.next_sibling_or_token() {
        Some(NodeOrToken::Token(t)) if t.kind() == SyntaxKind::NEWLINE => i + 2,
        _ => i + 1,
    };
    tree.splice_children(i..end, els);
    Ok(())
}

/// Block-rewrite a synthetic `[T/D]` table *inside an inline table*: remove every
/// member entry from the `{ … }` and splice the edited block's entries in at the
/// first member's slot — the inline mirror of `replace_dotted_table`. The block
/// keeps verbatim member keys (`x.y = 1`), must hold only single-line entries
/// (no `[…]` sections), and may drop or add members freely.
pub(crate) fn replace_inline_dotted_table(
    tree: &SyntaxNode,
    idx: &CstIndex,
    root: &Node,
    path: &[Seg],
    toml: &str,
) -> Result<(), MutateError> {
    let inline_len = inline_ancestor_len(root, path).ok_or(MutateError::NotFound)?;
    let members = inline_member_entries(idx, path);
    let first = members.first().ok_or(MutateError::NotFound)?.clone();
    let parse = taplo::parser::parse(toml);
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    let frag = parse.into_syntax();
    if frag.descendants().any(|n| {
        matches!(
            n.kind(),
            SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER
        )
    }) {
        return Err(MutateError::Illegal(
            "a table cannot live inside an inline table".into(),
        ));
    }
    let new_entries: Vec<String> = frag
        .children()
        .filter(|n| n.kind() == SyntaxKind::ENTRY)
        .map(|n| n.to_string().trim().to_string())
        .collect();
    if new_entries.iter().any(|e| e.contains('\n')) {
        return Err(MutateError::Fragment(
            "inline-table members must be single-line".into(),
        ));
    }
    // The landing slot: the first member's position among the `{ … }`'s entries.
    let table = first
        .parent()
        .filter(|p| p.kind() == SyntaxKind::INLINE_TABLE)
        .ok_or(MutateError::Unsupported)?;
    let base = table
        .children()
        .filter(|c| c.kind() == SyntaxKind::ENTRY)
        .position(|c| c == first)
        .ok_or(MutateError::Unsupported)?;
    for m in members.iter().rev() {
        if let Some(parent) = m.parent() {
            delete_seq_element(&parent, m.index());
        }
    }
    let inline_path = &path[..inline_len];
    for (k, etext) in new_entries.iter().enumerate() {
        let eparse = taplo::parser::parse(etext);
        if let Some(e) = eparse.errors.first() {
            return Err(MutateError::Fragment(e.to_string()));
        }
        let efrag = eparse.into_syntax().clone_for_update();
        let (_, idx2) = walk(tree, "");
        inline_table_insert(&idx2, inline_path, base + k, &efrag)?;
    }
    Ok(())
}

/// Rename the segment at position `path.len()-1` in all flat-ROOT member entries
/// of the synthetic `[T/D]` table at `path`.
pub(crate) fn rename_dotted_segment(
    _tree: &SyntaxNode,
    idx: &CstIndex,
    path: &[Seg],
    new_seg: &str,
) -> Result<(), MutateError> {
    if path.is_empty() {
        return Err(MutateError::Illegal("cannot rename root".into()));
    }
    // Validate: new_seg must be a valid TOML key.
    let parse = taplo::parser::parse(&format!("{new_seg} = 0\n"));
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }

    let seg_pos = path.len() - 1;
    let members = dotted_member_entries(idx, path);
    if members.is_empty() {
        return Err(MutateError::NotFound);
    }

    for entry_node in &members {
        // Each member key spells its own tail of the path (a scoped entry omits
        // its `[section]` prefix) — look its full path up to index end-relative.
        let owner_len = idx
            .iter()
            .find_map(|(p, t)| match t {
                Target::Entry(n) if n == entry_node => Some(p.len()),
                _ => None,
            })
            .ok_or(MutateError::NotFound)?;
        let key_node = entry_node
            .children()
            .find(|c| c.kind() == SyntaxKind::KEY)
            .ok_or(MutateError::NotFound)?;
        let at = entry_seg_idx(&key_node, owner_len, seg_pos).ok_or(MutateError::NotFound)?;
        rename_key_seg_at_pos(key_node, at, new_seg)?;
    }
    Ok(())
}
