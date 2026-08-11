//! Flow-collection (`{ … }` / `[ … ]`) edits — split out of `yaml/edit.rs`
//! (Task 15, 2026-08-11 audit remediation). Already a clearly delimited
//! section per the original file's own `flow_*`/`*_flow_*` naming.

use crate::model::document::{MutateError, OnCollision, Target as MutTarget};
use crate::model::yaml::syntax::{SyntaxKind, SyntaxNode};
use super::block::{commit_reparse, entry_key_text, item_key_name};

/// `true` if `node` sits inside an inline flow collection (so block-producing
/// edits — block expansion, literal/folded scalars — would break the one line).
pub(crate) fn node_in_flow(node: &SyntaxNode) -> bool {
    node.ancestors()
        .any(|a| matches!(a.kind(), SyntaxKind::FLOW_MAP | SyntaxKind::FLOW_SEQ))
}

/// The verbatim `key: value` texts of a FLOW_MAP's members, in order.
pub(crate) fn flow_map_member_texts(flow: &SyntaxNode) -> Vec<String> {
    flow.children()
        .filter(|c| c.kind() == SyntaxKind::FLOW_ENTRY)
        .map(|e| e.text().to_string())
        .collect()
}

/// The verbatim element texts of a FLOW_SEQ (scalar tokens + nested flow nodes).
pub(crate) fn flow_seq_element_texts(flow: &SyntaxNode) -> Vec<String> {
    flow.children_with_tokens()
        .filter_map(|c| match c {
            rowan::NodeOrToken::Token(t)
                if matches!(
                    t.kind(),
                    SyntaxKind::PLAIN | SyntaxKind::SINGLE | SyntaxKind::DOUBLE
                ) =>
            {
                Some(t.text().to_string())
            }
            rowan::NodeOrToken::Node(n)
                if matches!(n.kind(), SyntaxKind::FLOW_MAP | SyntaxKind::FLOW_SEQ) =>
            {
                Some(n.text().to_string())
            }
            _ => None,
        })
        .collect()
}

/// The `ord`-th element *node* of a FLOW_SEQ — counting scalar tokens **and**
/// nested flow nodes in document order (the same order the projection indexes
/// them), so this maps a path `Seg::Index` to its child. Returns `None` when the
/// element at `ord` is a scalar token (no collection to descend into) or `ord` is
/// out of range. Used by `find_container` to descend a path *through* a flow seq.
pub(crate) fn flow_seq_element_node(flow: &SyntaxNode, ord: usize) -> Option<SyntaxNode> {
    let mut i = 0usize;
    for c in flow.children_with_tokens() {
        match c {
            rowan::NodeOrToken::Token(t)
                if matches!(
                    t.kind(),
                    SyntaxKind::PLAIN | SyntaxKind::SINGLE | SyntaxKind::DOUBLE
                ) =>
            {
                if i == ord {
                    return None; // scalar element: nothing to descend into
                }
                i += 1;
            }
            rowan::NodeOrToken::Node(n)
                if matches!(n.kind(), SyntaxKind::FLOW_MAP | SyntaxKind::FLOW_SEQ) =>
            {
                if i == ord {
                    return Some(n);
                }
                i += 1;
            }
            _ => {}
        }
    }
    None
}

/// Re-emit a flow collection from member texts and splice it over `flow`'s range.
pub(crate) fn rebuild_flow(
    tree: &SyntaxNode,
    flow: &SyntaxNode,
    members: &[String],
) -> Result<(), MutateError> {
    let inner = members.join(", ");
    let text = if flow.kind() == SyntaxKind::FLOW_MAP {
        format!("{{{inner}}}")
    } else {
        format!("[{inner}]")
    };
    let full = tree.to_string();
    let start: usize = flow.text_range().start().into();
    let end: usize = flow.text_range().end().into();
    let new_doc = format!("{}{}{}", &full[..start], text, &full[end..]);
    commit_reparse(tree, &new_doc, MutateError::Illegal)
}

/// Replace a flow-map member (`FLOW_ENTRY`) with `fragment`, keeping it inline.
pub(crate) fn replace_flow_entry(
    tree: &SyntaxNode,
    member: &SyntaxNode,
    fragment: &str,
) -> Result<(), MutateError> {
    let frag = fragment.trim();
    if frag.contains('\n') {
        return Err(MutateError::Unsupported);
    }
    // A keyed `k: v` fragment is used as-is; a bare value re-uses the member's key.
    let new_text = if frag.contains(": ") || frag.ends_with(':') {
        frag.to_string()
    } else {
        format!("{}: {frag}", entry_key_text(member))
    };
    let full = tree.to_string();
    let start: usize = member.text_range().start().into();
    let end: usize = member.text_range().end().into();
    let new_doc = format!("{}{}{}", &full[..start], new_text, &full[end..]);
    commit_reparse(tree, &new_doc, MutateError::Illegal)
}

/// Delete a flow-map member by rebuilding the `{…}` without it.
pub(crate) fn delete_flow_member(tree: &SyntaxNode, member: &SyntaxNode) -> Result<(), MutateError> {
    let flow = member.parent().expect("flow member has a FLOW_MAP parent");
    let members: Vec<String> = flow
        .children()
        .filter(|c| c.kind() == SyntaxKind::FLOW_ENTRY && c != member)
        .map(|e| e.text().to_string())
        .collect();
    rebuild_flow(tree, &flow, &members)
}

/// Delete the `ord`-th element of a FLOW_SEQ by rebuilding `[…]` without it.
/// (A flow-seq element shares the whole FLOW_SEQ as its resolver target — unlike a
/// block SEQ_ENTRY — so a plain node removal would drop the entire sequence.)
pub(crate) fn delete_flow_seq_element(
    tree: &SyntaxNode,
    flow: &SyntaxNode,
    ord: usize,
) -> Result<(), MutateError> {
    let mut members = flow_seq_element_texts(flow);
    if ord >= members.len() {
        return Err(MutateError::NotFound);
    }
    members.remove(ord);
    rebuild_flow(tree, flow, &members)
}

/// Replace the `ord`-th element of a FLOW_SEQ with `fragment` (a bare value),
/// keeping the `[…]` inline.
pub(crate) fn replace_flow_seq_element(
    tree: &SyntaxNode,
    flow: &SyntaxNode,
    ord: usize,
    fragment: &str,
) -> Result<(), MutateError> {
    // The inline editor may hand back a `- value` element fragment; strip the dash.
    let frag = fragment.trim();
    let frag = frag.strip_prefix("- ").unwrap_or(frag).trim();
    if frag.contains('\n') {
        return Err(MutateError::Unsupported);
    }
    let mut members = flow_seq_element_texts(flow);
    if ord >= members.len() {
        return Err(MutateError::NotFound);
    }
    members[ord] = frag.to_string();
    rebuild_flow(tree, flow, &members)
}

/// Insert a new member/element into a flow collection at `target`.
pub(crate) fn insert_flow(
    tree: &SyntaxNode,
    flow: &SyntaxNode,
    target: &MutTarget,
    fragment: &str,
    on_collision: OnCollision,
) -> Result<(), MutateError> {
    let frag = fragment.trim();
    if flow.kind() == SyntaxKind::FLOW_MAP {
        // Build a single-line `key: value` member; a bare value gets a placeholder.
        let member = if frag.contains(": ") || frag.ends_with(':') {
            frag.to_string()
        } else {
            format!("placeholder: {frag}")
        };
        if member.contains('\n') {
            return Err(MutateError::Unsupported);
        }
        let mut members = flow_map_member_texts(flow);
        let new_key = item_key_name(&member);
        let mut final_member = member;
        if let Some(key) = &new_key {
            let existing: Vec<String> = members.iter().filter_map(|m| item_key_name(m)).collect();
            if existing.iter().any(|k| k == key) {
                match on_collision {
                    OnCollision::Cancel => return Err(MutateError::Collision(key.clone())),
                    OnCollision::Overwrite => {
                        if let Some(ci) = members
                            .iter()
                            .position(|m| item_key_name(m).as_deref() == Some(key))
                        {
                            members.remove(ci);
                        }
                    }
                    OnCollision::Rename => {
                        let val = final_member
                            .split_once(": ")
                            .map(|x| x.1)
                            .unwrap_or("")
                            .to_string();
                        let mut n = 2usize;
                        let renamed = loop {
                            let candidate = format!("{key}_{n}");
                            if !existing.iter().any(|k| k == &candidate) {
                                break format!("{candidate}: {val}");
                            }
                            n += 1;
                        };
                        final_member = renamed;
                    }
                }
            }
        }
        let idx = target.index.min(members.len());
        members.insert(idx, final_member);
        rebuild_flow(tree, flow, &members)
    } else {
        // Flow sequence: a bare value (strip a leading `- ` if present).
        let elem = frag.strip_prefix("- ").unwrap_or(frag).trim().to_string();
        if elem.contains('\n') {
            return Err(MutateError::Unsupported);
        }
        let mut elems = flow_seq_element_texts(flow);
        let idx = target.index.min(elems.len());
        elems.insert(idx, elem);
        rebuild_flow(tree, flow, &elems)
    }
}
