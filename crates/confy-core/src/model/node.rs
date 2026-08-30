use serde::{Deserialize, Serialize};

/// One segment of a path from Root to a Node.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Seg {
    Key(String),
    Index(usize),
}

pub type Path = Vec<Seg>;

/// The natural synthesized key for a bare scalar being pulled out of an
/// array element (move/paste): `<array's own key>_<index>`. `None` when the
/// array itself has no key of its own (a nested/unkeyed array, or a
/// root-level bare array) — callers fall back to the generic placeholder
/// key in that case, same as before this existed.
pub fn array_element_suggested_key(path: &[Seg]) -> Option<String> {
    let idx = match path.last() {
        Some(Seg::Index(i)) => *i,
        _ => return None,
    };
    let key_seg = path.len().checked_sub(2).and_then(|i| path.get(i))?;
    match key_seg {
        Seg::Key(name) => Some(format!("{name}_{idx}")),
        Seg::Index(_) => None,
    }
}

/// The first `"{base}_2"`, `"{base}_3"`, … candidate for which `is_taken` returns
/// `false`. Shared by every backend's `OnCollision::Rename` handling.
pub fn next_available_key(base: &str, is_taken: impl Fn(&str) -> bool) -> String {
    let mut n = 2;
    loop {
        let candidate = format!("{base}_{n}");
        if !is_taken(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KeySign {
    Bare,
    Quoted,
    Dotted,
    None,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NodeKind {
    Root,
    Table,
    ArrayOfTables,
    Array,
    InlineTable,
    Scalar(ScalarType),
    Comment(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    ///
    /// Coarse presentation facet only (it also carries `Dotted`, which
    /// `key_literal` cannot express). **Never** use it to reconstruct a key's
    /// spelling — read `key_literal` for that.
    pub key_sign: KeySign,
    /// The key exactly as authored in the source — quote characters, escape
    /// sequences and all — while `key`/`Seg::Key` hold the **decoded** key.
    ///
    /// - `key` / `Seg::Key` = semantic identity: path resolution, collision
    ///   checks, JSON-Schema property lookup, `to_value`/convert, serde.
    /// - `key_literal` = presentation + edit identity: the tree row label, the
    ///   Path line, the rename/edit buffer, fragment rebuilding.
    ///
    /// `None` for keyless nodes (array elements, AoT entries, Root, comments) —
    /// the same nodes where `key_text_range` is `None`. Filled once during
    /// projection from the key token already in hand, so no consumer ever has
    /// to re-walk the CST or synthesize a quote character.
    pub key_literal: Option<String>,
    pub trailing_comment: Option<String>,
    /// Read-only nodes (a JSONC `/* */` block comment, a Phase-3 opaque YAML
    /// node) display and copy but reject `e`/`d`/`x`/`r`/insert-into. Default false.
    pub read_only: bool,
    /// Byte range (half-open, UTF-8 byte offsets into the source text) of the
    /// whole node, including its key and value/children. Distinct from
    /// `CONTEXT.md`'s "Member spans" (the discrete, possibly-scattered source
    /// pieces that *constitute* a table) — this is a single contiguous
    /// representative range for editor symbol-tree purposes (VS Code Outline
    /// / breadcrumbs). See ADR 0006 for the anchoring policy on synthetic /
    /// scattered-definition nodes.
    pub text_range: std::ops::Range<usize>,
    /// Byte range of just the key token; `None` for keyless nodes (array
    /// elements, AoT entries, Root, comments) — the same nodes where
    /// `key_sign` is already `KeySign::None`.
    pub key_text_range: Option<std::ops::Range<usize>>,
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
            key_literal: None,
            trailing_comment: None,
            read_only: false,
            text_range: 0..0,
            key_text_range: None,
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
            key_literal: None,
            trailing_comment: None,
            read_only: false,
            text_range: 0..0,
            key_text_range: None,
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    /// Descends segment-by-segment: every child's path is its parent's path
    /// plus one segment (a projection invariant).
    pub fn node_at(&self, path: &[Seg]) -> Option<&Node> {
        let mut cur = &self.root;
        for i in 0..path.len() {
            cur = cur.children.iter().find(|c| c.path == path[..=i])?;
        }
        Some(cur)
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
