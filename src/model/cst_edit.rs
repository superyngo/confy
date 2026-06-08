//! Phase 3 of the CST migration: apply a [`Mutation`] to the rowan tree by
//! splicing green nodes/tokens (the mutable `clone_for_update` API).
//!
//! Resolution uses the same `path → element` index that `cst_project::walk`
//! produces, so the resolver can never disagree with the projection. Each `apply`
//! works on a `clone_for_update` of the document and returns the new tree only on
//! success, so a failed multi-step edit (e.g. `Move`) rolls back for free — the
//! caller keeps the original tree untouched.
//!
//! Implemented so far: `Replace` (whole-document on the empty path; a scalar value
//! inline edit) and `EditComment`. The remaining variants return `Unsupported`
//! until ported.

use crate::model::cst_project::{walk, CstIndex, Target};
use crate::model::document::{MutateError, Mutation, Target as InsTarget};
use crate::model::node::{Node, Seg};
use taplo::rowan::NodeOrToken;
use taplo::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

/// Apply `m` to a copy of `syntax`, returning the new tree. The original is never
/// mutated, so the caller commits only on `Ok`.
pub(crate) fn apply(syntax: &SyntaxNode, m: Mutation) -> Result<SyntaxNode, MutateError> {
    let tree = syntax.clone_for_update();
    match m {
        Mutation::Replace { path, toml, .. } => {
            if path.is_empty() {
                return reparse_document(&toml);
            }
            replace_value(&tree, &path, &toml)?;
            Ok(tree)
        }
        Mutation::EditComment { path, text } => {
            edit_comment(&tree, &path, &text)?;
            Ok(tree)
        }
        Mutation::Delete { path } => {
            delete(&tree, &path)?;
            Ok(tree)
        }
        Mutation::InsertComment { target, text } => {
            insert_comment(&tree, &target, &text)?;
            Ok(tree)
        }
        _ => Err(MutateError::Unsupported),
    }
}

/// Empty-path `Replace`: reparse the edited text as a whole new document, rejecting
/// invalid TOML (the document is left untouched because the caller keeps the old
/// tree on `Err`).
fn reparse_document(toml: &str) -> Result<SyntaxNode, MutateError> {
    let parse = taplo::parser::parse(toml);
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    Ok(parse.into_syntax().clone_for_update())
}

/// Replace a scalar value in place (inline value edit). `toml` is a `key = <value>`
/// fragment (array elements use a synthetic `__elem__ = <value>`); only the scalar
/// token is swapped, so a trailing EOL comment and any surrounding array indent are
/// preserved.
fn replace_value(tree: &SyntaxNode, path: &[Seg], toml: &str) -> Result<(), MutateError> {
    let (_, idx) = walk(tree, "");
    let target = idx
        .iter()
        .find(|(p, _)| p == path)
        .map(|(_, t)| t.clone())
        .ok_or(MutateError::NotFound)?;
    let value = match target {
        Target::Entry(entry) => entry
            .children()
            .find(|c| c.kind() == SyntaxKind::VALUE)
            .ok_or(MutateError::NotFound)?,
        Target::ArrayElement(value) => value,
        _ => return Err(MutateError::Unsupported),
    };

    // The new scalar token from the fragment's first ENTRY's VALUE.
    let parse = taplo::parser::parse(toml);
    if let Some(e) = parse.errors.first() {
        return Err(MutateError::Fragment(e.to_string()));
    }
    let frag = parse.into_syntax().clone_for_update();
    let new_value = frag
        .descendants()
        .find(|n| n.kind() == SyntaxKind::VALUE)
        .ok_or_else(|| MutateError::Fragment("fragment has no value".into()))?;
    let new_tok = scalar_token(&new_value)
        .ok_or_else(|| MutateError::Fragment("fragment value is not a scalar".into()))?;

    // Swap only the scalar token inside the target VALUE (keeps EOL comment/indent).
    let old_tok = scalar_token(&value).ok_or(MutateError::Unsupported)?;
    let i = old_tok.index();
    new_tok.detach();
    value.splice_children(i..i + 1, vec![NodeOrToken::Token(new_tok)]);
    Ok(())
}

/// Replace the text of the standalone comment block at `path`. The block is the run
/// of `COMMENT` tokens (separated by single newlines) starting at the indexed
/// token; it is spliced with `text`'s lines, each validated to start with `#`.
fn edit_comment(tree: &SyntaxNode, path: &[Seg], text: &str) -> Result<(), MutateError> {
    if text
        .lines()
        .any(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
    {
        return Err(MutateError::Fragment(
            "comment lines must start with #".into(),
        ));
    }
    let (_, idx) = walk(tree, "");
    let first = match idx.iter().find(|(p, _)| p == path).map(|(_, t)| t.clone()) {
        Some(Target::Comment(t)) => t,
        Some(_) => return Err(MutateError::Unsupported),
        None => return Err(MutateError::NotFound),
    };
    let parent = first.parent().ok_or(MutateError::NotFound)?;
    let (start, end) = comment_block_range(&parent, &first);

    // New COMMENT/NEWLINE elements from parsing the replacement (drop a trailing
    // newline — the block's following newline stays in place).
    let frag = taplo::parser::parse(text).into_syntax().clone_for_update();
    let mut els: Vec<_> = frag.children_with_tokens().collect();
    while matches!(els.last(), Some(NodeOrToken::Token(t)) if t.kind() == SyntaxKind::NEWLINE) {
        els.pop();
    }
    for e in &els {
        e.detach();
    }
    parent.splice_children(start..end, els);
    Ok(())
}

/// Delete the node at `path`. A keyed entry (leaf / array / inline table) at the
/// document or table scope is removed with its trailing newline; a comment block is
/// removed with its trailing newline. Because comments are independent nodes now,
/// deleting an entry leaves any adjacent comment in place for free.
fn delete(tree: &SyntaxNode, path: &[Seg]) -> Result<(), MutateError> {
    let (_, idx) = walk(tree, "");
    let target = idx
        .iter()
        .find(|(p, _)| p == path)
        .map(|(_, t)| t.clone())
        .ok_or(MutateError::NotFound)?;
    match target {
        Target::Comment(first) => {
            let parent = first.parent().ok_or(MutateError::NotFound)?;
            let (start, end) = comment_block_range(&parent, &first);
            let end = extend_over_newline(&parent, end);
            parent.splice_children(start..end, vec![]);
            Ok(())
        }
        Target::Entry(entry) => {
            let parent = entry.parent().ok_or(MutateError::NotFound)?;
            // Inline-table members carry a `,` separator (deferred); handle the
            // document/table scope where an entry occupies its own line.
            if parent.kind() != SyntaxKind::ROOT {
                return Err(MutateError::Unsupported);
            }
            let i = entry.index();
            let end = extend_over_newline(&parent, i + 1);
            parent.splice_children(i..end, vec![]);
            Ok(())
        }
        _ => Err(MutateError::Unsupported),
    }
}

/// Insert a standalone comment block at the projected `target` position. Comments
/// are independent nodes — no key, no collision.
fn insert_comment(tree: &SyntaxNode, target: &InsTarget, text: &str) -> Result<(), MutateError> {
    if text
        .lines()
        .any(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
    {
        return Err(MutateError::Fragment(
            "comment lines must start with #".into(),
        ));
    }
    let (proj, idx) = walk(tree, "");
    let at = resolve_insert_at(tree, &proj.root, &idx, target)?;
    // `# …\n` per line, so each comment lands on its own line before the anchor.
    let frag_text = if text.ends_with('\n') {
        text.to_string()
    } else {
        format!("{text}\n")
    };
    let frag = taplo::parser::parse(&frag_text)
        .into_syntax()
        .clone_for_update();
    let els: Vec<_> = frag.children_with_tokens().collect();
    for e in &els {
        e.detach();
    }
    tree.splice_children(at..at, els);
    Ok(())
}

/// Map a projected insertion `target` (`parent` path + child `index`) to a splice
/// position among the flat ROOT's children. Handles inserting *before* the child
/// currently at `index`, and appending at the end of the document or a simple table
/// scope. (Appending into a table that contains sub-tables is deferred.)
fn resolve_insert_at(
    tree: &SyntaxNode,
    root: &Node,
    idx: &CstIndex,
    target: &InsTarget,
) -> Result<usize, MutateError> {
    let parent = node_at(root, &target.parent).ok_or(MutateError::NotFound)?;
    if target.index < parent.children.len() {
        // Insert before the child currently at `index`.
        let anchor = &parent.children[target.index];
        return element_root_index(idx, anchor).ok_or(MutateError::Unsupported);
    }
    // Append at the end of the parent's scope.
    if target.parent.is_empty() {
        return Ok(tree.children_with_tokens().count());
    }
    // A table scope: after the last element belonging to it (header + children),
    // consuming the following newline so the insert starts on a fresh line.
    let header_pos = idx
        .iter()
        .find(|(p, t)| p == &target.parent && matches!(t, Target::Header(_)))
        .and_then(|(_, t)| match t {
            Target::Header(n) => Some(n.index()),
            _ => None,
        });
    let mut last = header_pos.ok_or(MutateError::Unsupported)?;
    for child in &parent.children {
        if let Some(p) = element_root_index(idx, child) {
            last = last.max(p);
        }
    }
    Ok(extend_over_newline(tree, last + 1))
}

/// The ROOT-child index of the syntax element backing `node` (an entry, header, AoT
/// entry, or comment — all flat ROOT children).
fn element_root_index(idx: &CstIndex, node: &Node) -> Option<usize> {
    let t = idx.iter().find(|(p, _)| p == &node.path).map(|(_, t)| t)?;
    match t {
        Target::Entry(n) | Target::Header(n) | Target::AotEntry(n) => Some(n.index()),
        Target::Comment(tok) => Some(tok.index()),
        // An AoT group has no single element; anchor on its first entry.
        Target::AotGroup => node
            .children
            .first()
            .and_then(|first| element_root_index(idx, first)),
        Target::ArrayElement(_) => None,
    }
}

/// Navigate the projected tree to the node at `path`.
fn node_at<'a>(root: &'a Node, path: &[Seg]) -> Option<&'a Node> {
    let mut cur = root;
    for i in 0..path.len() {
        cur = cur.children.iter().find(|c| c.path == path[..=i])?;
    }
    Some(cur)
}

/// If the element at `at` is a `NEWLINE`, return `at + 1` (so a splice consumes it),
/// else `at`.
fn extend_over_newline(parent: &SyntaxNode, at: usize) -> usize {
    let els: Vec<_> = parent.children_with_tokens().collect();
    if matches!(els.get(at), Some(NodeOrToken::Token(t)) if t.kind() == SyntaxKind::NEWLINE) {
        at + 1
    } else {
        at
    }
}

/// The `[start, end)` child-index range of the comment block beginning at `first`
/// within `parent`: consecutive `COMMENT` tokens separated by single newlines.
fn comment_block_range(parent: &SyntaxNode, first: &SyntaxToken) -> (usize, usize) {
    let els: Vec<_> = parent.children_with_tokens().collect();
    let start = first.index();
    let mut end = start + 1; // one past the first COMMENT
    let mut i = end;
    while i + 1 < els.len() {
        let sep_is_single_nl = matches!(&els[i], NodeOrToken::Token(t)
            if t.kind() == SyntaxKind::NEWLINE && t.text().matches('\n').count() == 1);
        let next_is_comment = matches!(&els[i + 1], NodeOrToken::Token(t)
            if t.kind() == SyntaxKind::COMMENT);
        if sep_is_single_nl && next_is_comment {
            end = i + 2;
            i += 2;
        } else {
            break;
        }
    }
    (start, end)
}

/// The scalar value token of a `VALUE` node (skips trivia and a trailing comment).
fn scalar_token(value: &SyntaxNode) -> Option<SyntaxToken> {
    value.children_with_tokens().find_map(|c| match c {
        NodeOrToken::Token(t) if is_scalar_kind(t.kind()) => Some(t),
        _ => None,
    })
}

fn is_scalar_kind(k: SyntaxKind) -> bool {
    use SyntaxKind as K;
    matches!(
        k,
        K::STRING
            | K::MULTI_LINE_STRING
            | K::STRING_LITERAL
            | K::MULTI_LINE_STRING_LITERAL
            | K::INTEGER
            | K::INTEGER_HEX
            | K::INTEGER_OCT
            | K::INTEGER_BIN
            | K::FLOAT
            | K::BOOL
            | K::DATE_TIME_OFFSET
            | K::DATE_TIME_LOCAL
            | K::DATE
            | K::TIME
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::document::ConfigDocument;
    use std::io::Write;

    fn doc(src: &str) -> crate::model::cst_doc::CstDocument {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(src.as_bytes()).unwrap();
        crate::model::cst_doc::CstDocument::load(f.path()).unwrap()
    }

    #[test]
    fn replace_scalar_value_keeps_everything_else() {
        let mut d = doc("a = 1\nb = 2\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("b".into())],
            toml: "b = 42\n".into(),
            sync_decor: false,
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\nb = 42\n");
    }

    #[test]
    fn replace_scalar_preserves_trailing_comment() {
        let mut d = doc("port = 8080  # http\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("port".into())],
            toml: "port = 9090\n".into(),
            sync_decor: false,
        })
        .unwrap();
        assert_eq!(d.serialize(), "port = 9090  # http\n");
    }

    #[test]
    fn replace_array_element_in_place() {
        let mut d = doc("arr = [0x1, 0o2, 3] # tail\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("arr".into()), Seg::Index(1)],
            toml: "__elem__ = 99\n".into(),
            sync_decor: false,
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [0x1, 99, 3] # tail\n");
    }

    #[test]
    fn replace_in_table_scope() {
        let mut d = doc("[server]\nport = 8080\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("server".into()), Seg::Key("port".into())],
            toml: "port = 1\n".into(),
            sync_decor: false,
        })
        .unwrap();
        assert_eq!(d.serialize(), "[server]\nport = 1\n");
    }

    #[test]
    fn replace_empty_path_reparses_document() {
        let mut d = doc("a = 1\nb = 2\n");
        d.apply(Mutation::Replace {
            path: vec![],
            toml: "a = 10\nc = 3\n".into(),
            sync_decor: true,
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 10\nc = 3\n");
    }

    #[test]
    fn replace_empty_path_rejects_invalid_and_leaves_doc_intact() {
        let mut d = doc("a = 1\n");
        let err = d
            .apply(Mutation::Replace {
                path: vec![],
                toml: "a = = bad".into(),
                sync_decor: true,
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Fragment(_)));
        assert_eq!(d.serialize(), "a = 1\n");
    }

    #[test]
    fn edit_single_line_comment() {
        let mut d = doc("# old\na = 1\n");
        d.apply(Mutation::EditComment {
            path: vec![Seg::Index(0)],
            text: "# new".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "# new\na = 1\n");
    }

    #[test]
    fn edit_multiline_comment_block() {
        let mut d = doc("# one\n# two\na = 1\n");
        d.apply(Mutation::EditComment {
            path: vec![Seg::Index(0)],
            text: "# uno\n# dos\n# tres".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "# uno\n# dos\n# tres\na = 1\n");
    }

    #[test]
    fn edit_comment_rejects_non_comment_text() {
        let mut d = doc("# old\na = 1\n");
        let err = d
            .apply(Mutation::EditComment {
                path: vec![Seg::Index(0)],
                text: "not a comment".into(),
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Fragment(_)));
        assert_eq!(d.serialize(), "# old\na = 1\n");
    }

    #[test]
    fn delete_leaf_entry() {
        let mut d = doc("a = 1\nb = 2\nc = 3\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("b".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\nc = 3\n");
    }

    #[test]
    fn delete_entry_leaves_adjacent_comment_behind() {
        // The migration's payoff: a comment is an independent node, so deleting the
        // entry below it does not remove the comment.
        let mut d = doc("# keep me\nb = 2\nc = 3\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("b".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "# keep me\nc = 3\n");
    }

    #[test]
    fn delete_single_and_multiline_comment() {
        let mut d = doc("# gone\na = 1\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Index(0)],
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\n");

        let mut d = doc("# one\n# two\na = 1\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Index(0)],
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\n");
    }

    #[test]
    fn delete_entry_in_table_scope() {
        let mut d = doc("[s]\nx = 1\ny = 2\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("s".into()), Seg::Key("x".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "[s]\ny = 2\n");
    }

    #[test]
    fn insert_comment_before_entry() {
        let mut d = doc("a = 1\nb = 2\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![],
                index: 1,
            },
            text: "# note".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\n# note\nb = 2\n");
    }

    #[test]
    fn insert_comment_at_document_end() {
        let mut d = doc("a = 1\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![],
                index: 9,
            },
            text: "# tail".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\n# tail\n");
    }

    #[test]
    fn insert_multiline_comment_before_entry() {
        let mut d = doc("a = 1\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![],
                index: 0,
            },
            text: "# one\n# two".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "# one\n# two\na = 1\n");
    }

    #[test]
    fn insert_comment_in_table_scope() {
        let mut d = doc("[s]\nx = 1\ny = 2\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![Seg::Key("s".into())],
                index: 1,
            },
            text: "# between".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "[s]\nx = 1\n# between\ny = 2\n");
    }

    #[test]
    fn insert_comment_rejects_non_comment() {
        let mut d = doc("a = 1\n");
        let err = d
            .apply(Mutation::InsertComment {
                target: InsTarget {
                    parent: vec![],
                    index: 0,
                },
                text: "nope".into(),
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Fragment(_)));
        assert_eq!(d.serialize(), "a = 1\n");
    }

    #[test]
    fn edit_comment_inside_table_scope() {
        let mut d = doc("[s]\n# explain\nport = 1\n");
        d.apply(Mutation::EditComment {
            path: vec![Seg::Key("s".into()), Seg::Index(0)],
            text: "# clarify".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "[s]\n# clarify\nport = 1\n");
    }
}
