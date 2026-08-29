//! Phase 3 of the CST migration: apply a [`Mutation`] to the rowan tree by
//! splicing green nodes/tokens (the mutable `clone_for_update` API).
//!
//! Resolution uses the same `path → element` index that `cst_project::walk`
//! produces, so the resolver can never disagree with the projection. Each `apply`
//! works on a `clone_for_update` of the document and returns the new tree only on
//! success, so a failed multi-step edit (e.g. `Move`) rolls back for free — the
//! caller keeps the original tree untouched.
//!
//! All eight `Mutation` variants are ported: `Replace` (whole-document, scalar
//! value, structured array/inline-table, and whole `[table]` section), `Insert`
//! (keyed into a table/root with Cancel/Overwrite/Rename collisions, and bare array
//! elements), `Delete` (entry, comment, array element, `[table]` section, `[[aot]]`
//! entry), `Rename`, `Remark`, `EditComment`, `InsertComment`, and `Move` (atomic;
//! comments stay put because they are independent nodes). Deferred long-tail edges:
//! inline-table member delete, AoT entry move/remark, whole-AoT delete/replace, and
//! byte-perfect multiline-array element insert/delete spacing.

mod aot_group;
mod convert;
mod dotted_table;
pub(crate) mod escape;
mod move_paste;
mod rename;
mod replace_delete;
mod tree_nav;

use crate::model::cst_project::{walk, Target};
use crate::model::document::{MutateError, Mutation};
use crate::model::node::{NodeKind, Seg};
use aot_group::{aot_entry_member_fragments, aot_group_span};
use convert::convert_kind;
use dotted_table::{
    dotted_ancestor_prefix_len, inline_ancestor_len, inline_member_entries, strip_key_prefix,
};
use move_paste::{insert, move_nodes};
use rename::rename;
use replace_delete::{
    delete, edit_comment, insert_comment, remark, reparse_document, replace_value, section_text,
    set_trailing_comment, table_fragment,
};
use taplo::rowan::NodeOrToken;
use taplo::syntax::{SyntaxKind, SyntaxNode};
use tree_nav::node_at;

/// Apply `m` to a copy of `syntax`, returning the new **immutable** tree and its
/// serialization. The original is never mutated, so the caller commits only on
/// `Ok`.
///
/// The mutation runs on a `clone_for_update` (mutable) tree, which must be
/// normalized back to an immutable tree so the next `apply` can
/// `clone_for_update` again. That normalization is a serialize + re-parse — and
/// `validate_semantics` needed exactly the same serialize + re-parse for its DOM
/// check, so the two used to run back-to-back on every mutation, doing the whole
/// job twice. They now share one serialize and one parse; the caller gets both
/// results back rather than recomputing them.
pub(crate) fn apply(syntax: &SyntaxNode, m: Mutation) -> Result<(SyntaxNode, String), MutateError> {
    let tree = syntax.clone_for_update();
    let result = match m {
        Mutation::Replace {
            path,
            fragment: toml,
            ..
        } => {
            if path.is_empty() {
                reparse_document(&toml)?
            } else {
                match replace_value(&tree, &path, &toml)? {
                    Some(comment) => set_trailing_comment(&tree, &path, Some(&comment))?,
                    None => tree,
                }
            }
        }
        Mutation::EditComment { path, text } => {
            edit_comment(&tree, &path, &text)?;
            tree
        }
        Mutation::Delete { path } => {
            delete(&tree, &path)?;
            tree
        }
        Mutation::InsertComment { target, text } => {
            insert_comment(&tree, &target, &text)?;
            tree
        }
        Mutation::Insert {
            target,
            fragment: toml,
            on_collision,
            suggested_key,
        } => {
            insert(
                &tree,
                &target,
                &toml,
                on_collision,
                suggested_key.as_deref(),
            )?;
            tree
        }
        Mutation::Rename { path, new_key } => {
            rename(&tree, &path, &new_key)?;
            tree
        }
        Mutation::Remark { path } => {
            remark(&tree, &path)?;
            tree
        }
        Mutation::Move {
            sources,
            target,
            on_collision,
        } => {
            move_nodes(&tree, &sources, &target, on_collision)?;
            tree
        }
        Mutation::ConvertKind { path, target } => {
            convert_kind(&tree, &path, target)?;
            tree
        }
        Mutation::SetTrailingComment { path, comment } => {
            set_trailing_comment(&tree, &path, comment.as_deref())?
        }
    };
    // One serialize, one parse, used for both the DOM check and the normalized
    // tree the caller commits. Cloning `Parse` only bumps the green node's
    // refcount, so both consuming views are effectively free.
    let text = result.to_string();
    let parse = taplo::parser::parse(&text);
    validate_dom(parse.clone().into_dom())?;
    Ok((parse.into_syntax(), text))
}

/// Semantic backstop run on every successful mutation before commit: taplo's
/// parser is syntax-only (a duplicate `[a]` section or re-defined key parses
/// clean), so the result is checked with taplo's DOM validation, which rejects
/// conflicting keys / table redefinitions while accepting every legal layout
/// (scattered `[a] … [a.sub]`, dotted siblings, AoT re-openings, the
/// `fruit.apple` mixed pattern). Catches duplicates the targeted pre-checks
/// can't see — e.g. a whole-document or block `$EDITOR` rewrite that introduces
/// a duplicate section.
///
/// Takes an already-built DOM so the mutation path can share its single parse
/// with the normalization re-parse (see `apply`).
fn validate_dom(dom: taplo::dom::Node) -> Result<(), MutateError> {
    if let Err(errors) = dom.validate() {
        if let Some(e) = errors.into_iter().next() {
            return Err(match &e {
                taplo::dom::Error::ConflictingKeys { key, .. } => {
                    MutateError::Collision(key.value().to_string())
                }
                other => MutateError::Illegal(other.to_string()),
            });
        }
    }
    Ok(())
}

/// Serialize the node at `path` as a standalone fragment (clipboard / `$EDITOR`).
/// In the CST a fragment is just the node's source text — comments are independent
/// nodes, so a node never carries an adjacent comment (`carry_comment` is moot).
pub(crate) fn serialize_fragment(syntax: &SyntaxNode, path: &[Seg]) -> String {
    serialize_fragment_impl(syntax, path, false)
}

/// Like [`serialize_fragment`] but **scope-relative**: a node copied out of a
/// `[T/D]` dotted table has its leading dotted-ancestor key segments dropped
/// (`dotted.test.bool_true` → `bool_true`; the `test` subtable's members →
/// `test.bool_true`). Used by copy/cut so a paste re-prefixes only for the new
/// destination instead of stacking the source's prefix. The plain
/// `serialize_fragment` (used by the `$EDITOR` block edit, which must keep full
/// keys for `replace_dotted_table`) is unaffected.
pub(crate) fn serialize_fragment_relative(syntax: &SyntaxNode, path: &[Seg]) -> String {
    serialize_fragment_impl(syntax, path, true)
}

pub(crate) fn serialize_fragment_impl(syntax: &SyntaxNode, path: &[Seg], relative: bool) -> String {
    let (proj, idx) = walk(syntax, "");
    // A comment node: its raw `# …` text.
    if let Some(node) = node_at(&proj.root, path) {
        if let NodeKind::Comment(t) = &node.kind {
            return t.clone();
        }
        // A table is an *open set* of member spans (dotted entries and/or
        // `[…]` sections, possibly scattered) — capture all of them.
        if matches!(node.kind, NodeKind::Table) && matches!(path.last(), Some(Seg::Key(_))) {
            if let Some(text) = table_fragment(syntax, &idx, &proj.root, path, relative) {
                return text;
            }
            // A synthetic `[T/D]` table *inside an inline table*: its members are
            // `x.y = 1` entries in the `{ … }`. Verbatim keys for the `$EDITOR`
            // block edit; relative drops the segments between the inline table and
            // the node (keeping the node's own key, like the flat capture).
            if let Some(inline_len) = inline_ancestor_len(&proj.root, path) {
                let members = inline_member_entries(&idx, path);
                if !members.is_empty() {
                    let strip = if relative {
                        path.len() - 1 - inline_len
                    } else {
                        0
                    };
                    return members
                        .iter()
                        .map(|m| format!("{}\n", strip_key_prefix(m, strip).trim()))
                        .collect();
                }
            }
        }
    }
    let target = match idx.iter().find(|(p, _)| p == path).map(|(_, t)| t) {
        Some(t) => t,
        None => return String::new(),
    };
    match target {
        Target::Entry(n) | Target::ArrayElement(n) => {
            let strip = if relative {
                dotted_ancestor_prefix_len(&idx, &proj.root, path)
            } else {
                0
            };
            let s = strip_key_prefix(n, strip);
            if s.ends_with('\n') {
                s
            } else {
                format!("{s}\n")
            }
        }
        // A table / AoT entry: the section's source text (header + its lines).
        Target::Header(h) => section_text(syntax, path, h.index(), false),
        // Relative (clipboard) capture splits the entry into its member
        // fragments (sub-sections flattened to dotted entries) — pasted out of
        // its array it becomes member nodes, like an inline-table element. A
        // nested `[[…]]` sub-group has no dotted form, so that entry falls back
        // to the full section capture. The full capture (the `$EDITOR` block
        // edit) keeps the `[[…]]` header.
        Target::AotEntry(h) => {
            if relative {
                match aot_entry_member_fragments(syntax, h) {
                    Ok(frags) => frags.concat(),
                    Err(_) => section_text(syntax, &[], h.index(), true),
                }
            } else {
                section_text(syntax, &[], h.index(), true)
            }
        }
        // The whole `[[x]]` group: all of its entries, in order.
        Target::AotGroup => match aot_group_span(syntax, path) {
            Some((start, end)) => {
                let els: Vec<_> = syntax.children_with_tokens().collect();
                els[start..end]
                    .iter()
                    .map(|e| match e {
                        NodeOrToken::Node(n) => n.to_string(),
                        NodeOrToken::Token(t) => t.text().to_string(),
                    })
                    .collect()
            }
            None => String::new(),
        },
        Target::Comment(_) => String::new(),
    }
}

/// True when `text` parses clean as a standalone TOML document with **no**
/// `[table]`/`[[aot]]` header — i.e. keyed entry lines that can be joined with
/// other fragments into one `[[…]]` entry body.
///
/// `pub` (not `pub(crate)`) so the TUI crate's paste-forming can pre-check it.
pub fn joinable_entry(text: &str) -> bool {
    let parse = taplo::parser::parse(text);
    parse.errors.is_empty()
        && !parse.into_syntax().descendants().any(|n| {
            matches!(
                n.kind(),
                SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::model::document::ConfigDocument;
    use crate::model::document::{OnCollision, Target as InsTarget};

    fn doc(src: &str) -> crate::model::cst_doc::CstDocument {
        crate::model::cst_doc::CstDocument::from_str(src).unwrap()
    }

    #[test]
    fn set_trailing_comment_add_change_clear() {
        let set = |c: Option<&str>| Mutation::SetTrailingComment {
            path: vec![Seg::Key("port".into())],
            comment: c.map(str::to_string),
        };
        // add
        let mut d = doc("port = 8080\n");
        d.apply(set(Some("# http"))).unwrap();
        assert_eq!(d.serialize(), "port = 8080  # http\n");
        // change
        let mut d = doc("port = 8080  # old\n");
        d.apply(set(Some("# new"))).unwrap();
        assert_eq!(d.serialize(), "port = 8080  # new\n");
        // clear
        let mut d = doc("port = 8080  # old\n");
        d.apply(set(None)).unwrap();
        assert_eq!(d.serialize(), "port = 8080\n");
        // a `#` inside a basic string is not the trailing comment
        let mut d = doc("port = \"a # b\"\n");
        d.apply(set(Some("# note"))).unwrap();
        assert_eq!(d.serialize(), "port = \"a # b\"  # note\n");
    }

    #[test]
    fn set_trailing_comment_on_table_and_aot_headers() {
        use crate::model::node::NodeKind;
        // [section] header: add / change / clear after the closing bracket.
        let set = |path: Vec<Seg>, c: Option<&str>| Mutation::SetTrailingComment {
            path,
            comment: c.map(str::to_string),
        };
        let mut d = doc("[srv]\nx = 1\n");
        d.apply(set(vec![Seg::Key("srv".into())], Some("# the server")))
            .unwrap();
        assert_eq!(d.serialize(), "[srv]  # the server\nx = 1\n");
        // it also projects onto the table node
        let tree = d.project();
        let srv = &tree.root.children[0];
        assert!(matches!(srv.kind, NodeKind::Table));
        assert_eq!(srv.trailing_comment.as_deref(), Some("# the server"));
        // change + clear
        d.apply(set(vec![Seg::Key("srv".into())], Some("# renamed")))
            .unwrap();
        assert_eq!(d.serialize(), "[srv]  # renamed\nx = 1\n");
        d.apply(set(vec![Seg::Key("srv".into())], None)).unwrap();
        assert_eq!(d.serialize(), "[srv]\nx = 1\n");

        // [[aot]] header: comment rides on the entry, after `]]`.
        let mut d = doc("[[item]]\nn = 1\n[[item]]\nn = 2\n");
        let entry0 = vec![Seg::Key("item".into()), Seg::Index(0)];
        d.apply(set(entry0.clone(), Some("# first"))).unwrap();
        assert_eq!(d.serialize(), "[[item]]  # first\nn = 1\n[[item]]\nn = 2\n");
        let tree = d.project();
        let item = &tree.root.children[0];
        assert_eq!(
            item.children[0].trailing_comment.as_deref(),
            Some("# first")
        );
        d.apply(set(entry0, None)).unwrap();
        assert_eq!(d.serialize(), "[[item]]\nn = 1\n[[item]]\nn = 2\n");
    }

    #[test]
    fn set_trailing_comment_on_multiline_array_element() {
        // A multiline-array element keeps its separator comma when a trailing
        // comment is added, and the comment clears cleanly.
        let set = |idx: usize, c: Option<&str>| Mutation::SetTrailingComment {
            path: vec![Seg::Key("arr".into()), Seg::Index(idx)],
            comment: c.map(str::to_string),
        };
        let mut d = doc("arr = [\n  1,\n  2,\n]\n");
        d.apply(set(0, Some("# first"))).unwrap();
        assert_eq!(d.serialize(), "arr = [\n  1,  # first\n  2,\n]\n");
        // clear it again
        d.apply(set(0, None)).unwrap();
        assert_eq!(d.serialize(), "arr = [\n  1,\n  2,\n]\n");
    }

    #[test]
    fn replace_scalar_value_keeps_everything_else() {
        let mut d = doc("a = 1\nb = 2\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("b".into())],
            fragment: "b = 42\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\nb = 42\n");
    }

    #[test]
    fn replace_scalar_preserves_trailing_comment() {
        let mut d = doc("port = 8080  # http\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("port".into())],
            fragment: "port = 9090\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "port = 9090  # http\n");
    }

    #[test]
    fn replace_scalar_applies_edited_trailing_comment() {
        let mut d = doc("port = 8080  # http\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("port".into())],
            fragment: "port = 9090  # https\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "port = 9090  # https\n");
    }

    #[test]
    fn replace_array_element_in_place() {
        let mut d = doc("arr = [0x1, 0o2, 3] # tail\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("arr".into()), Seg::Index(1)],
            fragment: "__elem__ = 99\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [0x1, 99, 3] # tail\n");
    }

    #[test]
    fn replace_member_of_inline_table_array_element() {
        // Group B item 5: a member of a `[T/I]` element of a multiline `[A/M]`
        // array (`arr[0].a`) is Replace-addressable in place.
        let mut d = doc("arr = [\n  { a = 1, b = 2 },\n  { c = 3 },\n]\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("arr".into()), Seg::Index(0), Seg::Key("a".into())],
            fragment: "a = 5\n".into(),
        })
        .unwrap();
        assert_eq!(
            d.serialize(),
            "arr = [\n  { a = 5, b = 2 },\n  { c = 3 },\n]\n"
        );
    }

    #[test]
    fn insert_member_into_inline_table_array_element() {
        // Group B item 6: inserting a member into a `[T/I]` element of an `[A/M]`
        // array rebuilds the `{ … }` in place (previously `Unsupported`).
        let mut d = doc("arr = [\n  { a = 1, b = 2 },\n  { c = 3 },\n]\n");
        d.apply(Mutation::Insert {
            target: crate::model::document::Target {
                parent: vec![Seg::Key("arr".into()), Seg::Index(0)],
                index: 2,
            },
            fragment: "d = 9\n".into(),
            on_collision: crate::model::document::OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(
            d.serialize(),
            "arr = [\n  { a = 1, b = 2, d = 9 },\n  { c = 3 },\n]\n"
        );
    }

    #[test]
    fn replace_in_table_scope() {
        let mut d = doc("[server]\nport = 8080\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("server".into()), Seg::Key("port".into())],
            fragment: "port = 1\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "[server]\nport = 1\n");
    }

    #[test]
    fn replace_empty_path_reparses_document() {
        let mut d = doc("a = 1\nb = 2\n");
        d.apply(Mutation::Replace {
            path: vec![],
            fragment: "a = 10\nc = 3\n".into(),
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
                fragment: "a = = bad".into(),
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
    fn replace_whole_array_value() {
        let mut d = doc("arr = [1, 2]\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("arr".into())],
            fragment: "arr = [9, 8, 7]\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [9, 8, 7]\n");
    }

    #[test]
    fn replace_inline_table_value_keeps_trailing_comment() {
        let mut d = doc("pt = { x = 1 }  # p\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("pt".into())],
            fragment: "pt = { x = 2, y = 3 }\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "pt = { x = 2, y = 3 }  # p\n");
    }

    #[test]
    fn replace_inline_table_value_applies_edited_trailing_comment() {
        let mut d = doc("pt = { x = 1 }  # p\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("pt".into())],
            fragment: "pt = { x = 2, y = 3 }  # q\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "pt = { x = 2, y = 3 }  # q\n");
    }

    #[test]
    fn delete_array_element_middle_and_last() {
        let mut d = doc("arr = [1, 2, 3]\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("arr".into()), Seg::Index(1)],
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [1, 3]\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("arr".into()), Seg::Index(1)],
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [1]\n");
    }

    #[test]
    fn delete_first_array_element() {
        let mut d = doc("arr = [1, 2, 3]\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("arr".into()), Seg::Index(0)],
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [2, 3]\n");
    }

    #[test]
    fn delete_whole_table_keeps_siblings() {
        let mut d = doc("[a]\nx = 1\n\n[b]\ny = 2\n\n[c]\nz = 3\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("b".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "[a]\nx = 1\n\n[c]\nz = 3\n");
    }

    #[test]
    fn delete_table_takes_nested_subtable() {
        let mut d = doc("[a]\nx = 1\n[a.sub]\nk = 1\n[b]\ny = 2\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("a".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "[b]\ny = 2\n");
    }

    #[test]
    fn replace_whole_table_section() {
        let mut d = doc("[s]\nport = 1\n[d]\nz = 9\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("s".into())],
            fragment: "[s]\nport = 2\nhost = \"x\"\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "[s]\nport = 2\nhost = \"x\"\n[d]\nz = 9\n");
    }

    #[test]
    fn delete_aot_entry() {
        let mut d = doc("[[p]]\nn = 1\n[[p]]\nn = 2\n[[p]]\nn = 3\n");
        // Delete the middle entry (child-position index 1 under `p`).
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("p".into()), Seg::Index(1)],
        })
        .unwrap();
        assert_eq!(d.serialize(), "[[p]]\nn = 1\n[[p]]\nn = 3\n");
    }

    #[test]
    fn array_insert_middle_end_and_empty() {
        let mut d = doc("arr = [1, 3]\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 1,
            },
            fragment: "2\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [1, 2, 3]\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 99,
            },
            fragment: "4\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [1, 2, 3, 4]\n");

        let mut e = doc("xs = []\n");
        e.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("xs".into())],
                index: 0,
            },
            fragment: "7\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(e.serialize(), "xs = [7]\n");
    }

    #[test]
    fn block_edit_dotted_table_consolidates_at_first_position() {
        // `$EDITOR` on a `[T/D]` table: members scattered around `x` get rewritten
        // and land where the first member was.
        let mut d = doc("a.b = 1\nx = 0\na.c = 2\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("a".into())],
            fragment: "a.b = 10\na.c = 20\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "a.b = 10\na.c = 20\nx = 0\n");
    }

    #[test]
    fn block_edit_contiguous_dotted_table() {
        let mut d = doc("a.b = 1\na.c = 2\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("a".into())],
            fragment: "a.b = 1\na.c = 2\na.d = 3\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "a.b = 1\na.c = 2\na.d = 3\n");
    }

    #[test]
    fn rename_plain_key_to_dotted_makes_table() {
        // `foo` → `foo.x` rewrites the key in place, projecting as a `[T/D]` table.
        let mut d = doc("foo = 1\n");
        d.apply(Mutation::Rename {
            path: vec![Seg::Key("foo".into())],
            new_key: "foo.x".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "foo.x = 1\n");
    }

    #[test]
    fn rename_dotted_leaf_swaps_last_segment() {
        let mut d = doc("a.b.c = 1\n");
        d.apply(Mutation::Rename {
            path: vec![
                Seg::Key("a".into()),
                Seg::Key("b".into()),
                Seg::Key("c".into()),
            ],
            new_key: "z".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "a.b.z = 1\n");
    }

    #[test]
    fn delete_dotted_table_removes_all_members() {
        // Delete on a `[T/D]` table fans out to every member (plain cascade).
        let mut d = doc("a.b = 1\nx = 0\na.c = 2\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("a".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "x = 0\n");
    }

    #[test]
    fn delete_last_dotted_leaf_drops_the_table() {
        // Deleting the only remaining member removes the implicit table too.
        let mut d = doc("a.b = 1\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("a".into()), Seg::Key("b".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "");
    }

    #[test]
    fn rename_whole_synthetic_dotted_table_updates_all_members() {
        // Renaming a synthetic `[T/D]` table renames its segment in all member entries.
        let mut d = doc("a.b.c = 1\n");
        d.apply(Mutation::Rename {
            path: vec![Seg::Key("a".into())],
            new_key: "x".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "x.b.c = 1\n");
    }

    #[test]
    fn rename_synthetic_dotted_table_intermediate_segment() {
        // Renaming an intermediate segment in a deeper dotted chain.
        let mut d = doc("a.b.c = 1\na.b.d = 2\n");
        d.apply(Mutation::Rename {
            path: vec![Seg::Key("a".into()), Seg::Key("b".into())],
            new_key: "z".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "a.z.c = 1\na.z.d = 2\n");
    }

    #[test]
    fn rename_scalar_inside_scope_table() {
        // A scoped entry's KEY spells only its own segment — the rename index
        // must be end-relative, not the absolute path position (regression:
        // this returned NotFound for every key under a `[section]`).
        let mut d = doc("[server]\nhost = \"x\"\nport = 1\n");
        d.apply(Mutation::Rename {
            path: vec![Seg::Key("server".into()), Seg::Key("host".into())],
            new_key: "hostname".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "[server]\nhostname = \"x\"\nport = 1\n");
    }

    #[test]
    fn rename_dotted_leaf_inside_scope_table() {
        let mut d = doc("[scope]\na.b = 1\n");
        d.apply(Mutation::Rename {
            path: vec![
                Seg::Key("scope".into()),
                Seg::Key("a".into()),
                Seg::Key("b".into()),
            ],
            new_key: "z".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "[scope]\na.z = 1\n");
    }

    #[test]
    fn rename_synthetic_dotted_table_inside_scope_table() {
        // The synthetic `[T/D]` intermediate under a `[section]`: member keys
        // start at the scope, so their rename index is offset by the scope depth.
        let mut d = doc("[scope]\na.b = 1\na.c = 2\n");
        d.apply(Mutation::Rename {
            path: vec![Seg::Key("scope".into()), Seg::Key("a".into())],
            new_key: "z".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "[scope]\nz.b = 1\nz.c = 2\n");
    }

    #[test]
    fn rename_sub_table_next_to_aot_group() {
        // `[grp.sub]` alongside `[[grp]]` projects at `grp.sub` (no Index seg).
        let mut d = doc("[[grp]]\nn = 1\n[grp.sub]\nx = 1\n");
        d.apply(Mutation::Rename {
            path: vec![Seg::Key("grp".into()), Seg::Key("sub".into())],
            new_key: "zzz".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "[[grp]]\nn = 1\n[grp.zzz]\nx = 1\n");
    }

    #[test]
    fn rename_aot_group_renames_nested_sub_headers_too() {
        let mut d = doc("[[grp]]\nn = 1\n[grp.sub]\nx = 1\n[[grp]]\nn = 2\n");
        d.apply(Mutation::Rename {
            path: vec![Seg::Key("grp".into())],
            new_key: "g2".into(),
        })
        .unwrap();
        assert_eq!(
            d.serialize(),
            "[[g2]]\nn = 1\n[g2.sub]\nx = 1\n[[g2]]\nn = 2\n"
        );
    }

    #[test]
    fn insert_into_dotted_table_writes_dotted_entry() {
        // Inserting a child into a synthetic `[T/D]` table writes a dotted entry
        // next to its siblings — no header.
        let mut d = doc("a.b = 1\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("a".into())],
                index: 1,
            },
            fragment: "x = 2\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "a.b = 1\na.x = 2\n");
    }

    #[test]
    fn insert_into_nested_dotted_table() {
        let mut d = doc("a.b.c = 1\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("a".into()), Seg::Key("b".into())],
                index: 1,
            },
            fragment: "d = 2\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "a.b.c = 1\na.b.d = 2\n");
    }

    #[test]
    fn insert_into_dotted_table_under_scope_is_scope_relative() {
        // A dotted table nested in a real `[scope]` prefixes only the dotted run.
        let mut d = doc("[server]\nhost.name = \"h\"\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("server".into()), Seg::Key("host".into())],
                index: 1,
            },
            fragment: "port = 80\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(
            d.serialize(),
            "[server]\nhost.name = \"h\"\nhost.port = 80\n"
        );
    }

    #[test]
    fn delete_inline_table_member() {
        let mut d = doc("pt = { x = 1, y = 2, z = 3 }\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("pt".into()), Seg::Key("y".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "pt = { x = 1, z = 3 }\n");
    }

    #[test]
    fn delete_whole_aot_group() {
        let mut d = doc("[[p]]\nn = 1\n[[p]]\nn = 2\n[q]\nz = 9\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("p".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "[q]\nz = 9\n");
    }

    #[test]
    fn remark_comments_out_and_back_a_table() {
        let mut d = doc("[s]\nport = 1\nhost = \"x\"\n");
        d.apply(Mutation::Remark {
            path: vec![Seg::Key("s".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "# [s]\n# port = 1\n# host = \"x\"\n");
        // Uncomment the block back to a live table.
        d.apply(Mutation::Remark {
            path: vec![Seg::Index(0)],
        })
        .unwrap();
        assert_eq!(d.serialize(), "[s]\nport = 1\nhost = \"x\"\n");
    }

    #[test]
    fn remark_implicit_table_with_no_own_header() {
        // `profile` has no `[profile]` header of its own — it exists only via
        // the child section `[profile.release]` (an implicit table).
        let mut d = doc("[profile.release]\nopt-level = 'z'\nlto = true\n");
        d.apply(Mutation::Remark {
            path: vec![Seg::Key("profile".into())],
        })
        .unwrap();
        assert_eq!(
            d.serialize(),
            "# [profile.release]\n# opt-level = 'z'\n# lto = true\n"
        );
        // Uncomment the block back to a live (still-implicit) table.
        d.apply(Mutation::Remark {
            path: vec![Seg::Index(0)],
        })
        .unwrap();
        assert_eq!(
            d.serialize(),
            "[profile.release]\nopt-level = 'z'\nlto = true\n"
        );
    }

    #[test]
    fn remark_comments_out_an_aot_entry() {
        let mut d = doc("[[p]]\nn = 1\n[[p]]\nn = 2\n");
        d.apply(Mutation::Remark {
            path: vec![Seg::Key("p".into()), Seg::Index(0)],
        })
        .unwrap();
        assert_eq!(d.serialize(), "# [[p]]\n# n = 1\n[[p]]\nn = 2\n");
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
    fn insert_node_below_a_comment() {
        // Phase 4: with comments as real ordered nodes, inserting a node right after
        // a comment row (cursor on the comment at index 0 → target index 1) places it
        // directly below the comment — the originally-requested capability.
        let mut d = doc("# section\na = 1\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![],
                index: 1,
            },
            fragment: "b = 2\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "# section\nb = 2\na = 1\n");
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
    fn insert_leaf_before_anchor() {
        let mut d = doc("a = 1\nc = 3\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![],
                index: 1,
            },
            fragment: "b = 2\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\nb = 2\nc = 3\n");
    }

    #[test]
    fn insert_leaf_at_end() {
        let mut d = doc("a = 1\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![],
                index: 9,
            },
            fragment: "z = 9\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\nz = 9\n");
    }

    #[test]
    fn insert_collision_cancel_errors() {
        let mut d = doc("a = 1\nb = 2\n");
        let err = d
            .apply(Mutation::Insert {
                target: InsTarget {
                    parent: vec![],
                    index: 0,
                },
                fragment: "b = 9\n".into(),
                on_collision: OnCollision::Cancel,
                suggested_key: None,
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Collision(k) if k == "b"));
        assert_eq!(d.serialize(), "a = 1\nb = 2\n");
    }

    #[test]
    fn insert_collision_overwrite_replaces_in_place() {
        let mut d = doc("a = 1\nb = 2\nc = 3\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![],
                index: 9,
            },
            fragment: "b = 99\n".into(),
            on_collision: OnCollision::Overwrite,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\nb = 99\nc = 3\n");
    }

    #[test]
    fn insert_collision_rename_suffixes_key() {
        let mut d = doc("b = 2\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![],
                index: 9,
            },
            fragment: "b = 9\n".into(),
            on_collision: OnCollision::Rename,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "b = 2\nb_2 = 9\n");
    }

    #[test]
    fn insert_section_into_scope_collides_with_existing_subtable() {
        // The header was re-prefixed to `[b.a]` before the collision check; the
        // check must use the absolute path (a phantom `b.b.a` lookup used to let
        // the duplicate through).
        let mut d = doc("[b]\nx = 1\n\n[b.a]\ny = 2\n");
        let err = d
            .apply(Mutation::Insert {
                target: InsTarget {
                    parent: vec![Seg::Key("b".into())],
                    index: 9,
                },
                fragment: "[a]\nz = 3\n".into(),
                on_collision: OnCollision::Cancel,
                suggested_key: None,
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Collision(_)), "got {err:?}");
        assert_eq!(d.serialize(), "[b]\nx = 1\n\n[b.a]\ny = 2\n");
    }

    #[test]
    fn replace_document_rejects_duplicate_sections() {
        // taplo's parser is syntax-only; the semantic backstop must reject a
        // whole-document rewrite that introduces a duplicate `[a]`.
        let mut d = doc("a = 1\n");
        let err = d
            .apply(Mutation::Replace {
                path: vec![],
                fragment: "[a]\nx = 1\n[c]\ny = 2\n[a]\nz = 3\n".into(),
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Collision(_)), "got {err:?}");
        assert_eq!(d.serialize(), "a = 1\n");
    }

    #[test]
    fn replace_section_rejects_resulting_duplicate() {
        // A block edit that renames `[a]` to an already-existing `[b]` would leave
        // two `[b]` sections — the backstop rejects it, doc untouched.
        let src = "[a]\nx = 1\n\n[b]\ny = 2\n";
        let mut d = doc(src);
        let err = d
            .apply(Mutation::Replace {
                path: vec![Seg::Key("a".into())],
                fragment: "[b]\nz = 3\n".into(),
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Collision(_)), "got {err:?}");
        assert_eq!(d.serialize(), src);
    }

    #[test]
    fn insert_keyed_into_aot_group_appends_new_entry() {
        let mut d = doc("[[p]]\na = 1\n\n[[p]]\na = 2\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("p".into())],
                index: 9,
            },
            fragment: "b = 3\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(
            d.serialize(),
            "[[p]]\na = 1\n\n[[p]]\na = 2\n[[p]]\nb = 3\n"
        );
    }

    #[test]
    fn insert_keyed_into_aot_group_at_front() {
        let mut d = doc("[[p]]\na = 1\n\n[[p]]\na = 2\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("p".into())],
                index: 0,
            },
            fragment: "b = 3\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(
            d.serialize(),
            "[[p]]\nb = 3\n[[p]]\na = 1\n\n[[p]]\na = 2\n"
        );
    }

    #[test]
    fn insert_multi_entry_fragment_packs_one_aot_entry() {
        // A joined multi-node fragment lands in ONE new [[…]] entry, not several.
        let mut d = doc("[[p]]\na = 1\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("p".into())],
                index: 9,
            },
            fragment: "x = 1\ny = 2\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "[[p]]\na = 1\n[[p]]\nx = 1\ny = 2\n");
    }

    #[test]
    fn insert_section_into_aot_group_is_illegal() {
        let src = "[[p]]\na = 1\n";
        let mut d = doc(src);
        let err = d
            .apply(Mutation::Insert {
                target: InsTarget {
                    parent: vec![Seg::Key("p".into())],
                    index: 9,
                },
                fragment: "[t]\nz = 1\n".into(),
                on_collision: OnCollision::Cancel,
                suggested_key: None,
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Illegal(_)), "got {err:?}");
        assert_eq!(d.serialize(), src);
    }

    #[test]
    fn move_two_scalars_into_aot_group_packs_one_entry() {
        let mut d = doc("k1 = 1\nk2 = 2\n\n[[p]]\na = 1\n");
        d.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("k1".into())], vec![Seg::Key("k2".into())]],
            target: InsTarget {
                parent: vec![Seg::Key("p".into())],
                index: 9,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        let out = d.serialize();
        assert_eq!(
            out.matches("[[p]]").count(),
            2,
            "exactly one new entry: {out}"
        );
        assert!(
            out.contains("[[p]]\nk1 = 1\nk2 = 2\n"),
            "packed into one entry: {out}"
        );
        assert!(
            !out.contains("k1 = 1\nk2 = 2\n\n[[p]]"),
            "sources moved: {out}"
        );
    }

    #[test]
    fn move_two_array_elements_including_last_index_succeeds() {
        // Regression: deleting same-array sources in ascending-index order
        // (as multi-select can hand them in) shifts the not-yet-deleted
        // higher index out from under its still-stale path once the
        // lower index is removed first — `sort_by_key(Reverse(len))` alone
        // doesn't break the tie, since both source paths are the same
        // length. Sources deliberately given ascending (idx 1 then idx 2)
        // to reproduce the bug regardless of caller ordering.
        let mut d = doc("arr = [\"{ }\", \"[ ]\", \"< >\"]\n\n[s]\nx = 1\n");
        d.apply(Mutation::Move {
            sources: vec![
                vec![Seg::Key("arr".into()), Seg::Index(1)],
                vec![Seg::Key("arr".into()), Seg::Index(2)],
            ],
            target: InsTarget {
                parent: vec![Seg::Key("s".into())],
                index: 9,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        let out = d.serialize();
        assert!(out.contains("arr = [\"{ }\"]"), "one element left: {out}");
        assert!(out.contains("arr_1 = \"[ ]\""), "idx1 keyed: {out}");
        assert!(out.contains("arr_2 = \"< >\""), "idx2 keyed: {out}");
    }

    #[test]
    fn move_all_array_elements_into_table_succeeds() {
        let mut d = doc("arr = [\"{ }\", \"[ ]\", \"< >\"]\n\n[s]\nx = 1\n");
        d.apply(Mutation::Move {
            sources: vec![
                vec![Seg::Key("arr".into()), Seg::Index(0)],
                vec![Seg::Key("arr".into()), Seg::Index(1)],
                vec![Seg::Key("arr".into()), Seg::Index(2)],
            ],
            target: InsTarget {
                parent: vec![Seg::Key("s".into())],
                index: 9,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        let out = d.serialize();
        assert!(out.contains("arr = []"), "array emptied: {out}");
        assert!(out.contains("arr_0 = \"{ }\""), "idx0 keyed: {out}");
        assert!(out.contains("arr_1 = \"[ ]\""), "idx1 keyed: {out}");
        assert!(out.contains("arr_2 = \"< >\""), "idx2 keyed: {out}");
    }

    #[test]
    fn move_aot_entry_out_into_scope_splits_into_members() {
        // An [A/T] entry ≡ an inline-table array element: moving it out splits
        // it into its member nodes inside the destination scope.
        let mut d = doc("[[p]]\na = 1\n\n[[p]]\nb = 2\nc = 3\n\n[s]\nx = 1\n");
        d.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("p".into()), Seg::Index(1)]],
            target: InsTarget {
                parent: vec![Seg::Key("s".into())],
                index: 9,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        let out = d.serialize();
        assert!(out.contains("[s]\nx = 1\nb = 2\nc = 3\n"), "members: {out}");
        assert_eq!(out.matches("[[p]]").count(), 1, "one entry left: {out}");
    }

    #[test]
    fn move_aot_entry_to_root_lands_members() {
        // Members land as plain root entries (index 0: the leaf partition).
        let mut d = doc("x = 0\n\n[[p]]\na = 1\n\n[[p]]\nb = 2\n");
        d.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("p".into()), Seg::Index(1)]],
            target: InsTarget {
                parent: vec![],
                index: 0,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        let out = d.serialize();
        assert!(out.starts_with("b = 2\nx = 0\n"), "member at root: {out}");
        assert_eq!(out.matches("[[p]]").count(), 1, "one entry left: {out}");
    }

    #[test]
    fn move_aot_entry_into_other_group_packs_one_entry() {
        let mut d = doc("[[p]]\na = 1\nb = 2\n\n[[q]]\nx = 1\n");
        d.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("p".into()), Seg::Index(0)]],
            target: InsTarget {
                parent: vec![Seg::Key("q".into())],
                index: 9,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        let out = d.serialize();
        assert!(out.contains("[[q]]\nx = 1\n"), "existing entry kept: {out}");
        assert!(
            out.contains("[[q]]\na = 1\nb = 2\n"),
            "ONE new entry: {out}"
        );
        assert!(!out.contains("[[p]]"), "source group emptied: {out}");
    }

    #[test]
    fn move_aot_entry_into_array_packs_one_element() {
        let mut d = doc("arr = [1]\n\n[[p]]\na = 1\nb = 2\n");
        d.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("p".into()), Seg::Index(0)]],
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 9,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        // The blank separator line that preceded the deleted `[[p]]` remains.
        assert_eq!(d.serialize(), "arr = [1, { a = 1, b = 2 }]\n\n");
    }

    #[test]
    fn move_aot_entry_flattens_subsections_to_dotted() {
        // A sub-section of the entry flattens to dotted entries; the source side
        // removes the sub-section with the entry.
        let mut d = doc(
            "[[fruit]]\nname = \"apple\"\n\n[fruit.physical]\ncolor = \"red\"\n\n[[fruit]]\nname = \"pear\"\n\n[s]\nx = 1\n",
        );
        d.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("fruit".into()), Seg::Index(0)]],
            target: InsTarget {
                parent: vec![Seg::Key("s".into())],
                index: 9,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        let out = d.serialize();
        assert!(
            out.contains("name = \"apple\"\nphysical.color = \"red\"\n"),
            "flattened: {out}"
        );
        assert!(!out.contains("[fruit.physical]"), "sub-section gone: {out}");
        assert_eq!(out.matches("[[fruit]]").count(), 1, "one entry left: {out}");
    }

    #[test]
    fn copy_inline_table_element_into_table_unpacks() {
        // The copy path matches the cut path: a keyless `{ … }` element pasted
        // into a table unpacks into its member entries (no placeholder key).
        let mut d = doc("arr = [{ a = 1, b = 2 }]\n\n[s]\nx = 1\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("s".into())],
                index: 9,
            },
            fragment: "{ a = 1, b = 2 }".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(
            d.serialize(),
            "arr = [{ a = 1, b = 2 }]\n\n[s]\nx = 1\na = 1\nb = 2\n"
        );
    }

    #[test]
    fn copy_inline_table_element_into_aot_group_packs_one_entry() {
        let mut d = doc("[[p]]\nx = 1\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("p".into())],
                index: 9,
            },
            fragment: "{ a = 1, b = 2 }".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "[[p]]\nx = 1\n[[p]]\na = 1\nb = 2\n");
    }

    #[test]
    fn copy_bare_scalar_into_table_keeps_placeholder() {
        let mut d = doc("[s]\nx = 1\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("s".into())],
                index: 9,
            },
            fragment: "42".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "[s]\nx = 1\nplaceholder = 42\n");
    }

    #[test]
    fn copy_bare_scalar_into_table_uses_suggested_key() {
        // The copy-paste path threads the source `<arrayKey>_<index>` through
        // `Mutation::Insert::suggested_key` — the bare value needs no placeholder.
        let mut d = doc("[dest]\nz = 0\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("dest".into())],
                index: 9,
            },
            fragment: "20".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: Some("arr_1".into()),
        })
        .unwrap();
        assert_eq!(d.serialize(), "[dest]\nz = 0\narr_1 = 20\n");
    }

    #[test]
    fn copy_aot_entry_relative_is_member_fragment() {
        let d = doc("[[p]]\na = 1\n\n[[p]]\nb = 2\nc = 3\n");
        let frag = d.serialize_fragment_relative(&[Seg::Key("p".into()), Seg::Index(1)]);
        assert_eq!(frag, "b = 2\nc = 3\n");
    }

    #[test]
    fn convert_aot_group_to_arrays() {
        use crate::model::document::KindTarget as KT;
        let k = |s: &str| vec![Seg::Key(s.into())];
        assert_eq!(
            convert(
                "[[p]]\na = 1\nb = 2\n[[p]]\nc = 3\n",
                k("p"),
                KT::ArrayInline
            ),
            "p = [{ a = 1, b = 2 }, { c = 3 }]\n"
        );
        assert_eq!(
            convert("[[p]]\na = 1\n[[p]]\nc = 3\n", k("p"), KT::ArrayMultiline),
            "p = [\n  { a = 1 },\n  { c = 3 },\n]\n"
        );
        // A nested group converts relative to its parent scope.
        assert_eq!(
            convert(
                "[s]\nx = 1\n[[s.p]]\na = 1\n",
                vec![Seg::Key("s".into()), Seg::Key("p".into())],
                KT::ArrayInline
            ),
            "[s]\nx = 1\np = [{ a = 1 }]\n"
        );
        // Position: the replacement entry would be captured by a foreign table.
        assert!(matches!(
            convert_err("[t]\nx = 1\n\n[[p]]\na = 1\n", k("p"), KT::ArrayInline),
            MutateError::Illegal(_)
        ));
        // A sub-section / a comment can't live in an inline-table element.
        assert!(matches!(
            convert_err("[[p]]\na = 1\n[p.sub]\nx = 1\n", k("p"), KT::ArrayInline),
            MutateError::Illegal(_)
        ));
        assert!(matches!(
            convert_err("[[p]]\n# c\na = 1\n", k("p"), KT::ArrayInline),
            MutateError::Illegal(_)
        ));
    }

    #[test]
    fn convert_array_of_inline_tables_to_aot() {
        use crate::model::document::KindTarget as KT;
        let k = |s: &str| vec![Seg::Key(s.into())];
        assert_eq!(
            convert(
                "p = [{ a = 1, b = 2 }, { c = 3 }]\n",
                k("p"),
                KT::ArrayOfTables
            ),
            "[[p]]\na = 1\nb = 2\n[[p]]\nc = 3\n"
        );
        // Inside a scope: full-path headers.
        assert_eq!(
            convert(
                "[s]\np = [{ a = 1 }]\n",
                vec![Seg::Key("s".into()), Seg::Key("p".into())],
                KT::ArrayOfTables
            ),
            "[s]\n[[s.p]]\na = 1\n"
        );
        // The `[[…]]` sections would capture the entry below.
        assert!(matches!(
            convert_err("p = [{ a = 1 }]\nx = 1\n", k("p"), KT::ArrayOfTables),
            MutateError::Illegal(_)
        ));
        // Mixed / non-inline-table elements can't become entries.
        assert!(matches!(
            convert_err("p = [{ a = 1 }, 2]\n", k("p"), KT::ArrayOfTables),
            MutateError::Illegal(_)
        ));
    }

    #[test]
    fn delete_aot_entry_removes_its_subsections() {
        let mut d = doc(
            "[[fruit]]\nname = \"apple\"\n\n[fruit.physical]\ncolor = \"red\"\n\n[[fruit]]\nname = \"pear\"\n",
        );
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("fruit".into()), Seg::Index(0)],
        })
        .unwrap();
        assert_eq!(d.serialize(), "[[fruit]]\nname = \"pear\"\n");
    }

    #[test]
    fn delete_dotted_table_keeps_adjacent_comment() {
        // A comment directly above a member is an independent node — it survives
        // the table's deletion.
        let mut d = doc("# note\na.b = 1\nx = 0\na.c = 2\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("a".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "# note\nx = 0\n");
    }

    #[test]
    fn move_dotted_table_leaves_adjacent_comment() {
        let mut d = doc("# note\na.b = 1\n\n[s]\nx = 1\n");
        d.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("a".into())]],
            target: InsTarget {
                parent: vec![Seg::Key("s".into())],
                index: 9,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        let out = d.serialize();
        assert!(out.starts_with("# note"), "comment stays put: {out}");
        assert!(out.contains("[s]\nx = 1\na.b = 1\n"), "member moved: {out}");
    }

    #[test]
    fn insert_comment_into_dotted_table_lands_above() {
        // A [T/D] table holds no comments: the paste lands directly above the
        // table's first member as an independent scope-level node.
        let mut d = doc("x = 0\na.b = 1\na.c = 2\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![Seg::Key("a".into())],
                index: 1,
            },
            text: "# new".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "x = 0\n# new\na.b = 1\na.c = 2\n");
    }

    fn convert(src: &str, path: Vec<Seg>, target: crate::model::document::KindTarget) -> String {
        let mut d = doc(src);
        d.apply(Mutation::ConvertKind { path, target }).unwrap();
        d.serialize()
    }

    fn convert_err(
        src: &str,
        path: Vec<Seg>,
        target: crate::model::document::KindTarget,
    ) -> MutateError {
        let mut d = doc(src);
        let err = d.apply(Mutation::ConvertKind { path, target }).unwrap_err();
        assert_eq!(d.serialize(), src, "doc must stay untouched on error");
        err
    }

    #[test]
    fn convert_string_notations() {
        use crate::model::document::KindTarget as KT;
        let k = |s: &str| vec![Seg::Key(s.into())];
        // basic ↔ literal (escapes resolved / re-applied), trailing comment kept.
        assert_eq!(
            convert("a = \"x\\\"y\" # c\n", k("a"), KT::StringLiteral),
            "a = 'x\"y' # c\n"
        );
        assert_eq!(
            convert("a = 'C:\\dir'\n", k("a"), KT::StringBasic),
            "a = \"C:\\\\dir\"\n"
        );
        // single-line → multiline forms.
        assert_eq!(
            convert("a = \"hi\"\n", k("a"), KT::StringMultiline),
            "a = \"\"\"hi\"\"\"\n"
        );
        assert_eq!(
            convert("a = \"hi\"\n", k("a"), KT::StringMultilineLiteral),
            "a = '''hi'''\n"
        );
        // multiline basic → single-line basic escapes the newline (lossless).
        assert_eq!(
            convert("a = \"\"\"l1\nl2\"\"\"\n", k("a"), KT::StringBasic),
            "a = \"l1\\nl2\"\n"
        );
        // … but a real newline can't live in a single-line literal,
        assert!(matches!(
            convert_err("a = \"\"\"l1\nl2\"\"\"\n", k("a"), KT::StringLiteral),
            MutateError::Illegal(_)
        ));
        // a `'` can't live in a literal, and `'''` not in a multiline literal.
        assert!(matches!(
            convert_err("a = \"it's\"\n", k("a"), KT::StringLiteral),
            MutateError::Illegal(_)
        ));
        assert!(matches!(
            convert_err("a = \"q'''q\"\n", k("a"), KT::StringMultilineLiteral),
            MutateError::Illegal(_)
        ));
        // a non-string doesn't convert to a string notation.
        assert!(matches!(
            convert_err("a = 42\n", k("a"), KT::StringBasic),
            MutateError::Illegal(_)
        ));
    }

    #[test]
    fn convert_integer_radices() {
        use crate::model::document::KindTarget as KT;
        let k = |s: &str| vec![Seg::Key(s.into())];
        assert_eq!(convert("a = 255\n", k("a"), KT::IntHex), "a = 0xff\n");
        assert_eq!(convert("a = 0xff\n", k("a"), KT::IntDecimal), "a = 255\n");
        assert_eq!(convert("a = 8\n", k("a"), KT::IntOctal), "a = 0o10\n");
        assert_eq!(convert("a = 5\n", k("a"), KT::IntBinary), "a = 0b101\n");
        // `_` separators parse; negatives have no prefixed form.
        assert_eq!(convert("a = 1_000\n", k("a"), KT::IntHex), "a = 0x3e8\n");
        assert!(matches!(
            convert_err("a = -1\n", k("a"), KT::IntHex),
            MutateError::Illegal(_)
        ));
        assert!(matches!(
            convert_err("a = 1.5\n", k("a"), KT::IntHex),
            MutateError::Illegal(_)
        ));
    }

    #[test]
    fn convert_float_notations() {
        use crate::model::document::KindTarget as KT;
        let k = |s: &str| vec![Seg::Key(s.into())];
        assert_eq!(
            convert("a = 150.0\n", k("a"), KT::FloatExponent),
            "a = 1.5e2\n"
        );
        assert_eq!(
            convert("a = 1.5e2\n", k("a"), KT::FloatPlain),
            "a = 150.0\n"
        );
        assert_eq!(convert("a = 1e0\n", k("a"), KT::FloatPlain), "a = 1.0\n");
        // inf/nan and non-floats don't convert.
        assert!(matches!(
            convert_err("a = inf\n", k("a"), KT::FloatExponent),
            MutateError::Illegal(_)
        ));
        assert!(matches!(
            convert_err("a = 1\n", k("a"), KT::FloatPlain),
            MutateError::Illegal(_)
        ));
    }

    #[test]
    fn convert_array_inline_multiline_roundtrip() {
        use crate::model::document::KindTarget as KT;
        let k = vec![Seg::Key("arr".into())];
        let multi = convert("arr = [1, 2]\n", k.clone(), KT::ArrayMultiline);
        assert!(
            multi.contains('\n') && multi.matches('\n').count() > 1,
            "{multi}"
        );
        let mut d = doc(&multi);
        d.apply(Mutation::ConvertKind {
            path: k.clone(),
            target: KT::ArrayInline,
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [1, 2]\n");
        // Comments block the collapse.
        assert!(matches!(
            convert_err("arr = [\n  1,\n  # c\n  2,\n]\n", k, KT::ArrayInline),
            MutateError::Illegal(_)
        ));
    }

    #[test]
    fn convert_inline_table_to_dotted_and_scope() {
        use crate::model::document::KindTarget as KT;
        let k = |s: &str| vec![Seg::Key(s.into())];
        assert_eq!(
            convert("t = { a = 1, b = 2 }\n", k("t"), KT::TableDotted),
            "t.a = 1\nt.b = 2\n"
        );
        assert_eq!(
            convert("x = 0\nt = { a = 1 }\n", k("t"), KT::TableScope),
            "x = 0\n[t]\na = 1\n"
        );
        // A [table] mid-entries would capture the keys below it.
        assert!(matches!(
            convert_err("t = { a = 1 }\nx = 0\n", k("t"), KT::TableScope),
            MutateError::Illegal(_)
        ));
    }

    #[test]
    fn convert_dotted_table_to_inline_and_scope() {
        use crate::model::document::KindTarget as KT;
        let k = |s: &str| vec![Seg::Key(s.into())];
        assert_eq!(
            convert("t.a = 1\nt.b = 2\n", k("t"), KT::TableInline),
            "t = { a = 1, b = 2 }\n"
        );
        assert_eq!(
            convert("x = 0\nt.a = 1\nt.b = 2\n", k("t"), KT::TableScope),
            "x = 0\n[t]\na = 1\nb = 2\n"
        );
        assert!(matches!(
            convert_err("t.a = 1\nx = 0\n", k("t"), KT::TableScope),
            MutateError::Illegal(_)
        ));
        // A comment above a member is an independent node — it stays put on
        // either conversion.
        assert_eq!(
            convert("# c\nt.a = 1\n", k("t"), KT::TableInline),
            "# c\nt = { a = 1 }\n"
        );
        assert_eq!(
            convert("# c\nt.a = 1\n", k("t"), KT::TableScope),
            "# c\n[t]\na = 1\n"
        );
    }

    #[test]
    fn convert_scope_table_to_dotted_and_inline() {
        use crate::model::document::KindTarget as KT;
        let k = |s: &str| vec![Seg::Key(s.into())];
        assert_eq!(
            convert("[t]\na = 1\nb = 2\n", k("t"), KT::TableDotted),
            "t.a = 1\nt.b = 2\n"
        );
        assert_eq!(
            convert("[t]\na = 1\n", k("t"), KT::TableInline),
            "t = { a = 1 }\n"
        );
        // Preceded by a foreign section: its lines would be captured.
        assert!(matches!(
            convert_err("[s]\nx = 1\n\n[t]\na = 1\n", k("t"), KT::TableDotted),
            MutateError::Illegal(_)
        ));
        // A nested sub-scope converts relative to its parent's capture.
        let mut d = doc("[s]\nx = 1\n\n[s.t]\na = 1\n");
        d.apply(Mutation::ConvertKind {
            path: vec![Seg::Key("s".into()), Seg::Key("t".into())],
            target: KT::TableDotted,
        })
        .unwrap();
        assert_eq!(d.serialize(), "[s]\nx = 1\n\nt.a = 1\n");
    }

    #[test]
    fn insert_into_table_scope() {
        let mut d = doc("[s]\nx = 1\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("s".into())],
                index: 9,
            },
            fragment: "y = 2\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "[s]\nx = 1\ny = 2\n");
    }

    #[test]
    fn insert_scalar_after_table_is_rejected() {
        // D5: a key appended after `[t]` would be re-keyed into `[t]` — reject,
        // leave the document untouched.
        let mut d = doc("a = 1\n[t]\nx = 1\n");
        let err = d
            .apply(Mutation::Insert {
                target: InsTarget {
                    parent: vec![],
                    index: 9, // append at root end (past [t])
                },
                fragment: "z = 9\n".into(),
                on_collision: OnCollision::Cancel,
                suggested_key: None,
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Illegal(_)), "got {err:?}");
        assert_eq!(d.serialize(), "a = 1\n[t]\nx = 1\n");
    }

    #[test]
    fn insert_scalar_before_table_is_ok() {
        // The split slot (index == first-header index) accepts a leaf: it lands in
        // the leading region, before the header.
        let mut d = doc("a = 1\n[t]\nx = 1\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![],
                index: 1, // between `a` and `[t]`
            },
            fragment: "b = 2\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\nb = 2\n[t]\nx = 1\n");
    }

    #[test]
    fn insert_table_before_scalar_is_rejected() {
        // D5 inverse: a `[t]` placed before `a` would capture `a` — reject.
        let mut d = doc("a = 1\n");
        let err = d
            .apply(Mutation::Insert {
                target: InsTarget {
                    parent: vec![],
                    index: 0,
                },
                fragment: "[t]\ny = 1\n".into(),
                on_collision: OnCollision::Cancel,
                suggested_key: None,
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Illegal(_)), "got {err:?}");
        assert_eq!(d.serialize(), "a = 1\n");
    }

    #[test]
    fn insert_keyed_into_array_wraps_as_inline_table() {
        // A keyed fragment pasted into an array is wrapped as `{ k = v }` so the key
        // is preserved (was: key dropped).
        let mut d = doc("arr = [1, 2]\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 9,
            },
            fragment: "x = 99\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [1, 2, { x = 99 }]\n");
    }

    #[test]
    fn insert_keyed_inline_table_into_array_nests() {
        // A keyed inline-table value becomes a nested inline table element.
        let mut d = doc("arr = [1]\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 9,
            },
            fragment: "foo = { a = 1 }\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [1, { foo = { a = 1 } }]\n");
    }

    #[test]
    fn insert_bare_inline_table_into_array_stays_bare() {
        // A keyless inline-table value keeps its element form (no wrapping).
        let mut d = doc("arr = [1]\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 9,
            },
            fragment: "{ a = 1 }\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [1, { a = 1 }]\n");
    }

    #[test]
    fn insert_bare_value_into_table_synthesizes_key() {
        // C2 / key+: a bare element value pasted into a table gets a `placeholder` key.
        let mut d = doc("a = 1\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![],
                index: 9,
            },
            fragment: "42\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\nplaceholder = 42\n");
    }

    #[test]
    fn insert_synthesized_key_auto_renames_on_collision() {
        // key+ never prompts: a `placeholder` clash auto-suffixes even under Cancel.
        let mut d = doc("placeholder = 1\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![],
                index: 9,
            },
            fragment: "42\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "placeholder = 1\nplaceholder_2 = 42\n");
    }

    #[test]
    fn suggested_key_auto_renames_on_collision() {
        // A suggested key is synthesized like `placeholder` — a clash
        // auto-suffixes even under Cancel, never prompts.
        let mut d = doc("[dest]\narr_1 = 0\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("dest".into())],
                index: 9,
            },
            fragment: "20".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: Some("arr_1".into()),
        })
        .unwrap();
        assert_eq!(d.serialize(), "[dest]\narr_1 = 0\narr_1_2 = 20\n");
    }

    #[test]
    fn edit_array_interior_comment() {
        // #6b: a standalone comment inside a multiline array edits in place.
        let mut d = doc("arr = [\n  1,\n  # c\n  2,\n]\n");
        d.apply(Mutation::EditComment {
            path: vec![Seg::Key("arr".into()), Seg::Index(1)],
            text: "# changed".into(),
        })
        .unwrap();
        let s = d.serialize();
        assert!(s.contains("# changed") && !s.contains("# c\n"), "s: {s:?}");
    }

    #[test]
    fn delete_array_interior_comment() {
        // #6c: deleting a standalone array comment removes it (and its line).
        let mut d = doc("arr = [\n  1,\n  # c\n  2,\n]\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("arr".into()), Seg::Index(1)],
        })
        .unwrap();
        let s = d.serialize();
        assert!(!s.contains("# c"), "comment removed: {s:?}");
        assert!(s.contains("1,") && s.contains("2,"), "elements kept: {s:?}");
    }

    #[test]
    fn delete_merged_array_comment_removes_whole_block() {
        // A merged multi-line comment node inside an array deletes as one block.
        let mut d = doc("arr = [\n  # a\n  # b\n  1,\n]\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("arr".into()), Seg::Index(0)],
        })
        .unwrap();
        let s = d.serialize();
        assert!(
            !s.contains("# a") && !s.contains("# b"),
            "both lines gone: {s:?}"
        );
        assert!(s.contains("1,"), "element kept: {s:?}");
    }

    #[test]
    fn edit_merged_array_comment_replaces_whole_block() {
        // Editing a merged array comment replaces every line of the block.
        let mut d = doc("arr = [\n  # a\n  # b\n  1,\n]\n");
        d.apply(Mutation::EditComment {
            path: vec![Seg::Key("arr".into()), Seg::Index(0)],
            text: "# x\n# y".into(),
        })
        .unwrap();
        let s = d.serialize();
        assert!(
            s.contains("# x") && s.contains("# y") && !s.contains("# a") && !s.contains("# b"),
            "block replaced: {s:?}"
        );
    }

    #[test]
    fn insert_comment_into_multiline_array() {
        // #6d: a comment lands on its own indented line before the slot element.
        let mut d = doc("arr = [\n  1,\n  2,\n]\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 1,
            },
            text: "# note".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [\n  1,\n  # note\n  2,\n]\n");
    }

    #[test]
    fn insert_comment_appends_at_array_end() {
        let mut d = doc("arr = [\n  1,\n  2,\n]\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 9,
            },
            text: "# tail".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [\n  1,\n  2,\n  # tail\n]\n");
    }

    #[test]
    fn insert_comment_into_single_line_array_upgrades_to_multiline() {
        // Reconstruct increment 3: instead of rejecting, the array is reformatted
        // to one element per line and the comment lands at the requested slot.
        let mut d = doc("arr = [1, 2]\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 0,
            },
            text: "# x".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [\n  # x\n  1,\n  2,\n]\n");
    }

    #[test]
    fn comment_upgrade_inserts_mid_and_tail() {
        let mut d = doc("arr = [1, 2]\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 1,
            },
            text: "# mid".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [\n  1,\n  # mid\n  2,\n]\n");

        let mut d = doc("arr = [1, 2]\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 9,
            },
            text: "# tail".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [\n  1,\n  2,\n  # tail\n]\n");
    }

    #[test]
    fn comment_upgrade_empty_array_and_trailing_comment() {
        // An empty array upgrades to hold just the comment.
        let mut d = doc("arr = []\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 0,
            },
            text: "# todo".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [\n  # todo\n]\n");

        // A trailing comment on the entry line is outside the ARRAY and stays put.
        let mut d = doc("arr = [1] # eol\n");
        d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 0,
            },
            text: "# in".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [\n  # in\n  1,\n] # eol\n");
    }

    #[test]
    fn replace_scalar_with_array_and_back() {
        // #1: a scalar↔structured type change round-trips through Replace.
        let mut d = doc("x = 5\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("x".into())],
            fragment: "x = [1, 2]\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "x = [1, 2]\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("x".into())],
            fragment: "x = 9\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "x = 9\n");
    }

    #[test]
    fn replace_scalar_with_inline_table() {
        let mut d = doc("x = 5\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("x".into())],
            fragment: "x = { a = 1 }\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "x = { a = 1 }\n");
    }

    #[test]
    fn replace_structured_array_element() {
        // #2 write-back: a structured array element (array-of-arrays) swaps in place.
        let mut d = doc("arr = [[1, 2]]\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("arr".into()), Seg::Index(0)],
            fragment: "x = [9]\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [[9]]\n");
    }

    #[test]
    fn replace_single_line_array_value_swaps_it() {
        // #7 write-back: inline-editing a single-line array commits a structured
        // Replace that swaps the whole array.
        let mut d = doc("arr = [1, 2]\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("arr".into())],
            fragment: "arr = [9]\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [9]\n");
    }

    #[test]
    fn insert_table_into_array_is_rejected() {
        // D1 ✗ cell: a `[table]` cannot become an array element (hard coerce).
        let mut d = doc("arr = [1]\n");
        let err = d
            .apply(Mutation::Insert {
                target: InsTarget {
                    parent: vec![Seg::Key("arr".into())],
                    index: 9,
                },
                fragment: "[t]\nx = 1\n".into(),
                on_collision: OnCollision::Cancel,
                suggested_key: None,
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Illegal(_)), "got {err:?}");
        assert_eq!(d.serialize(), "arr = [1]\n");
    }

    #[test]
    fn serialize_whole_aot_group_returns_all_entries() {
        // Regression: editing an AoT *group* node showed blank ($EDITOR got "").
        let d = doc("[[p]]\nx = 1\n\n[[p]]\nx = 2\n");
        let frag = d.serialize_fragment(&[Seg::Key("p".into())]);
        assert!(
            frag.contains("[[p]]") && frag.contains("x = 1") && frag.contains("x = 2"),
            "frag: {frag:?}"
        );
    }

    #[test]
    fn replace_whole_aot_group_swaps_all_entries() {
        let mut d = doc("[[p]]\nx = 1\n\n[[p]]\nx = 2\n");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("p".into())],
            fragment: "[[p]]\nx = 9\n".into(),
        })
        .unwrap();
        let s = d.serialize();
        assert!(s.contains("x = 9"), "s: {s:?}");
        assert!(!s.contains("x = 1") && !s.contains("x = 2"), "s: {s:?}");
    }

    #[test]
    fn rename_leaf_key_preserves_value_and_position() {
        let mut d = doc("a = 1\nb = 2\nc = 3\n");
        d.apply(Mutation::Rename {
            path: vec![Seg::Key("b".into())],
            new_key: "bee".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\nbee = 2\nc = 3\n");
    }

    #[test]
    fn rename_table_header() {
        let mut d = doc("[server]\nport = 8080\n");
        d.apply(Mutation::Rename {
            path: vec![Seg::Key("server".into())],
            new_key: "srv".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "[srv]\nport = 8080\n");
    }

    #[test]
    fn rename_scope_table_propagates_to_sub_headers() {
        // Renaming [product_table] must also fix [product_table.a] and [product_table.b].
        let mut d = doc(
            "[product_table]\n[product_table.a]\nname = \"Hammer\"\n[product_table.b]\nname = \"Nail\"\n",
        );
        d.apply(Mutation::Rename {
            path: vec![Seg::Key("product_table".into())],
            new_key: "item".into(),
        })
        .unwrap();
        assert_eq!(
            d.serialize(),
            "[item]\n[item.a]\nname = \"Hammer\"\n[item.b]\nname = \"Nail\"\n"
        );
    }

    #[test]
    fn rename_implicit_scope_table_propagates_to_sub_headers() {
        // No top-level [product_table] header; only sub-tables exist.
        // Renaming the implicit root must still fix all sub-headers.
        let mut d =
            doc("[product_table.a]\nname = \"Hammer\"\n[product_table.b]\nname = \"Nail\"\n");
        d.apply(Mutation::Rename {
            path: vec![Seg::Key("product_table".into())],
            new_key: "item".into(),
        })
        .unwrap();
        assert_eq!(
            d.serialize(),
            "[item.a]\nname = \"Hammer\"\n[item.b]\nname = \"Nail\"\n"
        );
    }

    #[test]
    fn rename_preserves_trailing_comment() {
        let mut d = doc("a = 1  # keep\n");
        d.apply(Mutation::Rename {
            path: vec![Seg::Key("a".into())],
            new_key: "aa".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "aa = 1  # keep\n");
    }

    #[test]
    fn rename_collision_errors() {
        let mut d = doc("a = 1\nb = 2\n");
        let err = d
            .apply(Mutation::Rename {
                path: vec![Seg::Key("a".into())],
                new_key: "b".into(),
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Collision(k) if k == "b"));
        assert_eq!(d.serialize(), "a = 1\nb = 2\n");
    }

    #[test]
    fn remark_comments_out_a_leaf() {
        let mut d = doc("a = 1\nb = 2\n");
        d.apply(Mutation::Remark {
            path: vec![Seg::Key("b".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\n# b = 2\n");
    }

    #[test]
    fn remark_uncomments_back_to_live() {
        let mut d = doc("a = 1\n# b = 2\n");
        d.apply(Mutation::Remark {
            path: vec![Seg::Index(1)],
        })
        .unwrap();
        assert_eq!(d.serialize(), "a = 1\nb = 2\n");
    }

    #[test]
    fn remark_roundtrips() {
        let mut d = doc("port = 8080\n");
        d.apply(Mutation::Remark {
            path: vec![Seg::Key("port".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "# port = 8080\n");
        d.apply(Mutation::Remark {
            path: vec![Seg::Index(0)],
        })
        .unwrap();
        assert_eq!(d.serialize(), "port = 8080\n");
    }

    #[test]
    fn move_reorders_within_scope() {
        let mut d = doc("a = 1\nb = 2\nc = 3\n");
        // Move `a` to the end (after c).
        d.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("a".into())]],
            target: InsTarget {
                parent: vec![],
                index: 9,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "b = 2\nc = 3\na = 1\n");
    }

    #[test]
    fn move_leaves_comment_behind() {
        // The whole point of the migration: a move repositions only the node; the
        // comment above it is an independent node and stays put.
        let mut d = doc("# header\nx = 1\ny = 2\n");
        d.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("x".into())]],
            target: InsTarget {
                parent: vec![],
                index: 9,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "# header\ny = 2\nx = 1\n");
    }

    #[test]
    fn move_node_down_before_trailing_comment() {
        // Moving `a` to just after `b` (the comment occupies the slot at index 2)
        // must land it BEFORE the trailing comment, not after it.
        let mut d = doc("a = 1\nb = 2\n# c\n");
        d.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("a".into())]],
            target: InsTarget {
                parent: vec![],
                index: 2,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "b = 2\na = 1\n# c\n");
    }

    #[test]
    fn move_table_reorders_at_top_level() {
        let mut d = doc("[a]\nx = 1\n\n[b]\ny = 2\n\n[c]\nz = 3\n");
        // Move `[a]` to the end (after c).
        d.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("a".into())]],
            target: InsTarget {
                parent: vec![],
                index: 9,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        let s = d.serialize();
        // `[a]` and its body now come after `[c]`; one of each table remains.
        assert!(s.find("[a]").unwrap() > s.find("[c]").unwrap(), "got:\n{s}");
        assert_eq!(s.matches("[a]").count(), 1);
        assert!(s.contains("x = 1") && s.contains("z = 3"));
    }

    #[test]
    fn move_into_table_scope() {
        let mut d = doc("a = 1\n[dest]\nx = 1\n");
        d.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("a".into())]],
            target: InsTarget {
                parent: vec![Seg::Key("dest".into())],
                index: 9,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "[dest]\nx = 1\na = 1\n");
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

    // Issue 1: a `{ … }` value member of a `[T/D]` table must not have its interior
    // entries pulled out by the block edit — only the flat dotted entries are members.
    #[test]
    fn replace_dotted_table_keeps_inline_table_value_intact() {
        let mut d = doc("dotted.a = 1\ndotted.t = {x=1}\n");
        // Re-emit the same block: the inline table's inner `x=1` must not surface.
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("dotted".into())],
            fragment: "dotted.a = 1\ndotted.t = {x=1}\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "dotted.a = 1\ndotted.t = {x=1}\n");
    }

    #[test]
    fn fragment_of_inline_value_member_is_not_a_separate_line() {
        let d = doc("dotted.a = 1\ndotted.t = {x=1}\n");
        // The block fragment for the whole `[T/D]` table lists exactly two members.
        let frag = d.serialize_fragment(&[Seg::Key("dotted".into())]);
        assert_eq!(frag, "dotted.a = 1\ndotted.t = {x=1}\n");
    }

    // Issue 2: copy/cut out of a `[T/D]` table drops the dotted-ancestor prefix.
    #[test]
    fn relative_fragment_strips_dotted_prefix_of_leaf() {
        let d = doc("dotted.test.bool_true = true\n");
        let frag = d.serialize_fragment_relative(&[
            Seg::Key("dotted".into()),
            Seg::Key("test".into()),
            Seg::Key("bool_true".into()),
        ]);
        assert_eq!(frag, "bool_true = true\n");
    }

    #[test]
    fn relative_fragment_strips_one_level_for_subtable() {
        let d = doc("dotted.test.a = 1\ndotted.test.b = 2\n");
        // Copying the `test` subtable strips only the `dotted` ancestor.
        let frag =
            d.serialize_fragment_relative(&[Seg::Key("dotted".into()), Seg::Key("test".into())]);
        assert_eq!(frag, "test.a = 1\ntest.b = 2\n");
    }

    #[test]
    fn plain_fragment_keeps_full_dotted_key() {
        // The `$EDITOR` path (non-relative) must keep full keys for the block rewrite.
        let d = doc("dotted.test.bool_true = true\n");
        let frag = d.serialize_fragment(&[
            Seg::Key("dotted".into()),
            Seg::Key("test".into()),
            Seg::Key("bool_true".into()),
        ]);
        assert_eq!(frag, "dotted.test.bool_true = true\n");
    }

    #[test]
    fn cut_out_of_dotted_table_drops_prefix() {
        let mut d = doc("dotted.test.flag = true\n[dest]\nx = 1\n");
        d.apply(Mutation::Move {
            sources: vec![vec![
                Seg::Key("dotted".into()),
                Seg::Key("test".into()),
                Seg::Key("flag".into()),
            ]],
            target: InsTarget {
                parent: vec![Seg::Key("dest".into())],
                index: 99,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "[dest]\nx = 1\nflag = true\n");
    }

    // Regression: inserting into the slot *before* a `[T/D]` synthetic table (which
    // has no backing element) must anchor on the table's first member line, not fail
    // as `Unsupported`. Mirrors "cut a scalar, paste after a multiline array that is
    // immediately followed by a `[T/D]` table".
    #[test]
    fn insert_before_dotted_table_anchors_on_first_member() {
        let mut d = doc("arr = [\n  1,\n]\ndotted.x = 1\n");
        // Insert `gg = 5` at root index 1 — the slot occupied by the `dotted` table.
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![],
                index: 1,
            },
            fragment: "gg = 5\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [\n  1,\n]\ngg = 5\ndotted.x = 1\n");
    }

    #[test]
    fn move_before_dotted_table_succeeds() {
        let mut d = doc("gg = 5\narr = [\n  1,\n]\ndotted.x = 1\n");
        // Move `gg` into the slot before the `dotted` table (after the array).
        d.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("gg".into())]],
            target: InsTarget {
                parent: vec![],
                index: 2,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [\n  1,\n]\ngg = 5\ndotted.x = 1\n");
    }

    // Move an array element out: into a table a single-key inline table unwraps to a
    // keyed entry, a multi-key one / bare value gets a synthesized placeholder; into
    // another array it stays a bare element.
    fn move_elem(initial: &str, src: Vec<Seg>, dst: Vec<Seg>) -> String {
        let mut d = doc(initial);
        d.apply(Mutation::Move {
            sources: vec![src],
            target: InsTarget {
                parent: dst,
                index: 99,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        d.serialize()
    }

    #[test]
    fn move_single_key_element_into_table_unwraps() {
        let s = move_elem(
            "arr = [{ foo = 1 }]\n[dest]\nz = 0\n",
            vec![Seg::Key("arr".into()), Seg::Index(0)],
            vec![Seg::Key("dest".into())],
        );
        assert_eq!(s, "arr = []\n[dest]\nz = 0\nfoo = 1\n");
    }

    #[test]
    fn move_multikey_element_into_table_unpacks_entries() {
        let s = move_elem(
            "arr = [{ a = 1, b = 2 }]\n[dest]\nz = 0\n",
            vec![Seg::Key("arr".into()), Seg::Index(0)],
            vec![Seg::Key("dest".into())],
        );
        assert_eq!(s, "arr = []\n[dest]\nz = 0\na = 1\nb = 2\n");
    }

    #[test]
    fn move_inline_element_out_collides_per_member() {
        // Unpacked members run the per-leaf collision check; the move is atomic,
        // so the document stays untouched.
        let src = "arr = [{ a = 1, b = 2 }]\n[dest]\nb = 0\n";
        let mut d = doc(src);
        let err = d
            .apply(Mutation::Move {
                sources: vec![vec![Seg::Key("arr".into()), Seg::Index(0)]],
                target: InsTarget {
                    parent: vec![Seg::Key("dest".into())],
                    index: 9,
                },
                on_collision: OnCollision::Cancel,
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Collision(_)), "got {err:?}");
        assert_eq!(d.serialize(), src);
    }

    #[test]
    fn move_two_keyed_nodes_into_array_packs_one_inline_table() {
        let mut d = doc("a = 1\nb = 2\narr = [0]\n");
        d.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("a".into())], vec![Seg::Key("b".into())]],
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 9,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [0, { a = 1, b = 2 }]\n");
    }

    #[test]
    fn insert_multi_entry_fragment_into_array_packs_one_inline_table() {
        // A copied [T/D] table's members arrive as one multi-entry fragment —
        // they pack into ONE `{ … }` element.
        let mut d = doc("arr = [0]\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("arr".into())],
                index: 9,
            },
            fragment: "t.x = 1\nt.y = 2\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "arr = [0, { t.x = 1, t.y = 2 }]\n");
    }

    #[test]
    fn move_bare_element_into_table_gets_suggested_key() {
        // The synthesized key names the source: `arr[0]` → `arr_0` (was the
        // generic `placeholder` before suggested keys existed).
        let s = move_elem(
            "arr = [42]\n[dest]\nz = 0\n",
            vec![Seg::Key("arr".into()), Seg::Index(0)],
            vec![Seg::Key("dest".into())],
        );
        assert_eq!(s, "arr = []\n[dest]\nz = 0\narr_0 = 42\n");
    }

    #[test]
    fn move_keyed_array_scalar_into_table_gets_array_index_key() {
        // A bare scalar pulled out of a *keyed* array suggests `<arrayKey>_<index>`
        // instead of the generic `placeholder` (here: `arr[1]` → `arr_1`).
        let s = move_elem(
            "arr = [10, 20, 30]\n[dest]\nz = 0\n",
            vec![Seg::Key("arr".into()), Seg::Index(1)],
            vec![Seg::Key("dest".into())],
        );
        assert_eq!(s, "arr = [10, 30]\n[dest]\nz = 0\narr_1 = 20\n");
    }

    #[test]
    fn move_nested_array_scalar_into_table_keeps_placeholder() {
        // A nested (unkeyed) inner array has no key to suggest — the generic
        // `placeholder` fallback still fires.
        let s = move_elem(
            "m = [[1, 2], [3, 4]]\n[dest]\nz = 0\n",
            vec![Seg::Key("m".into()), Seg::Index(0), Seg::Index(0)],
            vec![Seg::Key("dest".into())],
        );
        assert_eq!(s, "m = [[2], [3, 4]]\n[dest]\nz = 0\nplaceholder = 1\n");
    }

    #[test]
    fn move_element_into_array_stays_bare() {
        let s = move_elem(
            "arr = [{ foo = 1 }]\nbrr = [9]\n",
            vec![Seg::Key("arr".into()), Seg::Index(0)],
            vec![Seg::Key("brr".into())],
        );
        assert_eq!(s, "arr = []\nbrr = [9, { foo = 1 }]\n");
    }

    #[test]
    fn move_single_key_element_into_dotted_table_prefixes() {
        let s = move_elem(
            "arr = [{ foo = 1 }]\n[d]\ndd.x = 0\n",
            vec![Seg::Key("arr".into()), Seg::Index(0)],
            vec![Seg::Key("d".into()), Seg::Key("dd".into())],
        );
        assert_eq!(s, "arr = []\n[d]\ndd.x = 0\ndd.foo = 1\n");
    }

    // Phase 2: a whole synthetic `[T/D]` table moves by fanning out its members,
    // each re-prefixed for the destination.
    #[test]
    fn move_whole_dotted_table_into_scope() {
        let s = move_elem(
            "a.x = 1\na.y = 2\n[dest]\nz = 0\n",
            vec![Seg::Key("a".into())],
            vec![Seg::Key("dest".into())],
        );
        assert_eq!(s, "[dest]\nz = 0\na.x = 1\na.y = 2\n");
    }

    #[test]
    fn move_whole_dotted_table_into_dotted_adds_prefix() {
        let s = move_elem(
            "a.x = 1\nb.y = 2\n",
            vec![Seg::Key("a".into())],
            vec![Seg::Key("b".into())],
        );
        assert_eq!(s, "b.y = 2\nb.a.x = 1\n");
    }

    #[test]
    fn move_dotted_subtable_out_to_root_drops_prefix() {
        let s = move_elem(
            "dotted.test.p = 1\ndotted.test.q = 2\ndotted.keep = 9\n",
            vec![Seg::Key("dotted".into()), Seg::Key("test".into())],
            vec![],
        );
        assert_eq!(s, "dotted.keep = 9\ntest.p = 1\ntest.q = 2\n");
    }

    // Collision is exact full-path: a dotted entry sharing only a prefix merges into
    // the same `[T/D]` table instead of colliding.
    #[test]
    fn insert_dotted_sibling_merges_not_collides() {
        let mut d = doc("a.x = 1\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![],
                index: 99,
            },
            fragment: "a.y = 2\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "a.x = 1\na.y = 2\n");
    }

    #[test]
    fn insert_identical_dotted_key_still_collides() {
        let mut d = doc("a.x = 1\n");
        let r = d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![],
                index: 99,
            },
            fragment: "a.x = 9\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        });
        assert!(matches!(r, Err(MutateError::Collision(k)) if k == "a.x"));
    }

    #[test]
    fn copy_dotted_block_into_dotted_prefixes_every_member() {
        // Copy path: a multi-member [T/D] block inserted into a dotted dest re-prefixes
        // EVERY member (was: second member dropped).
        let mut d = doc("a.x = 1\na.y = 2\nb.k = 9\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("b".into())],
                index: 99,
            },
            fragment: "a.x = 1\na.y = 2\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(
            d.serialize(),
            "a.x = 1\na.y = 2\nb.k = 9\nb.a.x = 1\nb.a.y = 2\n"
        );
    }

    fn move_try(
        initial: &str,
        src: Vec<Seg>,
        dst: Vec<Seg>,
    ) -> Result<String, crate::model::document::MutateError> {
        let mut d = doc(initial);
        d.apply(Mutation::Move {
            sources: vec![src],
            target: InsTarget {
                parent: dst,
                index: 99,
            },
            on_collision: OnCollision::Cancel,
        })?;
        Ok(d.serialize())
    }

    // Phase 3: cross-type table moves.
    #[test]
    fn move_dotted_table_into_inline_table_flattens() {
        let s = move_try(
            "a.x = 1\na.y = 2\nt = { k = 0 }\n",
            vec![Seg::Key("a".into())],
            vec![Seg::Key("t".into())],
        )
        .unwrap();
        assert_eq!(s, "t = { k = 0, a.x = 1, a.y = 2 }\n");
    }

    #[test]
    fn move_scope_table_into_scope_nests_header() {
        let s = move_try(
            "[a]\nx = 1\n[b]\ny = 2\n",
            vec![Seg::Key("a".into())],
            vec![Seg::Key("b".into())],
        )
        .unwrap();
        assert_eq!(s, "[b]\ny = 2\n[b.a]\nx = 1\n");
    }

    #[test]
    fn move_scope_table_with_subtable_into_scope_nests_all_headers() {
        let s = move_try(
            "[a]\nx = 1\n[a.sub]\nz = 3\n[b]\ny = 2\n",
            vec![Seg::Key("a".into())],
            vec![Seg::Key("b".into())],
        )
        .unwrap();
        assert_eq!(s, "[b]\ny = 2\n[b.a]\nx = 1\n[b.a.sub]\nz = 3\n");
    }

    #[test]
    fn move_scope_table_into_dotted_is_illegal() {
        // `b` must be a *top-level* dotted table, so it precedes the `[a]` header
        // (entries after `[a]` would belong to `a`).
        let r = move_try(
            "b.k = 9\n[a]\nx = 1\n",
            vec![Seg::Key("a".into())],
            vec![Seg::Key("b".into())],
        );
        assert!(
            matches!(&r, Err(MutateError::Illegal(m)) if m.contains("dotted")),
            "got {r:?}"
        );
    }

    #[test]
    fn move_scope_table_into_inline_is_illegal() {
        let r = move_try(
            "t = { k = 0 }\n[a]\nx = 1\n",
            vec![Seg::Key("a".into())],
            vec![Seg::Key("t".into())],
        );
        assert!(
            matches!(&r, Err(MutateError::Illegal(m)) if m.contains("inline")),
            "got {r:?}"
        );
    }

    // An entry insert targeting a scope table clamps to its entry run instead of
    // being rejected when the index points past sub-section children (the paste
    // "Into" slot appends at `children.len()`).
    #[test]
    fn move_dotted_table_into_scope_with_subtables_clamps_to_entry_run() {
        let s = move_try(
            "d.x = 1\nd.y = 2\n[pt]\n[pt.a]\nname = \"H\"\n",
            vec![Seg::Key("d".into())],
            vec![Seg::Key("pt".into())],
        )
        .unwrap();
        assert_eq!(s, "[pt]\nd.x = 1\nd.y = 2\n[pt.a]\nname = \"H\"\n");
    }

    // The dual clamp: a header-like fragment targeted before the destination's
    // entries lands at the section run instead of "would capture" Illegal.
    #[test]
    fn move_scope_table_into_scope_at_front_clamps_past_entries() {
        let mut d = doc("[a]\nx = 1\n[b]\ny = 2\n");
        d.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("b".into())]],
            target: InsTarget {
                parent: vec![Seg::Key("a".into())],
                index: 0,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(d.serialize(), "[a]\nx = 1\n[a.b]\ny = 2\n");
    }

    // A copied [T/D] fragment (multi-entry, no header) keeps *all* members when
    // pasted into an inline table — the per-entry split must run before the
    // inline-table branch, which splices only the first entry.
    #[test]
    fn copy_dotted_table_into_inline_keeps_all_members() {
        let mut d = doc("a.x = 1\na.y = 2\na.gg = 3\nt = { k = 0 }\n");
        let frag = d.serialize_fragment_relative(&[Seg::Key("a".into())]);
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("t".into())],
                index: 99,
            },
            fragment: frag,
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(
            d.serialize(),
            "a.x = 1\na.y = 2\na.gg = 3\nt = { k = 0, a.x = 1, a.y = 2, a.gg = 3 }\n"
        );
    }

    // Multi-entry insert holds its slot with a stable anchor: the first members
    // merge into one projected child, so a drifting `index + k` would push later
    // members past the destination's own entries.
    #[test]
    fn copy_dotted_table_into_scope_lands_contiguously() {
        let mut d = doc("a.t.p = 1\na.t.q = 2\na.gg = 3\n\n[s]\nk = 0\n");
        let frag = d.serialize_fragment_relative(&[Seg::Key("a".into())]);
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("s".into())],
                index: 0,
            },
            fragment: frag,
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(
            d.serialize(),
            "a.t.p = 1\na.t.q = 2\na.gg = 3\n\n[s]\na.t.p = 1\na.t.q = 2\na.gg = 3\nk = 0\n"
        );
    }

    // ---- Synthetic [T/D] tables *inside* an inline table (decomposed dotted keys) ----

    const INLINE_DOTTED: &str = "t = { x.y = 1, x.z = 2, w = 3 }\n";

    #[test]
    fn insert_into_inline_dotted_table_prefixes_member() {
        let mut d = doc(INLINE_DOTTED);
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("t".into()), Seg::Key("x".into())],
                index: 99,
            },
            fragment: "q = 9\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "t = { x.y = 1, x.z = 2, w = 3, x.q = 9 }\n");
    }

    #[test]
    fn insert_exact_member_into_inline_collides_but_prefix_merges() {
        let mut d = doc(INLINE_DOTTED);
        let r = d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("t".into())],
                index: 0,
            },
            fragment: "x.y = 7\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        });
        assert!(matches!(r, Err(MutateError::Collision(k)) if k == "x.y"));
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("t".into())],
                index: 0,
            },
            fragment: "x.q = 7\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "t = { x.q = 7, x.y = 1, x.z = 2, w = 3 }\n");
    }

    #[test]
    fn delete_inline_dotted_table_removes_all_members() {
        let mut d = doc(INLINE_DOTTED);
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("t".into()), Seg::Key("x".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "t = { w = 3 }\n");
    }

    #[test]
    fn fragment_of_inline_dotted_table_keeps_own_key() {
        let d = doc(INLINE_DOTTED);
        let frag = d.serialize_fragment_relative(&[Seg::Key("t".into()), Seg::Key("x".into())]);
        assert_eq!(frag, "x.y = 1\nx.z = 2\n");
    }

    #[test]
    fn move_inline_dotted_table_out_to_root() {
        let s = move_try(
            INLINE_DOTTED,
            vec![Seg::Key("t".into()), Seg::Key("x".into())],
            vec![],
        )
        .unwrap();
        assert_eq!(s, "t = { w = 3 }\nx.y = 1\nx.z = 2\n");
    }

    #[test]
    fn move_inline_dotted_table_into_scope() {
        let s = move_try(
            "t = { x.y = 1, x.z = 2, w = 3 }\n[s]\nk = 0\n",
            vec![Seg::Key("t".into()), Seg::Key("x".into())],
            vec![Seg::Key("s".into())],
        )
        .unwrap();
        assert_eq!(s, "t = { w = 3 }\n[s]\nk = 0\nx.y = 1\nx.z = 2\n");
    }

    #[test]
    fn replace_inline_dotted_table_consolidates_at_first_member() {
        let mut d = doc(INLINE_DOTTED);
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("t".into()), Seg::Key("x".into())],
            fragment: "x.y = 5\nx.q = 6\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "t = { x.y = 5, x.q = 6, w = 3 }\n");
    }

    #[test]
    fn comment_into_inline_dotted_table_is_illegal() {
        let mut d = doc(INLINE_DOTTED);
        let r = d.apply(Mutation::InsertComment {
            target: InsTarget {
                parent: vec![Seg::Key("t".into()), Seg::Key("x".into())],
                index: 0,
            },
            text: "# hi".into(),
        });
        assert!(matches!(r, Err(MutateError::Illegal(_))), "got {r:?}");
    }

    // Issue 3: inserting a keyed entry into an inline table splices it inside `{ … }`.
    #[test]
    fn insert_into_inline_table() {
        let mut d = doc("t = { a = 1 }\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("t".into())],
                index: 99,
            },
            fragment: "b = 2\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "t = { a = 1, b = 2 }\n");
    }

    #[test]
    fn insert_into_inline_table_at_front() {
        let mut d = doc("t = { a = 1 }\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("t".into())],
                index: 0,
            },
            fragment: "b = 2\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "t = { b = 2, a = 1 }\n");
    }

    #[test]
    fn insert_into_empty_inline_table() {
        let mut d = doc("t = {}\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("t".into())],
                index: 0,
            },
            fragment: "a = 1\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "t = { a = 1 }\n");
    }

    #[test]
    fn insert_into_inline_table_collision_rejected() {
        let mut d = doc("t = { a = 1 }\n");
        let r = d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("t".into())],
                index: 99,
            },
            fragment: "a = 2\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        });
        assert!(matches!(r, Err(MutateError::Collision(_))));
    }

    // ── `[T/S]` discretization: a table's member set is its scattered sections ──

    /// `[a]`'s subtree is defined in two places, split by `[b]`.
    const SCATTERED: &str = "[a]\nx = 1\n\n[b]\ny = 2\n\n[a.sub]\nz = 3\n";

    #[test]
    fn delete_scattered_scope_table_takes_all_sections() {
        let mut d = doc(SCATTERED);
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("a".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "[b]\ny = 2\n\n");
    }

    #[test]
    fn serialize_scattered_scope_table_includes_all_sections() {
        let d = doc(SCATTERED);
        let frag = d.serialize_fragment(&[Seg::Key("a".into())]);
        assert_eq!(frag, "[a]\nx = 1\n\n[a.sub]\nz = 3\n");
    }

    #[test]
    fn move_scattered_scope_table_into_scope_nests_all_sections() {
        let s = move_try(
            SCATTERED,
            vec![Seg::Key("a".into())],
            vec![Seg::Key("b".into())],
        )
        .unwrap();
        assert_eq!(s, "[b]\ny = 2\n\n[b.a]\nx = 1\n\n[b.a.sub]\nz = 3\n");
    }

    #[test]
    fn move_nested_scope_table_out_strips_source_prefix() {
        // `[a.sub]` moved into `[b]` must become `[b.sub]`, not `[b.a.sub]`.
        let s = move_try(
            "[a]\nx = 1\n[a.sub]\nz = 3\n[b]\ny = 2\n",
            vec![Seg::Key("a".into()), Seg::Key("sub".into())],
            vec![Seg::Key("b".into())],
        )
        .unwrap();
        assert_eq!(s, "[a]\nx = 1\n[b]\ny = 2\n[b.sub]\nz = 3\n");
    }

    #[test]
    fn block_edit_scattered_scope_table_consolidates_at_first_definition() {
        let mut d = doc(SCATTERED);
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("a".into())],
            fragment: "[a]\nx = 9\n[a.sub]\nz = 9\n".into(),
        })
        .unwrap();
        assert_eq!(d.serialize(), "[a]\nx = 9\n[a.sub]\nz = 9\n[b]\ny = 2\n\n");
    }

    #[test]
    fn block_edit_scope_table_rejects_out_of_subtree_header() {
        let mut d = doc(SCATTERED);
        let r = d.apply(Mutation::Replace {
            path: vec![Seg::Key("a".into())],
            fragment: "[a]\nx = 9\n[c]\nq = 1\n".into(),
        });
        assert!(matches!(r, Err(MutateError::Illegal(_))), "got {r:?}");
        assert_eq!(d.serialize(), SCATTERED);
    }

    // ── Implicit scope table (`[a]` never written, only `[a.sub]`) ──

    #[test]
    fn serialize_implicit_scope_table_collects_sections() {
        let d = doc("[a.sub]\nz = 3\n[a.other]\nw = 4\n");
        let frag = d.serialize_fragment(&[Seg::Key("a".into())]);
        assert_eq!(frag, "[a.sub]\nz = 3\n[a.other]\nw = 4\n");
    }

    #[test]
    fn delete_implicit_scope_table_removes_all_sections() {
        let mut d = doc("[a.sub]\nz = 3\n[b]\ny = 2\n[a.other]\nw = 4\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("a".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "[b]\ny = 2\n");
    }

    #[test]
    fn insert_entry_into_implicit_scope_table_creates_header() {
        // An entry child needs an `[a]` section to live in — created at the
        // table's first definition.
        let mut d = doc("[a.sub]\nz = 3\n[b]\ny = 2\n");
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("a".into())],
                index: 99,
            },
            fragment: "x = 1\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(d.serialize(), "[a]\nx = 1\n[a.sub]\nz = 3\n[b]\ny = 2\n");
    }

    // ── Mixed table: dotted members + header-defined sub-sections ──

    /// `fruit.apple` is defined by a dotted key under `[fruit]` *and* a
    /// `[fruit.apple.texture]` sub-section (the TOML-spec `fruit.apple` pattern).
    const MIXED: &str =
        "[fruit]\nname = \"f\"\napple.color = \"red\"\n\n[fruit.apple.texture]\nsmooth = true\n";

    #[test]
    fn serialize_mixed_table_canonicalizes_to_scope_form() {
        let d = doc(MIXED);
        let frag = d.serialize_fragment(&[Seg::Key("fruit".into()), Seg::Key("apple".into())]);
        assert_eq!(
            frag,
            "[fruit.apple]\ncolor = \"red\"\n[fruit.apple.texture]\nsmooth = true\n"
        );
    }

    #[test]
    fn block_edit_mixed_table_consolidates_to_scope_form() {
        let mut d = doc(MIXED);
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("fruit".into()), Seg::Key("apple".into())],
            fragment: "[fruit.apple]\ncolor = \"green\"\n[fruit.apple.texture]\nsmooth = false\n"
                .into(),
        })
        .unwrap();
        // The removed member line takes its trailing newline token with it (which
        // also held the blank line), as any deleted entry line does.
        assert_eq!(
            d.serialize(),
            "[fruit]\nname = \"f\"\n[fruit.apple]\ncolor = \"green\"\n[fruit.apple.texture]\nsmooth = false\n"
        );
    }

    #[test]
    fn delete_mixed_table_removes_members_and_sections() {
        let mut d = doc(MIXED);
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("fruit".into()), Seg::Key("apple".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "[fruit]\nname = \"f\"\n");
    }

    #[test]
    fn insert_entry_into_mixed_table_writes_dotted_member() {
        // No `[fruit.apple]` header may be created while dotted definitions
        // remain (spec-invalid) — the new entry joins the dotted members.
        let mut d = doc(MIXED);
        d.apply(Mutation::Insert {
            target: InsTarget {
                parent: vec![Seg::Key("fruit".into()), Seg::Key("apple".into())],
                index: 99,
            },
            fragment: "size = 3\n".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        })
        .unwrap();
        assert_eq!(
            d.serialize(),
            "[fruit]\nname = \"f\"\napple.color = \"red\"\n\napple.size = 3\n[fruit.apple.texture]\nsmooth = true\n"
        );
    }

    // ── AoT sub-groups travel with their table ──

    #[test]
    fn delete_scope_table_takes_scattered_aot_subgroup() {
        let mut d = doc("[a]\nx = 1\n\n[[a.list]]\nv = 1\n\n[b]\ny = 2\n\n[[a.list]]\nv = 2\n");
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("a".into())],
        })
        .unwrap();
        assert_eq!(d.serialize(), "[b]\ny = 2\n\n");
    }
}
