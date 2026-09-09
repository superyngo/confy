//! `Mutation::Replace` and `Mutation::Delete` (plus the comment-token delete
//! path) — split out of `json/edit.rs` (F6, 2026-09-09).

use super::*;

pub(super) fn replace(tree: &SyntaxNode, path: &[Seg], fragment: &str) -> Result<(), MutateError> {
    if path.is_empty() {
        // Whole-document replace: parse fragment as a full JSON doc, splice its
        // ROOT children over the old ROOT children.
        // We need mutable SyntaxElements from a separate mutable tree.
        let green = crate::model::json::parse::parse(fragment).map_err(MutateError::Fragment)?;
        // Create an immutable root first, then clone_for_update to get mutable children.
        let new_root_immutable = SyntaxNode::new_root(green);
        let new_root = new_root_immutable.clone_for_update();
        let n = tree.children_with_tokens().count();
        // Children of a mutable node are already mutable — collect them directly.
        let new_children: Vec<_> = new_root.children_with_tokens().collect();
        tree.splice_children(0..n, new_children);
        return Ok(());
    }
    match resolve(tree, path).ok_or(MutateError::NotFound)? {
        Target::Member(member) => {
            let frag_comment = fragment_member_trailing_comment(fragment);
            if let Some(new_member) = parse_member_fragment(fragment) {
                replace_node(&member, new_member);
            } else {
                let value = member
                    .children()
                    .find(|n| n.kind() == SyntaxKind::VALUE)
                    .ok_or(MutateError::NotFound)?;
                let new_value = parse_value_fragment(fragment)?;
                replace_node(&value, new_value);
            }
            if let Some(c) = frag_comment {
                set_trailing_comment(tree, path, Some(&c))?;
            }
            Ok(())
        }
        Target::Element(value) => {
            let frag_comment = fragment_element_trailing_comment(fragment);
            let new_value = parse_value_fragment(fragment)?;
            replace_node(&value, new_value);
            if let Some(c) = frag_comment {
                set_trailing_comment(tree, path, Some(&c))?;
            }
            Ok(())
        }
        Target::Comment(_) | Target::Block(_) => Err(MutateError::Illegal(
            "use EditComment to edit a comment".into(),
        )),
    }
}

/// Replace a node in a mutable tree by splicing over its slot in its parent.
pub(super) fn replace_node(old: &SyntaxNode, new: SyntaxNode) {
    let parent = old.parent().expect("node has a parent");
    let idx = old.index();
    parent.splice_children(idx..idx + 1, vec![new.into()]);
}

pub(super) fn delete(tree: &SyntaxNode, path: &[Seg]) -> Result<(), MutateError> {
    match resolve(tree, path).ok_or(MutateError::NotFound)? {
        Target::Member(m) => delete_item(&m),
        Target::Element(v) => delete_item(&v),
        Target::Comment(tok) | Target::Block(tok) => delete_comment_tokens(&tok),
    }
    Ok(())
}

/// Delete a MEMBER or VALUE (array element) node from its parent, removing the
/// associated comma and leading indent+newline so the result is well-formed.
pub(super) fn delete_item(node: &SyntaxNode) {
    let parent = node.parent().expect("node has parent");
    let children: Vec<_> = parent.children_with_tokens().collect();
    let n = children.len();

    // Find `node`'s index in children_with_tokens (by identity).
    let node_idx = children
        .iter()
        .position(|c| match c {
            rowan::NodeOrToken::Node(sn) => sn == node,
            _ => false,
        })
        .expect("node is child of parent");

    let mut start = node_idx;
    let mut end = node_idx + 1; // exclusive

    // --- Forward scan: look for a trailing comma (and an optional space after it). ---
    let mut found_trailing_comma = false;
    let mut scan = end;
    while scan < n {
        match &children[scan] {
            rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::WHITESPACE => {
                scan += 1;
            }
            rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::COMMA => {
                // Include everything from `end` through this comma.
                end = scan + 1;
                found_trailing_comma = true;
                // Also eat one trailing WHITESPACE (space after comma in inline arrays).
                if end < n {
                    if let rowan::NodeOrToken::Token(next) = &children[end] {
                        if next.kind() == SyntaxKind::WHITESPACE {
                            end += 1;
                        }
                    }
                }
                break;
            }
            _ => break,
        }
    }

    // --- Backward scan: if no trailing comma, remove the preceding comma (last item). ---
    if !found_trailing_comma {
        let mut scan_back = start;
        while scan_back > 0 {
            scan_back -= 1;
            match &children[scan_back] {
                rowan::NodeOrToken::Token(t)
                    if matches!(t.kind(), SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE) =>
                {
                    // keep scanning over whitespace/newlines between node and comma
                }
                rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::COMMA => {
                    // Set start to the comma — include it in deletion range.
                    start = scan_back;
                    break;
                }
                _ => break,
            }
        }
    }

    // --- Backward scan: swallow the leading newline + indent (multiline containers). ---
    // Only for multiline: if the token immediately before `start` is WHITESPACE (indent)
    // and before that is a NEWLINE, absorb them.
    if start > 0 {
        let prev = start - 1;
        match &children[prev] {
            rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::WHITESPACE => {
                // Check if the token before the whitespace is a NEWLINE.
                if prev > 0 {
                    if let rowan::NodeOrToken::Token(t2) = &children[prev - 1] {
                        if t2.kind() == SyntaxKind::NEWLINE {
                            start = prev - 1; // include NEWLINE + WHITESPACE
                        }
                    }
                }
            }
            rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::NEWLINE => {
                start = prev; // just a NEWLINE (no indent)
            }
            _ => {}
        }
    }

    parent.splice_children(start..end, vec![]);
}

/// Delete a standalone comment block (LINE_COMMENT or BLOCK_COMMENT token).
/// Removes the token(s) plus their line's leading WHITESPACE and trailing NEWLINE.
pub(super) fn delete_comment_tokens(first_tok: &SyntaxToken) {
    let parent = first_tok.parent().expect("parent");
    let children: Vec<_> = parent.children_with_tokens().collect();
    let n = children.len();

    // Find the first token's index.
    let tok_idx = children
        .iter()
        .position(|c| match c {
            rowan::NodeOrToken::Token(t) => t == first_tok,
            _ => false,
        })
        .expect("token is child of parent");

    let mut start = tok_idx;
    let mut end = tok_idx + 1;

    // Extend `end` forward over the entire LINE_COMMENT block (consecutive // lines
    // joined by NEWLINE + optional WHITESPACE) and a BLOCK_COMMENT's trailing NEWLINE.
    if first_tok.kind() == SyntaxKind::LINE_COMMENT {
        // Walk forward consuming consecutive `// …` lines:
        // each NEWLINE followed by optional WHITESPACE + LINE_COMMENT extends the block.
        let mut scan = end;
        while scan < n {
            // Expect a NEWLINE token next.
            let is_newline = matches!(
                &children[scan],
                rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::NEWLINE
            );
            if !is_newline {
                break;
            }
            // Look past the NEWLINE for optional WS then another LINE_COMMENT.
            let mut s2 = scan + 1;
            while s2 < n {
                match &children[s2] {
                    rowan::NodeOrToken::Token(t2) if t2.kind() == SyntaxKind::WHITESPACE => {
                        s2 += 1;
                    }
                    rowan::NodeOrToken::Token(t2) if t2.kind() == SyntaxKind::LINE_COMMENT => {
                        // Continuation comment — extend end through it.
                        end = s2 + 1;
                        scan = end;
                        break;
                    }
                    _ => {
                        // The NEWLINE terminates the last comment line — include it.
                        end = scan + 1;
                        scan = n; // stop outer loop
                        break;
                    }
                }
            }
            if s2 >= n {
                // NEWLINE at EOF — include it.
                end = scan + 1;
                break;
            }
        }
    } else {
        // BLOCK_COMMENT: include the trailing NEWLINE.
        if end < n {
            if let rowan::NodeOrToken::Token(t) = &children[end] {
                if t.kind() == SyntaxKind::NEWLINE {
                    end += 1;
                }
            }
        }
    }

    // Extend `start` backward over the leading WHITESPACE (indent).
    if start > 0 {
        if let rowan::NodeOrToken::Token(t) = &children[start - 1] {
            if t.kind() == SyntaxKind::WHITESPACE {
                start -= 1;
            }
        }
    }

    parent.splice_children(start..end, vec![]);
}
