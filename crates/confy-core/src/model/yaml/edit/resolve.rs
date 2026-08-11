//! The indent engine (`reindent`), path resolver (`resolve`), and opaque
//! guard (`is_opaque`) — split out of `yaml/edit.rs` (Task 15, 2026-08-11
//! audit remediation).

use crate::model::node::Seg;
use crate::model::yaml::project::{walk, Target, YamlIndex};
use crate::model::yaml::syntax::SyntaxNode;

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
