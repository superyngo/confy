//! `[[array-of-tables]]` group span/insert/entry-extraction helpers — split
//! out of `cst_edit.rs` (Task 15, 2026-08-11 audit remediation).

use super::move_paste::rewrite_last_key;
use super::replace_delete::{path_key_display, section_end_strict};
use super::tree_nav::{element_root_index, fragment_key_segs};
use crate::model::cst_project::{header_path, CstIndex};
use crate::model::document::{MutateError, OnCollision};
use crate::model::node::{Node, Seg};
use taplo::rowan::NodeOrToken;
use taplo::syntax::{SyntaxKind, SyntaxNode};

/// The contiguous root-child span `[start, end)` covering every `[[x]]` entry of
/// the AoT group at `path`. `None` if the group's entries are interleaved with
/// other sections (so a single splice would touch foreign content) — the
/// whole-group serialize/replace then bails rather than corrupt.
pub(crate) fn aot_group_span(tree: &SyntaxNode, path: &[Seg]) -> Option<(usize, usize)> {
    let mut starts: Vec<usize> = tree
        .children_with_tokens()
        .enumerate()
        .filter_map(|(k, e)| match e {
            NodeOrToken::Node(n)
                if n.kind() == SyntaxKind::TABLE_ARRAY_HEADER && header_path(&n) == path =>
            {
                Some(k)
            }
            _ => None,
        })
        .collect();
    starts.sort_unstable();
    let first = *starts.first()?;
    // Contiguity: each entry's strict end must be exactly the next entry's start.
    for w in starts.windows(2) {
        if section_end_strict(tree, w[0]) != w[1] {
            return None;
        }
    }
    let end = section_end_strict(tree, *starts.last()?);
    Some((first, end))
}

/// Insert keyed entries into an `[A/T]` group as a **new `[[…]]` entry** at child
/// slot `index` (over the group's full child sequence — comments share the slot
/// space). The fragment's source becomes the entry body verbatim, so trailing
/// comments travel. Keys never collide with sibling entries (each `[[…]]` opens a
/// fresh namespace); duplicate keys *within* the body (several pasted nodes
/// sharing a key) follow `on_collision`: Cancel surfaces `Collision`, Rename
/// suffixes later duplicates, Overwrite drops the earlier occurrence.
pub(crate) fn aot_group_insert(
    tree: &SyntaxNode,
    idx: &CstIndex,
    group: &Node,
    group_path: &[Seg],
    index: usize,
    frag: &SyntaxNode,
    on_collision: OnCollision,
) -> Result<(), MutateError> {
    let entries: Vec<SyntaxNode> = frag
        .children()
        .filter(|n| n.kind() == SyntaxKind::ENTRY)
        .collect();
    if entries.is_empty() {
        return Err(MutateError::Fragment("fragment has no entries".into()));
    }
    let mut keys: Vec<String> = entries
        .iter()
        .map(|e| fragment_key_segs(e).join("."))
        .collect();
    let mut dropped: Vec<usize> = Vec::new();
    for i in 0..keys.len() {
        let dup = keys[..i]
            .iter()
            .enumerate()
            .position(|(j, k)| !dropped.contains(&j) && k == &keys[i]);
        let Some(j) = dup else { continue };
        match on_collision {
            OnCollision::Cancel => return Err(MutateError::Collision(keys[i].clone())),
            OnCollision::Overwrite => {
                remove_entry_line(frag, &entries[j]);
                dropped.push(j);
            }
            OnCollision::Rename => {
                let segs = fragment_key_segs(&entries[i]);
                let base = segs.last().cloned().unwrap_or_default();
                let mut n = 2;
                let new_last = loop {
                    let mut cand = segs.clone();
                    *cand.last_mut().unwrap() = format!("{base}_{n}");
                    if !keys.contains(&cand.join(".")) {
                        break cand;
                    }
                    n += 1;
                };
                rewrite_last_key(&entries[i], new_last.last().unwrap())?;
                keys[i] = new_last.join(".");
            }
        }
    }

    // Splice slot among the flat ROOT children: before the element backing the
    // first group child at/after `index`, else appended after the last entry's
    // section span.
    let at = match group
        .children
        .iter()
        .skip(index)
        .find_map(|c| element_root_index(idx, c))
    {
        Some(i) => i,
        None => {
            let last_header = group
                .children
                .iter()
                .filter_map(|c| element_root_index(idx, c))
                .max()
                .ok_or(MutateError::Unsupported)?;
            section_end_strict(tree, last_header)
        }
    };

    let body = frag.to_string();
    let text = format!("[[{}]]\n{}", path_key_display(group_path), body);
    let parse = taplo::parser::parse(&text);
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    let new = parse.into_syntax().clone_for_update();
    let els: Vec<_> = new.children_with_tokens().collect();
    for e in &els {
        e.detach();
    }
    tree.splice_children(at..at, els);
    Ok(())
}

/// Detach `entry` (a top-level `ENTRY` of `frag`) together with its trailing
/// `NEWLINE`, so the remaining body keeps clean lines.
pub(crate) fn remove_entry_line(frag: &SyntaxNode, entry: &SyntaxNode) {
    let i = entry.index();
    let els: Vec<_> = frag.children_with_tokens().collect();
    let end = if matches!(els.get(i + 1), Some(NodeOrToken::Token(t)) if t.kind() == SyntaxKind::NEWLINE)
    {
        i + 2
    } else {
        i + 1
    };
    frag.splice_children(i..end, Vec::new());
}

/// End (exclusive ROOT-child index) of the **full extent** of the `[[…]]` entry
/// at `header_idx`: its own strict section plus any following sub-sections under
/// the group path (`[fruit.physical]` after `[[fruit]]` belongs to that entry),
/// stopping at the group's next `[[…]]` entry or a foreign header.
pub(crate) fn aot_entry_end(tree: &SyntaxNode, group_path: &[Seg], header_idx: usize) -> usize {
    let els: Vec<_> = tree.children_with_tokens().collect();
    for (k, el) in els.iter().enumerate().skip(header_idx + 1) {
        if let NodeOrToken::Node(n) = el {
            if !matches!(
                n.kind(),
                SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER
            ) {
                continue;
            }
            let p = header_path(n);
            if p == group_path || !p.starts_with(group_path) {
                return k;
            }
        }
    }
    els.len()
}

/// One member event within a `[[…]]` entry's body span: a nested `[table]`
/// header (path relative to the entry's own group-path ancestor — empty
/// means "back at the entry's own top level") or a `key = value` entry
/// line's own source text. Shared by `aot_entry_member_fragments` (flattens
/// to dotted form for a table/root destination) and `aot_entry_section_body`
/// (keeps headers verbatim for an atomic AoT-group-destination move, ADR
/// 0004 §3) — the two output shapes each insert engine needs, over the
/// identical tree walk.
enum EntryEvent {
    Header(Vec<Seg>),
    Entry(String),
}

fn walk_aot_entry_body(
    tree: &SyntaxNode,
    header: &SyntaxNode,
) -> Result<Vec<EntryEvent>, MutateError> {
    let group_path = header_path(header);
    let i = header.index();
    let end = aot_entry_end(tree, &group_path, i);
    let els: Vec<_> = tree.children_with_tokens().collect();
    let mut events = Vec::new();
    for el in &els[i + 1..end] {
        if let NodeOrToken::Node(n) = el {
            match n.kind() {
                SyntaxKind::TABLE_ARRAY_HEADER => return Err(MutateError::Unsupported),
                SyntaxKind::TABLE_HEADER => {
                    events.push(EntryEvent::Header(
                        header_path(n)[group_path.len()..].to_vec(),
                    ));
                }
                SyntaxKind::ENTRY => {
                    events.push(EntryEvent::Entry(n.to_string().trim().to_string()));
                }
                _ => {}
            }
        }
    }
    Ok(events)
}

/// The member fragments of the `[[…]]` AoT entry backed by `header` — an `[A/T]`
/// group is equivalent to an array of inline tables, so moving/copying an entry
/// out of its array **splits it into member nodes**: the body `ENTRY` lines
/// verbatim (one fragment each), and every sub-section flattened to dotted
/// entries (`[fruit.physical]` + `color = "red"` → `physical.color = "red"`; the
/// prefix is the section's header path relative to the entry, deeper nesting the
/// same). `Err(Unsupported)` when the entry holds a nested `[[…]]` sub-group,
/// which has no dotted form.
pub(crate) fn aot_entry_member_fragments(
    tree: &SyntaxNode,
    header: &SyntaxNode,
) -> Result<Vec<String>, MutateError> {
    let mut prefix = String::new();
    let mut frags = Vec::new();
    for ev in walk_aot_entry_body(tree, header)? {
        match ev {
            EntryEvent::Header(rel) => prefix = path_key_display(&rel),
            EntryEvent::Entry(text) => frags.push(if prefix.is_empty() {
                format!("{text}\n")
            } else {
                format!("{prefix}.{text}\n")
            }),
        }
    }
    Ok(frags)
}

/// The full body of the `[[…]]` AoT entry backed by `header`, preserving
/// nested `[table]` sub-sections as *relative* headers (`[physical]`,
/// stripped of the entry's own group-path ancestor) instead of flattening
/// them to dotted keys — used when the destination is itself an `[A/T]`
/// group (ADR 0004 §3), so the caller's `prefix_section_headers`
/// pass re-qualifies each relative header against the *destination's* key,
/// reconstructing the same nested structure atomically instead of losing it
/// to a dotted-key rewrite. `Err(Unsupported)` on a nested `[[…]]` sub-group,
/// same as `aot_entry_member_fragments` (it has no dotted/atomic form either).
pub(crate) fn aot_entry_section_body(
    tree: &SyntaxNode,
    header: &SyntaxNode,
) -> Result<String, MutateError> {
    let mut body = String::new();
    for ev in walk_aot_entry_body(tree, header)? {
        match ev {
            EntryEvent::Header(rel) => body.push_str(&format!("[{}]\n", path_key_display(&rel))),
            EntryEvent::Entry(text) => {
                body.push_str(&text);
                body.push('\n');
            }
        }
    }
    Ok(body)
}

/// Whether a header node is a `[[aot]]` entry (vs a `[table]`).
pub(crate) fn idx_target_is_aot(header: &SyntaxNode) -> bool {
    header.kind() == SyntaxKind::TABLE_ARRAY_HEADER
}
