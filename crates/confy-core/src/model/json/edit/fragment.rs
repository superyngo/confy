//! Fragment parsing/adaptation helpers: what a caller's `Insert`/`Replace`
//! text means, and how it is reshaped for its destination — split out of
//! `json/edit.rs` (F6, 2026-09-09).

use super::*;

/// Parse `fragment` as one bare VALUE (clone_for_update). Error `Fragment` if it
/// is not exactly one value.
pub(super) fn parse_value_fragment(fragment: &str) -> Result<SyntaxNode, MutateError> {
    let green = crate::model::json::parse::parse(fragment).map_err(MutateError::Fragment)?;
    let root = SyntaxNode::new_root(green);
    let value = root
        .children()
        .find(|n| n.kind() == SyntaxKind::VALUE)
        .ok_or_else(|| MutateError::Fragment("fragment is not a value".into()))?;
    Ok(value.clone_for_update())
}

/// Parse `fragment` as a `"key": value` member by wrapping it in `{ … }`. Returns
/// None if it isn't a single member.
pub(super) fn parse_member_fragment(fragment: &str) -> Option<SyntaxNode> {
    let wrapped = format!("{{{fragment}\n}}");
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

/// Parse `fragment` as one bare array element by wrapping it in `[ … ]` —
/// the keyless counterpart of `parse_member_fragment`. Returns None if it
/// isn't exactly one element.
pub(super) fn parse_element_fragment(fragment: &str) -> Option<SyntaxNode> {
    let wrapped = format!("[{fragment}\n]");
    let green = crate::model::json::parse::parse(&wrapped).ok()?;
    let root = SyntaxNode::new_root(green);
    let arr = root.descendants().find(|n| n.kind() == SyntaxKind::ARRAY)?;
    let values: Vec<_> = arr
        .children()
        .filter(|n| n.kind() == SyntaxKind::VALUE)
        .collect();
    if values.len() == 1 {
        Some(values[0].clone_for_update())
    } else {
        None
    }
}

/// Split a recovered (un-`//`-prefixed) comment block into member item texts.
/// Each commented member contributed one or more `//` lines — a multi-line
/// member spans several — so accumulate greedily: extend the candidate until
/// it parses as a single member. A JSON value is self-delimiting (no proper
/// prefix of a member ever parses), so the greedy split is exact. Each
/// fragment's own trailing `//` comment (written by the remark direction or
/// by hand) splits off via the CST and re-merges with `TRAILING_MARKER` so
/// `rebuild_*` keeps it last, after the comma. Returns `None` if the block's
/// tail doesn't parse (e.g. an ordinary prose comment block).
pub(super) fn member_fragments(text: &str, is_object: bool) -> Option<Vec<String>> {
    let mut frags: Vec<String> = Vec::new();
    let mut candidate = String::new();
    for line in text.lines() {
        if candidate.is_empty() {
            candidate = line.to_string();
        } else {
            candidate.push('\n');
            candidate.push_str(line);
        }
        let parsed = if is_object {
            parse_member_fragment(&candidate)
        } else {
            parse_element_fragment(&candidate)
        };
        if let Some(node) = parsed {
            let bare = node.text().to_string().trim().to_string();
            let trailing = if is_object {
                fragment_member_trailing_comment(&candidate)
            } else {
                fragment_element_trailing_comment(&candidate)
            };
            let frag = match trailing {
                Some(c) => format!("{bare}{TRAILING_MARKER}{c}"),
                None => bare,
            };
            frags.push(frag);
            candidate = String::new();
        }
    }
    if candidate.trim().is_empty() {
        Some(frags)
    } else {
        None
    }
}

/// The external-edit fragment's own trailing `//` comment for an object
/// member (`"key": value  // c`), if it wrote one. Used so an edited comment
/// coming back from the popup editor / `$EDITOR` actually applies, instead of
/// `replace()`'s default of leaving the pre-edit comment untouched.
pub(crate) fn fragment_member_trailing_comment(fragment: &str) -> Option<String> {
    fragment_trailing_comment(&format!("{{{fragment}\n}}"))
}

/// Same as `fragment_member_trailing_comment` for a bare array-element
/// fragment (`value  // c`, no key) — wrapped under a synthetic key so the
/// same VALUE/COMMA/COMMENT position rules apply.
pub(crate) fn fragment_element_trailing_comment(fragment: &str) -> Option<String> {
    fragment_trailing_comment(&format!("{{\"__elem__\": {fragment}\n}}"))
}

/// Parse `wrapped` (a synthetic one-member JSON object) and read its single
/// member's projected trailing comment, or `None` if it doesn't parse as
/// exactly one member.
pub(super) fn fragment_trailing_comment(wrapped: &str) -> Option<String> {
    let green = crate::model::json::parse::parse(wrapped).ok()?;
    let root = SyntaxNode::new_root(green);
    let (tree, _) = walk(&root, "");
    tree.root.children.first()?.trailing_comment.clone()
}

/// Split an item produced by `collect_items_with_anchors` into its own text
/// and, if it carries one, its merged trailing same-line comment.
pub(super) fn split_trailing_marker(item: &str) -> (&str, Option<&str>) {
    match item.split_once(TRAILING_MARKER) {
        Some((main, comment)) => (main, Some(comment)),
        None => (item, None),
    }
}

/// Given item text of a member, extract its key name (strips surrounding quotes).
pub(super) fn member_key_of_text(text: &str) -> Option<String> {
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
/// - bare value → for objects: synthesize `"<suggested>": <value>` (falling back
///   to `"placeholder"` when no suggestion — e.g. the moved source wasn't a
///   keyed array's element); for arrays: use as-is
///
/// Returns `(item_text, Option<key_name>)`.
pub(super) fn adapt_fragment(
    fragment: &str,
    is_object: bool,
    suggested_key: Option<&str>,
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
            // Synthesize the caller's suggested key (`<arrayKey>_<index>` for a
            // moved array element) or the generic placeholder.
            let key = suggested_key.unwrap_or("placeholder");
            Ok((format!("\"{key}\": {val}"), Some(key.to_string())))
        } else {
            Ok((val, None))
        }
    }
}

/// Extract the bare value part from a keyed fragment `"k": <value>`, or return None.
pub(super) fn bare_value_of_fragment(fragment: &str) -> Option<&str> {
    // Find the colon and return everything after it (trimmed).
    let colon = fragment.find(':')?;
    let after = fragment[colon + 1..].trim();
    Some(after)
}

/// Returns true if an item string represents a comment (LINE_COMMENT or BLOCK_COMMENT).
pub(super) fn is_comment_item(item: &str) -> bool {
    let t = item.trim();
    t.starts_with("//") || t.starts_with("/*")
}

/// Escape the characters that would break a JSON string literal if `s` is
/// interpolated verbatim between quotes. Minimal on purpose (matches
/// `key_name_of`'s own minimal quote-stripping, not full JSON unescaping):
/// only the characters that can corrupt or prematurely terminate the probe
/// string are escaped.
pub(super) fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}
