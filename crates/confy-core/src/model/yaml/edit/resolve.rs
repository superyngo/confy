//! The indent engine (`reindent`), path resolver (`resolve`), and opaque
//! guard (`is_opaque`) — split out of `yaml/edit.rs` (Task 15, 2026-08-11
//! audit remediation).

use crate::model::document::MutateError;
use crate::model::node::Seg;
use crate::model::yaml::project::{walk, Target, YamlIndex};
use crate::model::yaml::syntax::{SyntaxKind, SyntaxNode};

/// Re-indent every line of `fragment` from `from` leading spaces to `to`.
/// Literal/folded block-scalar bodies shift with their header (uniform shift of
/// all lines preserves their *relative* indentation). Blank lines stay blank.
pub(crate) fn reindent(fragment: &str, from: usize, to: usize) -> String {
    let mut out = String::with_capacity(fragment.len());
    for line in fragment.split_inclusive('\n') {
        let (content, nl) = match line.strip_suffix('\n') {
            Some(c) => (c, "\n"),
            None => (line, ""),
        };
        if content.trim().is_empty() {
            out.push_str(content);
            out.push_str(nl);
            continue;
        }
        let stripped = content.strip_prefix(&" ".repeat(from)).unwrap_or(content);
        out.push_str(&" ".repeat(to));
        out.push_str(stripped);
        out.push_str(nl);
    }
    out
}

/// Resolve `path` to its source element using the projection's shared index.
/// Re-walks `syntax` (which may be a clone_for_update'd tree) so the returned
/// `Target` nodes are from the same tree as `syntax`.
pub(crate) fn resolve(syntax: &SyntaxNode, path: &[Seg]) -> Option<Target> {
    let (_, idx) = walk(syntax, "");
    resolve_in(&idx, path)
}

/// Resolve `path` against a prebuilt projection index (one `walk` shared across
/// every pre-mutation lookup in `apply` — a fresh `resolve` is only needed after
/// the tree has been spliced, when the old index is stale).
pub(crate) fn resolve_in(idx: &YamlIndex, path: &[Seg]) -> Option<Target> {
    idx.iter().find(|(p, _)| p == path).map(|(_, t)| t.clone())
}

/// Returns `true` if `path` itself or any strict ancestor path resolves to an
/// `Target::Opaque` — i.e. the path is inside (or is) an opaque span.
///
/// Precondition: `path` is non-empty. The root (`[]`) is never opaque and is
/// guarded out by the caller (`apply`); an empty path here always yields `false`.
pub(crate) fn is_opaque(idx: &YamlIndex, path: &[Seg]) -> bool {
    // Check the path itself first.
    if let Some(Target::Opaque(_)) = resolve_in(idx, path) {
        return true;
    }
    // Then check every strict prefix (ancestor).
    for len in 1..path.len() {
        if let Some(Target::Opaque(_)) = resolve_in(idx, &path[..len]) {
            return true;
        }
    }
    false
}

/// The byte offset just past the node at `path`'s **block extent** — its own
/// line plus every more-indented line beneath it — which is the anchor
/// `Mutation::SetTrailingBlankLines` splices at. Rejects an opaque node and an
/// entry whose *value* is opaque (every mutation on or into one is
/// `Unsupported`), and a flow member, which has no line of its own to trail.
pub(crate) fn extent_end_offset(
    tree: &SyntaxNode,
    idx: &YamlIndex,
    path: &[Seg],
) -> Result<usize, MutateError> {
    if is_opaque(idx, path) {
        return Err(MutateError::Unsupported);
    }
    let at = match resolve_in(idx, path).ok_or(MutateError::NotFound)? {
        Target::MapEntry(n) | Target::Element(n) => {
            if super::block::entry_has_opaque_value(&n) {
                return Err(MutateError::Unsupported);
            }
            // A flow member's node lives inside a `{…}`/`[…]` on one line, so
            // it has no trailing line of its own.
            if n.ancestors()
                .any(|a| matches!(a.kind(), SyntaxKind::FLOW_MAP | SyntaxKind::FLOW_SEQ))
            {
                return Err(MutateError::Unsupported);
            }
            usize::from(n.text_range().end())
        }
        // A comment *block*: past its last `#` line. Consecutive lines project
        // as one Comment node, so the run follows the whole block. The token
        // spans the block already (the lexer merges the run), so its own end is
        // the block's end.
        Target::Comment(t) => usize::from(t.text_range().end()),
        Target::Opaque(_) => return Err(MutateError::Unsupported),
    };
    let full = tree.to_string();
    // Belt and braces with the FLOW check above: the shared line-ownership rule
    // (`blank_lines::owns_line_tail`) states it for every backend.
    if !crate::model::blank_lines::owns_line_tail(&full, at, &["#"]) {
        return Err(MutateError::Unsupported);
    }
    Ok(crate::model::blank_lines::anchor_at(&full, at))
}
