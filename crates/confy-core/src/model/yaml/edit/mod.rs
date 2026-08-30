//! YAML mutation helpers.
//!
//! The indent engine (`reindent`), path resolver (`resolve`), and opaque guard
//! (`is_opaque`) feed the atomic dispatcher (`apply`), which routes each
//! `Mutation` variant to its byte-splice + whole-document reparse (via
//! `splice_byte_range`). Out-of-subset constructs reject as `Unsupported`.

mod block;
mod convert;
mod flow;
mod mutations;
mod resolve;

// Re-exported so external callers (yaml/doc.rs, model/convert.rs,
// yaml/project.rs) keep their existing crate::model::yaml::edit::X paths --
// pure code motion, the split shouldn't ripple into unrelated files.
pub(crate) use block::parse_map_entry_fragment;
pub(crate) use convert::decode_double;
pub use mutations::serialize_fragment;

use crate::model::document::{MutateError, Mutation};
use crate::model::node::Seg;
use crate::model::yaml::project::walk;
use crate::model::yaml::syntax::SyntaxNode;
use block::{delete, insert, replace};
use convert::convert_kind;
use mutations::{edit_comment, insert_comment, move_nodes, remark, rename, set_trailing_comment};
use resolve::is_opaque;

/// Backstop after a splice: re-parse and reject duplicate mapping keys
/// (Collision) or structural breakage (Illegal). Mirrors json/edit.rs's DOM
/// check using YAML re-parse + walk-based duplicate-key detection.
///
/// Returns the re-parsed **immutable** tree and its serialization. The mutation
/// runs on a `clone_for_update` tree that must be normalized back to an immutable
/// one anyway, and this re-parse already produces exactly that — so the caller
/// commits these instead of repeating the serialize + parse.
pub(crate) fn validate_semantics(tree: &SyntaxNode) -> Result<(SyntaxNode, String), MutateError> {
    let text = tree.to_string();
    let green = crate::model::yaml::parse::parse(&text).map_err(MutateError::Illegal)?;
    let reparsed = SyntaxNode::new_root(green);
    // Re-walk and check for duplicate keys at every mapping level.
    let (node_tree, _idx) = walk(&reparsed, "");
    check_duplicate_keys(&node_tree.root.children)?;
    Ok((reparsed, text))
}

/// Recursively check for duplicate key names among siblings at each level.
pub(crate) fn check_duplicate_keys(nodes: &[crate::model::node::Node]) -> Result<(), MutateError> {
    let mut seen = std::collections::HashSet::new();
    for node in nodes {
        if let crate::model::node::NodeKind::Comment(_) = &node.kind {
            // Comments use Index paths — not keyed, no collision.
        } else if let Some(Seg::Key(k)) = node.path.last() {
            if !seen.insert(k.clone()) {
                return Err(MutateError::Collision(k.clone()));
            }
        }
        check_duplicate_keys(&node.children)?;
    }
    Ok(())
}

/// Extract the primary path(s) from a mutation for the opaque pre-check.
pub(crate) fn mutation_paths(m: &Mutation) -> Vec<&Vec<Seg>> {
    match m {
        Mutation::Delete { path } => vec![path],
        Mutation::Insert { target, .. } => vec![&target.parent],
        Mutation::Replace { path, .. } => vec![path],
        Mutation::Rename { path, .. } => vec![path],
        Mutation::Remark { path } => vec![path],
        Mutation::EditComment { path, .. } => vec![path],
        Mutation::InsertComment { target, .. } => vec![&target.parent],
        Mutation::Move {
            sources, target, ..
        } => {
            let mut paths: Vec<&Vec<Seg>> = sources.iter().collect();
            paths.push(&target.parent);
            paths
        }
        Mutation::ConvertKind { path, .. } => vec![path],
        Mutation::SetTrailingComment { path, .. } => vec![path],
    }
}

/// Apply `m` to a copy of `syntax`, returning the new **immutable** tree and its
/// serialization — both produced by the single serialize + re-parse that
/// `validate_semantics` needs anyway, so the caller repeats neither.
pub fn apply(syntax: &SyntaxNode, m: Mutation) -> Result<(SyntaxNode, String), MutateError> {
    // One projection walk shared by the opaque pre-check and every variant's
    // initial (pre-mutation) resolve. Built on the clone so `Target`s point into
    // the tree the splices mutate. Post-splice lookups still re-resolve — the
    // index is stale once the tree changes.
    let tree = syntax.clone_for_update();
    let (proj, idx) = walk(&tree, "");

    // Opaque pre-check: any target path inside (or equal to) an opaque span → Unsupported.
    for path in mutation_paths(&m) {
        if !path.is_empty() && is_opaque(&idx, path) {
            return Err(MutateError::Unsupported);
        }
    }

    match m {
        Mutation::Replace { path, fragment } => replace(&tree, &idx, &path, &fragment)?,
        Mutation::Delete { path } => delete(&tree, &idx, &path)?,
        Mutation::Insert {
            target,
            fragment,
            on_collision,
            suggested_key,
        } => insert(
            &tree,
            &target,
            &fragment,
            suggested_key.as_deref(),
            on_collision,
        )?,
        Mutation::Rename { path, new_key } => rename(&idx, &path, &new_key)?,
        Mutation::Remark { path } => remark(&tree, &idx, &path)?,
        Mutation::EditComment { path, text } => edit_comment(&tree, &idx, &path, &text)?,
        Mutation::InsertComment { target, text } => insert_comment(&tree, &target, &text)?,
        Mutation::Move {
            sources,
            target,
            on_collision,
        } => move_nodes(&tree, &proj, &idx, &sources, &target, on_collision)?,
        Mutation::ConvertKind { path, target } => convert_kind(&tree, &idx, &path, target)?,
        Mutation::SetTrailingComment { path, comment } => {
            set_trailing_comment(&tree, &idx, &path, comment.as_deref())?
        }
    }
    validate_semantics(&tree)
}

// ── Test helpers (pub(crate) so later chunk tests can import them) ────────────

#[cfg(test)]
pub(crate) fn parse_syntax(src: &str) -> SyntaxNode {
    SyntaxNode::new_root(
        crate::model::yaml::parse::parse(src).unwrap_or_else(|e| panic!("parse failed: {e}")),
    )
}

/// Parse `src`, apply `m`, and return the serialized result.
/// Used by per-variant tests across later chunks.
#[cfg(test)]
pub(crate) fn apply_str(
    src: &str,
    m: crate::model::document::Mutation,
) -> Result<String, crate::model::document::MutateError> {
    let t = parse_syntax(src);
    apply(&t, m).map(|(_, text)| text)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
