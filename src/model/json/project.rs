//! JSON CST → `NodeTree` projection (mirrors `cst_project.rs`; golden tests).

use crate::model::json::syntax::SyntaxNode;
use crate::model::node::{Node, NodeKind, NodeTree};

pub fn project(_syntax: &SyntaxNode, filename: &str) -> NodeTree {
    NodeTree {
        root: Node::branch(filename, NodeKind::Root),
    }
}
