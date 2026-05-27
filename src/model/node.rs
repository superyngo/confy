/// One segment of a path from Root to a Node.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Seg {
    Key(String),
    Index(usize),
}

pub type Path = Vec<Seg>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarType {
    String,
    Integer,
    Float,
    Bool,
    Datetime,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    Root,
    Table,
    ArrayOfTables,
    Array,
    InlineTable,
    Scalar(ScalarType),
    Comment(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub key: String,
    pub path: Path,
    pub kind: NodeKind,
    pub children: Vec<Node>,
}

impl Node {
    pub fn branch(key: impl Into<String>, kind: NodeKind) -> Self {
        Node { key: key.into(), path: Vec::new(), kind, children: Vec::new() }
    }

    pub fn leaf(key: impl Into<String>, kind: NodeKind) -> Self {
        Node { key: key.into(), path: Vec::new(), kind, children: Vec::new() }
    }

    pub fn is_branch(&self) -> bool {
        matches!(
            self.kind,
            NodeKind::Root | NodeKind::Table | NodeKind::ArrayOfTables
                | NodeKind::Array | NodeKind::InlineTable
        )
    }

    pub fn is_leaf(&self) -> bool {
        !self.is_branch()
    }
}

/// The projected tree, rooted at the filename Node.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeTree {
    pub root: Node,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_and_leaf_classification() {
        let leaf = Node::leaf("port", NodeKind::Scalar(ScalarType::Integer));
        let branch = Node::branch("server", NodeKind::Table);
        assert!(leaf.is_leaf());
        assert!(!leaf.is_branch());
        assert!(branch.is_branch());
        assert!(!branch.is_leaf());
    }

    #[test]
    fn comment_is_leaf() {
        let c = Node::leaf("# note", NodeKind::Comment("# note".into()));
        assert!(c.is_leaf());
    }
}
