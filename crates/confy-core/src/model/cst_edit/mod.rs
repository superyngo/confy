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
#[path = "tests.rs"]
mod tests;
