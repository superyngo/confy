//! The one text splice behind a Block edit (design record
//! `docs/spec/2026-09-15-block-edit-whole-file-reparse-design.md` §4).
//!
//! Deliberately format-agnostic: everything format-specific already happened
//! in `ConfigDocument::node_text_spans`, and everything semantic happens
//! after, in the backend's whole-document `Replace`.

/// `text` with `replacement` written over the **first** span and every later
/// span removed. Later spans are cut in reverse document order so the earlier
/// offsets stay valid; `spans` need not be sorted.
///
/// Consolidating at the first span is what makes a scattered `[T/S]` or a
/// `[T/D]` dotted table editable as one buffer: the user sees one block of
/// text, and it lands where the node's definition started.
pub(crate) fn splice_spans(text: &str, spans: &[(usize, usize)], replacement: &str) -> String {
    if spans.is_empty() {
        return text.to_string();
    }
    let mut ordered: Vec<(usize, usize)> = spans.to_vec();
    ordered.sort_by_key(|(a, _)| *a);
    let mut out = text.to_string();
    for &(a, b) in ordered.iter().skip(1).rev() {
        out.replace_range(a..b, "");
    }
    let (a, b) = ordered[0];
    out.replace_range(a..b, replacement);
    out
}

#[cfg(test)]
mod tests {
    use super::splice_spans;

    #[test]
    fn single_span_is_replaced_in_place() {
        assert_eq!(splice_spans("ab_cd", &[(2, 3)], "XY"), "abXYcd");
    }

    #[test]
    fn multi_span_replaces_the_first_and_deletes_the_rest() {
        assert_eq!(splice_spans("A1-B-A2", &[(0, 2), (5, 7)], "Z"), "Z-B-");
    }

    #[test]
    fn spans_out_of_order_are_handled_by_offset_not_by_argument_order() {
        assert_eq!(splice_spans("A1-B-A2", &[(5, 7), (0, 2)], "Z"), "Z-B-");
    }

    #[test]
    fn an_empty_span_list_returns_the_text_unchanged() {
        assert_eq!(splice_spans("abc", &[], "Z"), "abc");
    }

    #[test]
    fn three_spans_collapse_to_the_first() {
        assert_eq!(
            splice_spans("a1.b.a2.c.a3", &[(0, 2), (5, 7), (10, 12)], "Z"),
            "Z.b..c."
        );
    }
}
