//! Path→`Target` resolution and fragment serialization for the JSON/JSONC
//! backend — split out of `json/edit.rs` (F6, 2026-09-09).

use super::*;

/// Resolve `path` to its source element using the projection's index.
pub(crate) fn resolve(syntax: &SyntaxNode, path: &[Seg]) -> Option<Target> {
    let (_, idx) = walk(syntax, "");
    resolve_in(&idx, path)
}

/// Resolve `path` against a prebuilt projection index (one `walk` shared across
/// several pre-mutation lookups — stale once the tree has been spliced).
pub(super) fn resolve_in(
    idx: &crate::model::json::project::JsonIndex,
    path: &[Seg],
) -> Option<Target> {
    idx.iter().find(|(p, _)| p == path).map(|(_, t)| t.clone())
}

/// Serialize the node at `path` as a standalone fragment.
pub fn serialize_fragment(syntax: &SyntaxNode, path: &[Seg]) -> String {
    fragment_of(resolve(syntax, path))
}

/// The fragment text of a resolved target (shared by `serialize_fragment` and
/// index-based lookups that already hold a `Target`).
pub(super) fn fragment_of(target: Option<Target>) -> String {
    match target {
        Some(Target::Member(m)) => with_comment(
            m.text().to_string().trim().to_string(),
            trailing_comment_of_node(&m),
        ),
        Some(Target::Element(v)) => with_comment(
            v.text().to_string().trim().to_string(),
            trailing_comment_of_node(&v),
        ),
        Some(Target::Comment(tok)) => comment_block_text(&tok),
        Some(Target::Block(tok)) => tok.text().to_string(),
        None => String::new(),
    }
}

/// Append a node's own trailing `//` comment (if any) to its fragment text,
/// two spaces before the comment — matches the source's own inline spacing
/// convention (see `set_trailing_comment` below).
pub(super) fn with_comment(mut text: String, comment: Option<String>) -> String {
    if let Some(c) = comment {
        text.push_str("  ");
        text.push_str(&c);
    }
    text
}

/// Re-collect a merged standalone `//` block from its first token: consecutive
/// LINE_COMMENT tokens separated only by a single NEWLINE (+ optional indent
/// WHITESPACE) join with `\n`. A second consecutive NEWLINE ends the block.
pub(super) fn comment_block_text(first: &SyntaxToken) -> String {
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
                out.push(el.as_token().unwrap().text().trim_end().to_string());
                newlines = 0;
            }
            _ => break,
        }
        sib = el.next_sibling_or_token();
    }
    out.join("\n")
}
