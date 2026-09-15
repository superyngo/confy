//! YAML `node_text_spans` — the byte ranges a multi-line **Block** edit owns
//! (`ConfigDocument::node_text_spans`, design record
//! `docs/spec/2026-09-15-block-edit-whole-file-reparse-design.md` §3).
//!
//! Two YAML-specific rules:
//!
//! 1. A span starts at the **line's indentation**, not at the key token. A
//!    Block is edited verbatim — no dedent on open, no reindent on commit — so
//!    the indent must be inside the span or the re-splice would lose it.
//! 2. An **opaque** node (anchor/alias/tag, `SyntaxKind::OPAQUE`) still gets a
//!    span. It is read-only in the narrow sense — no rename, kind switch,
//!    remark or paste-into, and `extent_end_offset` keeps refusing it, so every
//!    *structural* Mutation still answers `Unsupported` — but its **text** is
//!    editable, exactly as whole-document editing already rewrites it
//!    (`yaml/edit/mod.rs`'s guard skips the empty path). Opaque fencing is
//!    recomputed from scratch on every parse (`yaml/parse.rs`), so a span
//!    handed out here never outlives the reparse that follows.

use super::resolve::{extent_end_offset, resolve_in};
use crate::model::blank_lines;
use crate::model::node::Seg;
use crate::model::yaml::project::{walk, Target};
use crate::model::yaml::syntax::{SyntaxKind, SyntaxNode};

/// The single span the node at `path` owns, or empty when `path` does not
/// resolve.
pub(crate) fn node_text_spans(tree: &SyntaxNode, path: &[Seg]) -> Vec<(usize, usize)> {
    let full = tree.to_string();
    let (proj, idx) = walk(tree, "");
    let Some(target) = resolve_in(&idx, path) else {
        return Vec::new();
    };
    // Two range sources, each right for a different shape:
    //
    // - the **index target** is entry-scoped (`key: value`, `- value`), which
    //   is what a Block needs;
    // - the **projection**'s `text_range` is *value*-scoped for a leaf, but it
    //   is the only per-element answer for a flow-seq element, which shares
    //   the whole `FLOW_SEQ` as its index target (the ordinal lives in the
    //   path and `edit::flow` resolves it by counting items).
    let Some(node) = proj.node_at(path) else {
        return Vec::new();
    };
    let (node_start, node_end) = match &target {
        // A multi-line `#` block projects as ONE Comment node, but its
        // `text_range` (like the index's token) covers only the **first**
        // line, so the block's end has to be walked here.
        Target::Comment(_) => (
            node.text_range.start,
            comment_block_end(node.text_range.end, &full),
        ),
        Target::MapEntry(n) | Target::Element(n) | Target::Opaque(n) => {
            if matches!(n.kind(), SyntaxKind::FLOW_MAP | SyntaxKind::FLOW_SEQ) {
                (node.text_range.start, node.text_range.end)
            } else {
                (
                    usize::from(n.text_range().start()),
                    usize::from(n.text_range().end()),
                )
            }
        }
    };
    // Whether the node owns its line has to be asked of the node's **own**
    // start: a flow member shares its line with its siblings, and backing up
    // to the line start first would make every member look line-owning.
    let owns_line = owns_its_line(&full, node_start);
    // Rule 1: a line-owning node's span starts at the indentation, so the
    // indent travels with the Block.
    let start = if owns_line {
        line_start_floor(&full, node_start)
    } else {
        node_start
    };
    let span = if owns_line {
        // `extent_end_offset` covers a block node's more-indented children,
        // which `text_range` alone does not. It refuses an opaque node/value
        // (rule 2) and reports a comment block's *first* line only, so those
        // two fall back to the range resolved above.
        let end = match (&target, extent_end_offset(tree, &idx, path)) {
            (Target::Comment(_), _) => node_end,
            (_, Ok(end)) => end,
            (_, Err(_)) => blank_lines::line_boundary_at(&full, node_end),
        };
        (start, end + blank_lines::measure(&full, end).1)
    } else {
        // A flow member: its own trimmed range, so the separating comma and
        // the container's padding stay outside.
        trimmed(&full, node_start, node_end)
    };
    debug_assert!(span.0 <= span.1, "inverted YAML span {span:?}");
    vec![span]
}

/// The byte offset just past the last `#` line of the comment block whose
/// first line ends at `first_line_end`: consecutive comment lines, ended by a
/// blank line or any real node — the same run `CommentAccumulator` merges into
/// one projected node.
fn comment_block_end(first_line_end: usize, full: &str) -> usize {
    let mut end = blank_lines::line_boundary_at(full, first_line_end);
    loop {
        let rest = &full[end..];
        let line = rest.split_inclusive('\n').next().unwrap_or("");
        if line.trim_start().starts_with('#') {
            end += line.len();
        } else {
            return end;
        }
    }
}

/// `at`'s own line start — never past `at`.
fn line_start_floor(full: &str, at: usize) -> usize {
    full[..at].rfind('\n').map_or(0, |i| i + 1)
}

/// Is `start` the first non-whitespace content on its line? False for a member
/// sharing a line with a flow collection's other members.
fn owns_its_line(full: &str, start: usize) -> bool {
    full[line_start_floor(full, start)..start].trim().is_empty()
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
    use crate::model::node::Seg;
    use crate::model::yaml::doc::YamlDocument;

    fn spans_of(src: &str, path: &[Seg]) -> Vec<String> {
        let doc = YamlDocument::from_str(src).unwrap();
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
    fn mapping_leaf_span_keeps_its_indentation() {
        assert_eq!(
            spans_of("a:\n  b: 1\n  c: 2\n", &[key("a"), key("b")]),
            vec!["  b: 1\n"]
        );
    }

    #[test]
    fn mapping_branch_span_covers_its_children() {
        assert_eq!(
            spans_of("a:\n  b: 1\nz: 9\n", &[key("a")]),
            vec!["a:\n  b: 1\n"]
        );
    }

    #[test]
    fn leaf_span_includes_its_trailing_blank_run() {
        assert_eq!(
            spans_of("a: 1\n\n\nb: 2\n", &[key("a")]),
            vec!["a: 1\n\n\n"]
        );
    }

    #[test]
    fn sequence_item_span_is_its_own_line() {
        assert_eq!(
            spans_of("a:\n  - one\n  - two\n", &[key("a"), Seg::Index(1)]),
            vec!["  - two\n"]
        );
    }

    #[test]
    fn flow_sequence_element_span_is_trimmed_and_excludes_the_comma() {
        assert_eq!(
            spans_of("a: [ 1, 22 ]\n", &[key("a"), Seg::Index(1)]),
            vec!["22"]
        );
    }

    #[test]
    fn comment_node_span_is_its_whole_block() {
        assert_eq!(
            spans_of("# one\n# two\na: 1\n", &[Seg::Index(0)]),
            vec!["# one\n# two\n"]
        );
    }

    #[test]
    fn opaque_valued_entry_is_editable_as_a_block() {
        // Policy (design record §5): an alias entry is read-only
        // *structurally*, but its text is a Block — `E` already rewrites it.
        assert_eq!(
            spans_of("ref: *anchor\nk: 1\n", &[key("ref")]),
            vec!["ref: *anchor\n"]
        );
    }

    #[test]
    fn unresolvable_path_yields_no_spans() {
        assert!(spans_of("a: 1\n", &[key("nope")]).is_empty());
    }
}
