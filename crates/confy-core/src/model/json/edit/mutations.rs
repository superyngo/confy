//! The remaining `Mutation` variants: Rename, Remark, EditComment,
//! InsertComment, SetTrailingComment and SetTrailingBlankLines (with the
//! extent helpers they share) — split out of `json/edit.rs` (F6, 2026-09-09).

use super::*;

pub(super) fn rename(tree: &SyntaxNode, path: &[Seg], new_key: &str) -> Result<(), MutateError> {
    let member = match resolve(tree, path).ok_or(MutateError::NotFound)? {
        Target::Member(m) => m,
        _ => return Err(MutateError::Illegal("rename requires a member".into())),
    };

    // Locate the KEY node inside the member, then find its STRING token.
    let key_node = member
        .children()
        .find(|n| n.kind() == SyntaxKind::KEY)
        .ok_or(MutateError::NotFound)?;
    let children: Vec<_> = key_node.children_with_tokens().collect();
    let str_idx = children
        .iter()
        .position(|c| matches!(c, rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::STRING))
        .ok_or(MutateError::NotFound)?;

    // Build the probe key literal the same way TOML/YAML treat rename input:
    // as syntax that may already be quoted, not raw text to wrap blindly. An
    // already-quoted new_key (e.g. round-tripped from `Node::key_literal`) is
    // used as-is; a bare (unquoted) new_key is escaped and wrapped.
    let key_literal = if new_key.len() >= 2 && new_key.starts_with('"') && new_key.ends_with('"') {
        new_key.to_string()
    } else {
        format!("\"{}\"", json_escape(new_key))
    };

    // Build a new STRING token by parsing a minimal object and extracting its KEY's STRING.
    let probe = format!("{{{key_literal}: 0}}");
    let new_green = crate::model::json::parse::parse(&probe).map_err(MutateError::Fragment)?;
    let new_root = SyntaxNode::new_root(new_green).clone_for_update();
    let new_key_node = new_root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::KEY)
        .ok_or_else(|| MutateError::Fragment("new key does not parse as a member".into()))?;
    let decoded_new_key = key_name_of(&new_key_node);

    // Sibling collision check: compare decoded-to-decoded (matches YAML's
    // `entry_key_name(&sib) == decoded_new_key` pattern), not raw new_key
    // against decoded siblings.
    let parent = member.parent().expect("member has parent");
    for sib in parent.children().filter(|n| n.kind() == SyntaxKind::MEMBER) {
        if sib == member {
            continue;
        }
        if let Some(sib_key_node) = sib.children().find(|n| n.kind() == SyntaxKind::KEY) {
            if key_name_of(&sib_key_node) == decoded_new_key {
                return Err(MutateError::Collision(decoded_new_key));
            }
        }
    }

    let new_str_tok = new_key_node
        .children_with_tokens()
        .find_map(|c| match c {
            rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::STRING => Some(t),
            _ => None,
        })
        .ok_or(MutateError::NotFound)?;

    key_node.splice_children(str_idx..str_idx + 1, vec![new_str_tok.into()]);
    Ok(())
}

pub(super) fn remark(tree: &SyntaxNode, path: &[Seg]) -> Result<(), MutateError> {
    match resolve(tree, path).ok_or(MutateError::NotFound)? {
        // A live item — an object member or an array element. Both are the
        // same splice: the node's own lines become `//` lines in place. Only
        // the anchor differs, so `Element` shares this arm.
        Target::Member(member) | Target::Element(member) => {
            // Live member → comment it out.
            // Find the container (parent OBJECT or ARRAY node).
            let container = member.parent().expect("member has parent");

            // Find the member's position in item order, by node identity.
            let anchored = collect_items_with_anchors(&container);
            let member_text = member.text().to_string().trim().to_string();
            let member_pos = anchored
                .iter()
                .position(|(_, a)| matches!(a, rowan::NodeOrToken::Node(n) if n == &member))
                .ok_or(MutateError::NotFound)?;
            let items: Vec<String> = anchored.into_iter().map(|(s, _)| s).collect();

            // Build comment text: prefix each line with "// ". A trailing
            // same-line comment (folded into this item by
            // `collect_items_with_anchors`) stays on the commented block's
            // last line — replacing the item outright would drop it.
            let mut commented: String = member_text
                .lines()
                .map(|l| format!("// {l}"))
                .collect::<Vec<_>>()
                .join("\n");
            if let Some(trailing) = trailing_comment_of_node(&member) {
                commented.push_str("  ");
                commented.push_str(&trailing);
            }

            let mut new_items = items.clone();
            new_items[member_pos] = commented;

            // Remark needs a line of its own. Inside a single-line container
            // a `//` would swallow the rest of the line, so the gesture does
            // not apply there (`docs/reference/BEHAVIOR_MATRIX.md` §Remark).
            if !container.text().to_string().contains('\n') {
                return Err(MutateError::Unsupported);
            }
            let new_text = rebuild_multiline(&container, &new_items);
            let new_container =
                parse_container_text(&new_text, container.kind() == SyntaxKind::OBJECT)?;
            replace_node(&container, new_container);
            Ok(())
        }
        Target::Comment(first_tok) => {
            // Standalone // block → un-comment and restore as member.
            let container = first_tok.parent().expect("comment has parent");

            let anchored = collect_items_with_anchors(&container);
            // Find this comment block's item by its first token's identity.
            let block_text = comment_block_text(&first_tok);
            let comment_pos = anchored
                .iter()
                .position(|(_, a)| matches!(a, rowan::NodeOrToken::Token(t) if t == &first_tok))
                .ok_or(MutateError::NotFound)?;
            let items: Vec<String> = anchored.into_iter().map(|(s, _)| s).collect();

            // Strip "// " (or "//") prefix from each line to recover member text.
            let member_text: String = block_text
                .lines()
                .map(|l| {
                    if let Some(rest) = l.trim_start().strip_prefix("// ") {
                        rest.to_string()
                    } else if let Some(rest) = l.trim_start().strip_prefix("//") {
                        rest.to_string()
                    } else {
                        l.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");

            // Split the recovered text into member fragments (a merged block
            // holds SEVERAL remarked members). Each fragment may carry a
            // trailing same-line comment (written by the remark direction
            // above, or by hand): split it off via the CST — the MEMBER
            // node's text excludes it — and re-merge it with TRAILING_MARKER
            // so `rebuild_*` keeps it last, after the comma.
            let is_object = container.kind() == SyntaxKind::OBJECT;
            let not_a_member = || MutateError::Fragment("comment is not a valid member".into());
            let frags = member_fragments(&member_text, is_object).ok_or_else(not_a_member)?;
            let first = frags.first().ok_or_else(not_a_member)?;

            let mut new_items = items.clone();
            new_items[comment_pos] = first.clone();
            for (k, frag) in frags.iter().skip(1).enumerate() {
                new_items.insert(comment_pos + 1 + k, frag.clone());
            }

            let new_text = rebuild_multiline(&container, &new_items);
            let new_container = parse_container_text(&new_text, is_object)?;
            replace_node(&container, new_container);
            Ok(())
        }
        // A `/* … */` block comment is read-only (`Node.read_only`), so the
        // gesture does not apply. Same variant as an inline-container item,
        // so a host sees one "not here" outcome, not two.
        Target::Block(_) => Err(MutateError::Unsupported),
    }
}

pub(super) fn edit_comment(tree: &SyntaxNode, path: &[Seg], text: &str) -> Result<(), MutateError> {
    // Validate: every line must start with "//".
    for line in text.lines() {
        if !line.trim_start().starts_with("//") {
            return Err(MutateError::Fragment(
                "every line of a comment must start with //".into(),
            ));
        }
    }

    let first_tok = match resolve(tree, path).ok_or(MutateError::NotFound)? {
        Target::Comment(t) => t,
        Target::Block(_) => {
            return Err(MutateError::Illegal("block comments are read-only".into()))
        }
        _ => {
            return Err(MutateError::Illegal(
                "path does not resolve to a comment".into(),
            ))
        }
    };

    let container = first_tok.parent().expect("comment has parent");
    let anchored = collect_items_with_anchors(&container);
    let comment_pos = anchored
        .iter()
        .position(|(_, a)| matches!(a, rowan::NodeOrToken::Token(t) if t == &first_tok))
        .ok_or(MutateError::NotFound)?;
    let items: Vec<String> = anchored.into_iter().map(|(s, _)| s).collect();

    let mut new_items = items.clone();
    new_items[comment_pos] = text.to_string();

    let is_multiline = container.text().to_string().contains('\n');
    let new_text = if is_multiline {
        rebuild_multiline(&container, &new_items)
    } else {
        rebuild_inline(&container, &new_items)
    };
    let new_container = parse_container_text(&new_text, container.kind() == SyntaxKind::OBJECT)?;
    replace_node(&container, new_container);
    Ok(())
}

pub(super) fn insert_comment(
    tree: &SyntaxNode,
    target: &MutTarget,
    text: &str,
) -> Result<(), MutateError> {
    // Validate: every non-blank line must start with "//". A blank line is
    // allowed so a fragment can carry a separator (used to keep an inserted
    // comment a distinct node, not merged into a neighbour).
    for line in text.lines() {
        if !line.trim().is_empty() && !line.trim_start().starts_with("//") {
            return Err(MutateError::Fragment(
                "every line of a comment must start with //".into(),
            ));
        }
    }

    let container = find_container(tree, &target.parent)?;
    let mut items = collect_items(&container);
    let idx = target.index.min(items.len());
    items.insert(idx, text.to_string());

    let is_multiline = container.text().to_string().contains('\n');
    let new_text = if is_multiline {
        rebuild_multiline(&container, &items)
    } else {
        rebuild_inline(&container, &items)
    };
    let new_container = parse_container_text(&new_text, container.kind() == SyntaxKind::OBJECT)?;
    replace_node(&container, new_container);
    Ok(())
}

/// `Mutation::SetTrailingComment` — set/change/clear the EOL `//` comment of the
/// keyed scalar at `path`. The comment sits *after* the member (past an optional
/// trailing comma), so the splice keeps everything up to and including the comma
/// and rewrites from there to the line's terminating newline. Reparsed in place.
pub(super) fn set_trailing_comment(
    tree: &SyntaxNode,
    path: &[Seg],
    comment: Option<&str>,
) -> Result<(), MutateError> {
    let cut_start = member_line_end(tree, path)?;
    let full = tree.to_string();
    let cut_end = full[cut_start..]
        .find('\n')
        .map(|i| cut_start + i)
        .unwrap_or(full.len());
    let tail = match comment {
        Some(c) => format!("  {}", c.trim()),
        None => String::new(),
    };
    let new_text = format!("{}{}{}", &full[..cut_start], tail, &full[cut_end..]);
    let green = crate::model::json::parse::parse(&new_text).map_err(MutateError::Fragment)?;
    let new_root = SyntaxNode::new_root(green).clone_for_update();
    let n = tree.children_with_tokens().count();
    let children: Vec<_> = new_root.children_with_tokens().collect();
    tree.splice_children(0..n, children);
    Ok(())
}

/// The byte offset just past the member/element at `path` **and its separator
/// comma** — everything on the node's own line that belongs to the node. The
/// one implementation of the comma rule, shared by
/// `Mutation::SetTrailingComment` (which rewrites from here to the newline)
/// and [`extent_end_offset`] (which advances from here to past the newline).
pub(super) fn member_line_end(tree: &SyntaxNode, path: &[Seg]) -> Result<usize, MutateError> {
    // A keyed member or an array element (its VALUE node); both end the line the
    // same way, so the walk is identical.
    let anchor = match resolve(tree, path).ok_or(MutateError::NotFound)? {
        Target::Member(m) => m,
        Target::Element(v) => v,
        _ => return Err(MutateError::Unsupported),
    };
    let mut cut_start: usize = anchor.text_range().end().into();
    let mut sib = anchor.next_sibling_or_token();
    while let Some(s) = sib {
        match &s {
            rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::WHITESPACE => {
                sib = t.next_sibling_or_token();
            }
            rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::COMMA => {
                cut_start = t.text_range().end().into();
                break;
            }
            _ => break,
        }
    }
    Ok(cut_start)
}

/// The byte offset just past the node at `path`'s line — the anchor
/// `Mutation::SetTrailingBlankLines` splices at. A member's separator comma
/// belongs to its own line, so it stays *before* the blank run; anything else
/// on the line (a `//` trailing comment) does too. A standalone `//` **block**
/// anchors past its *last* line, not its first: consecutive `//` lines project
/// as one Comment node, so the run belongs after the whole block (a
/// `BLOCK_COMMENT` is read-only and stays `Unsupported`).
pub(crate) fn extent_end_offset(tree: &SyntaxNode, path: &[Seg]) -> Result<usize, MutateError> {
    let end = match resolve(tree, path) {
        Some(Target::Comment(tok)) => comment_block_end(&tok),
        _ => member_line_end(tree, path)?,
    };
    let full = tree.to_string();
    // A member of a single-line `{ … }`/`[ … ]` has no run of its own
    // (`blank_lines::owns_line_tail`).
    if !crate::model::blank_lines::owns_line_tail(&full, end, &["//", "/*"]) {
        return Err(MutateError::Unsupported);
    }
    Ok(crate::model::blank_lines::anchor_at(&full, end))
}

/// The byte offset just past the last `//` line of the standalone comment block
/// starting at `first` — the walk [`comment_block_text`] does, reported as an
/// offset instead of text, so the block's text and its extent cannot disagree.
pub(super) fn comment_block_end(first: &SyntaxToken) -> usize {
    let mut end: usize = first.text_range().end().into();
    let mut sib = first.next_sibling_or_token();
    let mut newlines = 0u32;
    while let Some(el) = sib {
        match el.kind() {
            SyntaxKind::WHITESPACE => {}
            SyntaxKind::NEWLINE => {
                newlines += 1;
                if newlines >= 2 {
                    break; // blank line ends the block
                }
            }
            SyntaxKind::LINE_COMMENT if newlines == 1 => {
                end = el.text_range().end().into();
                newlines = 0;
            }
            _ => break,
        }
        sib = el.next_sibling_or_token();
    }
    end
}

/// `Mutation::SetTrailingBlankLines` — rewrite the blank run after the member
/// at `path` to exactly `n` lines. The counting/normalization rule is the
/// shared `model::blank_lines` one; only the anchor is JSON's.
pub(super) fn set_trailing_blank_lines(
    tree: &SyntaxNode,
    path: &[Seg],
    n: usize,
) -> Result<(), MutateError> {
    let end = extent_end_offset(tree, path)?;
    let full = tree.to_string();
    let new_text = crate::model::blank_lines::splice(&full, end, n);
    let green = crate::model::json::parse::parse(&new_text).map_err(MutateError::Fragment)?;
    let new_root = SyntaxNode::new_root(green).clone_for_update();
    let count = tree.children_with_tokens().count();
    let children: Vec<_> = new_root.children_with_tokens().collect();
    tree.splice_children(0..count, children);
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────
