//! JSON rowan splice helpers: one fn per `Mutation` variant (mirrors `cst_edit.rs`).

use crate::model::document::{KindTarget, MutateError, Mutation, OnCollision, Target as MutTarget};
use crate::model::json::project::{walk, Target};
use crate::model::json::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use crate::model::node::Seg;

/// Resolve `path` to its source element using the projection's index.
pub(crate) fn resolve(syntax: &SyntaxNode, path: &[Seg]) -> Option<Target> {
    let (_, idx) = walk(syntax, "");
    idx.into_iter().find(|(p, _)| p == path).map(|(_, t)| t)
}

/// Serialize the node at `path` as a standalone fragment.
pub fn serialize_fragment(syntax: &SyntaxNode, path: &[Seg]) -> String {
    match resolve(syntax, path) {
        Some(Target::Member(m)) => m.text().to_string().trim().to_string(),
        Some(Target::Element(v)) => v.text().to_string().trim().to_string(),
        Some(Target::Comment(tok)) => comment_block_text(&tok),
        Some(Target::Block(tok)) => tok.text().to_string(),
        None => String::new(),
    }
}

/// Re-collect a merged standalone `//` block from its first token: consecutive
/// LINE_COMMENT tokens separated only by a single NEWLINE (+ optional indent
/// WHITESPACE) join with `\n`. A second consecutive NEWLINE ends the block.
fn comment_block_text(first: &SyntaxToken) -> String {
    let mut out = vec![first.text().trim_end().to_string()];
    let mut sib = first.next_sibling_or_token();
    let mut newlines = 0u32;
    while let Some(el) = sib {
        match el.kind() {
            SyntaxKind::WHITESPACE => {}
            SyntaxKind::NEWLINE => {
                newlines += 1;
                if newlines >= 2 {
                    break; // blank line ends the block
                }
            }
            SyntaxKind::LINE_COMMENT if newlines == 1 => {
                out.push(
                    el.as_token()
                        .unwrap()
                        .text()
                        .trim_end()
                        .to_string(),
                );
                newlines = 0;
            }
            _ => break,
        }
        sib = el.next_sibling_or_token();
    }
    out.join("\n")
}

/// Backstop after a splice: re-parse and reject duplicate object keys (Collision)
/// or structural breakage (Illegal). Mirrors cst_edit's DOM check.
fn validate_semantics(tree: &SyntaxNode) -> Result<(), MutateError> {
    let text = tree.to_string();
    let green = crate::model::json::parse::parse(&text).map_err(MutateError::Illegal)?;
    let reparsed = SyntaxNode::new_root(green);
    for obj in reparsed
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::OBJECT)
    {
        let mut seen = std::collections::HashSet::new();
        for member in obj.children().filter(|n| n.kind() == SyntaxKind::MEMBER) {
            if let Some(key) = member.children().find(|n| n.kind() == SyntaxKind::KEY) {
                let name = key.text().to_string();
                if !seen.insert(name.clone()) {
                    return Err(MutateError::Collision(name.trim_matches('"').to_string()));
                }
            }
        }
    }
    Ok(())
}

// ── Per-variant stubs (filled in by later tasks) ────────────────────────────

fn replace(tree: &SyntaxNode, path: &[Seg], fragment: &str) -> Result<(), MutateError> {
    if path.is_empty() {
        // Whole-document replace: parse fragment as a full JSON doc, splice its
        // ROOT children over the old ROOT children.
        // We need mutable SyntaxElements from a separate mutable tree.
        let green =
            crate::model::json::parse::parse(fragment).map_err(MutateError::Fragment)?;
        // Create an immutable root first, then clone_for_update to get mutable children.
        let new_root_immutable = SyntaxNode::new_root(green);
        let new_root = new_root_immutable.clone_for_update();
        let n = tree.children_with_tokens().count();
        // Children of a mutable node are already mutable — collect them directly.
        let new_children: Vec<_> = new_root.children_with_tokens().collect();
        tree.splice_children(0..n, new_children);
        return Ok(());
    }
    match resolve(tree, path).ok_or(MutateError::NotFound)? {
        Target::Member(member) => {
            if let Some(new_member) = parse_member_fragment(fragment) {
                replace_node(&member, new_member);
            } else {
                let value = member
                    .children()
                    .find(|n| n.kind() == SyntaxKind::VALUE)
                    .ok_or(MutateError::NotFound)?;
                let new_value = parse_value_fragment(fragment)?;
                replace_node(&value, new_value);
            }
            Ok(())
        }
        Target::Element(value) => {
            let new_value = parse_value_fragment(fragment)?;
            replace_node(&value, new_value);
            Ok(())
        }
        Target::Comment(_) | Target::Block(_) => {
            Err(MutateError::Illegal("use EditComment to edit a comment".into()))
        }
    }
}

/// Replace a node in a mutable tree by splicing over its slot in its parent.
fn replace_node(old: &SyntaxNode, new: SyntaxNode) {
    let parent = old.parent().expect("node has a parent");
    let idx = old.index();
    parent.splice_children(idx..idx + 1, vec![new.into()]);
}

/// Parse `fragment` as one bare VALUE (clone_for_update). Error `Fragment` if it
/// is not exactly one value.
fn parse_value_fragment(fragment: &str) -> Result<SyntaxNode, MutateError> {
    let green =
        crate::model::json::parse::parse(fragment).map_err(MutateError::Fragment)?;
    let root = SyntaxNode::new_root(green);
    let value = root
        .children()
        .find(|n| n.kind() == SyntaxKind::VALUE)
        .ok_or_else(|| MutateError::Fragment("fragment is not a value".into()))?;
    Ok(value.clone_for_update())
}

/// Parse `fragment` as a `"key": value` member by wrapping it in `{ … }`. Returns
/// None if it isn't a single member.
fn parse_member_fragment(fragment: &str) -> Option<SyntaxNode> {
    let wrapped = format!("{{{fragment}}}");
    let green = crate::model::json::parse::parse(&wrapped).ok()?;
    let root = SyntaxNode::new_root(green);
    let obj = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::OBJECT)?;
    let members: Vec<_> = obj
        .children()
        .filter(|n| n.kind() == SyntaxKind::MEMBER)
        .collect();
    if members.len() == 1 {
        Some(members[0].clone_for_update())
    } else {
        None
    }
}

#[allow(unused_variables)]
fn delete(_tree: &SyntaxNode, _path: &[Seg]) -> Result<(), MutateError> {
    Err(MutateError::Unsupported)
}

#[allow(unused_variables)]
fn insert(
    _tree: &SyntaxNode,
    _target: &MutTarget,
    _fragment: &str,
    _on_collision: OnCollision,
) -> Result<(), MutateError> {
    Err(MutateError::Unsupported)
}

#[allow(unused_variables)]
fn rename(_tree: &SyntaxNode, _path: &[Seg], _new_key: &str) -> Result<(), MutateError> {
    Err(MutateError::Unsupported)
}

#[allow(unused_variables)]
fn remark(_tree: &SyntaxNode, _path: &[Seg]) -> Result<(), MutateError> {
    Err(MutateError::Unsupported)
}

#[allow(unused_variables)]
fn edit_comment(_tree: &SyntaxNode, _path: &[Seg], _text: &str) -> Result<(), MutateError> {
    Err(MutateError::Unsupported)
}

#[allow(unused_variables)]
fn insert_comment(_tree: &SyntaxNode, _target: &MutTarget, _text: &str) -> Result<(), MutateError> {
    Err(MutateError::Unsupported)
}

#[allow(unused_variables)]
fn move_nodes(
    _tree: &SyntaxNode,
    _sources: &[Vec<Seg>],
    _target: &MutTarget,
    _on_collision: OnCollision,
) -> Result<(), MutateError> {
    Err(MutateError::Unsupported)
}

#[allow(unused_variables)]
fn convert_kind(_tree: &SyntaxNode, _path: &[Seg], _target: KindTarget) -> Result<(), MutateError> {
    Err(MutateError::Unsupported)
}

// ── Atomic dispatcher ────────────────────────────────────────────────────────

pub fn apply(syntax: &SyntaxNode, m: Mutation) -> Result<SyntaxNode, MutateError> {
    let tree = syntax.clone_for_update();
    match m {
        Mutation::Replace { path, fragment } => replace(&tree, &path, &fragment)?,
        Mutation::Delete { path } => delete(&tree, &path)?,
        Mutation::Insert {
            target,
            fragment,
            on_collision,
        } => insert(&tree, &target, &fragment, on_collision)?,
        Mutation::Rename { path, new_key } => rename(&tree, &path, &new_key)?,
        Mutation::Remark { path } => remark(&tree, &path)?,
        Mutation::EditComment { path, text } => edit_comment(&tree, &path, &text)?,
        Mutation::InsertComment { target, text } => insert_comment(&tree, &target, &text)?,
        Mutation::Move {
            sources,
            target,
            on_collision,
        } => move_nodes(&tree, &sources, &target, on_collision)?,
        Mutation::ConvertKind { path, target } => convert_kind(&tree, &path, target)?,
    }
    validate_semantics(&tree)?;
    Ok(tree)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::json::syntax::SyntaxNode;
    use crate::model::node::Seg;

    fn parse(src: &str) -> SyntaxNode {
        SyntaxNode::new_root(crate::model::json::parse::parse(src).unwrap())
    }

    #[test]
    fn fragment_of_member() {
        let t = parse("{\n  \"a\": 1,\n  \"b\": { \"x\": 2 }\n}\n");
        assert_eq!(serialize_fragment(&t, &[Seg::Key("a".into())]), "\"a\": 1");
        assert_eq!(
            serialize_fragment(&t, &[Seg::Key("b".into())]),
            "\"b\": { \"x\": 2 }"
        );
    }

    #[test]
    fn fragment_of_element() {
        let t = parse("[10, 20, 30]\n");
        assert_eq!(serialize_fragment(&t, &[Seg::Index(1)]), "20");
    }

    #[test]
    fn fragment_of_comment() {
        let t = parse("{\n  // hi\n  \"a\": 1\n}\n");
        assert_eq!(serialize_fragment(&t, &[Seg::Index(0)]), "// hi");
    }

    fn apply_str(src: &str, m: Mutation) -> String {
        let t = parse(src);
        super::apply(&t, m).unwrap().to_string()
    }

    #[test]
    fn replace_member_value() {
        let out = apply_str(
            "{\n  \"a\": 1\n}\n",
            Mutation::Replace {
                path: vec![Seg::Key("a".into())],
                fragment: "\"a\": 2".into(),
            },
        );
        assert_eq!(out, "{\n  \"a\": 2\n}\n");
    }

    #[test]
    fn replace_member_value_bare() {
        let out = apply_str(
            "{\n  \"a\": 1\n}\n",
            Mutation::Replace {
                path: vec![Seg::Key("a".into())],
                fragment: "2".into(),
            },
        );
        assert_eq!(out, "{\n  \"a\": 2\n}\n");
    }

    #[test]
    fn replace_element() {
        let out = apply_str(
            "[1, 2, 3]\n",
            Mutation::Replace {
                path: vec![Seg::Index(1)],
                fragment: "20".into(),
            },
        );
        assert_eq!(out, "[1, 20, 3]\n");
    }

    #[test]
    fn replace_whole_document() {
        let out = apply_str(
            "{ \"a\": 1 }\n",
            Mutation::Replace {
                path: vec![],
                fragment: "{ \"b\": 2 }\n".into(),
            },
        );
        assert_eq!(out, "{ \"b\": 2 }\n");
    }

    #[test]
    fn replace_invalid_fragment_rejected() {
        let t = parse("{ \"a\": 1 }\n");
        let r = super::apply(
            &t,
            Mutation::Replace {
                path: vec![Seg::Key("a".into())],
                fragment: "@@@".into(),
            },
        );
        assert!(matches!(r, Err(MutateError::Fragment(_)) | Err(MutateError::Illegal(_))));
    }

    #[test]
    fn stubbed_mutations_unsupported() {
        let t = parse("{ \"a\": 1 }\n");
        let r = apply(&t, Mutation::Delete { path: vec![Seg::Key("a".into())] });
        assert!(matches!(r, Err(MutateError::Unsupported)));
    }
}
