//! `Mutation::ConvertKind`: object/array Inline↔Multiline and float
//! Plain↔Exponent — split out of `json/edit.rs` (F6, 2026-09-09).

use super::*;

pub(super) fn convert_kind(
    tree: &SyntaxNode,
    path: &[Seg],
    target: KindTarget,
) -> Result<(), MutateError> {
    match target {
        KindTarget::ArrayInline | KindTarget::ArrayMultiline => convert_array(tree, path, target),
        KindTarget::TableInline | KindTarget::TableMultiline => convert_object(tree, path, target),
        KindTarget::FloatPlain | KindTarget::FloatExponent => convert_float(tree, path, target),
        _ => Err(MutateError::Unsupported),
    }
}

/// Find the ARRAY or OBJECT node at `path` (through a member value).
/// Returns the container node.
pub(super) fn find_value_container(
    tree: &SyntaxNode,
    path: &[Seg],
    expected_kind: SyntaxKind,
) -> Result<SyntaxNode, MutateError> {
    if path.is_empty() {
        // Root value
        let top_value = tree
            .children()
            .find(|n| n.kind() == SyntaxKind::VALUE)
            .ok_or(MutateError::NotFound)?;
        return top_value
            .children()
            .find(|n| n.kind() == expected_kind)
            .ok_or_else(|| MutateError::Illegal("node is not the expected container kind".into()));
    }
    match resolve(tree, path).ok_or(MutateError::NotFound)? {
        Target::Member(member) => {
            let value = member
                .children()
                .find(|n| n.kind() == SyntaxKind::VALUE)
                .ok_or(MutateError::NotFound)?;
            value
                .children()
                .find(|n| n.kind() == expected_kind)
                .ok_or_else(|| {
                    MutateError::Illegal("node is not the expected container kind".into())
                })
        }
        Target::Element(value) => value
            .children()
            .find(|n| n.kind() == expected_kind)
            .ok_or_else(|| MutateError::Illegal("node is not the expected container kind".into())),
        _ => Err(MutateError::Illegal(
            "path does not point to a container".into(),
        )),
    }
}

/// True when the container holds a real `//` or `/* */` comment token —
/// distinct from a string *value* that merely contains those characters.
pub(super) fn container_has_comment(container: &SyntaxNode) -> bool {
    container.descendants_with_tokens().any(|el| {
        matches!(
            el.kind(),
            SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT
        )
    })
}

pub(super) fn convert_array(
    tree: &SyntaxNode,
    path: &[Seg],
    target: KindTarget,
) -> Result<(), MutateError> {
    let container = find_value_container(tree, path, SyntaxKind::ARRAY)
        .map_err(|_| MutateError::Unsupported)?;
    let src = container.text().to_string();
    let is_multiline = src.contains('\n');

    match target {
        KindTarget::ArrayInline if !is_multiline => {
            // Already inline — no-op or treat as Unsupported
            return Err(MutateError::Unsupported);
        }
        KindTarget::ArrayMultiline if is_multiline => {
            return Err(MutateError::Unsupported);
        }
        _ => {}
    }

    // For collapse to inline: reject real comment tokens (a text scan would
    // false-positive on a string value containing "//" or "/*").
    if target == KindTarget::ArrayInline && container_has_comment(&container) {
        return Err(MutateError::Illegal(
            "cannot collapse array with comments to inline".into(),
        ));
    }

    // For collapse to inline: also reject if any element is multi-line
    if target == KindTarget::ArrayInline {
        let items = collect_items(&container);
        for item in &items {
            if item.contains('\n') {
                return Err(MutateError::Illegal(
                    "cannot collapse array with multi-line elements to inline".into(),
                ));
            }
        }
    }

    let items = collect_items(&container);
    let new_text = if target == KindTarget::ArrayMultiline {
        rebuild_multiline_from_inline(&container, &items)
    } else {
        rebuild_inline(&container, &items)
    };

    let new_container = parse_container_text(&new_text, false)?;
    replace_node(&container, new_container);
    Ok(())
}

pub(super) fn convert_object(
    tree: &SyntaxNode,
    path: &[Seg],
    target: KindTarget,
) -> Result<(), MutateError> {
    let container = find_value_container(tree, path, SyntaxKind::OBJECT)
        .map_err(|_| MutateError::Unsupported)?;
    let src = container.text().to_string();
    let is_multiline = src.contains('\n');

    match target {
        KindTarget::TableInline if !is_multiline => {
            return Err(MutateError::Unsupported);
        }
        KindTarget::TableMultiline if is_multiline => {
            return Err(MutateError::Unsupported);
        }
        _ => {}
    }

    // For collapse to inline: reject real comment tokens (a text scan would
    // false-positive on a string value containing "//" or "/*").
    if target == KindTarget::TableInline && container_has_comment(&container) {
        return Err(MutateError::Illegal(
            "cannot collapse object with comments to inline".into(),
        ));
    }

    let items = collect_items(&container);
    let new_text = if target == KindTarget::TableMultiline {
        rebuild_multiline_from_inline(&container, &items)
    } else {
        rebuild_inline(&container, &items)
    };

    let new_container = parse_container_text(&new_text, true)?;
    replace_node(&container, new_container);
    Ok(())
}

/// Build a multiline container from an inline container. Determines indent from context.
/// The new item indent = parent indent + 2 spaces; close indent = parent indent.
pub(super) fn rebuild_multiline_from_inline(container: &SyntaxNode, items: &[String]) -> String {
    let is_object = container.kind() == SyntaxKind::OBJECT;

    // Detect the container's own indent by walking up and finding the leading whitespace
    // on the line containing the opening brace/bracket.
    let own_indent = detect_container_line_indent(container);
    let item_indent = format!("{}  ", own_indent);
    let close_indent = own_indent;

    let mut lines: Vec<String> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if is_comment_item(item) {
            for line in item.lines() {
                lines.push(format!("{item_indent}{line}"));
            }
        } else {
            let has_later = items[i + 1..].iter().any(|it| !is_comment_item(it));
            let comma = if has_later { "," } else { "" };
            lines.push(format!("{item_indent}{item}{comma}"));
        }
    }

    let content = lines.join("\n");
    if is_object {
        format!("{{\n{content}\n{close_indent}}}")
    } else {
        format!("[\n{content}\n{close_indent}]")
    }
}

/// Detect the whitespace indent of the line that contains the container's opening token.
/// Walks ancestors to find a MEMBER whose line starts with whitespace (NEWLINE + WHITESPACE).
pub(super) fn detect_container_line_indent(container: &SyntaxNode) -> String {
    // Walk up to find the enclosing MEMBER, then look at the MEMBER's leading whitespace
    // in its parent's children sequence (NEWLINE + WHITESPACE pattern before the MEMBER).
    let mut node: Option<SyntaxNode> = container.parent();
    while let Some(n) = node {
        if n.kind() == SyntaxKind::MEMBER {
            // Look at the parent (OBJECT/ARRAY) for NEWLINE + WHITESPACE before this MEMBER.
            if let Some(parent) = n.parent() {
                let children: Vec<_> = parent.children_with_tokens().collect();
                let n_idx = children.iter().position(|c| match c {
                    rowan::NodeOrToken::Node(sn) => sn == &n,
                    _ => false,
                });
                if let Some(idx) = n_idx {
                    // Look back for WHITESPACE preceded by NEWLINE.
                    if idx >= 2 {
                        if let rowan::NodeOrToken::Token(ws) = &children[idx - 1] {
                            if ws.kind() == SyntaxKind::WHITESPACE {
                                if let rowan::NodeOrToken::Token(nl) = &children[idx - 2] {
                                    if nl.kind() == SyntaxKind::NEWLINE {
                                        return ws.text().to_string();
                                    }
                                }
                            }
                        }
                    }
                }
            }
            break;
        }
        node = n.parent();
    }
    // Fallback: no indent (top-level or inline ancestor with no own-line indent)
    String::new()
}

pub(super) fn convert_float(
    tree: &SyntaxNode,
    path: &[Seg],
    target: KindTarget,
) -> Result<(), MutateError> {
    // Locate the NUMBER token for the float scalar.
    let num_token = find_float_token(tree, path)?;

    let text = num_token.text().to_string();
    let value: f64 = text
        .parse()
        .map_err(|_| MutateError::Illegal("not a float".into()))?;

    let new_text = match target {
        KindTarget::FloatExponent => {
            // Use Rust's {:e} which gives e.g. "1.5e3"
            format!("{:e}", value)
        }
        KindTarget::FloatPlain => {
            // Rust's {} on f64 never emits e/E notation, but it drops a whole
            // float's `.0` (`1500.0` → "1500"), which the projection would
            // re-read as an Integer — a type change in a float→float convert.
            // Force a float-recognizable form.
            let s = format!("{}", value);
            if s.contains('.') || s.contains('e') || s.contains('E') {
                s
            } else {
                format!("{s}.0")
            }
        }
        _ => unreachable!(),
    };

    // Replace just the NUMBER token.
    let parent = num_token.parent().expect("token has parent");
    let children: Vec<_> = parent.children_with_tokens().collect();
    let tok_idx = children
        .iter()
        .position(|c| match c {
            rowan::NodeOrToken::Token(t) => t == &num_token,
            _ => false,
        })
        .ok_or(MutateError::NotFound)?;

    // Build a new NUMBER token via a minimal parse.
    let probe = new_text.clone();
    let new_green = crate::model::json::parse::parse(&probe).map_err(MutateError::Fragment)?;
    let new_root_immutable = SyntaxNode::new_root(new_green);
    let new_root = new_root_immutable.clone_for_update();
    let new_num_tok = new_root
        .descendants_with_tokens()
        .find_map(|el| match el {
            rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::NUMBER => Some(t),
            _ => None,
        })
        .ok_or_else(|| MutateError::Fragment("could not parse new float".into()))?;

    parent.splice_children(tok_idx..tok_idx + 1, vec![new_num_tok.into()]);
    Ok(())
}

/// Find the NUMBER token for the scalar at `path`. The path must resolve to a member
/// whose value is a NUMBER, or an array element that is a NUMBER.
pub(super) fn find_float_token(
    tree: &SyntaxNode,
    path: &[Seg],
) -> Result<SyntaxToken, MutateError> {
    let value_node = match resolve(tree, path).ok_or(MutateError::NotFound)? {
        Target::Member(member) => member
            .children()
            .find(|n| n.kind() == SyntaxKind::VALUE)
            .ok_or(MutateError::NotFound)?,
        Target::Element(v) => v,
        _ => return Err(MutateError::Unsupported),
    };

    // Descend into VALUE to find a NUMBER token.
    value_node
        .children_with_tokens()
        .find_map(|el| match el {
            rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::NUMBER => Some(t),
            _ => None,
        })
        .ok_or(MutateError::Unsupported)
}

// ── Atomic dispatcher ────────────────────────────────────────────────────────
