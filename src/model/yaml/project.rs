//! YAML CST → `NodeTree` projection stub (Task 3). Full implementation in Task 4.

use crate::model::node::{Node, NodeKind, NodeTree};
use crate::model::yaml::syntax::SyntaxNode;

pub fn project(_syntax: &SyntaxNode, filename: &str) -> NodeTree {
    NodeTree {
        root: Node::branch(filename, NodeKind::Root),
    }
}
