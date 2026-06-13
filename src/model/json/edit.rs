//! JSON rowan splice helpers: one fn per `Mutation` variant (mirrors `cst_edit.rs`).

use crate::model::document::{KindTarget, MutateError, Mutation, OnCollision, Target as MutTarget};
use crate::model::json::project::{walk, Target};
use crate::model::json::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use crate::model::node::Seg;

/// Resolve `path` to its source element using the projection's index.
pub(crate) fn resolve(syntax: &SyntaxNode, path: &[Seg]) -> Option<Target> {
    let (_, idx) = walk(syntax, "");
    idx.into_iter().find(|(p, _)| p == path).map(|(_, t)| t)
}

/// Serialize the node at `path` as a standalone fragment.
pub fn serialize_fragment(syntax: &SyntaxNode, path: &[Seg]) -> String {
    match resolve(syntax, path) {
        Some(Target::Member(m)) => m.text().to_string().trim().to_string(),
        Some(Target::Element(v)) => v.text().to_string().trim().to_string(),
        Some(Target::Comment(tok)) => comment_block_text(&tok),
        Some(Target::Block(tok)) => tok.text().to_string(),
        None => String::new(),
    }
}

/// Re-collect a merged standalone `//` block from its first token: consecutive
/// LINE_COMMENT tokens separated only by a single NEWLINE (+ optional indent
/// WHITESPACE) join with `\n`. A second consecutive NEWLINE ends the block.
fn comment_block_text(first: &SyntaxToken) -> String {
    let mut out = vec![first.text().trim_end().to_string()];
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
                out.push(
                    el.as_token()
                        .unwrap()
                        .text()
                        .trim_end()
                        .to_string(),
                );
                newlines = 0;
            }
            _ => break,
        }
        sib = el.next_sibling_or_token();
    }
    out.join("\n")
}

/// Backstop after a splice: re-parse and reject duplicate object keys (Collision)
/// or structural breakage (Illegal). Mirrors cst_edit's DOM check.
fn validate_semantics(tree: &SyntaxNode) -> Result<(), MutateError> {
    let text = tree.to_string();
    let green = crate::model::json::parse::parse(&text).map_err(MutateError::Illegal)?;
    let reparsed = SyntaxNode::new_root(green);
    for obj in reparsed
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::OBJECT)
    {
        let mut seen = std::collections::HashSet::new();
        for member in obj.children().filter(|n| n.kind() == SyntaxKind::MEMBER) {
            if let Some(key) = member.children().find(|n| n.kind() == SyntaxKind::KEY) {
                let name = key.text().to_string();
                if !seen.insert(name.clone()) {
                    return Err(MutateError::Collision(name.trim_matches('"').to_string()));
                }
            }
        }
    }
    Ok(())
}

// ── Per-variant stubs (filled in by later tasks) ────────────────────────────

fn replace(tree: &SyntaxNode, path: &[Seg], fragment: &str) -> Result<(), MutateError> {
    if path.is_empty() {
        // Whole-document replace: parse fragment as a full JSON doc, splice its
        // ROOT children over the old ROOT children.
        // We need mutable SyntaxElements from a separate mutable tree.
        let green =
            crate::model::json::parse::parse(fragment).map_err(MutateError::Fragment)?;
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
            Ok(())
        }
        Target::Element(value) => {
            let new_value = parse_value_fragment(fragment)?;
            replace_node(&value, new_value);
            Ok(())
        }
        Target::Comment(_) | Target::Block(_) => {
            Err(MutateError::Illegal("use EditComment to edit a comment".into()))
        }
    }
}

/// Replace a node in a mutable tree by splicing over its slot in its parent.
fn replace_node(old: &SyntaxNode, new: SyntaxNode) {
    let parent = old.parent().expect("node has a parent");
    let idx = old.index();
    parent.splice_children(idx..idx + 1, vec![new.into()]);
}

/// Parse `fragment` as one bare VALUE (clone_for_update). Error `Fragment` if it
/// is not exactly one value.
fn parse_value_fragment(fragment: &str) -> Result<SyntaxNode, MutateError> {
    let green =
        crate::model::json::parse::parse(fragment).map_err(MutateError::Fragment)?;
    let root = SyntaxNode::new_root(green);
    let value = root
        .children()
        .find(|n| n.kind() == SyntaxKind::VALUE)
        .ok_or_else(|| MutateError::Fragment("fragment is not a value".into()))?;
    Ok(value.clone_for_update())
}

/// Parse `fragment` as a `"key": value` member by wrapping it in `{ … }`. Returns
/// None if it isn't a single member.
fn parse_member_fragment(fragment: &str) -> Option<SyntaxNode> {
    let wrapped = format!("{{{fragment}}}");
    let green = crate::model::json::parse::parse(&wrapped).ok()?;
    let root = SyntaxNode::new_root(green);
    let obj = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::OBJECT)?;
    let members: Vec<_> = obj
        .children()
        .filter(|n| n.kind() == SyntaxKind::MEMBER)
        .collect();
    if members.len() == 1 {
        Some(members[0].clone_for_update())
    } else {
        None
    }
}

fn delete(tree: &SyntaxNode, path: &[Seg]) -> Result<(), MutateError> {
    match resolve(tree, path).ok_or(MutateError::NotFound)? {
        Target::Member(m) => delete_item(&m),
        Target::Element(v) => delete_item(&v),
        Target::Comment(tok) | Target::Block(tok) => delete_comment_tokens(&tok),
    }
    Ok(())
}

/// Delete a MEMBER or VALUE (array element) node from its parent, removing the
/// associated comma and leading indent+newline so the result is well-formed.
fn delete_item(node: &SyntaxNode) {
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
fn delete_comment_tokens(first_tok: &SyntaxToken) {
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

fn insert(
    tree: &SyntaxNode,
    target: &MutTarget,
    fragment: &str,
    on_collision: OnCollision,
) -> Result<(), MutateError> {
    // ── 1. Locate the container OBJECT or ARRAY node ────────────────────────
    let container = find_container(tree, &target.parent)?;

    let is_object = container.kind() == SyntaxKind::OBJECT;
    let is_multiline = container.text().to_string().contains('\n');

    // ── 2. Collect existing items as verbatim strings ───────────────────────
    // Items are MEMBER nodes (objects) or VALUE nodes (arrays), plus standalone
    // comment tokens (LINE_COMMENT / BLOCK_COMMENT). We capture them in
    // projected order — same slot ordering the TUI uses.
    let mut items: Vec<String> = collect_items(&container);

    // ── 3. Adapt the fragment to the destination ────────────────────────────
    let (new_item_text, new_key) = adapt_fragment(fragment, is_object)?;

    // ── 4. Collision check (objects only) ───────────────────────────────────
    let mut final_key = new_key.clone();
    if is_object {
        if let Some(key) = &final_key {
            let existing_keys: Vec<String> = existing_object_keys(&container);
            if existing_keys.iter().any(|k| k == key) {
                match on_collision {
                    OnCollision::Cancel => {
                        return Err(MutateError::Collision(key.clone()));
                    }
                    OnCollision::Overwrite => {
                        // Remove the colliding member from items list; we'll insert the new one.
                        let collision_idx = items.iter().position(|it| {
                            member_key_of_text(it).as_deref() == Some(key.as_str())
                        });
                        if let Some(ci) = collision_idx {
                            items.remove(ci);
                        }
                    }
                    OnCollision::Rename => {
                        // Find a free name: key_2, key_3, …
                        let mut n = 2usize;
                        loop {
                            let candidate = format!("{key}_{n}");
                            if !existing_keys.iter().any(|k| k == &candidate) {
                                final_key = Some(candidate.clone());
                                break;
                            }
                            n += 1;
                        }
                    }
                }
            }
        }
    }

    // Re-render the item text with the (possibly renamed) key.
    let item_text = if let Some(key) = &final_key {
        if new_key.as_deref() != Some(key.as_str()) {
            // Key was renamed — rebuild from fragment value part.
            let value_part = bare_value_of_fragment(fragment).unwrap_or(fragment);
            format!("\"{key}\": {value_part}")
        } else {
            new_item_text
        }
    } else {
        new_item_text
    };

    // ── 5. Insert at the projected index ────────────────────────────────────
    let idx = target.index.min(items.len());
    items.insert(idx, item_text);

    // ── 6. Rebuild the container as a string and splice ─────────────────────
    let new_container_text = if is_multiline {
        rebuild_multiline(&container, &items)
    } else {
        rebuild_inline(&container, &items)
    };

    // Parse the rebuilt container as a full document to get a mutable node.
    let new_container = parse_container_text(&new_container_text, is_object)?;
    replace_node(&container, new_container);
    Ok(())
}

// ── Helpers for insert ───────────────────────────────────────────────────────

/// Walk from the tree root down `parent` path to the innermost OBJECT or ARRAY node.
fn find_container(tree: &SyntaxNode, parent: &[Seg]) -> Result<SyntaxNode, MutateError> {
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

fn key_name_of(key_node: &SyntaxNode) -> String {
    let text = key_node.text().to_string();
    let t = text.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}

/// Collect items (members/elements/comments) as verbatim trimmed strings.
/// Order matches projection order (same as children_with_tokens order for
/// MEMBER/VALUE/comment tokens, skipping punctuation and trivia).
fn collect_items(container: &SyntaxNode) -> Vec<String> {
    let mut items = Vec::new();
    for child in container.children_with_tokens() {
        match &child {
            rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::MEMBER => {
                items.push(n.text().to_string().trim().to_string());
            }
            rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::VALUE => {
                items.push(n.text().to_string().trim().to_string());
            }
            rowan::NodeOrToken::Token(t)
                if t.kind() == SyntaxKind::LINE_COMMENT
                    || t.kind() == SyntaxKind::BLOCK_COMMENT =>
            {
                items.push(t.text().trim_end().to_string());
            }
            _ => {}
        }
    }
    items
}

/// Extract key names from all MEMBER children of an OBJECT container.
fn existing_object_keys(container: &SyntaxNode) -> Vec<String> {
    container
        .children()
        .filter(|n| n.kind() == SyntaxKind::MEMBER)
        .filter_map(|m| m.children().find(|c| c.kind() == SyntaxKind::KEY))
        .map(|k| key_name_of(&k))
        .collect()
}

/// Given item text of a member, extract its key name (strips surrounding quotes).
fn member_key_of_text(text: &str) -> Option<String> {
    let t = text.trim();
    if let Some(rest) = t.strip_prefix('"') {
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    } else {
        None
    }
}

/// Parse `fragment` and decide how to adapt it for the destination.
///
/// - keyed (`"k": v`) → member text as-is; key = Some("k")
/// - bare value → for objects: synthesize `"placeholder": <value>`; for arrays: use as-is
///
/// Returns `(item_text, Option<key_name>)`.
fn adapt_fragment(
    fragment: &str,
    is_object: bool,
) -> Result<(String, Option<String>), MutateError> {
    if let Some(member) = parse_member_fragment(fragment) {
        // Keyed fragment.
        let key = member
            .children()
            .find(|n| n.kind() == SyntaxKind::KEY)
            .map(|k| key_name_of(&k))
            .unwrap_or_default();
        let text = member.text().to_string().trim().to_string();
        if is_object {
            Ok((text, Some(key)))
        } else {
            // keyed fragment into ARRAY → wrap as single-member object element `{ "k": v }`
            Ok((format!("{{ {text} }}"), None))
        }
    } else {
        // Bare value fragment.
        // Validate it parses as a value.
        parse_value_fragment(fragment)?;
        let val = fragment.trim().to_string();
        if is_object {
            // Synthesize placeholder key.
            Ok((format!("\"placeholder\": {val}"), Some("placeholder".to_string())))
        } else {
            Ok((val, None))
        }
    }
}

/// Extract the bare value part from a keyed fragment `"k": <value>`, or return None.
fn bare_value_of_fragment(fragment: &str) -> Option<&str> {
    // Find the colon and return everything after it (trimmed).
    let colon = fragment.find(':')?;
    let after = fragment[colon + 1..].trim();
    Some(after)
}

/// Returns true if an item string represents a comment (LINE_COMMENT or BLOCK_COMMENT).
fn is_comment_item(item: &str) -> bool {
    let t = item.trim();
    t.starts_with("//") || t.starts_with("/*")
}

/// Rebuild a MULTILINE container (object or array) from item strings.
/// Detects indent from the first existing member and the container's own closing indent.
fn rebuild_multiline(container: &SyntaxNode, items: &[String]) -> String {
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
            // Comments are emitted as-is, one line per item (no comma).
            for line in item.lines() {
                lines.push(format!("{item_indent}{line}"));
            }
        } else {
            // Non-comment: comma if there is a later non-comment item.
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

/// Detect the whitespace before the closing R_BRACE or R_BRACK in a multiline container.
fn detect_close_indent(container: &SyntaxNode) -> String {
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
fn detect_indent(container: &SyntaxNode) -> String {
    // Walk children_with_tokens: after the first NEWLINE, collect WHITESPACE before
    // the first MEMBER/VALUE/comment.
    let mut after_newline = false;
    for child in container.children_with_tokens() {
        match &child {
            rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::NEWLINE => {
                after_newline = true;
            }
            rowan::NodeOrToken::Token(t)
                if t.kind() == SyntaxKind::WHITESPACE && after_newline =>
            {
                return t.text().to_string();
            }
            rowan::NodeOrToken::Node(n)
                if after_newline
                    && matches!(n.kind(), SyntaxKind::MEMBER | SyntaxKind::VALUE) =>
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
fn rebuild_inline(container: &SyntaxNode, items: &[String]) -> String {
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
fn parse_container_text(text: &str, _is_object: bool) -> Result<SyntaxNode, MutateError> {
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

fn rename(tree: &SyntaxNode, path: &[Seg], new_key: &str) -> Result<(), MutateError> {
    let member = match resolve(tree, path).ok_or(MutateError::NotFound)? {
        Target::Member(m) => m,
        _ => return Err(MutateError::Illegal("rename requires a member".into())),
    };

    // Sibling collision check: find parent container and check other members' keys.
    let parent = member.parent().expect("member has parent");
    for sib in parent.children().filter(|n| n.kind() == SyntaxKind::MEMBER) {
        if sib == member {
            continue;
        }
        if let Some(key_node) = sib.children().find(|n| n.kind() == SyntaxKind::KEY) {
            if key_name_of(&key_node) == new_key {
                return Err(MutateError::Collision(new_key.to_string()));
            }
        }
    }

    // Locate the KEY node inside the member, then find its STRING token.
    let key_node = member
        .children()
        .find(|n| n.kind() == SyntaxKind::KEY)
        .ok_or(MutateError::NotFound)?;

    // Find the STRING token's index among KEY's children_with_tokens.
    let children: Vec<_> = key_node.children_with_tokens().collect();
    let str_idx = children
        .iter()
        .position(|c| matches!(c, rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::STRING))
        .ok_or(MutateError::NotFound)?;

    // Build a new STRING token by parsing a minimal object and extracting its KEY's STRING.
    let probe = format!("{{\"{new_key}\": 0}}");
    let new_green = crate::model::json::parse::parse(&probe).map_err(MutateError::Fragment)?;
    let new_root = SyntaxNode::new_root(new_green).clone_for_update();
    let new_str_tok = new_root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::KEY)
        .and_then(|kn| {
            kn.children_with_tokens().find_map(|c| match c {
                rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::STRING => Some(t),
                _ => None,
            })
        })
        .ok_or(MutateError::NotFound)?;

    key_node.splice_children(str_idx..str_idx + 1, vec![new_str_tok.into()]);
    Ok(())
}

fn remark(tree: &SyntaxNode, path: &[Seg]) -> Result<(), MutateError> {
    match resolve(tree, path).ok_or(MutateError::NotFound)? {
        Target::Member(member) => {
            // Live member → comment it out.
            // Find the container (parent OBJECT or ARRAY node).
            let container = member.parent().expect("member has parent");

            // Find the member's position in collect_items order.
            let items = collect_items(&container);
            let member_text = member.text().to_string().trim().to_string();
            let member_pos = items
                .iter()
                .position(|it| it.trim() == member_text.as_str())
                .ok_or(MutateError::NotFound)?;

            // Build comment text: prefix each line with "// ".
            let commented: String = member_text
                .lines()
                .map(|l| format!("// {l}"))
                .collect::<Vec<_>>()
                .join("\n");

            let mut new_items = items.clone();
            new_items[member_pos] = commented;

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
        Target::Comment(first_tok) => {
            // Standalone // block → un-comment and restore as member.
            let container = first_tok.parent().expect("comment has parent");

            let items = collect_items(&container);
            // Find the item that matches this comment block.
            let block_text = comment_block_text(&first_tok);
            let comment_pos = items
                .iter()
                .position(|it| it.trim() == block_text.trim())
                .ok_or(MutateError::NotFound)?;

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

            // Validate the recovered text parses as a member.
            parse_member_fragment(&member_text)
                .ok_or_else(|| MutateError::Fragment("comment is not a valid member".into()))?;

            let mut new_items = items.clone();
            new_items[comment_pos] = member_text;

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
        Target::Block(_) => Err(MutateError::Illegal("cannot remark a block comment".into())),
        Target::Element(_) => Err(MutateError::Illegal("cannot remark an array element".into())),
    }
}

fn edit_comment(tree: &SyntaxNode, path: &[Seg], text: &str) -> Result<(), MutateError> {
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
        Target::Block(_) => return Err(MutateError::Illegal("block comments are read-only".into())),
        _ => return Err(MutateError::Illegal("path does not resolve to a comment".into())),
    };

    let container = first_tok.parent().expect("comment has parent");
    let items = collect_items(&container);
    let block_text = comment_block_text(&first_tok);
    let comment_pos = items
        .iter()
        .position(|it| it.trim() == block_text.trim())
        .ok_or(MutateError::NotFound)?;

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

fn insert_comment(tree: &SyntaxNode, target: &MutTarget, text: &str) -> Result<(), MutateError> {
    // Validate: every line must start with "//".
    for line in text.lines() {
        if !line.trim_start().starts_with("//") {
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

#[allow(unused_variables)]
fn move_nodes(
    _tree: &SyntaxNode,
    _sources: &[Vec<Seg>],
    _target: &MutTarget,
    _on_collision: OnCollision,
) -> Result<(), MutateError> {
    Err(MutateError::Unsupported)
}

#[allow(unused_variables)]
fn convert_kind(_tree: &SyntaxNode, _path: &[Seg], _target: KindTarget) -> Result<(), MutateError> {
    Err(MutateError::Unsupported)
}

// ── Atomic dispatcher ────────────────────────────────────────────────────────

pub fn apply(syntax: &SyntaxNode, m: Mutation) -> Result<SyntaxNode, MutateError> {
    let tree = syntax.clone_for_update();
    match m {
        Mutation::Replace { path, fragment } => replace(&tree, &path, &fragment)?,
        Mutation::Delete { path } => delete(&tree, &path)?,
        Mutation::Insert {
            target,
            fragment,
            on_collision,
        } => insert(&tree, &target, &fragment, on_collision)?,
        Mutation::Rename { path, new_key } => rename(&tree, &path, &new_key)?,
        Mutation::Remark { path } => remark(&tree, &path)?,
        Mutation::EditComment { path, text } => edit_comment(&tree, &path, &text)?,
        Mutation::InsertComment { target, text } => insert_comment(&tree, &target, &text)?,
        Mutation::Move {
            sources,
            target,
            on_collision,
        } => move_nodes(&tree, &sources, &target, on_collision)?,
        Mutation::ConvertKind { path, target } => convert_kind(&tree, &path, target)?,
    }
    validate_semantics(&tree)?;
    Ok(tree)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::json::syntax::SyntaxNode;
    use crate::model::node::Seg;

    fn parse(src: &str) -> SyntaxNode {
        SyntaxNode::new_root(crate::model::json::parse::parse(src).unwrap())
    }

    #[test]
    fn fragment_of_member() {
        let t = parse("{\n  \"a\": 1,\n  \"b\": { \"x\": 2 }\n}\n");
        assert_eq!(serialize_fragment(&t, &[Seg::Key("a".into())]), "\"a\": 1");
        assert_eq!(
            serialize_fragment(&t, &[Seg::Key("b".into())]),
            "\"b\": { \"x\": 2 }"
        );
    }

    #[test]
    fn fragment_of_element() {
        let t = parse("[10, 20, 30]\n");
        assert_eq!(serialize_fragment(&t, &[Seg::Index(1)]), "20");
    }

    #[test]
    fn fragment_of_comment() {
        let t = parse("{\n  // hi\n  \"a\": 1\n}\n");
        assert_eq!(serialize_fragment(&t, &[Seg::Index(0)]), "// hi");
    }

    fn apply_str(src: &str, m: Mutation) -> String {
        let t = parse(src);
        super::apply(&t, m).unwrap().to_string()
    }

    #[test]
    fn replace_member_value() {
        let out = apply_str(
            "{\n  \"a\": 1\n}\n",
            Mutation::Replace {
                path: vec![Seg::Key("a".into())],
                fragment: "\"a\": 2".into(),
            },
        );
        assert_eq!(out, "{\n  \"a\": 2\n}\n");
    }

    #[test]
    fn replace_member_value_bare() {
        let out = apply_str(
            "{\n  \"a\": 1\n}\n",
            Mutation::Replace {
                path: vec![Seg::Key("a".into())],
                fragment: "2".into(),
            },
        );
        assert_eq!(out, "{\n  \"a\": 2\n}\n");
    }

    #[test]
    fn replace_element() {
        let out = apply_str(
            "[1, 2, 3]\n",
            Mutation::Replace {
                path: vec![Seg::Index(1)],
                fragment: "20".into(),
            },
        );
        assert_eq!(out, "[1, 20, 3]\n");
    }

    #[test]
    fn replace_whole_document() {
        let out = apply_str(
            "{ \"a\": 1 }\n",
            Mutation::Replace {
                path: vec![],
                fragment: "{ \"b\": 2 }\n".into(),
            },
        );
        assert_eq!(out, "{ \"b\": 2 }\n");
    }

    #[test]
    fn replace_invalid_fragment_rejected() {
        let t = parse("{ \"a\": 1 }\n");
        let r = super::apply(
            &t,
            Mutation::Replace {
                path: vec![Seg::Key("a".into())],
                fragment: "@@@".into(),
            },
        );
        assert!(matches!(r, Err(MutateError::Fragment(_)) | Err(MutateError::Illegal(_))));
    }

    #[test]
    fn stubbed_mutations_unsupported() {
        let t = parse("{ \"a\": 1 }\n");
        // Move is still stubbed — use it to verify Unsupported.
        let r = apply(
            &t,
            Mutation::Move {
                sources: vec![vec![Seg::Key("a".into())]],
                target: crate::model::document::Target { parent: vec![], index: 0 },
                on_collision: OnCollision::Cancel,
            },
        );
        assert!(matches!(r, Err(MutateError::Unsupported)));
    }

    #[test]
    fn delete_middle_member() {
        let out = apply_str(
            "{\n  \"a\": 1,\n  \"b\": 2,\n  \"c\": 3\n}\n",
            Mutation::Delete { path: vec![Seg::Key("b".into())] },
        );
        assert_eq!(out, "{\n  \"a\": 1,\n  \"c\": 3\n}\n");
    }

    #[test]
    fn delete_last_member_fixes_comma() {
        let out = apply_str(
            "{\n  \"a\": 1,\n  \"b\": 2\n}\n",
            Mutation::Delete { path: vec![Seg::Key("b".into())] },
        );
        assert_eq!(out, "{\n  \"a\": 1\n}\n");
    }

    #[test]
    fn delete_only_member() {
        let out = apply_str(
            "{\n  \"a\": 1\n}\n",
            Mutation::Delete { path: vec![Seg::Key("a".into())] },
        );
        // The splice removes NEWLINE + WHITESPACE before "a" and the MEMBER node.
        // No comma exists, so the result is "{\n}" — valid JSON.
        assert_eq!(out, "{\n}\n");
    }

    #[test]
    fn delete_middle_element() {
        let out = apply_str(
            "[1, 2, 3]\n",
            Mutation::Delete { path: vec![Seg::Index(1)] },
        );
        assert_eq!(out, "[1, 3]\n");
    }

    #[test]
    fn delete_last_element() {
        let out = apply_str(
            "[1, 2]\n",
            Mutation::Delete { path: vec![Seg::Index(1)] },
        );
        assert_eq!(out, "[1]\n");
    }

    #[test]
    fn delete_comment() {
        let out = apply_str(
            "{\n  // gone\n  \"a\": 1\n}\n",
            Mutation::Delete { path: vec![Seg::Index(0)] },
        );
        assert_eq!(out, "{\n  \"a\": 1\n}\n");
    }

    use crate::model::document::{OnCollision, Target as MTarget};

    #[test]
    fn insert_member_into_object() {
        let out = apply_str(
            "{\n  \"a\": 1\n}\n",
            Mutation::Insert {
                target: MTarget { parent: vec![], index: 1 },
                fragment: "\"b\": 2".into(),
                on_collision: OnCollision::Cancel,
            },
        );
        assert_eq!(out, "{\n  \"a\": 1,\n  \"b\": 2\n}\n");
    }

    #[test]
    fn insert_member_at_front() {
        let out = apply_str(
            "{\n  \"a\": 1\n}\n",
            Mutation::Insert {
                target: MTarget { parent: vec![], index: 0 },
                fragment: "\"b\": 2".into(),
                on_collision: OnCollision::Cancel,
            },
        );
        assert_eq!(out, "{\n  \"b\": 2,\n  \"a\": 1\n}\n");
    }

    #[test]
    fn insert_element_into_array() {
        let out = apply_str(
            "[1, 2]\n",
            Mutation::Insert {
                target: MTarget { parent: vec![], index: 2 },
                fragment: "3".into(),
                on_collision: OnCollision::Cancel,
            },
        );
        assert_eq!(out, "[1, 2, 3]\n");
    }

    #[test]
    fn insert_keyed_into_array_wraps() {
        let out = apply_str(
            "[1]\n",
            Mutation::Insert {
                target: MTarget { parent: vec![], index: 1 },
                fragment: "\"k\": 2".into(),
                on_collision: OnCollision::Cancel,
            },
        );
        assert_eq!(out, "[1, { \"k\": 2 }]\n");
    }

    #[test]
    fn insert_bare_into_object_placeholder() {
        let out = apply_str(
            "{\n  \"a\": 1\n}\n",
            Mutation::Insert {
                target: MTarget { parent: vec![], index: 1 },
                fragment: "42".into(),
                on_collision: OnCollision::Rename,
            },
        );
        assert_eq!(out, "{\n  \"a\": 1,\n  \"placeholder\": 42\n}\n");
    }

    #[test]
    fn insert_collision_cancels() {
        let t = parse("{ \"a\": 1 }\n");
        let r = super::apply(&t, Mutation::Insert {
            target: MTarget { parent: vec![], index: 1 },
            fragment: "\"a\": 2".into(),
            on_collision: OnCollision::Cancel,
        });
        assert!(matches!(r, Err(MutateError::Collision(_))));
    }

    #[test]
    fn insert_into_nested_multiline_object() {
        let out = apply_str(
            "{\n  \"o\": {\n    \"a\": 1\n  }\n}\n",
            Mutation::Insert {
                target: MTarget { parent: vec![Seg::Key("o".into())], index: 1 },
                fragment: "\"b\": 2".into(),
                on_collision: OnCollision::Cancel,
            },
        );
        assert_eq!(out, "{\n  \"o\": {\n    \"a\": 1,\n    \"b\": 2\n  }\n}\n");
    }

    #[test]
    fn rename_member_key() {
        let out = apply_str(
            "{ \"a\": 1 }\n",
            Mutation::Rename { path: vec![Seg::Key("a".into())], new_key: "b".into() },
        );
        assert_eq!(out, "{ \"b\": 1 }\n");
    }

    #[test]
    fn rename_collision() {
        let t = parse("{ \"a\": 1, \"b\": 2 }\n");
        let r = super::apply(&t, Mutation::Rename {
            path: vec![Seg::Key("a".into())], new_key: "b".into(),
        });
        assert!(matches!(r, Err(MutateError::Collision(_))));
    }

    #[test]
    fn remark_member_to_comment() {
        let out = apply_str(
            "{\n  \"a\": 1\n}\n",
            Mutation::Remark { path: vec![Seg::Key("a".into())] },
        );
        assert_eq!(out, "{\n  // \"a\": 1\n}\n");
    }

    #[test]
    fn remark_comment_to_member() {
        let out = apply_str(
            "{\n  // \"a\": 1\n}\n",
            Mutation::Remark { path: vec![Seg::Index(0)] },
        );
        assert_eq!(out, "{\n  \"a\": 1\n}\n");
    }

    #[test]
    fn edit_comment_text() {
        let out = apply_str(
            "{\n  // old\n  \"a\": 1\n}\n",
            Mutation::EditComment { path: vec![Seg::Index(0)], text: "// new".into() },
        );
        assert_eq!(out, "{\n  // new\n  \"a\": 1\n}\n");
    }

    #[test]
    fn edit_comment_rejects_non_comment() {
        let t = parse("{\n  // old\n  \"a\": 1\n}\n");
        let r = super::apply(&t, Mutation::EditComment { path: vec![Seg::Index(0)], text: "not a comment".into() });
        assert!(matches!(r, Err(MutateError::Fragment(_))));
    }

    #[test]
    fn insert_comment_block() {
        let out = apply_str(
            "{\n  \"a\": 1\n}\n",
            Mutation::InsertComment {
                target: crate::model::document::Target { parent: vec![], index: 0 },
                text: "// note".into(),
            },
        );
        assert_eq!(out, "{\n  // note\n  \"a\": 1\n}\n");
    }
}
