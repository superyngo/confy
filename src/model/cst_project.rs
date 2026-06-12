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

use crate::model::node::{Format, KeySign, Node, NodeKind, NodeTree, ScalarType, Seg};
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
        key_sign: KeySign::None,
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
                    project_entry_into(&mut root, &current, &n, &mut idx);
                }
                SyntaxKind::TABLE_HEADER => {
                    let path = header_path(&n);
                    let signs = header_key_signs(&n);
                    let parent = path[..path.len().saturating_sub(1)].to_vec();
                    let pending = finalize_blocks!();
                    ensure_table_path(&mut root, &parent, &signs);
                    flush_comments(&mut root, &parent, pending, &mut idx);
                    ensure_table_path(&mut root, &path, &signs);
                    idx.push((path.clone(), Target::Header(n.clone())));
                    current = path;
                }
                SyntaxKind::TABLE_ARRAY_HEADER => {
                    let path = header_path(&n);
                    let signs = header_key_signs(&n);
                    let parent = path[..path.len().saturating_sub(1)].to_vec();
                    let aot_key = match path.last() {
                        Some(Seg::Key(k)) => k.clone(),
                        _ => continue,
                    };
                    let pending = finalize_blocks!();
                    ensure_table_path(&mut root, &parent, &signs);
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
                            key_sign: signs.last().copied().unwrap_or(KeySign::None),
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
                        key_sign: KeySign::None,
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
            key_sign: KeySign::None,
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
/// intermediate tables for a dotted header like `[x.a]`). `signs` is the
/// per-segment `KeySign` of the header's KEY, aligned with `path`. No-op for
/// the empty path.
fn ensure_table_path(root: &mut Node, path: &[Seg], signs: &[KeySign]) {
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
            format: Format::Scope,
            key_sign: signs.get(i).copied().unwrap_or(KeySign::None),
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

/// Per-segment `KeySign` of a `KEY` node, aligned with `key_segments`. taplo
/// lexes a quoted key as an `IDENT` whose text keeps the quotes, so the sign
/// comes from the text, not the token kind.
fn key_signs(key: &SyntaxNode) -> Vec<KeySign> {
    key.children_with_tokens()
        .filter_map(|c| match c {
            NodeOrToken::Token(t) => match t.kind() {
                SyntaxKind::IDENT | SyntaxKind::IDENT_WITH_GLOB => {
                    Some(if t.text().starts_with(['"', '\'']) {
                        KeySign::Quoted
                    } else {
                        KeySign::Bare
                    })
                }
                SyntaxKind::STRING | SyntaxKind::STRING_LITERAL => Some(KeySign::Quoted),
                _ => None,
            },
            NodeOrToken::Node(_) => None,
        })
        .collect()
}

/// Per-segment `KeySign`s of a header's `KEY` (empty if the header has none).
fn header_key_signs(header: &SyntaxNode) -> Vec<KeySign> {
    header
        .children()
        .find(|c| c.kind() == SyntaxKind::KEY)
        .map(|k| key_signs(&k))
        .unwrap_or_default()
}

/// `KeySign` of an entry's own key: a dotted key (`a.b.c = 1`) collapses into
/// one `Dotted` node; a single segment is `Bare` or `Quoted` by its token kind.
fn key_sign_of(key: &SyntaxNode) -> KeySign {
    let signs = key_signs(key);
    match signs.as_slice() {
        [] => KeySign::None,
        [one] => *one,
        _ => KeySign::Dotted,
    }
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
    let (display, segs, sign) = match &key_node {
        Some(k) => (key_display(k), key_segments(k), key_sign_of(k)),
        None => (String::new(), Vec::new(), KeySign::None),
    };
    let mut path = scope.to_vec();
    path.extend(segs);
    idx.push((path.clone(), Target::Entry(entry.clone())));
    let value = entry.children().find(|c| c.kind() == SyntaxKind::VALUE);
    let mut node = match value {
        Some(v) => project_value_node(&v, &display, path, idx),
        None => leaf(
            &display,
            NodeKind::Scalar(ScalarType::String),
            path,
            None,
            None,
        ),
    };
    node.key_sign = sign;
    node
}

/// Project a top-level / scope-level `ENTRY` into `root`, decomposing a
/// multi-segment dotted key (`a.b.c = 1`) into a chain of synthetic `Dotted`
/// `Table` nodes with the value as the leaf — so dotted keys nest like real
/// tables and offer an insertion target. A single-segment key behaves exactly as
/// before (one leaf appended under `scope`). The leaf keeps the **full** path for
/// `Target::Entry`, so mutation addressing is unchanged; the synthetic
/// intermediates carry no index target (like an implicit header table). Inline
/// table members keep using `project_entry` (their dotted keys are not split).
fn project_entry_into(root: &mut Node, scope: &[Seg], entry: &SyntaxNode, idx: &mut CstIndex) {
    let key_node = entry.children().find(|c| c.kind() == SyntaxKind::KEY);
    let segs = key_node.as_ref().map(key_segments).unwrap_or_default();

    // Single (or zero) segment: unchanged — one node directly under `scope`.
    if segs.len() <= 1 {
        let node = project_entry(entry, scope, idx);
        append_child(root, scope, node);
        return;
    }
    let mut full = scope.to_vec();
    full.extend(segs.iter().cloned());
    idx.push((full.clone(), Target::Entry(entry.clone())));

    let leaf_key = match segs.last() {
        Some(Seg::Key(k)) => k.clone(),
        _ => String::new(),
    };
    let value = entry.children().find(|c| c.kind() == SyntaxKind::VALUE);
    let mut node = match value {
        Some(v) => project_value_node(&v, &leaf_key, full.clone(), idx),
        None => leaf(
            &leaf_key,
            NodeKind::Scalar(ScalarType::String),
            full.clone(),
            None,
            None,
        ),
    };
    node.key_sign = KeySign::Dotted;

    ensure_dotted_chain(root, scope.len(), &full);
    append_child(root, &full[..full.len() - 1], node);

    // A `[T/D]` table projects at the position of its **first** definition in the
    // scope (where a consolidating block-rewrite will place it) — the chain node
    // stays where `ensure_dotted_chain` first created it.
}

/// Ensure the synthetic `Dotted` `Table` chain for a dotted-key entry exists under
/// an already-projected scope. `full` is the entry's absolute path, `scope_len`
/// the count of leading segments the enclosing scope already provides (never
/// recreated). Creates every segment from `scope_len` up to but **excluding** the
/// last (the leaf, appended by the caller). Every synthetic node reads
/// `KeySign::Dotted` — the whole decomposed chain signals its dotted-key origin.
fn ensure_dotted_chain(root: &mut Node, scope_len: usize, full: &[Seg]) {
    for i in scope_len..full.len().saturating_sub(1) {
        if node_at(root, &full[..=i]).is_some() {
            continue;
        }
        let key = match &full[i] {
            Seg::Key(k) => k.clone(),
            Seg::Index(_) => return,
        };
        let node = Node {
            key,
            path: full[..=i].to_vec(),
            kind: NodeKind::Table,
            children: Vec::new(),
            value: None,
            format: Format::Dotted,
            key_sign: KeySign::Dotted,
            trailing_comment: None,
        };
        append_child(root, &full[..i], node);
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
                    // `inf`/`nan` are FLOAT tokens; only the text tells them apart.
                    let fmt = if st == ScalarType::Float {
                        match t.text().trim_start_matches(['+', '-']) {
                            "inf" => Format::Inf,
                            "nan" => Format::Nan,
                            _ => fmt,
                        }
                    } else {
                        fmt
                    };
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
    // The same distinction is the array's Format facet.
    let repr = arr.to_string();
    if !repr.contains('\n') {
        n.value = Some(repr);
        n.format = Format::Inline;
    } else {
        n.format = Format::Multiline;
    }
    // Array children — elements and *standalone* interior comments — share one
    // `Seg::Index` slot sequence (like a table's children). A COMMENT on the same
    // line as the preceding element (no NEWLINE since it) is that element's trailing
    // comment; a COMMENT after a NEWLINE is a standalone Comment node.
    let mut k = 0usize;
    let mut newline_since_value = true; // a comment before any element is standalone
    for c in arr.children_with_tokens() {
        match c {
            NodeOrToken::Node(node) if node.kind() == SyntaxKind::VALUE => {
                let mut p = path.clone();
                p.push(Seg::Index(k));
                idx.push((p.clone(), Target::ArrayElement(node.clone())));
                n.children
                    .push(project_value_node(&node, &format!("[{k}]"), p, idx));
                k += 1;
                newline_since_value = false;
            }
            NodeOrToken::Token(t) => match t.kind() {
                SyntaxKind::NEWLINE => newline_since_value = true,
                SyntaxKind::COMMENT => {
                    let text = t.text().trim().to_string();
                    let attached = if newline_since_value {
                        false
                    } else if let Some(last) = n.children.last_mut() {
                        // Trailing comment of the element on this line.
                        last.trailing_comment = Some(text.clone());
                        true
                    } else {
                        false
                    };
                    if !attached {
                        let mut p = path.clone();
                        p.push(Seg::Index(k));
                        idx.push((p.clone(), Target::Comment(t.clone())));
                        n.children.push(Node {
                            key: text.clone(),
                            path: p,
                            kind: NodeKind::Comment(text.clone()),
                            children: Vec::new(),
                            value: Some(text),
                            format: Format::Plain,
                            key_sign: KeySign::None,
                            trailing_comment: None,
                        });
                        k += 1;
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    n
}

fn project_inline(it: &SyntaxNode, key: &str, path: Vec<Seg>, idx: &mut CstIndex) -> Node {
    let mut n = branch(key, NodeKind::InlineTable, path.clone());
    n.format = Format::Inline;
    // Inline tables are single-line; carry the one-line repr as `value` for display
    // and inline editing (guard on newline anyway, for safety).
    let repr = it.to_string();
    if !repr.contains('\n') {
        n.value = Some(repr);
    }
    for c in it.children() {
        if c.kind() != SyntaxKind::ENTRY {
            continue;
        }
        let key_node = c.children().find(|k| k.kind() == SyntaxKind::KEY);
        let segs = key_node.as_ref().map(key_segments).unwrap_or_default();
        if segs.len() <= 1 {
            n.children.push(project_entry(&c, &path, idx));
            continue;
        }
        // A multi-segment dotted key nests as a synthetic `[T/D]` chain, mirroring
        // `project_entry_into`: members sharing a prefix merge under one table, the
        // leaf keeps the full path for `Target::Entry`, the intermediates carry no
        // index target, and the whole chain reads `KeySign::Dotted`.
        let mut full = path.clone();
        full.extend(segs.iter().cloned());
        idx.push((full.clone(), Target::Entry(c.clone())));
        let leaf_key = match segs.last() {
            Some(Seg::Key(k)) => k.clone(),
            _ => String::new(),
        };
        let value = c.children().find(|v| v.kind() == SyntaxKind::VALUE);
        let mut node = match value {
            Some(v) => project_value_node(&v, &leaf_key, full.clone(), idx),
            None => leaf(
                &leaf_key,
                NodeKind::Scalar(ScalarType::String),
                full.clone(),
                None,
                None,
            ),
        };
        node.key_sign = KeySign::Dotted;
        let mut cur = &mut n;
        for i in path.len()..full.len() - 1 {
            let sub = &full[..=i];
            let pos = match cur.children.iter().position(|ch| ch.path == sub) {
                Some(p) => p,
                None => {
                    let key = match &full[i] {
                        Seg::Key(k) => k.clone(),
                        Seg::Index(_) => unreachable!("key segments only"),
                    };
                    cur.children.push(Node {
                        key,
                        path: sub.to_vec(),
                        kind: NodeKind::Table,
                        children: Vec::new(),
                        value: None,
                        format: Format::Dotted,
                        key_sign: KeySign::Dotted,
                        trailing_comment: None,
                    });
                    cur.children.len() - 1
                }
            };
            cur = &mut cur.children[pos];
        }
        cur.children.push(node);
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
        key_sign: KeySign::None,
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
        key_sign: KeySign::None,
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

    #[test]
    fn array_interior_comments_project() {
        // #6a: standalone interior comments become Comment nodes (sharing the
        // element index slots); a same-line comment is the element's trailing.
        let t = cst_tree("arr = [\n  1,  # one\n  2,\n  # standalone\n  3,\n]\n");
        let arr = &t.root.children[0];
        assert_eq!(arr.kind, NodeKind::Array);
        assert_eq!(arr.children.len(), 4, "3 elements + 1 standalone comment");
        assert_eq!(
            arr.children[0].trailing_comment.as_deref(),
            Some("# one"),
            "same-line comment is the element's trailing"
        );
        assert!(
            matches!(arr.children[2].kind, NodeKind::Comment(_))
                && arr.children[2].key == "# standalone",
            "standalone comment is a node at slot [2]"
        );
        assert_eq!(
            arr.children[3].key, "[3]",
            "element after a comment keeps full-sequence index"
        );
    }

    /// One-line-per-node rendering of a projection for the golden tests below.
    /// The expected strings were frozen from toml_edit-parity output when that
    /// legacy backend was retired (plus the CST-only one-line Array/InlineTable
    /// `value` reprs, which the old backend left `None`; `sign=`/container
    /// `fmt=` facets were regenerated when KeySign and container Formats landed).
    fn norm(n: &Node, depth: usize, out: &mut String) {
        let pad = "  ".repeat(depth);
        out.push_str(&format!(
            "{pad}{:?} key={:?} sign={:?} val={:?} fmt={:?} trail={:?}\n",
            n.kind,
            n.key,
            n.key_sign,
            n.value.as_deref().map(str::trim),
            n.format,
            n.trailing_comment
        ));
        for c in &n.children {
            norm(c, depth + 1, out);
        }
    }

    #[track_caller]
    fn assert_projection(src: &str, expected: &str) {
        let t = cst_tree(src);
        let mut a = String::new();
        for c in &t.root.children {
            norm(c, 0, &mut a);
        }
        assert_eq!(a, expected, "projection mismatch for:\n{src}\n--got--\n{a}");
    }

    #[test]
    fn golden_scalars_and_tables() {
        assert_projection(
            "title = \"x\"\n[server]\nport = 8080\n",
            r##"Scalar(String) key="title" sign=Bare val=Some("\"x\"") fmt=BasicString trail=None
Table key="server" sign=Bare val=None fmt=Scope trail=None
  Scalar(Integer) key="port" sign=Bare val=Some("8080") fmt=Decimal trail=None
"##,
        );
    }

    #[test]
    fn golden_comments_all_positions() {
        assert_projection(
            "# top\na = 1\n# mid\nb = 2\n# tail\n",
            r##"Comment("# top") key="# top" sign=None val=Some("# top") fmt=Plain trail=None
Scalar(Integer) key="a" sign=Bare val=Some("1") fmt=Decimal trail=None
Comment("# mid") key="# mid" sign=None val=Some("# mid") fmt=Plain trail=None
Scalar(Integer) key="b" sign=Bare val=Some("2") fmt=Decimal trail=None
Comment("# tail") key="# tail" sign=None val=Some("# tail") fmt=Plain trail=None
"##,
        );
        assert_projection(
            "# about\n[server]\nport = 8080\n",
            r##"Comment("# about") key="# about" sign=None val=Some("# about") fmt=Plain trail=None
Table key="server" sign=Bare val=None fmt=Scope trail=None
  Scalar(Integer) key="port" sign=Bare val=Some("8080") fmt=Decimal trail=None
"##,
        );
        assert_projection(
            "[s]\np = 1\n# mid\n[d]\nn = \"t\"\n",
            r##"Table key="s" sign=Bare val=None fmt=Scope trail=None
  Scalar(Integer) key="p" sign=Bare val=Some("1") fmt=Decimal trail=None
Comment("# mid") key="# mid" sign=None val=Some("# mid") fmt=Plain trail=None
Table key="d" sign=Bare val=None fmt=Scope trail=None
  Scalar(String) key="n" sign=Bare val=Some("\"t\"") fmt=BasicString trail=None
"##,
        );
        assert_projection(
            "[server]\n# explain\nport = 8080\n",
            r##"Table key="server" sign=Bare val=None fmt=Scope trail=None
  Comment("# explain") key="# explain" sign=None val=Some("# explain") fmt=Plain trail=None
  Scalar(Integer) key="port" sign=Bare val=Some("8080") fmt=Decimal trail=None
"##,
        );
    }

    #[test]
    fn dotted_member_adjacent_comment_stays_independent() {
        // A comment directly above a dotted member is an independent scope-level
        // node — comments are never inside a [T/D] table.
        assert_projection(
            "x = 0\n# why\na.b = 1\na.c = 2\n",
            r##"Scalar(Integer) key="x" sign=Bare val=Some("0") fmt=Decimal trail=None
Comment("# why") key="# why" sign=None val=Some("# why") fmt=Plain trail=None
Table key="a" sign=Dotted val=None fmt=Dotted trail=None
  Scalar(Integer) key="b" sign=Dotted val=Some("1") fmt=Decimal trail=None
  Scalar(Integer) key="c" sign=Dotted val=Some("2") fmt=Decimal trail=None
"##,
        );
    }

    #[test]
    fn dotted_member_blank_separated_comment_stays_outside() {
        // A blank line breaks adjacency too: the comment stays a scope-level node.
        assert_projection(
            "# free\n\na.b = 1\n",
            r##"Comment("# free") key="# free" sign=None val=Some("# free") fmt=Plain trail=None
Table key="a" sign=Dotted val=None fmt=Dotted trail=None
  Scalar(Integer) key="b" sign=Dotted val=Some("1") fmt=Decimal trail=None
"##,
        );
    }

    #[test]
    fn golden_comment_grouping() {
        assert_projection(
            "# one\n# two\na = 1\n",
            r##"Comment("# one\n# two") key="# one\n# two" sign=None val=Some("# one\n# two") fmt=Plain trail=None
Scalar(Integer) key="a" sign=Bare val=Some("1") fmt=Decimal trail=None
"##,
        );
        assert_projection(
            "# a1\n# a2\n\n# b1\na = 1\n",
            r##"Comment("# a1\n# a2") key="# a1\n# a2" sign=None val=Some("# a1\n# a2") fmt=Plain trail=None
Comment("# b1") key="# b1" sign=None val=Some("# b1") fmt=Plain trail=None
Scalar(Integer) key="a" sign=Bare val=Some("1") fmt=Decimal trail=None
"##,
        );
        assert_projection(
            "# just\n# comments\n",
            r##"Comment("# just\n# comments") key="# just\n# comments" sign=None val=Some("# just\n# comments") fmt=Plain trail=None
"##,
        );
    }

    #[test]
    fn trailing_eol_comment_extracted_not_lumped_into_value() {
        let t = cst_tree("port = 8080  # http\n");
        let port = &t.root.children[0];
        assert_eq!(port.value.as_deref(), Some("8080"));
        assert_eq!(port.trailing_comment.as_deref(), Some("# http"));
    }

    #[test]
    fn golden_arrays_inline_aot() {
        assert_projection(
            "nums = [1, 2]\npt = { x = 1 }\n[[item]]\nn = 1\n[[item]]\nn = 2\n",
            r##"Array key="nums" sign=Bare val=Some("[1, 2]") fmt=Inline trail=None
  Scalar(Integer) key="[0]" sign=None val=Some("1") fmt=Decimal trail=None
  Scalar(Integer) key="[1]" sign=None val=Some("2") fmt=Decimal trail=None
InlineTable key="pt" sign=Bare val=Some("{ x = 1 }") fmt=Inline trail=None
  Scalar(Integer) key="x" sign=Bare val=Some("1") fmt=Decimal trail=None
ArrayOfTables key="item" sign=Bare val=None fmt=Plain trail=None
  Table key="[0]" sign=None val=None fmt=Plain trail=None
    Scalar(Integer) key="n" sign=Bare val=Some("1") fmt=Decimal trail=None
  Table key="[1]" sign=None val=None fmt=Plain trail=None
    Scalar(Integer) key="n" sign=Bare val=Some("2") fmt=Decimal trail=None
"##,
        );
        assert_projection(
            "[[s]]\na = 1\n# mid\n[[s]]\nb = 2\n",
            r##"ArrayOfTables key="s" sign=Bare val=None fmt=Plain trail=None
  Table key="[0]" sign=None val=None fmt=Plain trail=None
    Scalar(Integer) key="a" sign=Bare val=Some("1") fmt=Decimal trail=None
  Comment("# mid") key="# mid" sign=None val=Some("# mid") fmt=Plain trail=None
  Table key="[1]" sign=None val=None fmt=Plain trail=None
    Scalar(Integer) key="b" sign=Bare val=Some("2") fmt=Decimal trail=None
"##,
        );
    }

    #[test]
    fn golden_dotted_key_and_header() {
        // A multi-segment dotted key nests into synthetic `Dotted` tables; the
        // whole decomposed chain (tables + leaf) reads `Dotted`.
        assert_projection(
            "a.b.c = 1\n",
            r##"Table key="a" sign=Dotted val=None fmt=Dotted trail=None
  Table key="b" sign=Dotted val=None fmt=Dotted trail=None
    Scalar(Integer) key="c" sign=Dotted val=Some("1") fmt=Decimal trail=None
"##,
        );
        // Scattered dotted entries sharing a prefix merge under one Dotted table,
        // which sits at its **first** member's scope position (before `x` here),
        // matching where a consolidating block-rewrite lands.
        assert_projection(
            "a.b = 1\nx = 0\na.c = 2\n",
            r##"Table key="a" sign=Dotted val=None fmt=Dotted trail=None
  Scalar(Integer) key="b" sign=Dotted val=Some("1") fmt=Decimal trail=None
  Scalar(Integer) key="c" sign=Dotted val=Some("2") fmt=Decimal trail=None
Scalar(Integer) key="x" sign=Bare val=Some("0") fmt=Decimal trail=None
"##,
        );
        // Dotted keys under a `[scope]` nest below it; the scope stays `Scope`.
        assert_projection(
            "[server]\nhost.name = \"h\"\nhost.port = 80\n",
            r##"Table key="server" sign=Bare val=None fmt=Scope trail=None
  Table key="host" sign=Dotted val=None fmt=Dotted trail=None
    Scalar(String) key="name" sign=Dotted val=Some("\"h\"") fmt=BasicString trail=None
    Scalar(Integer) key="port" sign=Dotted val=Some("80") fmt=Decimal trail=None
"##,
        );
        // A dotted key *inside an inline table* decomposes into a `[T/D]` chain too;
        // members sharing a prefix merge under one synthetic table.
        assert_projection(
            "p = { x.y = 1, x.z = 2, w = 3 }\n",
            r##"InlineTable key="p" sign=Bare val=Some("{ x.y = 1, x.z = 2, w = 3 }") fmt=Inline trail=None
  Table key="x" sign=Dotted val=None fmt=Dotted trail=None
    Scalar(Integer) key="y" sign=Dotted val=Some("1") fmt=Decimal trail=None
    Scalar(Integer) key="z" sign=Dotted val=Some("2") fmt=Decimal trail=None
  Scalar(Integer) key="w" sign=Bare val=Some("3") fmt=Decimal trail=None
"##,
        );
        assert_projection(
            "[x.a]\nname = \"A\"\n\n[x.b]\nname = \"B\"\n",
            r##"Table key="x" sign=Bare val=None fmt=Scope trail=None
  Table key="a" sign=Bare val=None fmt=Scope trail=None
    Scalar(String) key="name" sign=Bare val=Some("\"A\"") fmt=BasicString trail=None
  Table key="b" sign=Bare val=None fmt=Scope trail=None
    Scalar(String) key="name" sign=Bare val=Some("\"B\"") fmt=BasicString trail=None
"##,
        );
    }

    #[test]
    fn golden_scalar_types_and_formats() {
        assert_projection(
            "dec = 255\nhx = 0xFF\noc = 0o377\nbn = 0b1111_1111\n",
            r##"Scalar(Integer) key="dec" sign=Bare val=Some("255") fmt=Decimal trail=None
Scalar(Integer) key="hx" sign=Bare val=Some("0xFF") fmt=Hex trail=None
Scalar(Integer) key="oc" sign=Bare val=Some("0o377") fmt=Octal trail=None
Scalar(Integer) key="bn" sign=Bare val=Some("0b1111_1111") fmt=Binary trail=None
"##,
        );
        assert_projection(
            "b = \"hi\"\nl = 'hi'\nmb = \"\"\"hi\"\"\"\nml = '''hi'''\n",
            r##"Scalar(String) key="b" sign=Bare val=Some("\"hi\"") fmt=BasicString trail=None
Scalar(String) key="l" sign=Bare val=Some("'hi'") fmt=Literal trail=None
Scalar(String) key="mb" sign=Bare val=Some("\"\"\"hi\"\"\"") fmt=MultilineBasic trail=None
Scalar(String) key="ml" sign=Bare val=Some("'''hi'''") fmt=MultilineLiteral trail=None
"##,
        );
        assert_projection(
            "odt = 2021-01-01T00:00:00Z\nldt = 2021-01-01T00:00:00\nld = 2021-01-01\nlt = 12:34:56\n",
            r##"Scalar(OffsetDatetime) key="odt" sign=Bare val=Some("2021-01-01T00:00:00Z") fmt=Plain trail=None
Scalar(LocalDatetime) key="ldt" sign=Bare val=Some("2021-01-01T00:00:00") fmt=Plain trail=None
Scalar(LocalDate) key="ld" sign=Bare val=Some("2021-01-01") fmt=Plain trail=None
Scalar(LocalTime) key="lt" sign=Bare val=Some("12:34:56") fmt=Plain trail=None
"##,
        );
    }

    #[test]
    fn golden_key_signs_and_new_formats() {
        // Quoted key, inf/nan float formats, and the Inline/Multiline array facet.
        assert_projection(
            "\"q k\" = 1\npi = inf\nnn = -nan\nml = [\n  1,\n]\n",
            r##"Scalar(Integer) key="\"q k\"" sign=Quoted val=Some("1") fmt=Decimal trail=None
Scalar(Float) key="pi" sign=Bare val=Some("inf") fmt=Inf trail=None
Scalar(Float) key="nn" sign=Bare val=Some("-nan") fmt=Nan trail=None
Array key="ml" sign=Bare val=None fmt=Multiline trail=None
  Scalar(Integer) key="[0]" sign=None val=Some("1") fmt=Decimal trail=None
"##,
        );
    }

    /// Every repo fixture must parse and project without panicking (the
    /// byte-identical round-trip lives in `tests/roundtrip.rs`).
    #[test]
    fn repo_fixtures_project() {
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
            let t = cst_tree(&src);
            assert!(
                !t.root.children.is_empty(),
                "fixture {f:?} projected to an empty tree"
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
