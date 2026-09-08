//! Flow-collection (`{ … }` / `[ … ]`) edits — split out of `yaml/edit.rs`
//! (Task 15, 2026-08-11 audit remediation). Already a clearly delimited
//! section per the original file's own `flow_*`/`*_flow_*` naming.

use super::block::{commit_reparse, entry_key_text, item_key_name};
use crate::model::document::{MutateError, OnCollision, Target as MutTarget};
use crate::model::yaml::syntax::{SyntaxKind, SyntaxNode};

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

/// A member/element span with its **trailing whitespace excluded**. A plain
/// scalar token swallows the spaces before the closing `}`/`]` — in
/// `{ a: 1, b: 2 }` the last member is `b: 2 ` — so splicing over the raw range
/// (or measuring the collection's padding from it) would eat that padding.
fn trimmed_span(tree_text: &str, range: rowan::TextRange) -> (usize, usize) {
    let start: usize = range.start().into();
    let end: usize = range.end().into();
    let slice = &tree_text[start..end];
    (start, end - (slice.len() - slice.trim_end().len()))
}

/// The spans of a flow collection's members (FLOW_MAP) or elements (FLOW_SEQ),
/// in document order — the same order the projection indexes them.
fn flow_item_spans(tree_text: &str, flow: &SyntaxNode) -> Vec<(usize, usize)> {
    if flow.kind() == SyntaxKind::FLOW_MAP {
        flow.children()
            .filter(|c| c.kind() == SyntaxKind::FLOW_ENTRY)
            .map(|e| trimmed_span(tree_text, e.text_range()))
            .collect()
    } else {
        flow.children_with_tokens()
            .filter_map(|c| match c {
                rowan::NodeOrToken::Token(t)
                    if matches!(
                        t.kind(),
                        SyntaxKind::PLAIN | SyntaxKind::SINGLE | SyntaxKind::DOUBLE
                    ) =>
                {
                    Some(t.text_range())
                }
                rowan::NodeOrToken::Node(n)
                    if matches!(n.kind(), SyntaxKind::FLOW_MAP | SyntaxKind::FLOW_SEQ) =>
                {
                    Some(n.text_range())
                }
                _ => None,
            })
            .map(|r| trimmed_span(tree_text, r))
            .collect()
    }
}

/// The verbatim text of a flow collection's `ord`-th item, **trailing
/// whitespace excluded**. A flow-seq *scalar* element has no `Target` of its
/// own — the projection indexes it as `Target::Element(<the whole FLOW_SEQ>)`,
/// since every edit needs the collection plus an ordinal — so a fragment
/// capture (copy, `$EDITOR`, `Move`) resolving that path would otherwise take
/// the entire `[ … ]` and nest the collection into its own element.
pub(crate) fn flow_item_text(flow: &SyntaxNode, ord: usize) -> Option<String> {
    let root = flow.ancestors().last().unwrap_or_else(|| flow.clone());
    let text = root.to_string();
    let (s, e) = flow_item_spans(&text, flow).into_iter().nth(ord)?;
    Some(text[s..e].to_string())
}

/// The author's own inner spacing of a flow collection, so a rebuild re-emits
/// their style instead of a canonical `{a, b}`: the padding after the opener,
/// the padding before the closer, and the member separator (`, ` / `,`).
struct FlowStyle {
    open: String,
    sep: String,
    close: String,
}

fn flow_style(tree_text: &str, flow: &SyntaxNode, spans: &[(usize, usize)]) -> FlowStyle {
    // Inner bounds: just inside the delimiters (an unterminated collection keeps
    // the node's own end). Nested collections are child nodes, so only this
    // collection's own delimiters are direct token children.
    let mut inner_start: usize = flow.text_range().start().into();
    let mut inner_end: usize = flow.text_range().end().into();
    for c in flow.children_with_tokens() {
        if let rowan::NodeOrToken::Token(t) = &c {
            match t.kind() {
                SyntaxKind::L_BRACE | SyntaxKind::L_BRACK => {
                    inner_start = t.text_range().end().into()
                }
                SyntaxKind::R_BRACE | SyntaxKind::R_BRACK => {
                    inner_end = t.text_range().start().into()
                }
                _ => {}
            }
        }
    }
    let slice = |a: usize, b: usize| tree_text.get(a..b).unwrap_or("").to_string();
    let sep = match (spans.first(), spans.get(1)) {
        (Some(first), Some(second)) => {
            let gap = slice(first.1, second.0);
            // A gap with no comma means the source is not a comma list; don't
            // propagate it as a separator.
            if gap.contains(',') {
                gap
            } else {
                ", ".to_string()
            }
        }
        _ => ", ".to_string(),
    };
    FlowStyle {
        open: spans
            .first()
            .map(|s| slice(inner_start, s.0))
            .unwrap_or_default(),
        sep,
        close: spans
            .last()
            .map(|s| slice(s.1, inner_end))
            .unwrap_or_default(),
    }
}

/// Re-emit a flow collection from member texts and splice it over `flow`'s
/// range, keeping the author's inner spacing (`flow_style`).
pub(crate) fn rebuild_flow(
    tree: &SyntaxNode,
    flow: &SyntaxNode,
    members: &[String],
) -> Result<(), MutateError> {
    let full = tree.to_string();
    let style = flow_style(&full, flow, &flow_item_spans(&full, flow));
    let items: Vec<&str> = members.iter().map(|m| m.trim()).collect();
    let inner = if items.is_empty() {
        String::new()
    } else {
        format!("{}{}{}", style.open, items.join(&style.sep), style.close)
    };
    let text = if flow.kind() == SyntaxKind::FLOW_MAP {
        format!("{{{inner}}}")
    } else {
        format!("[{inner}]")
    };
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
    // Splice over the member's span *without* its trailing whitespace, so the
    // collection's closing padding (`{ a: 1, b: 2 }`) survives the rewrite.
    let (start, end) = trimmed_span(&full, member.text_range());
    let new_doc = format!("{}{}{}", &full[..start], new_text, &full[end..]);
    commit_reparse(tree, &new_doc, MutateError::Illegal)
}

/// Delete a flow-map member by rebuilding the `{…}` without it.
pub(crate) fn delete_flow_member(
    tree: &SyntaxNode,
    member: &SyntaxNode,
) -> Result<(), MutateError> {
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
    // Splice over the element's own span (trailing whitespace excluded) rather
    // than rebuilding the whole `[…]`, so an untouched element round-trips and
    // the collection's padding and separators are left exactly as authored.
    let full = tree.to_string();
    let (start, end) = *flow_item_spans(&full, flow)
        .get(ord)
        .ok_or(MutateError::NotFound)?;
    let new_doc = format!("{}{}{}", &full[..start], frag, &full[end..]);
    commit_reparse(tree, &new_doc, MutateError::Illegal)
}

/// Insert a new member/element into a flow collection at `target`.
///
/// `suggested_key` names the key synthesized for a bare value landed in a
/// FLOW_MAP (`<arrayKey>_<index>` for a moved scalar array element); `None`
/// keeps the generic `placeholder`.
pub(crate) fn insert_flow(
    tree: &SyntaxNode,
    flow: &SyntaxNode,
    target: &MutTarget,
    fragment: &str,
    suggested_key: Option<&str>,
    on_collision: OnCollision,
) -> Result<(), MutateError> {
    let frag = fragment.trim();
    if flow.kind() == SyntaxKind::FLOW_MAP {
        // Build a single-line `key: value` member; a bare value gets a
        // suggested-or-placeholder key.
        let member = if frag.contains(": ") || frag.ends_with(':') {
            frag.to_string()
        } else {
            format!("{}: {frag}", suggested_key.unwrap_or("placeholder"))
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
                        let candidate = crate::model::node::next_available_key(key, |c| {
                            existing.iter().any(|k| k == c)
                        });
                        final_member = format!("{candidate}: {val}");
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
