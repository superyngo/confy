//! YAML mutation stubs (Task 3). Full implementation in Tasks 5–6.

use crate::model::document::{MutateError, Mutation};
use crate::model::node::Seg;
use crate::model::yaml::syntax::SyntaxNode;

pub fn apply(syntax: &SyntaxNode, _m: Mutation) -> Result<SyntaxNode, MutateError> {
    Ok(syntax.clone_for_update())
}

pub fn serialize_fragment(_syntax: &SyntaxNode, _path: &[Seg]) -> String {
    String::new()
}
