//! YAML CST → `NodeTree` projection (mirrors `json/project.rs`; golden tests).
//!
//! Tree shape summary (from parse.rs):
//!   ROOT → trivia + one MAPPING or SEQUENCE child (or VALUE for scalar-root)
//!   MAPPING → MAP_ENTRY* + OPAQUE* (merge keys), trivia floating at this level
//!   MAP_ENTRY → KEY(PLAIN|SINGLE|DOUBLE) + COLON + value-child
//!               value-child is one of: MAPPING, SEQUENCE, FLOW_MAP, FLOW_SEQ,
//!               VALUE(SCALAR|BLOCK_SCALAR|OPAQUE), or nothing (implicit null)
//!   SEQUENCE → SEQ_ENTRY* + trivia
//!   SEQ_ENTRY → DASH + value-child (same set as MAP_ENTRY, plus nested SEQUENCE/MAPPING)
//!   VALUE → wrapper node for SCALAR, BLOCK_SCALAR, or OPAQUE inline values
//!   SCALAR → PLAIN | SINGLE | DOUBLE tokens (the actual text)
//!   BLOCK_SCALAR → BLOCK_HEADER + indented content lines
//!   FLOW_MAP / FLOW_SEQ → inline { } / [ ] collections
//!   OPAQUE → out-of-subset span; projected read-only

use crate::model::node::{Format, KeySign, Node, NodeKind, NodeTree, ScalarType, Seg};
use crate::model::yaml::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use rowan::NodeOrToken;

/// The syntax element a projected node was built from, for mutation resolution.
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) enum Target {
    /// A `MAP_ENTRY` node (`key: value`).
    MapEntry(SyntaxNode),
    /// A `SEQ_ENTRY` node (`- value`).
    Element(SyntaxNode),
    /// The first `COMMENT` token of a standalone `#` block.
    Comment(SyntaxToken),
    /// An `OPAQUE` node (read-only out-of-subset span).
    Opaque(SyntaxNode),
}

pub(crate) type YamlIndex = Vec<(Vec<Seg>, Target)>;

pub fn project(syntax: &SyntaxNode, filename: &str) -> NodeTree {
    walk(syntax, filename).0
}

/// Shared traversal: build the `NodeTree` and resolver index together.
pub(crate) fn walk(syntax: &SyntaxNode, filename: &str) -> (NodeTree, YamlIndex) {
    let mut root = Node {
        key: filename.to_string(),
        path: Vec::new(),
        kind: NodeKind::Root,
        children: Vec::new(),
        value: None,
        format: Format::Plain,
        key_sign: KeySign::None,
        trailing_comment: None,
        read_only: false,
    };
    let mut idx: YamlIndex = Vec::new();

    let parent_path: Vec<Seg> = vec![];
    walk_root_node(syntax, &parent_path, &mut root.children, &mut idx);

    (NodeTree { root }, idx)
}

/// Emit an accumulated run of consecutive standalone `#` lines as a single
/// multi-line Comment node, then clear the accumulator. Shared verbatim by the
/// root/mapping/sequence walkers (the old `flush_comments!` macro).
fn flush_comment_block(
    comment_lines: &mut Vec<String>,
    first_comment_tok: &mut Option<SyntaxToken>,
    parent_path: &[Seg],
    out: &mut Vec<Node>,
    idx: &mut YamlIndex,
) {
    if comment_lines.is_empty() {
        return;
    }
    let text = comment_lines.join("\n");
    let tok = first_comment_tok.take().expect("set when non-empty");
    let i = out.len();
    let mut path = parent_path.to_vec();
    path.push(Seg::Index(i));
    idx.push((path.clone(), Target::Comment(tok)));
    out.push(Node {
        key: text.clone(),
        path,
        kind: NodeKind::Comment(text.clone()),
        children: Vec::new(),
        value: Some(text),
        format: Format::Plain,
        key_sign: KeySign::None,
        trailing_comment: None,
        read_only: false,
    });
    comment_lines.clear();
}

/// Walk the ROOT node. Its direct children may be trivia tokens, comments, and
/// a single MAPPING or SEQUENCE (or VALUE for scalar root).
fn walk_root_node(
    root: &SyntaxNode,
    parent_path: &[Seg],
    out: &mut Vec<Node>,
    idx: &mut YamlIndex,
) {
    // Comment accumulator for consecutive `#` lines.
    let mut comment_lines: Vec<String> = Vec::new();
    let mut first_comment_tok: Option<SyntaxToken> = None;
    let mut seen_newline_after_comment = false;

    macro_rules! flush_comments {
        () => {
            flush_comment_block(
                &mut comment_lines,
                &mut first_comment_tok,
                parent_path,
                out,
                idx,
            )
        };
    }

    for child in root.children_with_tokens() {
        match child {
            NodeOrToken::Token(tok) => match tok.kind() {
                SyntaxKind::COMMENT => {
                    if comment_lines.is_empty() {
                        first_comment_tok = Some(tok.clone());
                    }
                    comment_lines.push(tok.text().trim_end().to_string());
                    seen_newline_after_comment = false;
                }
                SyntaxKind::NEWLINE => {
                    if !comment_lines.is_empty() {
                        if seen_newline_after_comment {
                            flush_comments!();
                        } else {
                            seen_newline_after_comment = true;
                        }
                    }
                }
                _ => {} // INDENT, WHITESPACE, DOC_MARKER, etc.
            },
            NodeOrToken::Node(node) => match node.kind() {
                SyntaxKind::MAPPING => {
                    flush_comments!();
                    walk_mapping(&node, parent_path, out, idx);
                }
                SyntaxKind::SEQUENCE => {
                    flush_comments!();
                    walk_sequence(&node, parent_path, out, idx);
                }
                SyntaxKind::VALUE => {
                    flush_comments!();
                    // Scalar at root: one keyless leaf.
                    let i = out.len();
                    let mut path = parent_path.to_vec();
                    path.push(Seg::Index(i));
                    // A VALUE at root level (a bare scalar document) is unusual
                    // but handled: a keyless leaf, indexed so edit.rs can resolve it.
                    let raw = node.text().to_string();
                    idx.push((path.clone(), Target::Element(node.clone())));
                    out.push(Node {
                        key: raw.clone(),
                        path,
                        kind: NodeKind::Scalar(ScalarType::String),
                        children: Vec::new(),
                        value: Some(raw),
                        format: Format::Plain,
                        key_sign: KeySign::None,
                        trailing_comment: None,
                        read_only: false,
                    });
                }
                SyntaxKind::OPAQUE => {
                    flush_comments!();
                    let i = out.len();
                    let mut path = parent_path.to_vec();
                    path.push(Seg::Index(i));
                    let raw = node.text().to_string().trim_end().to_string();
                    idx.push((path.clone(), Target::Opaque(node.clone())));
                    out.push(Node {
                        key: raw.clone(),
                        path,
                        kind: NodeKind::Scalar(ScalarType::String),
                        children: Vec::new(),
                        value: Some(raw),
                        format: Format::Plain,
                        key_sign: KeySign::None,
                        trailing_comment: None,
                        read_only: true,
                    });
                }
                _ => {}
            },
        }
    }

    flush_comments!();
}

/// Walk a MAPPING node — emit MAP_ENTRY children and OPAQUE children (merge keys).
/// Comments and newlines float at this level as trivia tokens.
fn walk_mapping(
    mapping: &SyntaxNode,
    parent_path: &[Seg],
    out: &mut Vec<Node>,
    idx: &mut YamlIndex,
) {
    let mut comment_lines: Vec<String> = Vec::new();
    let mut first_comment_tok: Option<SyntaxToken> = None;
    let mut seen_newline_after_comment = false;

    macro_rules! flush_comments {
        () => {
            flush_comment_block(
                &mut comment_lines,
                &mut first_comment_tok,
                parent_path,
                out,
                idx,
            )
        };
    }

    for child in mapping.children_with_tokens() {
        match child {
            NodeOrToken::Token(tok) => match tok.kind() {
                SyntaxKind::COMMENT => {
                    if is_standalone_comment(&tok) {
                        if comment_lines.is_empty() {
                            first_comment_tok = Some(tok.clone());
                        }
                        comment_lines.push(tok.text().trim_end().to_string());
                        seen_newline_after_comment = false;
                    }
                    // trailing comments captured via trailing_comment_of_entry
                }
                SyntaxKind::NEWLINE => {
                    if !comment_lines.is_empty() {
                        if seen_newline_after_comment {
                            flush_comments!();
                        } else {
                            seen_newline_after_comment = true;
                        }
                    }
                }
                _ => {}
            },
            NodeOrToken::Node(node) => match node.kind() {
                SyntaxKind::MAP_ENTRY => {
                    flush_comments!();
                    project_map_entry(&node, parent_path, out, idx);
                }
                SyntaxKind::OPAQUE => {
                    flush_comments!();
                    // Merge-key or other opaque entry at mapping level.
                    let i = out.len();
                    let mut path = parent_path.to_vec();
                    path.push(Seg::Index(i));
                    let raw = node.text().to_string().trim_end().to_string();
                    idx.push((path.clone(), Target::Opaque(node.clone())));
                    out.push(Node {
                        key: raw.clone(),
                        path,
                        kind: NodeKind::Scalar(ScalarType::String),
                        children: Vec::new(),
                        value: Some(raw),
                        format: Format::Plain,
                        key_sign: KeySign::None,
                        trailing_comment: None,
                        read_only: true,
                    });
                }
                _ => {}
            },
        }
    }

    flush_comments!();
}

/// Walk a SEQUENCE node — emit SEQ_ENTRY children.
fn walk_sequence(
    sequence: &SyntaxNode,
    parent_path: &[Seg],
    out: &mut Vec<Node>,
    idx: &mut YamlIndex,
) {
    let mut comment_lines: Vec<String> = Vec::new();
    let mut first_comment_tok: Option<SyntaxToken> = None;
    let mut seen_newline_after_comment = false;

    macro_rules! flush_comments {
        () => {
            flush_comment_block(
                &mut comment_lines,
                &mut first_comment_tok,
                parent_path,
                out,
                idx,
            )
        };
    }

    for child in sequence.children_with_tokens() {
        match child {
            NodeOrToken::Token(tok) => match tok.kind() {
                SyntaxKind::COMMENT => {
                    if is_standalone_comment(&tok) {
                        if comment_lines.is_empty() {
                            first_comment_tok = Some(tok.clone());
                        }
                        comment_lines.push(tok.text().trim_end().to_string());
                        seen_newline_after_comment = false;
                    }
                }
                SyntaxKind::NEWLINE => {
                    if !comment_lines.is_empty() {
                        if seen_newline_after_comment {
                            flush_comments!();
                        } else {
                            seen_newline_after_comment = true;
                        }
                    }
                }
                _ => {}
            },
            NodeOrToken::Node(node) => {
                if node.kind() == SyntaxKind::SEQ_ENTRY {
                    flush_comments!();
                    project_seq_entry(&node, parent_path, out, idx);
                }
            }
        }
    }

    flush_comments!();
}

/// A COMMENT token is standalone iff the nearest preceding non-WHITESPACE/INDENT
/// sibling token is a NEWLINE or doesn't exist (start of container).
/// If it comes right after a value token with only WHITESPACE between, it's trailing.
fn is_standalone_comment(tok: &SyntaxToken) -> bool {
    let mut prev = tok.prev_sibling_or_token();
    while let Some(p) = prev {
        match p {
            NodeOrToken::Token(t) => match t.kind() {
                SyntaxKind::WHITESPACE | SyntaxKind::INDENT => {
                    prev = t.prev_sibling_or_token();
                }
                SyntaxKind::NEWLINE => return true,
                _ => return false,
            },
            NodeOrToken::Node(_) => return false,
        }
    }
    true // first token in container
}

/// Extract the trailing comment from after a MAP_ENTRY (or SEQ_ENTRY) node.
/// The comment is on the same line as the entry's value, after optional WHITESPACE.
/// We look at the entry's own token children (INDENT/WHITESPACE/COMMENT/NEWLINE
/// at the very end) OR we look at the next siblings at the MAPPING level.
/// In the YAML tree, a trailing `# comment` after a scalar value is emitted as
/// a COMMENT token inside the MAP_ENTRY (via bump_trailing in parse_scalar_value).
fn trailing_comment_of_entry(entry: &SyntaxNode) -> Option<String> {
    // Walk the entry's own trailing tokens: WHITESPACE* COMMENT? NEWLINE?
    // The COMMENT token just before the terminal NEWLINE (if any) is trailing.
    // We scan from the end backward.
    let mut last_comment: Option<String> = None;
    for c in entry.children_with_tokens() {
        if let NodeOrToken::Token(tok) = c {
            if tok.kind() == SyntaxKind::COMMENT {
                last_comment = Some(tok.text().trim_end().to_string());
            } else if tok.kind() == SyntaxKind::NEWLINE {
                // After a NEWLINE we're on a new line, any earlier comment was trailing.
                break;
            }
        }
    }
    last_comment
}

/// Project a MAP_ENTRY node into a child of `out`.
/// MAP_ENTRY → KEY(scalar-token) + COLON + value-child
fn project_map_entry(
    entry: &SyntaxNode,
    parent_path: &[Seg],
    out: &mut Vec<Node>,
    idx: &mut YamlIndex,
) {
    let (key_name, key_sign) = key_name_and_sign(entry);

    let mut path = parent_path.to_vec();
    path.push(Seg::Key(key_name.clone()));

    idx.push((path.clone(), Target::MapEntry(entry.clone())));
    let trailing = trailing_comment_of_entry(entry);

    // Find the value child: could be MAPPING, SEQUENCE (direct from parse_value),
    // or VALUE (wrapping SCALAR/BLOCK_SCALAR/FLOW_MAP/FLOW_SEQ/OPAQUE),
    // or absent (implicit null).
    let value_child = entry.children().find(|c| {
        matches!(
            c.kind(),
            SyntaxKind::MAPPING | SyntaxKind::SEQUENCE | SyntaxKind::VALUE | SyntaxKind::OPAQUE
        )
    });

    let node = match value_child {
        None => {
            // Implicit null value (e.g. `key:\n`)
            Node {
                key: key_name,
                path,
                kind: NodeKind::Scalar(ScalarType::Null),
                children: Vec::new(),
                value: None,
                format: Format::Plain,
                key_sign,
                trailing_comment: trailing,
                read_only: false,
            }
        }
        Some(vc) => build_value_node_from_child(&vc, &key_name, key_sign, path, trailing, idx),
    };

    out.push(node);
}

/// Project a SEQ_ENTRY node into a child of `out`.
/// SEQ_ENTRY → DASH + value-child (MAPPING/SEQUENCE/FLOW_MAP/FLOW_SEQ/VALUE/OPAQUE)
fn project_seq_entry(
    entry: &SyntaxNode,
    parent_path: &[Seg],
    out: &mut Vec<Node>,
    idx: &mut YamlIndex,
) {
    let i = out.len();
    let mut path = parent_path.to_vec();
    path.push(Seg::Index(i));
    let key_label = format!("[{i}]");

    idx.push((path.clone(), Target::Element(entry.clone())));
    let trailing = trailing_comment_of_entry(entry);

    let value_child = entry.children().find(|c| {
        matches!(
            c.kind(),
            SyntaxKind::MAPPING | SyntaxKind::SEQUENCE | SyntaxKind::VALUE | SyntaxKind::OPAQUE
        )
    });

    let node = match value_child {
        None => Node {
            key: key_label,
            path,
            kind: NodeKind::Scalar(ScalarType::Null),
            children: Vec::new(),
            value: None,
            format: Format::Plain,
            key_sign: KeySign::None,
            trailing_comment: trailing,
            read_only: false,
        },
        Some(vc) => {
            build_value_node_from_child(&vc, &key_label, KeySign::None, path, trailing, idx)
        }
    };

    out.push(node);
}

/// Build a Node from a value child node (MAPPING/SEQUENCE/VALUE/OPAQUE).
/// Note: under a MAP_ENTRY, FLOW_MAP/FLOW_SEQ arrive VALUE-wrapped (via
/// `parse_value`); under a SEQ_ENTRY they arrive unwrapped (via
/// `parse_flow_or_opaque`), so the FLOW_MAP/FLOW_SEQ arms below are reachable.
fn build_value_node_from_child(
    child: &SyntaxNode,
    key: &str,
    key_sign: KeySign,
    path: Vec<Seg>,
    trailing: Option<String>,
    idx: &mut YamlIndex,
) -> Node {
    match child.kind() {
        SyntaxKind::MAPPING => {
            let mut n = Node {
                key: key.to_string(),
                path: path.clone(),
                kind: NodeKind::Table,
                children: Vec::new(),
                value: None,
                format: Format::Block,
                key_sign,
                trailing_comment: trailing,
                read_only: false,
            };
            walk_mapping(child, &path, &mut n.children, idx);
            n
        }
        SyntaxKind::SEQUENCE => {
            let mut n = Node {
                key: key.to_string(),
                path: path.clone(),
                kind: NodeKind::Array,
                children: Vec::new(),
                value: None,
                format: Format::Block,
                key_sign,
                trailing_comment: trailing,
                read_only: false,
            };
            walk_sequence(child, &path, &mut n.children, idx);
            n
        }
        SyntaxKind::FLOW_MAP => {
            let repr = child.text().to_string();
            let mut n = Node {
                key: key.to_string(),
                path: path.clone(),
                kind: NodeKind::InlineTable,
                children: Vec::new(),
                value: Some(repr),
                format: Format::Inline,
                key_sign,
                trailing_comment: trailing,
                read_only: false,
            };
            walk_flow_map_entries(child, &path, &mut n.children, idx);
            n
        }
        SyntaxKind::FLOW_SEQ => {
            let repr = child.text().to_string();
            let mut n = Node {
                key: key.to_string(),
                path: path.clone(),
                kind: NodeKind::Array,
                children: Vec::new(),
                value: Some(repr),
                format: Format::Inline,
                key_sign,
                trailing_comment: trailing,
                read_only: false,
            };
            walk_flow_seq(child, &path, &mut n.children, idx);
            n
        }
        SyntaxKind::VALUE => {
            // VALUE wraps SCALAR, BLOCK_SCALAR, or OPAQUE.
            build_value_node(child, key, key_sign, path, trailing, idx)
        }
        SyntaxKind::OPAQUE => {
            let raw = child.text().to_string().trim_end().to_string();
            Node {
                key: key.to_string(),
                path,
                kind: NodeKind::Scalar(ScalarType::String),
                children: Vec::new(),
                value: Some(raw),
                format: Format::Plain,
                key_sign,
                trailing_comment: trailing,
                read_only: true,
            }
        }
        _ => Node {
            key: key.to_string(),
            path,
            kind: NodeKind::Scalar(ScalarType::Null),
            children: Vec::new(),
            value: None,
            format: Format::Plain,
            key_sign,
            trailing_comment: trailing,
            read_only: false,
        },
    }
}

/// Build a leaf or container Node from a VALUE wrapper node.
/// VALUE → SCALAR(PLAIN|SINGLE|DOUBLE) | BLOCK_SCALAR | OPAQUE | FLOW_MAP | FLOW_SEQ
fn build_value_node(
    value: &SyntaxNode,
    key: &str,
    key_sign: KeySign,
    path: Vec<Seg>,
    trailing: Option<String>,
    idx: &mut YamlIndex,
) -> Node {
    // Find the inner content node.
    let inner = value.children().next();
    match inner {
        Some(inner_node) => match inner_node.kind() {
            SyntaxKind::FLOW_MAP => {
                let repr = inner_node.text().to_string();
                let mut n = Node {
                    key: key.to_string(),
                    path: path.clone(),
                    kind: NodeKind::InlineTable,
                    children: Vec::new(),
                    value: Some(repr),
                    format: Format::Inline,
                    key_sign,
                    trailing_comment: trailing,
                    read_only: false,
                };
                walk_flow_map_entries(&inner_node, &path, &mut n.children, idx);
                n
            }
            SyntaxKind::FLOW_SEQ => {
                let repr = inner_node.text().to_string();
                let mut n = Node {
                    key: key.to_string(),
                    path: path.clone(),
                    kind: NodeKind::Array,
                    children: Vec::new(),
                    value: Some(repr),
                    format: Format::Inline,
                    key_sign,
                    trailing_comment: trailing,
                    read_only: false,
                };
                walk_flow_seq(&inner_node, &path, &mut n.children, idx);
                n
            }
            SyntaxKind::SCALAR => {
                // SCALAR has PLAIN, SINGLE, or DOUBLE token children.
                let scalar_tok = inner_node.children_with_tokens().find_map(|c| {
                    if let NodeOrToken::Token(t) = c {
                        match t.kind() {
                            SyntaxKind::PLAIN | SyntaxKind::SINGLE | SyntaxKind::DOUBLE => Some(t),
                            _ => None,
                        }
                    } else {
                        None
                    }
                });
                match scalar_tok {
                    Some(tok) => {
                        let (kind, format, val_text) = classify_scalar_token(&tok);
                        Node {
                            key: key.to_string(),
                            path,
                            kind,
                            children: Vec::new(),
                            value: Some(val_text),
                            format,
                            key_sign,
                            trailing_comment: trailing,
                            read_only: false,
                        }
                    }
                    None => {
                        // Empty scalar node
                        Node {
                            key: key.to_string(),
                            path,
                            kind: NodeKind::Scalar(ScalarType::Null),
                            children: Vec::new(),
                            value: None,
                            format: Format::Plain,
                            key_sign,
                            trailing_comment: trailing,
                            read_only: false,
                        }
                    }
                }
            }
            SyntaxKind::BLOCK_SCALAR => {
                // Detect `|` (literal) vs `>` (folded) from BLOCK_HEADER token.
                let header_tok = inner_node.children_with_tokens().find_map(|c| {
                    if let NodeOrToken::Token(t) = c {
                        if t.kind() == SyntaxKind::BLOCK_HEADER {
                            Some(t)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                });
                let fmt = match header_tok {
                    Some(tok) if tok.text().starts_with('>') => Format::Folded,
                    _ => Format::LiteralBlock,
                };
                let raw = value.text().to_string();
                Node {
                    key: key.to_string(),
                    path,
                    kind: NodeKind::Scalar(ScalarType::String),
                    children: Vec::new(),
                    value: Some(raw),
                    format: fmt,
                    key_sign,
                    trailing_comment: trailing,
                    read_only: false,
                }
            }
            SyntaxKind::OPAQUE => {
                let raw = inner_node.text().to_string().trim_end().to_string();
                Node {
                    key: key.to_string(),
                    path,
                    kind: NodeKind::Scalar(ScalarType::String),
                    children: Vec::new(),
                    value: Some(raw),
                    format: Format::Plain,
                    key_sign,
                    trailing_comment: trailing,
                    read_only: true,
                }
            }
            _ => Node {
                key: key.to_string(),
                path,
                kind: NodeKind::Scalar(ScalarType::Null),
                children: Vec::new(),
                value: None,
                format: Format::Plain,
                key_sign,
                trailing_comment: trailing,
                read_only: false,
            },
        },
        None => {
            // VALUE with no inner node — implicit null.
            Node {
                key: key.to_string(),
                path,
                kind: NodeKind::Scalar(ScalarType::Null),
                children: Vec::new(),
                value: None,
                format: Format::Plain,
                key_sign,
                trailing_comment: trailing,
                read_only: false,
            }
        }
    }
}

/// Project a FLOW_MAP's members. Each member is a `FLOW_ENTRY` child node
/// (`KEY COLON value`), so projection mirrors `project_map_entry`: the member is
/// individually addressable (its own `Target::MapEntry(flow_entry)`) and a nested
/// flow `{…}`/`[…]` value is a real child node that recurses, not a flat token run.
fn walk_flow_map_entries(
    flow_map: &SyntaxNode,
    parent_path: &[Seg],
    out: &mut Vec<Node>,
    idx: &mut YamlIndex,
) {
    for entry in flow_map
        .children()
        .filter(|c| c.kind() == SyntaxKind::FLOW_ENTRY)
    {
        let (key_name, key_sign) = key_name_and_sign(&entry);
        let mut path = parent_path.to_vec();
        path.push(Seg::Key(key_name.clone()));
        idx.push((path.clone(), Target::MapEntry(entry.clone())));

        // Value child: VALUE wrapper (SCALAR / nested FLOW_MAP / FLOW_SEQ), or
        // absent for an implicit null member (`{x:}`).
        let value_child = entry.children().find(|c| {
            matches!(
                c.kind(),
                SyntaxKind::MAPPING | SyntaxKind::SEQUENCE | SyntaxKind::VALUE | SyntaxKind::OPAQUE
            )
        });
        let node = match value_child {
            None => Node {
                key: key_name,
                path,
                kind: NodeKind::Scalar(ScalarType::Null),
                children: Vec::new(),
                value: None,
                format: Format::Plain,
                key_sign,
                trailing_comment: None,
                read_only: false,
            },
            Some(vc) => build_value_node_from_child(&vc, &key_name, key_sign, path, None, idx),
        };
        out.push(node);
    }
}

/// Extract the key name and sign from a MAP_ENTRY / FLOW_ENTRY's KEY child.
fn key_name_and_sign(entry: &SyntaxNode) -> (String, KeySign) {
    match entry.children().find(|c| c.kind() == SyntaxKind::KEY) {
        Some(kn) => {
            let inner = kn.children_with_tokens().find_map(|c| match c {
                NodeOrToken::Token(t)
                    if matches!(
                        t.kind(),
                        SyntaxKind::PLAIN | SyntaxKind::SINGLE | SyntaxKind::DOUBLE
                    ) =>
                {
                    Some(t)
                }
                _ => None,
            });
            match inner {
                Some(tok) => key_text_and_sign(&tok),
                None => (kn.text().to_string().trim().to_string(), KeySign::Bare),
            }
        }
        None => (String::new(), KeySign::Bare),
    }
}

/// Walk a FLOW_SEQ node: emit elements (scalar tokens or nested nodes).
fn walk_flow_seq(
    flow_seq: &SyntaxNode,
    parent_path: &[Seg],
    out: &mut Vec<Node>,
    idx: &mut YamlIndex,
) {
    for child in flow_seq.children_with_tokens() {
        match child {
            NodeOrToken::Token(tok) => match tok.kind() {
                SyntaxKind::PLAIN | SyntaxKind::SINGLE | SyntaxKind::DOUBLE => {
                    let i = out.len();
                    let mut path = parent_path.to_vec();
                    path.push(Seg::Index(i));
                    let (kind, fmt, val_text) = classify_scalar_token(&tok);
                    idx.push((path.clone(), Target::Element(flow_seq.clone())));
                    out.push(Node {
                        key: format!("[{i}]"),
                        path,
                        kind,
                        children: Vec::new(),
                        value: Some(val_text),
                        format: fmt,
                        key_sign: KeySign::None,
                        trailing_comment: None,
                        read_only: false,
                    });
                }
                _ => {}
            },
            NodeOrToken::Node(node) => {
                // Nested flow map or flow seq or opaque.
                let i = out.len();
                let mut path = parent_path.to_vec();
                path.push(Seg::Index(i));
                let key_label = format!("[{i}]");
                let n =
                    build_value_node_from_child(&node, &key_label, KeySign::None, path, None, idx);
                out.push(n);
            }
        }
    }
}

/// Extract the text and key sign from a key scalar token.
fn key_text_and_sign(tok: &SyntaxToken) -> (String, KeySign) {
    match tok.kind() {
        SyntaxKind::SINGLE => {
            let t = tok.text();
            // Strip surrounding single quotes.
            let inner = if t.len() >= 2 && t.starts_with('\'') && t.ends_with('\'') {
                t[1..t.len() - 1].replace("''", "'")
            } else {
                t.to_string()
            };
            (inner, KeySign::Quoted)
        }
        SyntaxKind::DOUBLE => {
            let t = tok.text();
            // Strip surrounding double quotes, then decode escape sequences so a
            // key like `"a\tb"` projects as `a<TAB>b` rather than literal `\t`.
            let inner = if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
                crate::model::yaml::edit::decode_double(&t[1..t.len() - 1])
            } else {
                t.to_string()
            };
            (inner, KeySign::Quoted)
        }
        _ => (tok.text().to_string(), KeySign::Bare),
    }
}

/// Classify a scalar token into (NodeKind, Format, value_text).
/// Implements YAML core schema type detection.
fn classify_scalar_token(tok: &SyntaxToken) -> (NodeKind, Format, String) {
    match tok.kind() {
        SyntaxKind::SINGLE => {
            let text = tok.text().to_string();
            (
                NodeKind::Scalar(ScalarType::String),
                Format::SingleQuoted,
                text,
            )
        }
        SyntaxKind::DOUBLE => {
            let text = tok.text().to_string();
            (
                NodeKind::Scalar(ScalarType::String),
                Format::DoubleQuoted,
                text,
            )
        }
        SyntaxKind::PLAIN => {
            let text = tok.text().to_string();
            let (kind, fmt) = classify_plain_scalar(text.trim());
            (kind, fmt, text)
        }
        _ => {
            let text = tok.text().to_string();
            (NodeKind::Scalar(ScalarType::String), Format::Plain, text)
        }
    }
}

/// Core-schema type detection for plain scalars.
fn classify_plain_scalar(s: &str) -> (NodeKind, Format) {
    // Null
    if s == "null" || s == "~" || s == "Null" || s == "NULL" {
        return (NodeKind::Scalar(ScalarType::Null), Format::Plain);
    }
    // Bool
    if matches!(s, "true" | "True" | "TRUE" | "false" | "False" | "FALSE") {
        return (NodeKind::Scalar(ScalarType::Bool), Format::Plain);
    }
    // Inf / NaN
    if matches!(s, ".inf" | ".Inf" | ".INF" | "+.inf" | "+.Inf" | "+.INF") {
        return (NodeKind::Scalar(ScalarType::Float), Format::Inf);
    }
    if matches!(s, "-.inf" | "-.Inf" | "-.INF") {
        return (NodeKind::Scalar(ScalarType::Float), Format::Inf);
    }
    if matches!(s, ".nan" | ".NaN" | ".NAN") {
        return (NodeKind::Scalar(ScalarType::Float), Format::Nan);
    }
    // Hex integer: 0x[0-9A-Fa-f]+
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        if !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return (NodeKind::Scalar(ScalarType::Integer), Format::Hex);
        }
    }
    // Octal integer: 0o[0-7]+
    if let Some(oct) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
        if !oct.is_empty() && oct.chars().all(|c| matches!(c, '0'..='7')) {
            return (NodeKind::Scalar(ScalarType::Integer), Format::Octal);
        }
    }
    // Decimal integer: optional sign, all digits
    {
        let digits = s.trim_start_matches(['+', '-']);
        if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
            return (NodeKind::Scalar(ScalarType::Integer), Format::Decimal);
        }
    }
    // Float with exponent
    if has_exponent(s) {
        return (NodeKind::Scalar(ScalarType::Float), Format::Exponent);
    }
    // Float with decimal point
    if is_float(s) {
        return (NodeKind::Scalar(ScalarType::Float), Format::Plain);
    }
    // Everything else (including date-looking strings)
    (NodeKind::Scalar(ScalarType::String), Format::Plain)
}

/// Check if a plain scalar looks like a float (has a dot and is otherwise numeric).
fn is_float(s: &str) -> bool {
    if !s.contains('.') {
        return false;
    }
    let digits = s.trim_start_matches(['+', '-']);
    if digits.is_empty() {
        return false;
    }
    // Must have at least one digit on each side of the dot, or be like `.5`/`5.`
    // Simple check: strip sign, split on '.', both parts are digits (possibly empty).
    let mut parts = digits.splitn(2, '.');
    let int_part = parts.next().unwrap_or("");
    let frac_part = parts.next().unwrap_or("");
    // At least one part must be non-empty digits.
    let int_ok = int_part.is_empty() || int_part.chars().all(|c| c.is_ascii_digit());
    let frac_ok = frac_part.is_empty() || frac_part.chars().all(|c| c.is_ascii_digit());
    // But we need at least one digit total.
    int_ok
        && frac_ok
        && (!int_part.is_empty() || !frac_part.is_empty())
        && (int_part.chars().any(|c| c.is_ascii_digit())
            || frac_part.chars().any(|c| c.is_ascii_digit()))
}

/// Check if a plain scalar has an exponent notation.
fn has_exponent(s: &str) -> bool {
    // Pattern: optional sign, digits, optional dot+digits, [eE] sign? digits
    let core = s.trim_start_matches(['+', '-']);
    if let Some(e_pos) = core.find(['e', 'E']) {
        let mantissa = &core[..e_pos];
        let exp_part = &core[e_pos + 1..];
        let exp_digits = exp_part.trim_start_matches(['+', '-']);
        // Mantissa must look numeric (digits + optional dot).
        let mantissa_ok =
            !mantissa.is_empty() && mantissa.chars().all(|c| c.is_ascii_digit() || c == '.');
        let exp_ok = !exp_digits.is_empty() && exp_digits.chars().all(|c| c.is_ascii_digit());
        mantissa_ok && exp_ok
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::{Format, KeySign, NodeKind, ScalarType, Seg};

    fn tree(src: &str) -> NodeTree {
        let g =
            crate::model::yaml::parse::parse(src).unwrap_or_else(|e| panic!("parse failed: {e}"));
        project(
            &crate::model::yaml::syntax::SyntaxNode::new_root(g),
            "c.yaml",
        )
    }

    #[test]
    fn scalars_core_schema() {
        let t = tree(
            "s: hello\nq: 'x'\ni: 42\nh: 0x1A\nf: 3.14\ne: 6e2\ninf: .inf\nb: true\nnul: ~\nd: 2026-06-13\n",
        );
        let by = |k: &str| t.root.children.iter().find(|c| c.key == k).unwrap();
        assert_eq!(by("s").kind, NodeKind::Scalar(ScalarType::String));
        assert_eq!(by("s").key_sign, KeySign::Bare);
        assert_eq!(by("q").format, Format::SingleQuoted);
        assert_eq!(by("i").kind, NodeKind::Scalar(ScalarType::Integer));
        assert_eq!(by("h").format, Format::Hex);
        assert_eq!(by("f").kind, NodeKind::Scalar(ScalarType::Float));
        assert_eq!(by("e").format, Format::Exponent);
        assert_eq!(by("inf").format, Format::Inf);
        assert_eq!(by("b").kind, NodeKind::Scalar(ScalarType::Bool));
        assert_eq!(by("nul").kind, NodeKind::Scalar(ScalarType::Null));
        assert_eq!(by("d").kind, NodeKind::Scalar(ScalarType::String));
    }

    #[test]
    fn double_quoted_key_is_unescaped() {
        let t = tree("\"a\\tb\": 1\n");
        let n = &t.root.children[0];
        assert_eq!(n.key, "a\tb");
        assert_eq!(n.key_sign, KeySign::Quoted);
    }

    #[test]
    fn block_mapping_and_sequence() {
        let t = tree("srv:\n  host: localhost\n  ports:\n    - 80\n    - 443\n");
        let srv = t.root.children.iter().find(|c| c.key == "srv").unwrap();
        assert_eq!(srv.kind, NodeKind::Table);
        assert_eq!(srv.format, Format::Block);
        let ports = srv.children.iter().find(|c| c.key == "ports").unwrap();
        assert_eq!(ports.kind, NodeKind::Array);
        assert_eq!(ports.children.len(), 2);
        assert_eq!(
            ports.children[0].path,
            vec![
                Seg::Key("srv".into()),
                Seg::Key("ports".into()),
                Seg::Index(0)
            ]
        );
    }

    #[test]
    fn flow_collections() {
        let t = tree("pt: {x: 1, y: 2}\nls: [a, b]\n");
        let pt = t.root.children.iter().find(|c| c.key == "pt").unwrap();
        assert_eq!(pt.kind, NodeKind::InlineTable);
        assert_eq!(pt.format, Format::Inline);
        let ls = t.root.children.iter().find(|c| c.key == "ls").unwrap();
        assert_eq!(ls.kind, NodeKind::Array);
        assert_eq!(ls.format, Format::Inline);
    }

    #[test]
    fn nested_flow_map_does_not_flatten() {
        // R3: a flow map value inside a flow map must project as a nested
        // InlineTable with its own members — not collapse to null with the inner
        // keys leaking out as siblings.
        let t = tree("server: {host: a, inner: {x: 1, y: 2}}\n");
        let server = t.root.children.iter().find(|c| c.key == "server").unwrap();
        assert_eq!(server.kind, NodeKind::InlineTable);
        // Exactly two members: host and inner (no leaked x/y).
        assert_eq!(server.children.len(), 2);
        let inner = server.children.iter().find(|c| c.key == "inner").unwrap();
        assert_eq!(inner.kind, NodeKind::InlineTable);
        assert_eq!(inner.children.len(), 2);
        assert_eq!(inner.children[0].key, "x");
        assert_eq!(
            inner.children[0].kind,
            NodeKind::Scalar(ScalarType::Integer)
        );
        assert_eq!(
            inner.children[1].path,
            vec![
                Seg::Key("server".into()),
                Seg::Key("inner".into()),
                Seg::Key("y".into())
            ]
        );
    }

    #[test]
    fn nested_flow_seq_of_maps() {
        // A flow seq holding flow maps: each element is an addressable InlineTable.
        let t = tree("items: [{a: 1}, {b: 2}]\n");
        let items = t.root.children.iter().find(|c| c.key == "items").unwrap();
        assert_eq!(items.kind, NodeKind::Array);
        assert_eq!(items.children.len(), 2);
        assert_eq!(items.children[0].kind, NodeKind::InlineTable);
        assert_eq!(items.children[0].children[0].key, "a");
    }

    #[test]
    fn block_scalars() {
        let t = tree("lit: |\n  a\n  b\nfold: >\n  c d\n");
        assert_eq!(
            t.root
                .children
                .iter()
                .find(|c| c.key == "lit")
                .unwrap()
                .format,
            Format::LiteralBlock
        );
        assert_eq!(
            t.root
                .children
                .iter()
                .find(|c| c.key == "fold")
                .unwrap()
                .format,
            Format::Folded
        );
    }

    #[test]
    fn comments_standalone_trailing_merged() {
        let t = tree("# a\n# b\nk: 1 # tail\n");
        assert!(matches!(t.root.children[0].kind, NodeKind::Comment(_)));
        assert_eq!(t.root.children[0].value.as_deref(), Some("# a\n# b"));
        let k = t.root.children.iter().find(|c| c.key == "k").unwrap();
        assert_eq!(k.trailing_comment.as_deref(), Some("# tail"));
    }

    #[test]
    fn opaque_is_read_only() {
        let t = tree("ref: *anchor\ntag: !!str 7\n");
        let r = t.root.children.iter().find(|c| c.key == "ref").unwrap();
        assert!(r.read_only);
        let g = t.root.children.iter().find(|c| c.key == "tag").unwrap();
        assert!(g.read_only);
    }

    #[test]
    fn merge_key_is_read_only_node() {
        let t = tree("base:\n  <<: *d\n  x: 1\n");
        let base = t.root.children.iter().find(|c| c.key == "base").unwrap();
        assert!(base.children.iter().any(|c| c.read_only));
    }
}
