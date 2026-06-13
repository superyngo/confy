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

#[allow(unused_variables)]
fn replace(_tree: &SyntaxNode, _path: &[Seg], _fragment: &str) -> Result<(), MutateError> {
    Err(MutateError::Unsupported)
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

    #[test]
    fn stubbed_mutations_unsupported() {
        let t = parse("{ \"a\": 1 }\n");
        let r = apply(&t, Mutation::Delete { path: vec![Seg::Key("a".into())] });
        assert!(matches!(r, Err(MutateError::Unsupported)));
    }
}
