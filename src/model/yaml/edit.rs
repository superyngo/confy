//! YAML mutation helpers (Tasks 5–6).
//!
//! Sub-task 5a: indent engine (`reindent`), path resolver (`resolve`), opaque
//! guard (`is_opaque`).
//! Sub-task 5b: atomic dispatcher (`apply`), `serialize_fragment`, opaque
//! rejection pre-check.
//!
//! Per-variant splice implementations come in Tasks 5c–6; every variant returns
//! `Err(MutateError::Unsupported)` until then.

use crate::model::document::{MutateError, Mutation, Target as MutTarget};
use crate::model::node::Seg;
use crate::model::yaml::project::{walk, Target};
use crate::model::yaml::syntax::SyntaxNode;

// ── Indent engine ─────────────────────────────────────────────────────────────

/// Re-indent every line of `fragment` from `from` leading spaces to `to`.
/// Literal/folded block-scalar bodies shift with their header (uniform shift of
/// all lines preserves their *relative* indentation). Blank lines stay blank.
#[allow(dead_code)] // used by per-variant splice fns in later tasks
pub(crate) fn reindent(fragment: &str, from: usize, to: usize) -> String {
    let mut out = String::with_capacity(fragment.len());
    for line in fragment.split_inclusive('\n') {
        let (content, nl) = match line.strip_suffix('\n') {
            Some(c) => (c, "\n"),
            None => (line, ""),
        };
        if content.trim().is_empty() {
            out.push_str(content);
            out.push_str(nl);
            continue;
        }
        let stripped = content.strip_prefix(&" ".repeat(from)).unwrap_or(content);
        out.push_str(&" ".repeat(to));
        out.push_str(stripped);
        out.push_str(nl);
    }
    out
}

// ── Resolver ──────────────────────────────────────────────────────────────────

/// Resolve `path` to its source element using the projection's shared index.
/// Re-walks `syntax` (which may be a clone_for_update'd tree) so the returned
/// `Target` nodes are from the same tree as `syntax`.
pub(crate) fn resolve(syntax: &SyntaxNode, path: &[Seg]) -> Option<Target> {
    let (_, idx) = walk(syntax, "");
    idx.into_iter().find(|(p, _)| p == path).map(|(_, t)| t)
}

// ── Opaque guard ──────────────────────────────────────────────────────────────

/// Returns `true` if `path` itself or any strict ancestor path resolves to an
/// `Target::Opaque` — i.e. the path is inside (or is) an opaque span.
///
/// Precondition: `path` is non-empty. The root (`[]`) is never opaque and is
/// guarded out by the caller (`apply`); an empty path here always yields `false`.
fn is_opaque(syntax: &SyntaxNode, path: &[Seg]) -> bool {
    // Check the path itself first.
    if let Some(Target::Opaque(_)) = resolve(syntax, path) {
        return true;
    }
    // Then check every strict prefix (ancestor).
    for len in 1..path.len() {
        if let Some(Target::Opaque(_)) = resolve(syntax, &path[..len]) {
            return true;
        }
    }
    false
}

// ── Per-variant stubs (filled in by Tasks 5c–6) ───────────────────────────────

fn delete(_tree: &SyntaxNode, _path: &[Seg]) -> Result<(), MutateError> {
    Err(MutateError::Unsupported)
}

fn insert(
    _tree: &SyntaxNode,
    _target: &MutTarget,
    _fragment: &str,
    _on_collision: crate::model::document::OnCollision,
) -> Result<(), MutateError> {
    Err(MutateError::Unsupported)
}

fn replace(_tree: &SyntaxNode, _path: &[Seg], _fragment: &str) -> Result<(), MutateError> {
    Err(MutateError::Unsupported)
}

fn rename(_tree: &SyntaxNode, _path: &[Seg], _new_key: &str) -> Result<(), MutateError> {
    Err(MutateError::Unsupported)
}

fn remark(_tree: &SyntaxNode, _path: &[Seg]) -> Result<(), MutateError> {
    Err(MutateError::Unsupported)
}

fn edit_comment(_tree: &SyntaxNode, _path: &[Seg], _text: &str) -> Result<(), MutateError> {
    Err(MutateError::Unsupported)
}

fn insert_comment(_tree: &SyntaxNode, _target: &MutTarget, _text: &str) -> Result<(), MutateError> {
    Err(MutateError::Unsupported)
}

fn move_nodes(
    _tree: &SyntaxNode,
    _sources: &[Vec<Seg>],
    _target: &MutTarget,
    _on_collision: crate::model::document::OnCollision,
) -> Result<(), MutateError> {
    Err(MutateError::Unsupported)
}

fn convert_kind(
    _tree: &SyntaxNode,
    _path: &[Seg],
    _target: crate::model::document::KindTarget,
) -> Result<(), MutateError> {
    Err(MutateError::Unsupported)
}

// ── Semantic validator ────────────────────────────────────────────────────────

/// Backstop after a splice: re-parse and reject duplicate mapping keys
/// (Collision) or structural breakage (Illegal). Mirrors json/edit.rs's DOM
/// check using YAML re-parse + walk-based duplicate-key detection.
fn validate_semantics(tree: &SyntaxNode) -> Result<(), MutateError> {
    let text = tree.to_string();
    let green = crate::model::yaml::parse::parse(&text).map_err(MutateError::Illegal)?;
    let reparsed = SyntaxNode::new_root(green);
    // Re-walk and check for duplicate keys at every mapping level.
    let (node_tree, _idx) = walk(&reparsed, "");
    check_duplicate_keys(&node_tree.root.children)?;
    Ok(())
}

/// Recursively check for duplicate key names among siblings at each level.
fn check_duplicate_keys(nodes: &[crate::model::node::Node]) -> Result<(), MutateError> {
    let mut seen = std::collections::HashSet::new();
    for node in nodes {
        if let crate::model::node::NodeKind::Comment(_) = &node.kind {
            // Comments use Index paths — not keyed, no collision.
        } else if let Some(Seg::Key(k)) = node.path.last() {
            if !seen.insert(k.clone()) {
                return Err(MutateError::Collision(k.clone()));
            }
        }
        check_duplicate_keys(&node.children)?;
    }
    Ok(())
}

// ── Serialize fragment ────────────────────────────────────────────────────────

/// Collect the raw text of a merged standalone `#` comment block from its first
/// COMMENT token: consecutive COMMENT tokens separated only by a single NEWLINE
/// (+ optional INDENT/WHITESPACE) join with `\n`. A second consecutive NEWLINE
/// ends the block (matches project.rs comment-merge logic).
fn comment_block_text(first: &crate::model::yaml::syntax::SyntaxToken) -> String {
    use crate::model::yaml::syntax::SyntaxKind;
    use rowan::NodeOrToken;

    let mut out = vec![first.text().trim_end().to_string()];
    let mut sib = first.next_sibling_or_token();
    let mut newlines = 0u32;
    while let Some(el) = sib {
        match el.kind() {
            SyntaxKind::WHITESPACE | SyntaxKind::INDENT => {}
            SyntaxKind::NEWLINE => {
                newlines += 1;
                if newlines >= 2 {
                    break;
                }
            }
            SyntaxKind::COMMENT if newlines == 1 => {
                if let NodeOrToken::Token(tok) = &el {
                    out.push(tok.text().trim_end().to_string());
                }
                newlines = 0;
            }
            _ => break,
        }
        sib = el.next_sibling_or_token();
    }
    out.join("\n")
}

/// Serialize the node at `path` as a standalone fragment (for clipboard / `$EDITOR`).
pub fn serialize_fragment(syntax: &SyntaxNode, path: &[Seg]) -> String {
    match resolve(syntax, path) {
        Some(Target::MapEntry(entry)) => entry.text().to_string().trim_end().to_string(),
        Some(Target::Element(entry)) => entry.text().to_string().trim_end().to_string(),
        Some(Target::Comment(tok)) => comment_block_text(&tok),
        Some(Target::Opaque(node)) => node.text().to_string().trim_end().to_string(),
        None => String::new(),
    }
}

// ── Atomic dispatcher ─────────────────────────────────────────────────────────

/// Extract the primary path(s) from a mutation for the opaque pre-check.
fn mutation_paths(m: &Mutation) -> Vec<&Vec<Seg>> {
    match m {
        Mutation::Delete { path } => vec![path],
        Mutation::Insert { target, .. } => vec![&target.parent],
        Mutation::Replace { path, .. } => vec![path],
        Mutation::Rename { path, .. } => vec![path],
        Mutation::Remark { path } => vec![path],
        Mutation::EditComment { path, .. } => vec![path],
        Mutation::InsertComment { target, .. } => vec![&target.parent],
        Mutation::Move {
            sources, target, ..
        } => {
            let mut paths: Vec<&Vec<Seg>> = sources.iter().collect();
            paths.push(&target.parent);
            paths
        }
        Mutation::ConvertKind { path, .. } => vec![path],
    }
}

pub fn apply(syntax: &SyntaxNode, m: Mutation) -> Result<SyntaxNode, MutateError> {
    // Opaque pre-check: any target path inside (or equal to) an opaque span → Unsupported.
    for path in mutation_paths(&m) {
        if !path.is_empty() && is_opaque(syntax, path) {
            return Err(MutateError::Unsupported);
        }
    }

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

// ── Test helpers (pub(crate) so later chunk tests can import them) ────────────

#[cfg(test)]
pub(crate) fn parse_syntax(src: &str) -> SyntaxNode {
    SyntaxNode::new_root(
        crate::model::yaml::parse::parse(src).unwrap_or_else(|e| panic!("parse failed: {e}")),
    )
}

/// Parse `src`, apply `m`, and return the serialized result.
/// Used by per-variant tests across later chunks.
#[cfg(test)]
pub(crate) fn apply_str(
    src: &str,
    m: crate::model::document::Mutation,
) -> Result<String, crate::model::document::MutateError> {
    let t = parse_syntax(src);
    apply(&t, m).map(|tree| tree.to_string())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::document::{MutateError, Mutation};
    use crate::model::node::Seg;

    // ── Indent engine tests ───────────────────────────────────────────────────

    #[test]
    fn reindent_shifts_every_line() {
        assert_eq!(
            reindent("a: 1\nb:\n  c: 2\n", 0, 4),
            "    a: 1\n    b:\n      c: 2\n"
        );
        assert_eq!(reindent("    x: 1\n", 4, 0), "x: 1\n");
    }

    #[test]
    fn reindent_preserves_block_scalar_body_relative_indent() {
        let frag = "note: |\n  line one\n  line two\n";
        assert_eq!(
            reindent(frag, 0, 2),
            "  note: |\n    line one\n    line two\n"
        );
    }

    // ── Opaque rejection test ─────────────────────────────────────────────────

    #[test]
    fn mutations_on_opaque_are_unsupported() {
        let src = "ref: *anchor\nk: 1\n";
        let s = parse_syntax(src);
        let m = Mutation::Delete {
            path: vec![Seg::Key("ref".into())],
        };
        assert!(matches!(apply(&s, m), Err(MutateError::Unsupported)));
    }

    // ── serialize_fragment tests ─────────────────────────────────────────────

    #[test]
    fn fragment_of_map_entry() {
        let s = parse_syntax("a: 1\nb: hello\n");
        assert_eq!(serialize_fragment(&s, &[Seg::Key("a".into())]), "a: 1");
        assert_eq!(serialize_fragment(&s, &[Seg::Key("b".into())]), "b: hello");
    }

    #[test]
    fn fragment_of_seq_entry() {
        let s = parse_syntax("- 10\n- 20\n- 30\n");
        assert_eq!(serialize_fragment(&s, &[Seg::Index(1)]), "- 20");
    }

    #[test]
    fn fragment_of_comment() {
        let s = parse_syntax("# hello\na: 1\n");
        // Comment is at index 0.
        assert_eq!(serialize_fragment(&s, &[Seg::Index(0)]), "# hello");
    }

    #[test]
    fn fragment_of_unknown_path_is_empty() {
        let s = parse_syntax("a: 1\n");
        assert_eq!(serialize_fragment(&s, &[Seg::Key("nope".into())]), "");
    }

    // ── apply stub returns Unsupported for non-opaque mutations ──────────────

    #[test]
    fn non_opaque_mutations_return_unsupported() {
        // Since all variant fns are stubs, a normal (non-opaque) path also
        // returns Unsupported — this confirms the opaque check doesn't swallow
        // regular paths.
        let r = apply_str(
            "k: 1\n",
            Mutation::Delete {
                path: vec![Seg::Key("k".into())],
            },
        );
        assert!(matches!(r, Err(MutateError::Unsupported)));
    }

    // ── apply_str helper smoke test ──────────────────────────────────────────

    #[test]
    fn apply_str_returns_unsupported_for_all_stubs() {
        let mutations: Vec<Mutation> = vec![
            Mutation::Delete {
                path: vec![Seg::Key("a".into())],
            },
            Mutation::Replace {
                path: vec![Seg::Key("a".into())],
                fragment: "a: 2".into(),
            },
            Mutation::Rename {
                path: vec![Seg::Key("a".into())],
                new_key: "b".into(),
            },
        ];
        for m in mutations {
            let r = apply_str("a: 1\n", m);
            assert!(
                matches!(r, Err(MutateError::Unsupported)),
                "expected Unsupported"
            );
        }
    }
}
