//! JSON CST → `NodeTree` projection (mirrors `cst_project.rs`; golden tests).

use crate::model::json::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use crate::model::node::{Format, KeySign, Node, NodeKind, NodeTree, ScalarType, Seg};
use rowan::NodeOrToken;

/// The syntax element a projected node was built from, for mutation resolution.
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) enum Target {
    /// A `MEMBER` node (one `"key": value` pair).
    Member(SyntaxNode),
    /// A `VALUE` node that is an array element.
    Element(SyntaxNode),
    /// The first `LINE_COMMENT` token of a standalone `//` block.
    Comment(SyntaxToken),
    /// A `BLOCK_COMMENT` token (read-only).
    Block(SyntaxToken),
}

pub(crate) type JsonIndex = Vec<(Vec<Seg>, Target)>;

pub fn project(syntax: &SyntaxNode, filename: &str) -> NodeTree {
    walk(syntax, filename).0
}

/// Shared traversal: build the `NodeTree` and the resolver index together.
pub(crate) fn walk(syntax: &SyntaxNode, filename: &str) -> (NodeTree, JsonIndex) {
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
    let mut idx: JsonIndex = Vec::new();

    // Walk ROOT's direct children_with_tokens:
    // - Collect leading standalone comments into root.children
    // - When we hit the single VALUE child, adopt its container children
    //   (if OBJECT/ARRAY) into root.children, or push a scalar leaf.
    // - Trailing standalone comments after the VALUE also go to root.children.
    let parent_path: Vec<Seg> = vec![];
    walk_container_tokens(syntax, &parent_path, &mut root.children, &mut idx, true);

    (NodeTree { root }, idx)
}

/// A LINE_COMMENT token is *standalone* iff the nearest preceding non-WHITESPACE
/// sibling token (within the same parent) is a NEWLINE, the container-open
/// brace/bracket, or the token is first in the sequence (no preceding sibling).
/// If it comes right after a value token / COMMA with only WHITESPACE between,
/// it is a trailing comment (already captured on the member/element's node).
fn is_standalone_line_comment(tok: &SyntaxToken) -> bool {
    // Walk backwards through sibling tokens in the parent.
    let mut prev = tok.prev_sibling_or_token();
    while let Some(p) = prev {
        match p {
            NodeOrToken::Token(t) => match t.kind() {
                SyntaxKind::WHITESPACE => {
                    // keep scanning
                    prev = t.prev_sibling_or_token();
                }
                SyntaxKind::NEWLINE | SyntaxKind::L_BRACE | SyntaxKind::L_BRACK => {
                    return true;
                }
                _ => {
                    // Something structural (COMMA, R_BRACE, R_BRACK, STRING, NUMBER,
                    // TRUE, FALSE, NULL, BLOCK_COMMENT, another LINE_COMMENT, etc.) —
                    // the comment is trailing or after a block comment.
                    return false;
                }
            },
            NodeOrToken::Node(_) => {
                // A MEMBER or VALUE node precedes us — the comment is trailing.
                return false;
            }
        }
    }
    // Nothing precedes: it's the very first token — standalone.
    true
}

/// Trailing comment of a node (MEMBER for object entries, VALUE for array elements):
/// the first LINE_COMMENT that appears after `anchor` in its parent's token sequence,
/// before any NEWLINE, skipping WHITESPACE and (at most one) COMMA.
///
/// For a MEMBER like `"a": 1, // trailing`, the COMMA and LINE_COMMENT are
/// siblings of the MEMBER inside the OBJECT — so we walk the MEMBER's next siblings.
/// For an array element VALUE, we walk the VALUE's next siblings inside the ARRAY.
fn trailing_comment_of_node(anchor: &SyntaxNode) -> Option<String> {
    let mut skipped_comma = false;
    let mut sib = anchor.next_sibling_or_token();
    while let Some(s) = sib {
        match s {
            NodeOrToken::Token(t) => match t.kind() {
                SyntaxKind::WHITESPACE => {
                    sib = t.next_sibling_or_token();
                }
                SyntaxKind::COMMA if !skipped_comma => {
                    skipped_comma = true;
                    sib = t.next_sibling_or_token();
                }
                SyntaxKind::LINE_COMMENT => {
                    return Some(t.text().trim_end().to_string());
                }
                // NEWLINE or anything else stops search
                _ => return None,
            },
            NodeOrToken::Node(_) => return None,
        }
    }
    None
}

/// Returns the first non-trivia child of a VALUE node (as NodeOrToken).
fn value_inner(value: &SyntaxNode) -> Option<NodeOrToken<SyntaxNode, SyntaxToken>> {
    value.children_with_tokens().find(|c| match c {
        NodeOrToken::Token(t) => !matches!(
            t.kind(),
            SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT
        ),
        NodeOrToken::Node(_) => true,
    })
}

/// Strip surrounding `"` from a KEY node's text.
fn key_name(key_node: &SyntaxNode) -> String {
    let text = key_node.text().to_string();
    let t = text.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}

/// Inline vs Multiline container format.
fn container_format(container: &SyntaxNode) -> Format {
    if container.text().to_string().contains('\n') {
        Format::Multiline
    } else {
        Format::Inline
    }
}

/// Classify a scalar token into (NodeKind, Format, value_text).
fn classify_scalar(tok: &SyntaxToken) -> (NodeKind, Format, String) {
    let text = tok.text().to_string();
    match tok.kind() {
        SyntaxKind::STRING => (NodeKind::Scalar(ScalarType::String), Format::Plain, text),
        SyntaxKind::TRUE | SyntaxKind::FALSE => {
            (NodeKind::Scalar(ScalarType::Bool), Format::Plain, text)
        }
        SyntaxKind::NULL => (NodeKind::Scalar(ScalarType::Null), Format::Plain, text),
        SyntaxKind::NUMBER => {
            if text.contains('e') || text.contains('E') {
                (
                    NodeKind::Scalar(ScalarType::Float),
                    Format::Exponent,
                    text,
                )
            } else if text.contains('.') {
                (NodeKind::Scalar(ScalarType::Float), Format::Plain, text)
            } else {
                (NodeKind::Scalar(ScalarType::Integer), Format::Decimal, text)
            }
        }
        _ => (NodeKind::Scalar(ScalarType::String), Format::Plain, text),
    }
}

/// Build a Node from a VALUE syntax node. Pushes `(path, Target)` into `idx`.
/// `key` is the display key, `key_sign` the sign for this node's key.
/// `src_element` is the `Target` variant to record for this path in the index.
/// `trailing_anchor` is the node whose next siblings are scanned for a trailing
/// comment: a MEMBER node for object members, the VALUE node itself for array
/// elements (since the COMMA/LINE_COMMENT sit at the ARRAY level after the VALUE).
fn build_value_node(
    value: &SyntaxNode,
    trailing_anchor: &SyntaxNode,
    key: &str,
    key_sign: KeySign,
    path: Vec<Seg>,
    src_element: Target,
    idx: &mut JsonIndex,
) -> Node {
    idx.push((path.clone(), src_element));
    let trailing = trailing_comment_of_node(trailing_anchor);

    match value_inner(value) {
        Some(NodeOrToken::Node(container)) => match container.kind() {
            SyntaxKind::OBJECT => {
                let fmt = container_format(&container);
                let repr = if fmt == Format::Inline {
                    Some(container.text().to_string())
                } else {
                    None
                };
                let mut n = Node {
                    key: key.to_string(),
                    path: path.clone(),
                    kind: NodeKind::Table,
                    children: Vec::new(),
                    value: repr,
                    format: fmt,
                    key_sign,
                    trailing_comment: trailing,
                    read_only: false,
                };
                walk_container_tokens(&container, &path, &mut n.children, idx, false);
                n
            }
            SyntaxKind::ARRAY => {
                let fmt = container_format(&container);
                let repr = if fmt == Format::Inline {
                    Some(container.text().to_string())
                } else {
                    None
                };
                let mut n = Node {
                    key: key.to_string(),
                    path: path.clone(),
                    kind: NodeKind::Array,
                    children: Vec::new(),
                    value: repr,
                    format: fmt,
                    key_sign,
                    trailing_comment: trailing,
                    read_only: false,
                };
                walk_container_tokens(&container, &path, &mut n.children, idx, false);
                n
            }
            _ => {
                // Unexpected inner node — treat as opaque scalar
                Node {
                    key: key.to_string(),
                    path,
                    kind: NodeKind::Scalar(ScalarType::String),
                    children: Vec::new(),
                    value: None,
                    format: Format::Plain,
                    key_sign,
                    trailing_comment: trailing,
                    read_only: false,
                }
            }
        },
        Some(NodeOrToken::Token(tok)) => {
            let (kind, fmt, val_text) = classify_scalar(&tok);
            Node {
                key: key.to_string(),
                path,
                kind,
                children: Vec::new(),
                value: Some(val_text),
                format: fmt,
                key_sign,
                trailing_comment: trailing,
                read_only: false,
            }
        }
        None => Node {
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

/// Walk a container node's (OBJECT, ARRAY, or ROOT) `children_with_tokens()`,
/// accumulating comments and projecting members/elements into `out`.
///
/// When `is_root` is true, we are walking the ROOT node directly:
///   - Standalone comments before/after the value go into `out`.
///   - When we hit the VALUE node, we adopt its container's children (if
///     OBJECT/ARRAY) into `out` via a recursive call, or push a scalar leaf.
///
/// When `is_root` is false, we are walking an OBJECT or ARRAY:
///   - MEMBER nodes → keyed children (OBJECT mode).
///   - VALUE nodes → indexed elements (ARRAY mode).
///   - Standalone LINE_COMMENT → Comment node (shared slot space).
///   - BLOCK_COMMENT → read-only Comment node.
fn walk_container_tokens(
    container: &SyntaxNode,
    parent_path: &[Seg],
    out: &mut Vec<Node>,
    idx: &mut JsonIndex,
    is_root: bool,
) {
    // Comment accumulator for consecutive `//` lines.
    let mut lines: Vec<String> = Vec::new();
    let mut first_tok: Option<SyntaxToken> = None;

    // Flush the current accumulated `//` block as a Comment node.
    macro_rules! flush_line_comments {
        () => {
            if !lines.is_empty() {
                let text = lines.join("\n");
                let tok = first_tok.take().expect("first_tok set when lines non-empty");
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
                lines.clear();
            }
        };
    }

    for child in container.children_with_tokens() {
        match child {
            NodeOrToken::Token(tok) => match tok.kind() {
                SyntaxKind::LINE_COMMENT => {
                    if is_standalone_line_comment(&tok) {
                        // Accumulate consecutive `//` lines.
                        if lines.is_empty() {
                            first_tok = Some(tok.clone());
                        }
                        lines.push(tok.text().trim_end().to_string());
                    }
                    // Trailing comments are captured by trailing_comment_of() on the
                    // VALUE node — skip them here.
                }
                SyntaxKind::NEWLINE => {
                    // A NEWLINE with 2+ `\n` characters (blank line) breaks a comment
                    // accumulation block.
                    let newline_count = tok.text().matches('\n').count();
                    if newline_count >= 2 && !lines.is_empty() {
                        flush_line_comments!();
                    }
                }
                SyntaxKind::BLOCK_COMMENT => {
                    // Flush any pending `//` block first.
                    flush_line_comments!();
                    // Emit a read-only block comment node.
                    let text = tok.text().trim_end().to_string();
                    let i = out.len();
                    let mut path = parent_path.to_vec();
                    path.push(Seg::Index(i));
                    idx.push((path.clone(), Target::Block(tok.clone())));
                    out.push(Node {
                        key: text.clone(),
                        path,
                        kind: NodeKind::Comment(text.clone()),
                        children: Vec::new(),
                        value: Some(text),
                        format: Format::Plain,
                        key_sign: KeySign::None,
                        trailing_comment: None,
                        read_only: true,
                    });
                }
                _ => {} // WHITESPACE, COMMA, L_BRACE, R_BRACE, L_BRACK, R_BRACK, etc.
            },
            NodeOrToken::Node(node) => match node.kind() {
                SyntaxKind::MEMBER => {
                    // Flush pending comments before this member.
                    flush_line_comments!();
                    // Extract key name and build child node.
                    let key_node = node.children().find(|c| c.kind() == SyntaxKind::KEY);
                    let name = key_node
                        .as_ref()
                        .map(key_name)
                        .unwrap_or_default();
                    let mut path = parent_path.to_vec();
                    path.push(Seg::Key(name.clone()));
                    let value_node = node.children().find(|c| c.kind() == SyntaxKind::VALUE);
                    let member_node = node.clone(); // MEMBER node — trailing comment is after it
                    let child = match value_node {
                        Some(v) => build_value_node(
                            &v,
                            &member_node,
                            &name,
                            KeySign::Quoted,
                            path,
                            Target::Member(member_node.clone()),
                            idx,
                        ),
                        None => Node {
                            key: name.clone(),
                            path,
                            kind: NodeKind::Scalar(ScalarType::Null),
                            children: Vec::new(),
                            value: None,
                            format: Format::Plain,
                            key_sign: KeySign::Quoted,
                            trailing_comment: None,
                            read_only: false,
                        },
                    };
                    out.push(child);
                }
                SyntaxKind::VALUE if !is_root => {
                    // Array element. The trailing comment (if any) is after this
                    // VALUE node in the ARRAY's token sequence (after optional COMMA).
                    flush_line_comments!();
                    let i = out.len();
                    let mut path = parent_path.to_vec();
                    path.push(Seg::Index(i));
                    let key_label = format!("[{i}]");
                    let child = build_value_node(
                        &node,
                        &node, // trailing_anchor = VALUE itself (COMMA/comment at ARRAY level)
                        &key_label,
                        KeySign::None,
                        path,
                        Target::Element(node.clone()),
                        idx,
                    );
                    out.push(child);
                }
                SyntaxKind::VALUE if is_root => {
                    // The top-level value in ROOT.
                    flush_line_comments!();
                    match value_inner(&node) {
                        Some(NodeOrToken::Node(container))
                            if matches!(
                                container.kind(),
                                SyntaxKind::OBJECT | SyntaxKind::ARRAY
                            ) =>
                        {
                            // Adopt container's children into root.children.
                            walk_container_tokens(&container, parent_path, out, idx, false);
                        }
                        _ => {
                            // Scalar at root: one keyless leaf.
                            let i = out.len();
                            let mut path = parent_path.to_vec();
                            path.push(Seg::Index(i));
                            let child = build_value_node(
                                &node,
                                &node, // trailing_anchor: VALUE's siblings in ROOT
                                "",
                                KeySign::None,
                                path,
                                Target::Element(node.clone()),
                                idx,
                            );
                            out.push(child);
                        }
                    }
                    // After the root value, any remaining tokens are trailing
                    // comments — the `//` accumulation continues in the outer loop
                    // and flush_line_comments! will pick them up at end.
                }
                _ => {}
            },
        }
    }

    // Flush any trailing comment block at end of container.
    flush_line_comments!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::json::syntax::SyntaxNode;
    use crate::model::node::{Format, KeySign, NodeKind, ScalarType, Seg};

    fn tree(src: &str) -> NodeTree {
        let green = crate::model::json::parse::parse(src).unwrap();
        project(&SyntaxNode::new_root(green), "c.json")
    }

    #[test]
    fn scalars_and_object() {
        let t = tree("{\n  \"s\": \"x\",\n  \"i\": 8080,\n  \"f\": 1.5,\n  \"e\": 1e3,\n  \"b\": true,\n  \"n\": null\n}\n");
        let root = &t.root;
        assert_eq!(root.kind, NodeKind::Root);
        let by_key = |k: &str| root.children.iter().find(|c| c.key == k).unwrap();
        assert_eq!(by_key("s").kind, NodeKind::Scalar(ScalarType::String));
        assert_eq!(by_key("s").key_sign, KeySign::Quoted);
        assert_eq!(by_key("i").kind, NodeKind::Scalar(ScalarType::Integer));
        assert_eq!(by_key("i").format, Format::Decimal);
        assert_eq!(by_key("f").kind, NodeKind::Scalar(ScalarType::Float));
        assert_eq!(by_key("f").format, Format::Plain);
        assert_eq!(by_key("e").format, Format::Exponent);
        assert_eq!(by_key("b").kind, NodeKind::Scalar(ScalarType::Bool));
        assert_eq!(by_key("n").kind, NodeKind::Scalar(ScalarType::Null));
        assert_eq!(by_key("i").path, vec![Seg::Key("i".into())]);
    }

    #[test]
    fn nested_object_and_array() {
        let t = tree("{\n  \"o\": { \"a\": 1 },\n  \"arr\": [1, 2]\n}\n");
        let o = t.root.children.iter().find(|c| c.key == "o").unwrap();
        assert_eq!(o.kind, NodeKind::Table);
        assert_eq!(o.format, Format::Inline);
        assert_eq!(o.children[0].key, "a");
        assert_eq!(o.children[0].path, vec![Seg::Key("o".into()), Seg::Key("a".into())]);
        let arr = t.root.children.iter().find(|c| c.key == "arr").unwrap();
        assert_eq!(arr.kind, NodeKind::Array);
        assert_eq!(arr.format, Format::Inline);
        assert_eq!(arr.children.len(), 2);
        assert_eq!(arr.children[0].path, vec![Seg::Key("arr".into()), Seg::Index(0)]);
    }

    #[test]
    fn root_array_document() {
        let t = tree("[1, 2, 3]\n");
        assert_eq!(t.root.kind, NodeKind::Root);
        assert_eq!(t.root.children.len(), 3);
        assert_eq!(t.root.children[1].path, vec![Seg::Index(1)]);
    }

    #[test]
    fn standalone_and_trailing_comments() {
        let t = tree("{\n  // above\n  \"a\": 1, // trailing\n  \"b\": 2\n}\n");
        assert!(matches!(t.root.children[0].kind, NodeKind::Comment(_)));
        let a = t.root.children.iter().find(|c| c.key == "a").unwrap();
        assert_eq!(a.trailing_comment.as_deref(), Some("// trailing"));
    }

    #[test]
    fn merged_multiline_comment() {
        let t = tree("{\n  // l1\n  // l2\n  \"a\": 1\n}\n");
        let c = &t.root.children[0];
        assert!(matches!(c.kind, NodeKind::Comment(_)));
        assert_eq!(c.value.as_deref(), Some("// l1\n// l2"));
    }

    #[test]
    fn block_comment_is_read_only() {
        let t = tree("{\n  /* note */\n  \"a\": 1\n}\n");
        let c = &t.root.children[0];
        assert!(matches!(c.kind, NodeKind::Comment(_)));
        assert!(c.read_only);
        assert_eq!(c.value.as_deref(), Some("/* note */"));
    }

    #[test]
    fn comment_index_shares_slot_space() {
        // a comment before an element bumps the element's Seg::Index
        let t = tree("[\n  // c\n  10,\n  20\n]\n");
        // children: [Comment@0, 10@1, 20@2]
        assert!(matches!(t.root.children[0].kind, NodeKind::Comment(_)));
        assert_eq!(t.root.children[0].path, vec![Seg::Index(0)]);
        assert_eq!(t.root.children[1].path, vec![Seg::Index(1)]);
        assert_eq!(t.root.children[2].path, vec![Seg::Index(2)]);
    }

    #[test]
    fn jsonc_fixture_shape() {
        let src = include_str!("../../../tests/fixtures/sample.jsonc");
        let t = project(&SyntaxNode::new_root(crate::model::json::parse::parse(src).unwrap()), "sample.jsonc");
        assert!(matches!(t.root.children[0].kind, NodeKind::Comment(_)));
        let name = t.root.children.iter().find(|c| c.key == "name").unwrap();
        assert_eq!(name.trailing_comment.as_deref(), Some("// trailing comment"));
        assert!(t.root.children.iter().any(|c| matches!(c.kind, NodeKind::Comment(_)) && c.read_only));
    }
}
