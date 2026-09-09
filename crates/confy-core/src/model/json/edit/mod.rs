//! JSON rowan splice helpers: one fn per `Mutation` variant (mirrors
//! `cst_edit`). Split by construct (F6, 2026-09-09) — `resolve` (path lookup),
//! `fragment` (what the caller's text means), `container` (destination
//! machinery), then one file per `Mutation` family.

mod container;
mod convert;
mod fragment;
mod insert;
mod mutations;
mod replace_delete;
mod resolve;

use container::*;
use convert::*;
use fragment::*;
pub(crate) use fragment::{fragment_element_trailing_comment, fragment_member_trailing_comment};
use insert::*;
pub(crate) use mutations::extent_end_offset;
use mutations::*;
use replace_delete::*;
use resolve::*;
pub(crate) use resolve::{resolve, serialize_fragment};

use crate::model::document::{KindTarget, MutateError, Mutation, OnCollision, Target as MutTarget};
use crate::model::json::project::{trailing_comment_of_node, walk, Target};
use crate::model::json::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use crate::model::node::Seg;

/// Backstop after a splice: re-parse and reject duplicate object keys (Collision)
/// or structural breakage (Illegal). Mirrors cst_edit's DOM check.
///
/// Returns the re-parsed **immutable** tree and its serialization. The mutation
/// runs on a `clone_for_update` tree that must be normalized back to an immutable
/// one anyway, and this re-parse already produces exactly that — so the caller
/// commits these instead of repeating the serialize + parse.
fn validate_semantics(tree: &SyntaxNode) -> Result<(SyntaxNode, String), MutateError> {
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
    Ok((reparsed, text))
}

// ── Per-variant stubs (filled in by later tasks) ────────────────────────────

/// Apply `m` to a copy of `syntax`, returning the new **immutable** tree and its
/// serialization — both produced by the single serialize + re-parse that
/// `validate_semantics` needs anyway, so the caller repeats neither.
pub fn apply(syntax: &SyntaxNode, m: Mutation) -> Result<(SyntaxNode, String), MutateError> {
    let tree = syntax.clone_for_update();
    match m {
        Mutation::Replace { path, fragment } => replace(&tree, &path, &fragment)?,
        Mutation::Delete { path } => delete(&tree, &path)?,
        Mutation::Insert {
            target,
            fragment,
            on_collision,
            suggested_key,
        } => insert(
            &tree,
            &target,
            &fragment,
            on_collision,
            suggested_key.as_deref(),
        )?,
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
        Mutation::SetTrailingComment { path, comment } => {
            set_trailing_comment(&tree, &path, comment.as_deref())?
        }
        Mutation::SetTrailingBlankLines { path, n } => set_trailing_blank_lines(&tree, &path, n)?,
    }
    validate_semantics(&tree)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
