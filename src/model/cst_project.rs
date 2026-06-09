//! Phase 2/3 of the CST migration: a single `walk` over the flat `taplo` rowan
//! tree that produces both the hierarchical [`NodeTree`] (for display) and a
//! `path → syntax-element` index (for mutation resolution in `cst_edit`). Building
//! both in one traversal keeps the projection and the resolver from diverging.
//!
//! taplo's `ROOT` is a *flat, line-oriented* sequence of `COMMENT` / `NEWLINE` /
//! `ENTRY` / `TABLE_HEADER` / `TABLE_ARRAY_HEADER`. This module reconstructs the
//! nesting (tables, dotted headers, array-of-tables). Standalone comments are real
//! positioned tokens → ordered nodes addressed by `Seg::Index` (their child-vector
//! slot); consecutive `#` lines merge into one Comment node (a blank line splits).
//! A trailing end-of-line comment lives inside the entry's `VALUE` and projects as
//! `trailing_comment`, never a standalone node.

use crate::model::node::{Format, Node, NodeKind, NodeTree, ScalarType, Seg};
use taplo::rowan::NodeOrToken;
use taplo::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

/// The syntax element a projected node was built from, for mutation resolution.
/// Some variants are consumed by Phase-3 mutations not yet ported (Insert / Delete /
/// Rename / Move / Remark on tables and array-of-tables).
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) enum Target {
    /// An `ENTRY` node (leaf / array / inline-table / dotted leaf, and inline-table
    /// members).
    Entry(SyntaxNode),
    /// A `TABLE_HEADER` node (`[table]`).
    Header(SyntaxNode),
    /// A `TABLE_ARRAY_HEADER` node — one `[[x]]` entry.
    AotEntry(SyntaxNode),
    /// The synthetic array-of-tables group node (no single source element).
    AotGroup,
    /// The inner `VALUE` node of an array element.
    ArrayElement(SyntaxNode),
    /// The first `COMMENT` token of a (possibly multi-line) comment block.
    Comment(SyntaxToken),
}

pub(crate) type CstIndex = Vec<(Vec<Seg>, Target)>;

pub fn project(syntax: &SyntaxNode, filename: &str) -> NodeTree {
    walk(syntax, filename).0
}

/// The shared traversal: build the `NodeTree` and the resolver index together.
pub(crate) fn walk(syntax: &SyntaxNode, filename: &str) -> (NodeTree, CstIndex) {
    let mut root = Node {
        key: filename.to_string(),
        path: Vec::new(),
        kind: NodeKind::Root,
        children: Vec::new(),
        value: None,
        format: Format::Plain,
        trailing_comment: None,
    };
    let mut idx: CstIndex = Vec::new();

    // Pending standalone-comment blocks not yet attached to a scope, each paired
    // with its first `COMMENT` token (for the index). `lines` is the block currently
    // accumulating; a blank line moves it into `blocks`. The destination scope is
    // decided by the next real item (entry / header / EOD).
    let mut blocks: Vec<(String, SyntaxToken)> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    let mut first_tok: Option<SyntaxToken> = None;
    let mut current: Vec<Seg> = Vec::new();

    macro_rules! finalize_blocks {
        () => {{
            if !lines.is_empty() {
                blocks.push((lines.join("\n"), first_tok.take().expect("token for block")));
                lines.clear();
            }
            std::mem::take(&mut blocks)
        }};
    }

    for child in syntax.children_with_tokens() {
        match child {
            NodeOrToken::Token(t) => match t.kind() {
                SyntaxKind::COMMENT => {
                    if lines.is_empty() {
                        first_tok = Some(t.clone());
                    }
                    lines.push(t.text().trim().to_string());
                }
                SyntaxKind::NEWLINE => {
                    if t.text().matches('\n').count() >= 2 && !lines.is_empty() {
                        blocks.push((lines.join("\n"), first_tok.take().expect("token")));
                        lines.clear();
                    }
                }
                _ => {}
            },
            NodeOrToken::Node(n) => match n.kind() {
                SyntaxKind::ENTRY => {
                    let pending = finalize_blocks!();
                    flush_comments(&mut root, &current, pending, &mut idx);
                    let node = project_entry(&n, &current, &mut idx);
                    append_child(&mut root, &current, node);
                }
                SyntaxKind::TABLE_HEADER => {
                    let path = header_path(&n);
                    let parent = path[..path.len().saturating_sub(1)].to_vec();
                    let pending = finalize_blocks!();
                    ensure_table_path(&mut root, &parent);
                    flush_comments(&mut root, &parent, pending, &mut idx);
                    ensure_table_path(&mut root, &path);
                    idx.push((path.clone(), Target::Header(n.clone())));
                    current = path;
                }
                SyntaxKind::TABLE_ARRAY_HEADER => {
                    let path = header_path(&n);
                    let parent = path[..path.len().saturating_sub(1)].to_vec();
                    let aot_key = match path.last() {
                        Some(Seg::Key(k)) => k.clone(),
                        _ => continue,
                    };
                    let pending = finalize_blocks!();
                    ensure_table_path(&mut root, &parent);
                    let exists = node_at(&root, &path).is_some();
                    if exists {
                        flush_comments(&mut root, &path, pending, &mut idx);
                    } else {
                        flush_comments(&mut root, &parent, pending, &mut idx);
                        let aot = Node {
                            key: aot_key,
                            path: path.clone(),
                            kind: NodeKind::ArrayOfTables,
                            children: Vec::new(),
                            value: None,
                            format: Format::Plain,
                            trailing_comment: None,
                        };
                        append_child(&mut root, &parent, aot);
                        idx.push((path.clone(), Target::AotGroup));
                    }
                    let aot = node_at_mut(&mut root, &path).expect("aot just ensured");
                    let ordinal = aot
                        .children
                        .iter()
                        .filter(|c| !matches!(c.kind, NodeKind::Comment(_)))
                        .count();
                    let mut entry_path = path.clone();
                    entry_path.push(Seg::Index(aot.children.len()));
                    aot.children.push(Node {
                        key: format!("[{ordinal}]"),
                        path: entry_path.clone(),
                        kind: NodeKind::Table,
                        children: Vec::new(),
                        value: None,
                        format: Format::Plain,
                        trailing_comment: None,
                    });
                    idx.push((entry_path.clone(), Target::AotEntry(n.clone())));
                    current = entry_path;
                }
                _ => {}
            },
        }
    }
    let pending = finalize_blocks!();
    flush_comments(&mut root, &current, pending, &mut idx);

    (NodeTree { root }, idx)
}

fn flush_comments(
    root: &mut Node,
    scope: &[Seg],
    blocks: Vec<(String, SyntaxToken)>,
    idx: &mut CstIndex,
) {
    if blocks.is_empty() {
        return;
    }
    let container = node_at_mut(root, scope).expect("scope must exist");
    for (text, tok) in blocks {
        let i = container.children.len();
        let mut path = scope.to_vec();
        path.push(Seg::Index(i));
        container.children.push(Node {
            key: text.clone(),
            path: path.clone(),
            kind: NodeKind::Comment(text.clone()),
            children: Vec::new(),
            value: Some(text),
            format: Format::Plain,
            trailing_comment: None,
        });
        idx.push((path, Target::Comment(tok)));
    }
}

fn append_child(root: &mut Node, scope: &[Seg], node: Node) {
    node_at_mut(root, scope)
        .expect("scope must exist")
        .children
        .push(node);
}

fn node_at<'a>(root: &'a Node, path: &[Seg]) -> Option<&'a Node> {
    let mut cur = root;
    for i in 0..path.len() {
        cur = cur.children.iter().find(|c| c.path == path[..=i])?;
    }
    Some(cur)
}

fn node_at_mut<'a>(root: &'a mut Node, path: &[Seg]) -> Option<&'a mut Node> {
    let mut cur = root;
    for i in 0..path.len() {
        cur = cur.children.iter_mut().find(|c| c.path == path[..=i])?;
    }
    Some(cur)
}

/// Ensure the chain of `Table` nodes named by `path` exists (creating implicit
/// intermediate tables for a dotted header like `[x.a]`). No-op for the empty path.
fn ensure_table_path(root: &mut Node, path: &[Seg]) {
    for i in 0..path.len() {
        if node_at(root, &path[..=i]).is_some() {
            continue;
        }
        let key = match &path[i] {
            Seg::Key(k) => k.clone(),
            Seg::Index(_) => return,
        };
        let node = Node {
            key,
            path: path[..=i].to_vec(),
            kind: NodeKind::Table,
            children: Vec::new(),
            value: None,
            format: Format::Plain,
            trailing_comment: None,
        };
        append_child(root, &path[..i], node);
    }
}

/// The absolute key path named by a `TABLE_HEADER` / `TABLE_ARRAY_HEADER`'s `KEY`.
pub(crate) fn header_path(header: &SyntaxNode) -> Vec<Seg> {
    header
        .children()
        .find(|c| c.kind() == SyntaxKind::KEY)
        .map(|k| key_segments(&k))
        .unwrap_or_default()
}

/// The `Seg::Key` segments of a `KEY` node (its `IDENT` / quoted-string parts).
fn key_segments(key: &SyntaxNode) -> Vec<Seg> {
    key.children_with_tokens()
        .filter_map(|c| match c {
            NodeOrToken::Token(t) => match t.kind() {
                SyntaxKind::IDENT | SyntaxKind::IDENT_WITH_GLOB => {
                    Some(Seg::Key(t.text().to_string()))
                }
                SyntaxKind::STRING | SyntaxKind::STRING_LITERAL => {
                    Some(Seg::Key(unquote(t.text())))
                }
                _ => None,
            },
            NodeOrToken::Node(_) => None,
        })
        .collect()
}

/// The dotted display join of a `KEY` (e.g. `a.b.c`).
fn key_display(key: &SyntaxNode) -> String {
    key_segments(key)
        .iter()
        .map(|s| match s {
            Seg::Key(k) => k.clone(),
            Seg::Index(_) => String::new(),
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn unquote(s: &str) -> String {
    let t = s.trim();
    let b = t.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[b.len() - 1] == b[0] {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}

fn project_entry(entry: &SyntaxNode, scope: &[Seg], idx: &mut CstIndex) -> Node {
    let key_node = entry.children().find(|c| c.kind() == SyntaxKind::KEY);
    let (display, segs) = match &key_node {
        Some(k) => (key_display(k), key_segments(k)),
        None => (String::new(), Vec::new()),
    };
    let mut path = scope.to_vec();
    path.extend(segs);
    idx.push((path.clone(), Target::Entry(entry.clone())));
    let value = entry.children().find(|c| c.kind() == SyntaxKind::VALUE);
    match value {
        Some(v) => project_value_node(&v, &display, path, idx),
        None => leaf(
            &display,
            NodeKind::Scalar(ScalarType::String),
            path,
            None,
            None,
        ),
    }
}

/// Project a `VALUE` node — a scalar token, an `ARRAY`, or an `INLINE_TABLE`.
fn project_value_node(value: &SyntaxNode, key: &str, path: Vec<Seg>, idx: &mut CstIndex) -> Node {
    let trailing = value
        .children_with_tokens()
        .find_map(|c| match c {
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::COMMENT => {
                Some(t.text().trim().to_string())
            }
            _ => None,
        })
        .filter(|s| s.starts_with('#'));

    for c in value.children_with_tokens() {
        match c {
            NodeOrToken::Node(n) if n.kind() == SyntaxKind::ARRAY => {
                return project_array(&n, key, path, idx);
            }
            NodeOrToken::Node(n) if n.kind() == SyntaxKind::INLINE_TABLE => {
                return project_inline(&n, key, path, idx);
            }
            NodeOrToken::Token(t) => {
                if let Some((st, fmt)) = scalar_kind(t.kind()) {
                    return leaf(
                        key,
                        NodeKind::Scalar(st),
                        path,
                        Some(t.text().to_string()),
                        trailing,
                    )
                    .with_format(fmt);
                }
            }
            _ => {}
        }
    }
    leaf(
        key,
        NodeKind::Scalar(ScalarType::String),
        path,
        None,
        trailing,
    )
}

fn project_array(arr: &SyntaxNode, key: &str, path: Vec<Seg>, idx: &mut CstIndex) -> Node {
    let mut n = branch(key, NodeKind::Array, path.clone());
    // A single-line array carries its one-line repr as `value` so the VALUE column
    // shows it and it is inline-editable; a multiline array leaves `value` None.
    let repr = arr.to_string();
    if !repr.contains('\n') {
        n.value = Some(repr);
    }
    let mut i = 0;
    for c in arr.children() {
        if c.kind() == SyntaxKind::VALUE {
            let mut p = path.clone();
            p.push(Seg::Index(i));
            idx.push((p.clone(), Target::ArrayElement(c.clone())));
            n.children
                .push(project_value_node(&c, &format!("[{i}]"), p, idx));
            i += 1;
        }
    }
    n
}

fn project_inline(it: &SyntaxNode, key: &str, path: Vec<Seg>, idx: &mut CstIndex) -> Node {
    let mut n = branch(key, NodeKind::InlineTable, path.clone());
    // Inline tables are single-line; carry the one-line repr as `value` for display
    // and inline editing (guard on newline anyway, for safety).
    let repr = it.to_string();
    if !repr.contains('\n') {
        n.value = Some(repr);
    }
    for c in it.children() {
        if c.kind() == SyntaxKind::ENTRY {
            n.children.push(project_entry(&c, &path, idx));
        }
    }
    n
}

/// Map a scalar token kind to (`ScalarType`, `Format`).
fn scalar_kind(k: SyntaxKind) -> Option<(ScalarType, Format)> {
    use ScalarType as S;
    use SyntaxKind as K;
    Some(match k {
        K::STRING => (S::String, Format::BasicString),
        K::MULTI_LINE_STRING => (S::String, Format::MultilineBasic),
        K::STRING_LITERAL => (S::String, Format::Literal),
        K::MULTI_LINE_STRING_LITERAL => (S::String, Format::MultilineLiteral),
        K::INTEGER => (S::Integer, Format::Decimal),
        K::INTEGER_HEX => (S::Integer, Format::Hex),
        K::INTEGER_OCT => (S::Integer, Format::Octal),
        K::INTEGER_BIN => (S::Integer, Format::Binary),
        K::FLOAT => (S::Float, Format::Plain),
        K::BOOL => (S::Bool, Format::Plain),
        K::DATE_TIME_OFFSET => (S::OffsetDatetime, Format::Plain),
        K::DATE_TIME_LOCAL => (S::LocalDatetime, Format::Plain),
        K::DATE => (S::LocalDate, Format::Plain),
        K::TIME => (S::LocalTime, Format::Plain),
        _ => return None,
    })
}

fn leaf(
    key: &str,
    kind: NodeKind,
    path: Vec<Seg>,
    value: Option<String>,
    trailing_comment: Option<String>,
) -> Node {
    Node {
        key: key.to_string(),
        path,
        kind,
        children: Vec::new(),
        value,
        format: Format::Plain,
        trailing_comment,
    }
}

fn branch(key: &str, kind: NodeKind, path: Vec<Seg>) -> Node {
    Node {
        key: key.to_string(),
        path,
        kind,
        children: Vec::new(),
        value: None,
        format: Format::Plain,
        trailing_comment: None,
    }
}

impl Node {
    fn with_format(mut self, f: Format) -> Self {
        self.format = f;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::document::ConfigDocument;
    use std::io::Write;

    fn cst_tree(src: &str) -> NodeTree {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(src.as_bytes()).unwrap();
        crate::model::cst_doc::CstDocument::load(f.path())
            .unwrap()
            .project()
    }

    fn toml_tree(src: &str) -> NodeTree {
        let doc = src.parse::<toml_edit::DocumentMut>().unwrap();
        crate::model::project::project(&doc, "f.toml")
    }

    fn norm(n: &Node, depth: usize, out: &mut String) {
        let pad = "  ".repeat(depth);
        // The one-line repr `value` on single-line Array/InlineTable nodes is a
        // CST-only enhancement (the legacy toml_edit backend leaves it None), so it
        // is excluded from the projection-parity comparison.
        let val = match n.kind {
            NodeKind::Array | NodeKind::InlineTable => None,
            _ => n.value.as_deref().map(str::trim),
        };
        out.push_str(&format!(
            "{pad}{:?} key={:?} val={:?} fmt={:?} trail={:?}\n",
            n.kind, n.key, val, n.format, n.trailing_comment
        ));
        for c in &n.children {
            norm(c, depth + 1, out);
        }
    }

    fn assert_parity(src: &str) {
        let (cst, toml) = (cst_tree(src), toml_tree(src));
        let mut a = String::new();
        for c in &cst.root.children {
            norm(c, 0, &mut a);
        }
        let mut b = String::new();
        for c in &toml.root.children {
            norm(c, 0, &mut b);
        }
        assert_eq!(
            a, b,
            "projection parity mismatch for:\n{src}\n--CST--\n{a}\n--TOML--\n{b}"
        );
    }

    #[test]
    fn parity_scalars_and_tables() {
        assert_parity("title = \"x\"\n[server]\nport = 8080\n");
    }

    #[test]
    fn parity_comments_all_positions() {
        assert_parity("# top\na = 1\n# mid\nb = 2\n# tail\n");
        assert_parity("# about\n[server]\nport = 8080\n");
        assert_parity("[s]\np = 1\n# mid\n[d]\nn = \"t\"\n");
        assert_parity("[server]\n# explain\nport = 8080\n");
    }

    #[test]
    fn parity_comment_grouping() {
        assert_parity("# one\n# two\na = 1\n");
        assert_parity("# a1\n# a2\n\n# b1\na = 1\n");
        assert_parity("# just\n# comments\n");
    }

    #[test]
    fn trailing_eol_comment_extracted_not_lumped_into_value() {
        let t = cst_tree("port = 8080  # http\n");
        let port = &t.root.children[0];
        assert_eq!(port.value.as_deref(), Some("8080"));
        assert_eq!(port.trailing_comment.as_deref(), Some("# http"));
    }

    #[test]
    fn parity_arrays_inline_aot() {
        assert_parity("nums = [1, 2]\npt = { x = 1 }\n[[item]]\nn = 1\n[[item]]\nn = 2\n");
        assert_parity("[[s]]\na = 1\n# mid\n[[s]]\nb = 2\n");
    }

    #[test]
    fn parity_dotted_key_and_header() {
        assert_parity("a.b.c = 1\n");
        assert_parity("[x.a]\nname = \"A\"\n\n[x.b]\nname = \"B\"\n");
    }

    #[test]
    fn parity_scalar_types_and_formats() {
        assert_parity("dec = 255\nhx = 0xFF\noc = 0o377\nbn = 0b1111_1111\n");
        assert_parity("b = \"hi\"\nl = 'hi'\nmb = \"\"\"hi\"\"\"\nml = '''hi'''\n");
        assert_parity(
            "odt = 2021-01-01T00:00:00Z\nldt = 2021-01-01T00:00:00\nld = 2021-01-01\nlt = 12:34:56\n",
        );
    }

    /// Structural rendering (kind + key + shape) for fixtures, which contain EOL
    /// comments where value/trailing intentionally differ from the old projection.
    fn norm_struct(n: &Node, depth: usize, out: &mut String) {
        let pad = "  ".repeat(depth);
        let tag = match &n.kind {
            NodeKind::Comment(_) => "Comment".to_string(),
            other => format!("{other:?}"),
        };
        out.push_str(&format!("{pad}{tag} key={:?}\n", n.key));
        for c in &n.children {
            norm_struct(c, depth + 1, out);
        }
    }

    #[test]
    fn parity_repo_fixtures_structural() {
        let mut files = vec![std::path::PathBuf::from("test.toml")];
        let fx = std::path::Path::new("tests/fixtures");
        if fx.is_dir() {
            for e in std::fs::read_dir(fx).unwrap() {
                let p = e.unwrap().path();
                if p.extension().map(|x| x == "toml").unwrap_or(false) {
                    files.push(p);
                }
            }
        }
        for f in &files {
            let src = std::fs::read_to_string(f).unwrap();
            let (cst, toml) = (cst_tree(&src), toml_tree(&src));
            let mut a = String::new();
            for c in &cst.root.children {
                norm_struct(c, 0, &mut a);
            }
            let mut b = String::new();
            for c in &toml.root.children {
                norm_struct(c, 0, &mut b);
            }
            assert_eq!(
                a, b,
                "structural parity mismatch for {f:?}\n--CST--\n{a}\n--TOML--\n{b}"
            );
        }
    }

    #[test]
    fn comments_use_index_addressing_not_synthetic_keys() {
        let t = cst_tree("# top\na = 1\n");
        let c = &t.root.children[0];
        assert!(matches!(c.kind, NodeKind::Comment(_)));
        assert_eq!(c.path, vec![Seg::Index(0)]);
        assert_eq!(t.root.children[1].path, vec![Seg::Key("a".into())]);
    }

    /// The index must contain an element for every projected node's path (the
    /// resolver and the projection agree), and synthetic AoT-group paths aside,
    /// the path sets match.
    /// Every projected node must be resolvable via the index, *except* an implicit
    /// `Table` branch (one created by a dotted header like `[a.b]` with no `[a]`),
    /// which has no source header element. This ties the resolver to the projection.
    #[test]
    fn index_covers_every_projected_path() {
        fn collect<'a>(n: &'a Node, out: &mut Vec<(&'a Vec<Seg>, &'a NodeKind)>) {
            if !n.path.is_empty() {
                out.push((&n.path, &n.kind));
            }
            for c in &n.children {
                collect(c, out);
            }
        }
        let src = std::fs::read_to_string("test.toml").unwrap();
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(src.as_bytes()).unwrap();
        let doc = crate::model::cst_doc::CstDocument::load(f.path()).unwrap();
        let (tree, idx) = walk(&doc.syntax, "test.toml");
        let mut paths = Vec::new();
        collect(&tree.root, &mut paths);
        for (p, kind) in &paths {
            if idx.iter().any(|(ip, _)| &ip == p) {
                continue;
            }
            assert!(
                matches!(kind, NodeKind::Table),
                "index missing non-table path {p:?} ({kind:?})"
            );
        }
    }
}
