//! Phase 2 of the CST migration: project a flat `taplo` rowan tree into the
//! hierarchical [`NodeTree`].
//!
//! taplo's `ROOT` is a *flat, line-oriented* sequence of `COMMENT` / `NEWLINE` /
//! `ENTRY` / `TABLE_HEADER` / `TABLE_ARRAY_HEADER`. This module reconstructs the
//! nesting (tables, dotted headers, array-of-tables) from that stream. Standalone
//! comments are **real tokens with positions**, so they project as ordered nodes
//! addressed by `Seg::Index` (their slot in the parent's child vector) — no
//! synthetic `#comment:N` key, no decor sniffing. Consecutive `#` lines still merge
//! into one Comment node (a blank line — a `NEWLINE` token with ≥2 newlines —
//! splits the block), matching the established display behaviour. A trailing
//! end-of-line comment (`x = 1  # c`) lives inside the entry's `VALUE` and projects
//! as the node's `trailing_comment`, never a standalone node.

use crate::model::node::{Format, Node, NodeKind, NodeTree, ScalarType, Seg};
use taplo::rowan::NodeOrToken;
use taplo::syntax::{SyntaxKind, SyntaxNode};

pub fn project(syntax: &SyntaxNode, filename: &str) -> NodeTree {
    let mut root = Node {
        key: filename.to_string(),
        path: Vec::new(),
        kind: NodeKind::Root,
        children: Vec::new(),
        value: None,
        format: Format::Plain,
        trailing_comment: None,
    };

    // Pending standalone-comment blocks not yet attached to a scope. `lines` is the
    // block currently accumulating; a blank line moves it into `blocks`. The
    // destination scope is decided by the next real item (entry / header / EOD).
    let mut blocks: Vec<String> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    // The currently-open table scope (where entries attach); empty = root.
    let mut current: Vec<Seg> = Vec::new();

    macro_rules! finalize_blocks {
        () => {{
            if !lines.is_empty() {
                blocks.push(lines.join("\n"));
                lines.clear();
            }
            std::mem::take(&mut blocks)
        }};
    }

    for child in syntax.children_with_tokens() {
        match child {
            NodeOrToken::Token(t) => match t.kind() {
                SyntaxKind::COMMENT => lines.push(t.text().trim().to_string()),
                SyntaxKind::NEWLINE => {
                    // A blank line (≥2 newlines) closes the current block.
                    if t.text().matches('\n').count() >= 2 && !lines.is_empty() {
                        blocks.push(lines.join("\n"));
                        lines.clear();
                    }
                }
                _ => {}
            },
            NodeOrToken::Node(n) => match n.kind() {
                SyntaxKind::ENTRY => {
                    let pending = finalize_blocks!();
                    flush_comments(&mut root, &current, pending);
                    let node = project_entry(&n, &current);
                    append_child(&mut root, &current, node);
                }
                SyntaxKind::TABLE_HEADER => {
                    let path = header_path(&n, &current /*unused*/);
                    let parent = &path[..path.len().saturating_sub(1)];
                    let pending = finalize_blocks!();
                    ensure_table_path(&mut root, parent);
                    flush_comments(&mut root, parent, pending);
                    ensure_table_path(&mut root, &path);
                    current = path;
                }
                SyntaxKind::TABLE_ARRAY_HEADER => {
                    let path = header_path(&n, &current);
                    let parent = path[..path.len().saturating_sub(1)].to_vec();
                    let aot_key = match path.last() {
                        Some(Seg::Key(k)) => k.clone(),
                        _ => continue,
                    };
                    let pending = finalize_blocks!();
                    ensure_table_path(&mut root, &parent);
                    // Existing AoT? (a child of `parent` with this path)
                    let exists = node_at(&root, &path).is_some();
                    if exists {
                        // Subsequent entry: comments become children of the AoT,
                        // before the new entry.
                        flush_comments(&mut root, &path, pending);
                    } else {
                        // First entry: comments are siblings before the AoT node.
                        flush_comments(&mut root, &parent, pending);
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
                    }
                    let aot = node_at_mut(&mut root, &path).expect("aot just ensured");
                    // Display key is the entry ordinal (`[0]`, `[1]`); the path
                    // `Seg::Index` is the *child-vector position* (uniform with
                    // comments), so an interleaved comment child never collides.
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
                    current = entry_path;
                }
                _ => {}
            },
        }
    }
    // End of document: remaining comments attach to the current scope.
    let pending = finalize_blocks!();
    flush_comments(&mut root, &current, pending);

    NodeTree { root }
}

/// Append each comment block as a `Comment` node (addressed by its child index) to
/// the node at `scope`.
fn flush_comments(root: &mut Node, scope: &[Seg], blocks: Vec<String>) {
    if blocks.is_empty() {
        return;
    }
    let container = node_at_mut(root, scope).expect("scope must exist");
    for text in blocks {
        let idx = container.children.len();
        let mut path = scope.to_vec();
        path.push(Seg::Index(idx));
        container.children.push(Node {
            key: text.clone(),
            path,
            kind: NodeKind::Comment(text.clone()),
            children: Vec::new(),
            value: Some(text),
            format: Format::Plain,
            trailing_comment: None,
        });
    }
}

/// Append `node` to the children of the node at `scope`.
fn append_child(root: &mut Node, scope: &[Seg], node: Node) {
    let container = node_at_mut(root, scope).expect("scope must exist");
    container.children.push(node);
}

/// Navigate to the node at `path` (matching each child by its full path prefix).
fn node_at<'a>(root: &'a Node, path: &[Seg]) -> Option<&'a Node> {
    let mut cur = root;
    for i in 0..path.len() {
        let target = &path[..=i];
        cur = cur.children.iter().find(|c| c.path == target)?;
    }
    Some(cur)
}

fn node_at_mut<'a>(root: &'a mut Node, path: &[Seg]) -> Option<&'a mut Node> {
    let mut cur = root;
    for i in 0..path.len() {
        let target = &path[..=i];
        cur = cur.children.iter_mut().find(|c| c.path == target)?;
    }
    Some(cur)
}

/// Ensure the chain of `Table` nodes named by `path` exists (creating implicit
/// intermediate tables for a dotted header like `[x.a]`). No-op for the empty path.
fn ensure_table_path(root: &mut Node, path: &[Seg]) {
    for i in 0..path.len() {
        let prefix = &path[..=i];
        if node_at(root, prefix).is_some() {
            continue;
        }
        let parent = &path[..i];
        let key = match &path[i] {
            Seg::Key(k) => k.clone(),
            Seg::Index(_) => return,
        };
        let node = Node {
            key,
            path: prefix.to_vec(),
            kind: NodeKind::Table,
            children: Vec::new(),
            value: None,
            format: Format::Plain,
            trailing_comment: None,
        };
        append_child(root, parent, node);
    }
}

/// The absolute key path named by a `TABLE_HEADER` / `TABLE_ARRAY_HEADER`'s `KEY`.
fn header_path(header: &SyntaxNode, _current: &[Seg]) -> Vec<Seg> {
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

/// The dotted display join of a `KEY` (e.g. `a.b.c`), using the same texts as
/// [`key_segments`].
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

/// Project a top-level (or in-table) `ENTRY` node into a leaf/array/inline-table
/// node. `scope` is the parent path; the entry's own `KEY` segments are appended.
fn project_entry(entry: &SyntaxNode, scope: &[Seg]) -> Node {
    let key_node = entry.children().find(|c| c.kind() == SyntaxKind::KEY);
    let (display, segs) = match &key_node {
        Some(k) => (key_display(k), key_segments(k)),
        None => (String::new(), Vec::new()),
    };
    let mut path = scope.to_vec();
    path.extend(segs);
    let value = entry.children().find(|c| c.kind() == SyntaxKind::VALUE);
    match value {
        Some(v) => project_value_node(&v, &display, path),
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
fn project_value_node(value: &SyntaxNode, key: &str, path: Vec<Seg>) -> Node {
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
                return project_array(&n, key, path);
            }
            NodeOrToken::Node(n) if n.kind() == SyntaxKind::INLINE_TABLE => {
                return project_inline(&n, key, path);
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

fn project_array(arr: &SyntaxNode, key: &str, path: Vec<Seg>) -> Node {
    let mut n = branch(key, NodeKind::Array, path.clone());
    let mut i = 0;
    for c in arr.children() {
        if c.kind() == SyntaxKind::VALUE {
            let mut p = path.clone();
            p.push(Seg::Index(i));
            n.children
                .push(project_value_node(&c, &format!("[{i}]"), p));
            i += 1;
        }
    }
    n
}

fn project_inline(it: &SyntaxNode, key: &str, path: Vec<Seg>) -> Node {
    let mut n = branch(key, NodeKind::InlineTable, path.clone());
    for c in it.children() {
        if c.kind() == SyntaxKind::ENTRY {
            n.children.push(project_entry(&c, &path));
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

    /// A path-independent, comparable rendering of a node (the addressing scheme
    /// intentionally differs between backends, so `path` is excluded).
    fn norm(n: &Node, depth: usize, out: &mut String) {
        let pad = "  ".repeat(depth);
        // toml_edit's value repr carries the decor space after `=` (e.g. " 8080");
        // the CST gives the clean token. Trim to compare the logical value.
        let val = n.value.as_deref().map(str::trim);
        out.push_str(&format!(
            "{pad}{:?} key={:?} val={:?} fmt={:?} trail={:?}\n",
            n.kind, n.key, val, n.format, n.trailing_comment
        ));
        for c in &n.children {
            norm(c, depth + 1, out);
        }
    }

    fn assert_parity(src: &str) {
        // Both backends use a different root filename; compare children only.
        let cst = cst_tree(src);
        let toml = toml_tree(src);
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
            "projection parity mismatch for:\n{src}\n--- CST ---\n{a}\n--- TOML ---\n{b}"
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
        // The CST correctly splits an end-of-line comment off the value; the old
        // toml_edit projection lumped it into the value string (a known quirk), so
        // this is an intentional *improvement*, asserted directly rather than by parity.
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

    /// Structural rendering (kind + key + shape only) — value/trailing differ by
    /// the intended trailing-comment/clean-value improvements, so fixtures (which
    /// contain EOL comments) are compared on structure.
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
        // The migration's payoff: comments are real ordered nodes addressed by
        // Seg::Index, with no `#comment:N` synthetic key.
        let t = cst_tree("# top\na = 1\n");
        let c = &t.root.children[0];
        assert!(matches!(c.kind, NodeKind::Comment(_)));
        assert_eq!(c.path, vec![Seg::Index(0)]);
        assert_eq!(t.root.children[1].path, vec![Seg::Key("a".into())]);
    }
}
