//! Container machinery shared by every splice: locating the destination
//! OBJECT/ARRAY, reading its items back as text, and rebuilding it inline or
//! multiline with the right indent — split out of `json/edit.rs` (F6, 2026-09-09).

use super::*;

/// Walk from the tree root down `parent` path to the innermost OBJECT or ARRAY node.
pub(super) fn find_container(tree: &SyntaxNode, parent: &[Seg]) -> Result<SyntaxNode, MutateError> {
    // The top-level value's OBJECT/ARRAY is accessed via ROOT → VALUE → OBJECT/ARRAY.
    let top_value = tree
        .children()
        .find(|n| n.kind() == SyntaxKind::VALUE)
        .ok_or(MutateError::NotFound)?;
    let top_container = top_value
        .children()
        .find(|n| matches!(n.kind(), SyntaxKind::OBJECT | SyntaxKind::ARRAY))
        .ok_or(MutateError::NotFound)?;

    if parent.is_empty() {
        return Ok(top_container);
    }

    // Walk into nested containers following each segment.
    let mut container = top_container;
    for seg in parent {
        let inner = match seg {
            Seg::Key(k) => {
                // Find the MEMBER with this key, then its VALUE's inner container.
                let member = container
                    .children()
                    .filter(|n| n.kind() == SyntaxKind::MEMBER)
                    .find(|m| {
                        m.children()
                            .find(|c| c.kind() == SyntaxKind::KEY)
                            .map(|kn| key_name_of(&kn) == k.as_str())
                            .unwrap_or(false)
                    })
                    .ok_or(MutateError::NotFound)?;
                let val = member
                    .children()
                    .find(|n| n.kind() == SyntaxKind::VALUE)
                    .ok_or(MutateError::NotFound)?;
                val.children()
                    .find(|n| matches!(n.kind(), SyntaxKind::OBJECT | SyntaxKind::ARRAY))
                    .ok_or(MutateError::NotFound)?
            }
            Seg::Index(i) => {
                // Find the i-th VALUE child (array element).
                let elem = container
                    .children()
                    .filter(|n| n.kind() == SyntaxKind::VALUE)
                    .nth(*i)
                    .ok_or(MutateError::NotFound)?;
                elem.children()
                    .find(|n| matches!(n.kind(), SyntaxKind::OBJECT | SyntaxKind::ARRAY))
                    .ok_or(MutateError::NotFound)?
            }
        };
        container = inner;
    }
    Ok(container)
}

pub(super) fn key_name_of(key_node: &SyntaxNode) -> String {
    let text = key_node.text().to_string();
    let t = text.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}

/// An item's identity anchor: the MEMBER/VALUE node, or — for a standalone `//`
/// block — its FIRST LINE_COMMENT token. Used to locate an item by node/token
/// identity (not text) so duplicate-text siblings resolve correctly.
type ItemAnchor = rowan::NodeOrToken<SyntaxNode, SyntaxToken>;

/// Sentinel separating a merged item's own text from its trailing same-line
/// comment (see `collect_items_with_anchors`) — never appears in real JSON
/// text, so it's safe as an internal-only marker between `collect_items*`
/// and `rebuild_multiline`/`rebuild_inline`.
pub(super) const TRAILING_MARKER: char = '\u{0}';

/// Collect items (members/elements/comments) as verbatim trimmed strings, each
/// paired with its identity anchor. Order matches projection order (same as
/// children_with_tokens order for MEMBER/VALUE/comment tokens, skipping
/// punctuation and trivia).
///
/// A trailing same-line comment (`"a": 1  // c`) is merged into its owning
/// member/element's item (separated internally by `TRAILING_MARKER`) rather
/// than becoming its own item — the row/`Node` projection folds it into its
/// owning member's single row (`Node.trailing_comment`), never a row of its
/// own, so item-space must match that one-node-one-slot shape. Without this,
/// `MutTarget::index` (a projected slot index) and `items.len()` disagree by
/// one whenever a trailing comment is present, and an insert anchored right
/// after such a row lands between the value and its comment instead of after
/// the whole line. `rebuild_multiline`/`rebuild_inline` un-merge via
/// `split_trailing_marker` to place the comma before the comment, not after.
pub(super) fn collect_items_with_anchors(container: &SyntaxNode) -> Vec<(String, ItemAnchor)> {
    use crate::model::json::project::is_standalone_line_comment;
    let mut items: Vec<(String, ItemAnchor)> = Vec::new();
    // Pending standalone `//` block: consecutive LINE_COMMENTs separated by a
    // single NEWLINE merge into ONE item (a blank line — a second NEWLINE — ends
    // the block). This mirrors the projection's comment merging so item-space
    // equals the projection's slot-space (one merged block = one node = one slot).
    let mut block: Vec<String> = Vec::new();
    let mut block_anchor: Option<ItemAnchor> = None;
    let mut seen_newline = false;
    macro_rules! flush_block {
        () => {
            if !block.is_empty() {
                items.push((block.join("\n"), block_anchor.take().expect("block anchor")));
                block.clear();
            }
        };
    }
    for child in container.children_with_tokens() {
        match &child {
            rowan::NodeOrToken::Node(n)
                if matches!(n.kind(), SyntaxKind::MEMBER | SyntaxKind::VALUE) =>
            {
                flush_block!();
                items.push((n.text().to_string().trim().to_string(), child.clone()));
            }
            rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::LINE_COMMENT => {
                if is_standalone_line_comment(t) {
                    if block.is_empty() {
                        block_anchor = Some(child.clone());
                    }
                    block.push(t.text().trim_end().to_string());
                    seen_newline = false;
                } else {
                    // Trailing comment (after a member/element on the same
                    // line) — merge into the owning item's text instead of
                    // pushing a separate item (see the doc comment above).
                    match items.last_mut() {
                        Some(last) => {
                            last.0.push(TRAILING_MARKER);
                            last.0.push_str(t.text().trim_end());
                        }
                        None => {
                            // No preceding item to attach to (shouldn't happen
                            // for a well-formed document) — fall back to a
                            // standalone item rather than losing the comment.
                            flush_block!();
                            items.push((t.text().trim_end().to_string(), child.clone()));
                        }
                    }
                }
            }
            rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::BLOCK_COMMENT => {
                flush_block!();
                items.push((t.text().trim_end().to_string(), child.clone()));
            }
            rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::NEWLINE => {
                if !block.is_empty() {
                    if seen_newline {
                        flush_block!();
                    } else {
                        seen_newline = true;
                    }
                }
            }
            _ => {}
        }
    }
    flush_block!();
    items
}

/// Collect item texts only (see `collect_items_with_anchors`).
pub(super) fn collect_items(container: &SyntaxNode) -> Vec<String> {
    collect_items_with_anchors(container)
        .into_iter()
        .map(|(s, _)| s)
        .collect()
}

/// Extract key names from all MEMBER children of an OBJECT container.
pub(super) fn existing_object_keys(container: &SyntaxNode) -> Vec<String> {
    container
        .children()
        .filter(|n| n.kind() == SyntaxKind::MEMBER)
        .filter_map(|m| m.children().find(|c| c.kind() == SyntaxKind::KEY))
        .map(|k| key_name_of(&k))
        .collect()
}

/// Rebuild a MULTILINE container (object or array) from item strings.
/// Detects indent from the first existing member and the container's own closing indent.
pub(super) fn rebuild_multiline(container: &SyntaxNode, items: &[String]) -> String {
    let is_object = container.kind() == SyntaxKind::OBJECT;

    // Detect the indent used by existing items (look at first MEMBER/VALUE's
    // leading whitespace from the container's text).
    let item_indent = detect_indent(container);

    // Detect the closing brace/bracket indent (the whitespace before R_BRACE/R_BRACK).
    let close_indent = detect_close_indent(container);

    // Build the container content. Comments never get commas; non-comment items
    // get a comma if the *next* non-comment item exists.
    let mut lines: Vec<String> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if is_comment_item(item) {
            // Comments are emitted as-is, one line per item (no comma). A
            // blank separator line inside the block stays *empty* — indenting
            // it would write trailing whitespace no author typed.
            for line in item.lines() {
                if line.trim().is_empty() {
                    lines.push(String::new());
                } else {
                    lines.push(format!("{item_indent}{line}"));
                }
            }
        } else {
            // Non-comment: comma if there is a later non-comment item. A
            // merged trailing comment (see `collect_items_with_anchors`)
            // must stay LAST on the line, after the comma — otherwise the
            // comma would land inside the `//` comment and vanish from the
            // real syntax.
            let (main, trailing) = split_trailing_marker(item);
            let has_later = items[i + 1..].iter().any(|it| !is_comment_item(it));
            let comma = if has_later { "," } else { "" };
            match trailing {
                Some(comment) => lines.push(format!("{item_indent}{main}{comma}  {comment}")),
                None => lines.push(format!("{item_indent}{main}{comma}")),
            }
        }
    }

    let content = lines.join("\n");
    if is_object {
        format!("{{\n{content}\n{close_indent}}}")
    } else {
        format!("[\n{content}\n{close_indent}]")
    }
}

/// Detect the whitespace before the closing R_BRACE or R_BRACK in a multiline container.
pub(super) fn detect_close_indent(container: &SyntaxNode) -> String {
    let children: Vec<_> = container.children_with_tokens().collect();
    // Walk from the end: find R_BRACE or R_BRACK, then look at the preceding WHITESPACE.
    for i in (0..children.len()).rev() {
        match &children[i] {
            rowan::NodeOrToken::Token(t)
                if t.kind() == SyntaxKind::R_BRACE || t.kind() == SyntaxKind::R_BRACK =>
            {
                // Look backwards for WHITESPACE preceded by NEWLINE.
                if i >= 2 {
                    if let rowan::NodeOrToken::Token(ws) = &children[i - 1] {
                        if ws.kind() == SyntaxKind::WHITESPACE {
                            return ws.text().to_string();
                        }
                    }
                    // Could be NEWLINE directly before R_BRACE (no indent).
                }
                return String::new();
            }
            _ => {}
        }
    }
    String::new()
}

/// Detect the per-item indent string from an existing MULTILINE container.
/// Looks at the leading whitespace before the first MEMBER or VALUE child.
pub(super) fn detect_indent(container: &SyntaxNode) -> String {
    // Walk children_with_tokens: after the first NEWLINE, collect WHITESPACE before
    // the first MEMBER/VALUE/comment.
    let mut after_newline = false;
    for child in container.children_with_tokens() {
        match &child {
            rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::NEWLINE => {
                after_newline = true;
            }
            rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::WHITESPACE && after_newline => {
                return t.text().to_string();
            }
            rowan::NodeOrToken::Node(n)
                if after_newline && matches!(n.kind(), SyntaxKind::MEMBER | SyntaxKind::VALUE) =>
            {
                // No indent whitespace found — use 2 spaces.
                return "  ".to_string();
            }
            rowan::NodeOrToken::Node(n)
                if matches!(n.kind(), SyntaxKind::MEMBER | SyntaxKind::VALUE) =>
            {
                break;
            }
            _ => {}
        }
    }
    "  ".to_string()
}

/// Rebuild an INLINE container from item strings.
pub(super) fn rebuild_inline(container: &SyntaxNode, items: &[String]) -> String {
    let is_object = container.kind() == SyntaxKind::OBJECT;
    let joined = items.join(", ");
    if is_object {
        // Detect if original had inner spaces: `{ ... }` vs `{...}`.
        let orig = container.text().to_string();
        let inner = orig.trim();
        let has_space = inner.starts_with("{ ") || inner == "{}";
        if has_space || items.is_empty() {
            if items.is_empty() {
                "{}".to_string()
            } else {
                format!("{{ {joined} }}")
            }
        } else {
            format!("{{{joined}}}")
        }
    } else {
        format!("[{joined}]")
    }
}

/// Parse a container text (OBJECT or ARRAY string) as a standalone JSON doc and
/// return the inner OBJECT/ARRAY node (mutable, via clone_for_update).
pub(super) fn parse_container_text(
    text: &str,
    _is_object: bool,
) -> Result<SyntaxNode, MutateError> {
    // Wrap in a doc so the parser is happy (the text IS already an object/array).
    let green = crate::model::json::parse::parse(text).map_err(MutateError::Illegal)?;
    let root_immutable = SyntaxNode::new_root(green);
    let root = root_immutable.clone_for_update();
    // Navigate: ROOT → VALUE → OBJECT/ARRAY
    let value = root
        .children()
        .find(|n| n.kind() == SyntaxKind::VALUE)
        .ok_or_else(|| MutateError::Illegal("no VALUE in rebuilt container".into()))?;
    let container = value
        .children()
        .find(|n| matches!(n.kind(), SyntaxKind::OBJECT | SyntaxKind::ARRAY))
        .ok_or_else(|| MutateError::Illegal("no container in rebuilt text".into()))?;
    Ok(container)
}
