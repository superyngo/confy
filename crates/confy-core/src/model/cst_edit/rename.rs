//! `Mutation::Rename` — key renaming across entries, headers, and AoT
//! groups — split out of `cst_edit.rs` (Task 15, 2026-08-11 audit
//! remediation).

use crate::model::cst_project::{walk, Target};
use crate::model::document::MutateError;
use crate::model::node::Seg;
use taplo::rowan::NodeOrToken;
use taplo::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use super::dotted_table::{rename_dotted_segment};
use super::tree_nav::{find_parent};

/// Rename the key at `path` to `new_key`, swapping the relevant segment token(s)
/// in place (position/decor preserved). Handles all node types:
///
/// - Entry/Header/AotEntry: renames the last key segment AND propagates the same
///   segment rename to all sub-scope headers under `path` (e.g. renaming
///   `[product_table]` also fixes `[product_table.a]`, `[product_table.b]`).
/// - AotGroup: renames ALL `[[group]]` headers + any nested sub-scope headers.
/// - Path not in index but has sub-scope headers (implicit scope table): renames
///   the segment in all those headers.
/// - Path not in index and no sub-scope headers (synthetic `[T/D]` intermediate):
///   renames the segment at `path.len()-1` in all member dotted-key entries.
pub(crate) fn rename(tree: &SyntaxNode, path: &[Seg], new_key: &str) -> Result<(), MutateError> {
    // Validate the new key fragment up front (shared by all branches).
    let parse = taplo::parser::parse(&format!("{new_key} = 0\n"));
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }

    let (proj, idx) = walk(tree, "");
    let maybe_target = idx.iter().find(|(p, _)| p == path).map(|(_, t)| t.clone());

    // The segment position to rename within each key's token list.
    // For a node at path depth N, the segment at index N-1 is the "own" segment.
    // This is also the segment that sub-scope headers share as a prefix.
    let seg_pos = path.len().saturating_sub(1);

    // All [section] headers anywhere under `path` (may be sub-tables or nested
    // scope tables inside AoT entries). These always need the same segment renamed
    // whenever the owning path is renamed.
    let sub_scope_headers: Vec<SyntaxNode> = idx
        .iter()
        .filter_map(|(p, t)| {
            if p.len() > path.len() && p[..path.len()] == *path {
                if let Target::Header(n) = t {
                    Some(n.clone())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    match maybe_target {
        // Concrete entry/header/AoT entry: rename its last key segment.
        // Also propagate to all sub-scope headers that share the prefix.
        Some(
            ref target @ (Target::Entry(ref n) | Target::Header(ref n) | Target::AotEntry(ref n)),
        ) => {
            let key_node = n
                .children()
                .find(|c| c.kind() == SyntaxKind::KEY)
                .ok_or(MutateError::NotFound)?;
            // An entry's KEY spells only its own (possibly dotted) tail of the
            // path — a scoped entry omits its `[section]` prefix — so its token
            // index is end-relative; header KEYs spell the absolute path.
            let own_idx = if matches!(target, Target::Entry(_)) {
                entry_seg_idx(&key_node, path.len(), seg_pos).ok_or(MutateError::NotFound)?
            } else {
                header_seg_idx(path, seg_pos)
            };

            // Collision check on the direct parent.
            if let Some((parent, node)) = find_parent(&proj.root, path) {
                let mut segs: Vec<&str> = node.key.split('.').collect();
                if let Some(last) = segs.last_mut() {
                    *last = new_key;
                }
                let new_display = segs.join(".");
                if parent.children.iter().any(|c| {
                    !matches!(c.kind, crate::model::node::NodeKind::Comment(_))
                        && c.path != *path
                        && c.key == new_display
                }) {
                    return Err(MutateError::Collision(new_key.to_string()));
                }
            }

            // Rename this node's key segment (last for own node).
            rename_key_seg_at_pos(key_node, own_idx, new_key)?;

            // Rename the same segment in all sub-scope headers.
            let sub_idx = header_seg_idx(path, seg_pos);
            for sub in &sub_scope_headers {
                let kn = sub
                    .children()
                    .find(|c| c.kind() == SyntaxKind::KEY)
                    .ok_or(MutateError::NotFound)?;
                rename_key_seg_at_pos(kn, sub_idx, new_key)?;
            }
            Ok(())
        }

        // AoT group: rename ALL `[[group]]` entry headers + any nested sub-scope headers.
        Some(Target::AotGroup) => {
            // Collect the AoT entry headers.
            let entry_nodes: Vec<SyntaxNode> = idx
                .iter()
                .filter_map(|(p, t)| {
                    if p.len() == path.len() + 1
                        && p[..path.len()] == *path
                        && matches!(p.last(), Some(Seg::Index(_)))
                    {
                        if let Target::AotEntry(n) = t {
                            Some(n.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect();

            if entry_nodes.is_empty() && sub_scope_headers.is_empty() {
                return Err(MutateError::NotFound);
            }

            // Collision check at the sibling level.
            if let Some((parent, _)) = find_parent(&proj.root, path) {
                if parent.children.iter().any(|c| {
                    !matches!(c.kind, crate::model::node::NodeKind::Comment(_))
                        && c.path != *path
                        && c.key == new_key
                }) {
                    return Err(MutateError::Collision(new_key.to_string()));
                }
            }

            // Rename each [[group]] entry header.
            let hdr_idx = header_seg_idx(path, seg_pos);
            for entry_node in &entry_nodes {
                let kn = entry_node
                    .children()
                    .find(|c| c.kind() == SyntaxKind::KEY)
                    .ok_or(MutateError::NotFound)?;
                rename_key_seg_at_pos(kn, hdr_idx, new_key)?;
            }
            // Rename the same segment in nested sub-scope headers.
            for sub in &sub_scope_headers {
                let kn = sub
                    .children()
                    .find(|c| c.kind() == SyntaxKind::KEY)
                    .ok_or(MutateError::NotFound)?;
                rename_key_seg_at_pos(kn, hdr_idx, new_key)?;
            }
            Ok(())
        }

        // Path not in index: implicit scope table (only sub-headers, no own [header])
        // OR a synthetic [T/D] intermediate table (only dotted member entries).
        None => {
            if !sub_scope_headers.is_empty() {
                // Implicit scope table: rename the segment in all sub-headers.
                let hdr_idx = header_seg_idx(path, seg_pos);
                for sub in &sub_scope_headers {
                    let kn = sub
                        .children()
                        .find(|c| c.kind() == SyntaxKind::KEY)
                        .ok_or(MutateError::NotFound)?;
                    rename_key_seg_at_pos(kn, hdr_idx, new_key)?;
                }
                Ok(())
            } else {
                // Synthetic [T/D] table: rename segment in all dotted member entries.
                rename_dotted_segment(tree, &idx, path, new_key)
            }
        }

        Some(_) => Err(MutateError::Unsupported),
    }
}

/// Token index of the absolute path segment `seg_pos` within an *entry* KEY.
/// An entry's key spells only the last `k` segments of its own path (a scoped
/// entry omits its `[section]` prefix), so the index is end-relative. `None`
/// when `seg_pos` falls outside the segments the key actually spells.
pub(crate) fn entry_seg_idx(key_node: &SyntaxNode, owner_len: usize, seg_pos: usize) -> Option<usize> {
    let k = key_node
        .children_with_tokens()
        .filter(|c| matches!(c, NodeOrToken::Token(t) if is_key_seg(t.kind())))
        .count();
    (seg_pos + k).checked_sub(owner_len).filter(|i| *i < k)
}

/// Token index of the absolute path segment `seg_pos` within a `[section]` /
/// `[[group]]` header KEY: headers spell the full path from root with
/// `Seg::Index` positions (AoT entry slots) dropped.
pub(crate) fn header_seg_idx(path: &[Seg], seg_pos: usize) -> usize {
    path[..seg_pos]
        .iter()
        .filter(|s| matches!(s, Seg::Key(_)))
        .count()
}

/// Replace the key segment at `seg_pos` (0-indexed) in `key_node` with fresh
/// tokens built from `new_seg`. Used by all rename paths.
/// A fresh parse is required per call because rowan tokens cannot be reused.
pub(crate) fn rename_key_seg_at_pos(
    key_node: SyntaxNode,
    seg_pos: usize,
    new_seg: &str,
) -> Result<(), MutateError> {
    // Find the token at the target segment position.
    let seg_tokens: Vec<SyntaxToken> = key_node
        .children_with_tokens()
        .filter_map(|c| c.into_token().filter(|t| is_key_seg(t.kind())))
        .collect();
    let old_tok = seg_tokens.get(seg_pos).ok_or(MutateError::NotFound)?;
    let tok_idx = old_tok.index();

    // Build replacement tokens from a fresh parse.
    let nk_root = taplo::parser::parse(&format!("{new_seg} = 0\n"))
        .into_syntax()
        .clone_for_update();
    let mut nk_tokens: Vec<_> = nk_root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::KEY)
        .ok_or_else(|| MutateError::Fragment("invalid key".into()))?
        .children_with_tokens()
        .collect();
    let last_seg_idx = nk_tokens
        .iter()
        .rposition(|c| matches!(c, NodeOrToken::Token(t) if is_key_seg(t.kind())))
        .ok_or_else(|| MutateError::Fragment("invalid key".into()))?;
    nk_tokens.truncate(last_seg_idx + 1);

    for t in &nk_tokens {
        t.detach();
    }
    key_node.splice_children(tok_idx..tok_idx + 1, nk_tokens);
    Ok(())
}

pub(crate) fn is_key_seg(k: SyntaxKind) -> bool {
    matches!(
        k,
        SyntaxKind::IDENT
            | SyntaxKind::IDENT_WITH_GLOB
            | SyntaxKind::STRING
            | SyntaxKind::STRING_LITERAL
    )
}

pub(crate) fn key_seg_token(c: taplo::syntax::SyntaxElement) -> Option<SyntaxToken> {
    c.into_token().filter(|t| is_key_seg(t.kind()))
}
