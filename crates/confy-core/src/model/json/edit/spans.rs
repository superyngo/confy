//! JSON/JSONC `node_text_spans` — the byte ranges a multi-line **Block** edit
//! owns (`ConfigDocument::node_text_spans`, design record
//! `docs/spec/2026-09-15-block-edit-whole-file-reparse-design.md` §3).
//!
//! JSON has no scattered or dotted shapes, so **every JSON Block is exactly
//! one span**.
//!
//! The comma rule differs by position, which is the most confusable part of
//! this module:
//!
//! - a **full-line** member keeps its trailing `,` **inside** the span, because
//!   the comma terminates that line and a buffer replacing the line must be
//!   free to re-emit or drop it (`member_line_end` already walks over it);
//! - a **flow** member — one of several on a single line — keeps the `,`
//!   **outside**, because there the comma separates two members and belongs to
//!   the container, not to either of them.

use super::mutations::extent_end_offset;
use super::resolve::resolve;
use crate::model::blank_lines;
use crate::model::json::project::Target;
use crate::model::json::syntax::SyntaxNode;
use crate::model::node::Seg;

/// The single span the node at `path` owns, or empty when `path` does not
/// resolve.
pub(crate) fn node_text_spans(tree: &SyntaxNode, path: &[Seg]) -> Vec<(usize, usize)> {
    let full = tree.to_string();
    let Some(target) = resolve(tree, path) else {
        return Vec::new();
    };
    let (start, raw_end) = match &target {
        Target::Member(n) | Target::Element(n) => (
            usize::from(n.text_range().start()),
            usize::from(n.text_range().end()),
        ),
        Target::Comment(t) | Target::Block(t) => (
            usize::from(t.text_range().start()),
            usize::from(t.text_range().end()),
        ),
    };
    let span = match extent_end_offset(tree, path) {
        Ok(end) => {
            // A line-owning node absorbs the blank-line run that follows it;
            // the shared anchor deliberately stops in front of it.
            (start, end + blank_lines::measure(&full, end).1)
        }
        // `Unsupported` = the node shares its line with a flow collection's
        // other members, so it owns neither a line tail nor a blank run. Its
        // Block is its own trimmed token range.
        Err(_) => trimmed(&full, start, raw_end),
    };
    debug_assert!(span.0 <= span.1, "inverted JSON span {span:?}");
    vec![span]
}

/// `(start, end)` with surrounding whitespace excluded.
fn trimmed(full: &str, start: usize, end: usize) -> (usize, usize) {
    let s = &full[start..end.min(full.len())];
    let lead = s.len() - s.trim_start().len();
    let trail = s.len() - s.trim_end().len();
    (start + lead, start + s.len() - trail)
}

#[cfg(test)]
mod tests {
    use crate::model::document::ConfigDocument;
    use crate::model::json::doc::JsonDocument;
    use crate::model::node::Seg;

    fn spans_of(src: &str, path: &[Seg]) -> Vec<String> {
        let doc = JsonDocument::from_str(src).unwrap();
        let text = doc.serialize();
        doc.node_text_spans(path)
            .into_iter()
            .map(|(a, b)| text[a..b].to_string())
            .collect()
    }

    fn key(k: &str) -> Seg {
        Seg::Key(k.to_string())
    }

    #[test]
    fn object_member_span_is_its_own_line_with_comma_and_comment() {
        assert_eq!(
            spans_of("{\n  \"a\": 1, // note\n  \"b\": 2\n}\n", &[key("a")]),
            vec!["\"a\": 1, // note\n"]
        );
    }

    #[test]
    fn member_span_includes_its_trailing_blank_run() {
        assert_eq!(
            spans_of("{\n  \"a\": 1,\n\n\n  \"b\": 2\n}\n", &[key("a")]),
            vec!["\"a\": 1,\n\n\n"]
        );
    }

    #[test]
    fn nested_object_span_covers_the_whole_object() {
        assert_eq!(
            spans_of(
                "{\n  \"a\": {\n    \"x\": 1\n  },\n  \"b\": 2\n}\n",
                &[key("a")]
            ),
            vec!["\"a\": {\n    \"x\": 1\n  },\n"]
        );
    }

    #[test]
    fn inline_object_member_span_is_trimmed_and_excludes_the_comma() {
        assert_eq!(
            spans_of("{ \"a\": { \"x\": 1, \"y\": 2 } }\n", &[key("a"), key("x")]),
            vec!["\"x\": 1"]
        );
    }

    #[test]
    fn array_element_span_is_trimmed_and_excludes_the_comma() {
        assert_eq!(
            spans_of("{ \"a\": [ 1, 22 ] }\n", &[key("a"), Seg::Index(1)]),
            vec!["22"]
        );
    }

    #[test]
    fn line_comment_node_span_is_its_whole_block() {
        assert_eq!(
            spans_of("{\n  // one\n  // two\n  \"a\": 1\n}\n", &[Seg::Index(0)]),
            vec!["// one\n  // two\n"]
        );
    }

    #[test]
    fn unresolvable_path_yields_no_spans() {
        assert!(spans_of("{\"a\": 1}\n", &[key("nope")]).is_empty());
    }
}
