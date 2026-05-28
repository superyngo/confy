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
        debug_assert!(
            matches!(
                kind,
                NodeKind::Root | NodeKind::Table | NodeKind::ArrayOfTables
                    | NodeKind::Array | NodeKind::InlineTable
            ),
            "Node::branch called with a leaf kind"
        );
        Node { key: key.into(), path: Vec::new(), kind, children: Vec::new() }
    }

    pub fn leaf(key: impl Into<String>, kind: NodeKind) -> Self {
        debug_assert!(
            matches!(kind, NodeKind::Scalar(_) | NodeKind::Comment(_)),
            "Node::leaf called with a branch kind"
        );
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

#[derive(Clone, Debug)]
pub struct VisibleRow<'a> {
    pub node: &'a Node,
    pub depth: usize,
}

impl NodeTree {
    /// Flatten honoring expanded state. `is_expanded(path)` decides whether a
    /// Branch node's children are shown. The Root is always shown and always
    /// treated as expanded.
    pub fn flatten<'a>(&'a self, is_expanded: &dyn Fn(&Path) -> bool) -> Vec<VisibleRow<'a>> {
        let mut rows = Vec::new();
        fn walk<'a>(n: &'a Node, depth: usize, is_root: bool,
                    is_expanded: &dyn Fn(&Path) -> bool, rows: &mut Vec<VisibleRow<'a>>) {
            rows.push(VisibleRow { node: n, depth });
            let expand = is_root || (n.is_branch() && is_expanded(&n.path));
            if expand {
                for c in &n.children {
                    walk(c, depth + 1, false, is_expanded, rows);
                }
            }
        }
        walk(&self.root, 0, true, is_expanded, &mut rows);
        rows
    }
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

    #[test]
    fn flatten_respects_expanded_set() {
        // root > server(branch) > port(leaf)
        let mut port = Node::leaf("port", NodeKind::Scalar(ScalarType::Integer));
        port.path = vec![Seg::Key("server".into()), Seg::Key("port".into())];
        let mut server = Node::branch("server", NodeKind::Table);
        server.path = vec![Seg::Key("server".into())];
        server.children = vec![port];
        let mut root = Node::branch("f.toml", NodeKind::Root);
        root.children = vec![server];
        let tree = NodeTree { root };

        // collapsed: only root + server visible (root always shown, expanded)
        let collapsed = tree.flatten(&|_p| false);
        assert_eq!(collapsed.iter().map(|r| r.node.key.clone()).collect::<Vec<_>>(),
            vec!["f.toml".to_string(), "server".to_string()]);

        // expand server -> port appears, depth 2
        let expanded = tree.flatten(&|p| p == &vec![Seg::Key("server".into())]);
        assert_eq!(expanded.len(), 3);
        assert_eq!(expanded[2].node.key, "port");
        assert_eq!(expanded[2].depth, 2);
    }
}
