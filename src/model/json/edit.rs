//! JSON rowan splice helpers: one fn per `Mutation` variant (mirrors `cst_edit.rs`).

use crate::model::document::{MutateError, Mutation};
use crate::model::json::syntax::SyntaxNode;
use crate::model::node::Seg;

pub fn apply(_syntax: &SyntaxNode, _m: Mutation) -> Result<SyntaxNode, MutateError> {
    Err(MutateError::Unsupported)
}

pub fn serialize_fragment(_syntax: &SyntaxNode, _path: &[Seg]) -> String {
    String::new()
}
