//! TOML `node_text_spans` — the byte ranges a multi-line **Block** edit owns
//! (`ConfigDocument::node_text_spans`, design record
//! `docs/spec/2026-09-15-block-edit-whole-file-reparse-design.md` §3).
//!
//! Read-only: nothing here mutates a tree, so it takes a plain `&SyntaxNode`
//! rather than a `clone_for_update` copy. The end offsets come from the same
//! helpers `Delete` and `SetTrailingBlankLines` already use, so a Block covers
//! exactly the extent a delete would — plus that extent's trailing blank run,
//! which the shared anchor deliberately stops in front of.

use super::replace_delete::{
    extent_end_offset, section_end_strict_from, table_member_spans, MemberSpan,
};
use crate::model::blank_lines;
use crate::model::cst_project::{walk, Target};
use crate::model::node::Seg;
use taplo::syntax::SyntaxNode;

/// Every span the node at `path` owns, ascending and non-overlapping. Empty
/// when `path` does not resolve.
pub(crate) fn node_text_spans(tree: &SyntaxNode, path: &[Seg]) -> Vec<(usize, usize)> {
    let full = tree.to_string();
    let (_proj, idx) = walk(tree, "");

    // A table's definition is an open set of root-child pieces (`[T/S]`
    // scattered runs, `[T/D]` dotted member lines, and the mixed case), so ask
    // the member-span list first: it is the only source that sees every piece.
    // A contiguous table answers with adjacent pieces, which `merge` folds back
    // into the single span a reader expects.
    let mut raw: Vec<(usize, usize)> = member_spans(tree, &idx, path, &full);
    if raw.is_empty() {
        raw = single_span(tree, &idx, path, &full).into_iter().collect();
    }
    let mut out = merge(raw);
    // Each span absorbs the blank-line run that follows it — but only where it
    // ends at a line boundary. A flow member shares its line with its siblings
    // and owns no run of its own.
    for (_, end) in out.iter_mut() {
        if full[..*end].ends_with('\n') {
            *end += blank_lines::measure(&full, *end).1;
        }
    }
    out
}

/// The byte ranges of `path`'s table member spans, or empty when `path` is not
/// a root-level table.
fn member_spans(
    tree: &SyntaxNode,
    idx: &crate::model::cst_project::CstIndex,
    path: &[Seg],
    full: &str,
) -> Vec<(usize, usize)> {
    let els: Vec<_> = tree.children_with_tokens().collect();
    let idx_to_offset = |i: usize| -> usize {
        els.get(i)
            .map(|e| usize::from(e.text_range().start()))
            .unwrap_or_else(|| full.len())
    };
    table_member_spans(idx, path)
        .iter()
        .map(|s| match s {
            // One dotted member line: its own range, normalized to the line
            // boundary so the run absorption above sees a clean anchor.
            MemberSpan::Entry(n) => {
                let start = usize::from(n.text_range().start());
                let end = blank_lines::line_boundary_at(full, usize::from(n.text_range().end()));
                (start, end)
            }
            // One `[…]` run: header line through the next header of any kind,
            // so a parent and its sub-sections arrive as adjacent pieces and
            // `merge` folds them together.
            MemberSpan::Section(h) => (
                usize::from(h.text_range().start()),
                idx_to_offset(section_end_strict_from(h)),
            ),
        })
        .collect()
}

/// The one span of a non-table node: a keyed entry, an array element, an AoT
/// entry or group, or a comment block.
fn single_span(
    tree: &SyntaxNode,
    idx: &crate::model::cst_project::CstIndex,
    path: &[Seg],
    full: &str,
) -> Option<(usize, usize)> {
    let target = idx
        .iter()
        .find(|(p, _)| p == path)
        .map(|(_, t)| t.clone())?;
    let start = match &target {
        Target::Entry(n) | Target::ArrayElement(n) | Target::Header(n) | Target::AotEntry(n) => {
            usize::from(n.text_range().start())
        }
        Target::Comment(t) => usize::from(t.text_range().start()),
        // An AoT *group* is synthetic: its first `[[x]]` entry starts it, and
        // that entry is the first index row whose path extends this one.
        Target::AotGroup => idx
            .iter()
            .filter_map(|(p, t)| match t {
                Target::AotEntry(h) if p.len() > path.len() && p[..path.len()] == *path => {
                    Some(usize::from(h.text_range().start()))
                }
                _ => None,
            })
            .min()?,
    };
    match extent_end_offset(tree, path) {
        Ok(end) => Some((start, end)),
        // `Unsupported` = the node shares its line with a flow collection's
        // other members, so it has no line tail and no blank run. Its Block is
        // its own token range trimmed of the padding taplo bakes in
        // (`[ 1 ]` ⇒ `VALUE "1 "`); the separating `,` stays outside, because a
        // comma belongs to the container, not to any member.
        // An AoT *group* has no single token range to fall back on, and it
        // never shares a line with a flow collection, so this arm is not
        // reachable for it — refuse rather than build a backwards range.
        Err(_) => Some(trimmed(full, start, end_of(&target)?)),
    }
}

fn end_of(target: &Target) -> Option<usize> {
    match target {
        Target::Entry(n) | Target::ArrayElement(n) | Target::Header(n) | Target::AotEntry(n) => {
            Some(usize::from(n.text_range().end()))
        }
        Target::Comment(t) => Some(usize::from(t.text_range().end())),
        Target::AotGroup => None,
    }
}

/// `(start, end)` with surrounding whitespace excluded.
fn trimmed(full: &str, start: usize, end: usize) -> (usize, usize) {
    let s = &full[start..end.min(full.len())];
    let lead = s.len() - s.trim_start().len();
    let trail = s.len() - s.trim_end().len();
    (start + lead, start + s.len() - trail)
}

/// Sort ascending and fold spans that touch, so a contiguous section assembled
/// from a parent plus its sub-sections reads as the one span it looks like.
fn merge(mut spans: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    spans.sort_by_key(|(a, _)| *a);
    let mut out: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for (a, b) in spans {
        match out.last_mut() {
            Some(prev) if a <= prev.1 => prev.1 = prev.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::model::document::ConfigDocument;
    use crate::model::node::Seg;

    fn spans_of(src: &str, path: &[Seg]) -> Vec<String> {
        let doc = crate::model::cst_doc::CstDocument::from_str(src).unwrap();
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
    fn leaf_span_is_its_own_line() {
        assert_eq!(
            spans_of("[s]\nk1 = 1\nk2 = 2\n", &[key("s"), key("k1")]),
            vec!["k1 = 1\n"]
        );
    }

    #[test]
    fn leaf_span_includes_its_eol_comment_and_trailing_blank_run() {
        assert_eq!(
            spans_of("[s]\nk1 = 1  # note\n\n\nk2 = 2\n", &[key("s"), key("k1")]),
            vec!["k1 = 1  # note\n\n\n"]
        );
    }

    #[test]
    fn leaf_span_excludes_a_preceding_standalone_comment() {
        assert_eq!(
            spans_of("[s]\n# lead\nk1 = 1\n", &[key("s"), key("k1")]),
            vec!["k1 = 1\n"]
        );
    }

    #[test]
    fn section_span_covers_header_members_and_subsections() {
        assert_eq!(
            spans_of("[a]\nk = 1\n[a.sub]\nj = 2\n[b]\nm = 3\n", &[key("a")]),
            vec!["[a]\nk = 1\n[a.sub]\nj = 2\n"]
        );
    }

    #[test]
    fn scattered_table_yields_one_span_per_run_in_document_order() {
        assert_eq!(
            spans_of("[a]\nk = 1\n[b]\nm = 2\n[a]\nj = 3\n", &[key("a")]),
            vec!["[a]\nk = 1\n", "[a]\nj = 3\n"]
        );
    }

    #[test]
    fn dotted_table_yields_one_span_per_member_line() {
        assert_eq!(
            spans_of("a.x = 1\nb = 9\na.y = 2\n", &[key("a")]),
            vec!["a.x = 1\n", "a.y = 2\n"]
        );
    }

    #[test]
    fn array_element_span_is_trimmed_and_excludes_the_comma() {
        assert_eq!(
            spans_of("a = [ 1, 22 ]\n", &[key("a"), Seg::Index(1)]),
            vec!["22"]
        );
    }

    #[test]
    fn inline_table_member_span_is_trimmed_and_excludes_the_comma() {
        assert_eq!(
            spans_of("a = { x = 1, y = 2 }\n", &[key("a"), key("x")]),
            vec!["x = 1"]
        );
    }

    #[test]
    fn comment_node_span_is_its_whole_block_not_its_first_line() {
        // Consecutive `#` lines project as ONE Comment node, so its Block is
        // the whole run — the same extent `EditComment` rewrites.
        assert_eq!(
            spans_of("# one\n# two\nk = 1\n", &[Seg::Index(0)]),
            vec!["# one\n# two\n"]
        );
    }

    #[test]
    fn aot_entry_span_is_its_own_section() {
        assert_eq!(
            spans_of("[[p]]\nn = 1\n[[p]]\nn = 2\n", &[key("p"), Seg::Index(0)]),
            vec!["[[p]]\nn = 1\n"]
        );
    }

    #[test]
    fn aot_group_span_covers_every_entry() {
        assert_eq!(
            spans_of("[[p]]\nn = 1\n[[p]]\nn = 2\n", &[key("p")]),
            vec!["[[p]]\nn = 1\n[[p]]\nn = 2\n"]
        );
    }

    #[test]
    fn scattered_aot_group_yields_one_span_per_run() {
        // Spec §3's third multi-span shape: a foreign section splits the group,
        // so it is two spans, not the one a contiguous group merges to.
        assert_eq!(
            spans_of("[[p]]\nn = 1\n[q]\nm = 2\n[[p]]\nn = 3\n", &[key("p")]),
            vec!["[[p]]\nn = 1\n", "[[p]]\nn = 3\n"]
        );
    }

    #[test]
    fn unresolvable_path_yields_no_spans() {
        assert!(spans_of("k = 1\n", &[key("nope")]).is_empty());
    }
}
