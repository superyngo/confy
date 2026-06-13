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
    Null,
    OffsetDatetime,
    LocalDatetime,
    LocalDate,
    LocalTime,
}

/// Writing style of a scalar or container — orthogonal to `ScalarType`/`NodeKind`.
/// Derived from the syntax during projection (read-only); the eventual
/// format-toggle feature (§future) is the write-side counterpart. Nodes with a
/// single possible style (bool, datetimes, Root, AoT groups/entries, comments)
/// are `Plain`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Single writing style (bool, datetimes, Root, AoT, comments).
    Plain,
    // String
    BasicString,
    MultilineBasic,
    Literal,
    MultilineLiteral,
    // Integer
    Decimal,
    Hex,
    Octal,
    Binary,
    // Float (plain floats stay `Plain`)
    Inf,
    Nan,
    /// Float written in exponent notation (`1e5`, `1.2E-3`). New in the JSON
    /// backend; the TOML projection still detects exponent from value text.
    Exponent,
    // Container: array / inline table written on one line vs. spread over lines
    Inline,
    Multiline,
    /// A standard `[table]` scope (inline tables are `Inline`).
    Scope,
    /// A table that exists only because dotted keys (`a.b.c = 1`) defined it —
    /// no `[table]` header. Synthetic intermediate node, rendered `[T/D]`.
    Dotted,
    // YAML containers / scalar styles (block collections + 4 explicit string
    // styles; flow collections reuse `Inline`, plain scalars stay `Plain`).
    /// YAML block mapping/sequence (`key:\n  …`, `- …`). Rendered `[T/B]`/`[A/B]`.
    Block,
    /// YAML 'single quoted' scalar.
    SingleQuoted,
    /// YAML "double quoted" scalar.
    DoubleQuoted,
    /// YAML literal block scalar `|` (newlines preserved).
    LiteralBlock,
    /// YAML folded block scalar `>` (newlines folded).
    Folded,
}

/// How a node's own key is written in the source — `None` for keyless nodes
/// (array elements, comments, AoT entries, Root). Derived read-only during
/// projection, like `Format`. A dotted-key entry (`a.b.c = 1`) collapses into
/// one node, which is `Dotted`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeySign {
    Bare,
    Quoted,
    Dotted,
    None,
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
    pub value: Option<String>,
    /// Writing style of a scalar leaf or container; `Plain` where only one
    /// style exists (Root, AoT, comments, bool, datetimes, plain floats).
    pub format: Format,
    /// How this node's own key is written; `None` for keyless nodes.
    pub key_sign: KeySign,
    pub trailing_comment: Option<String>,
    /// Read-only nodes (a JSONC `/* */` block comment, a Phase-3 opaque YAML
    /// node) display and copy but reject `e`/`d`/`x`/`r`/insert-into. Default false.
    pub read_only: bool,
}

impl Node {
    pub fn branch(key: impl Into<String>, kind: NodeKind) -> Self {
        debug_assert!(
            matches!(
                kind,
                NodeKind::Root
                    | NodeKind::Table
                    | NodeKind::ArrayOfTables
                    | NodeKind::Array
                    | NodeKind::InlineTable
            ),
            "Node::branch called with a leaf kind"
        );
        Node {
            key: key.into(),
            path: Vec::new(),
            kind,
            children: Vec::new(),
            value: None,
            format: Format::Plain,
            key_sign: KeySign::None,
            trailing_comment: None,
            read_only: false,
        }
    }

    pub fn leaf(key: impl Into<String>, kind: NodeKind) -> Self {
        debug_assert!(
            matches!(kind, NodeKind::Scalar(_) | NodeKind::Comment(_)),
            "Node::leaf called with a branch kind"
        );
        Node {
            key: key.into(),
            path: Vec::new(),
            kind,
            children: Vec::new(),
            value: None,
            format: Format::Plain,
            key_sign: KeySign::None,
            trailing_comment: None,
            read_only: false,
        }
    }

    pub fn is_branch(&self) -> bool {
        matches!(
            self.kind,
            NodeKind::Root
                | NodeKind::Table
                | NodeKind::ArrayOfTables
                | NodeKind::Array
                | NodeKind::InlineTable
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
    /// Branch node's children are shown. The Root (empty path) is treated like
    /// any other branch, so it is collapsible too — the App seeds the empty path
    /// into the expanded set so the file node starts open.
    pub fn flatten<'a>(&'a self, is_expanded: &dyn Fn(&Path) -> bool) -> Vec<VisibleRow<'a>> {
        let mut rows = Vec::new();
        fn walk<'a>(
            n: &'a Node,
            depth: usize,
            is_expanded: &dyn Fn(&Path) -> bool,
            rows: &mut Vec<VisibleRow<'a>>,
        ) {
            rows.push(VisibleRow { node: n, depth });
            if n.is_branch() && is_expanded(&n.path) {
                for c in &n.children {
                    walk(c, depth + 1, is_expanded, rows);
                }
            }
        }
        walk(&self.root, 0, is_expanded, &mut rows);
        rows
    }

    /// Find a node by its exact projected path (Root has the empty path).
    pub fn node_at(&self, path: &[Seg]) -> Option<&Node> {
        fn walk<'a>(n: &'a Node, path: &[Seg]) -> Option<&'a Node> {
            if n.path == path {
                return Some(n);
            }
            n.children.iter().find_map(|c| walk(c, path))
        }
        walk(&self.root, path)
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

        // root collapsed (empty path not expanded): only the root row shows.
        let root_collapsed = tree.flatten(&|_p| false);
        assert_eq!(
            root_collapsed
                .iter()
                .map(|r| r.node.key.clone())
                .collect::<Vec<_>>(),
            vec!["f.toml".to_string()]
        );

        // root expanded, server collapsed: root + server visible.
        let collapsed = tree.flatten(&|p| p.is_empty());
        assert_eq!(
            collapsed
                .iter()
                .map(|r| r.node.key.clone())
                .collect::<Vec<_>>(),
            vec!["f.toml".to_string(), "server".to_string()]
        );

        // root + server expanded -> port appears, depth 2
        let expanded = tree.flatten(&|p| p.is_empty() || p == &vec![Seg::Key("server".into())]);
        assert_eq!(expanded.len(), 3);
        assert_eq!(expanded[2].node.key, "port");
        assert_eq!(expanded[2].depth, 2);
    }

    #[test]
    fn node_at_resolves_paths() {
        let mut port = Node::leaf("port", NodeKind::Scalar(ScalarType::Integer));
        port.path = vec![Seg::Key("server".into()), Seg::Key("port".into())];
        let mut server = Node::branch("server", NodeKind::Table);
        server.path = vec![Seg::Key("server".into())];
        server.children = vec![port];
        let mut root = Node::branch("f.toml", NodeKind::Root);
        root.children = vec![server];
        let tree = NodeTree { root };

        assert!(tree.node_at(&[]).is_some_and(|n| n.key == "f.toml"));
        let p = vec![Seg::Key("server".into()), Seg::Key("port".into())];
        assert!(tree.node_at(&p).is_some_and(|n| n.key == "port"));
        assert!(tree.node_at(&[Seg::Key("nope".into())]).is_none());
    }
}
