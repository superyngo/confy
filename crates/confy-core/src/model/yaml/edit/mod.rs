//! YAML mutation helpers.
//!
//! The indent engine (`reindent`), path resolver (`resolve`), and opaque guard
//! (`is_opaque`) feed the atomic dispatcher (`apply`), which routes each
//! `Mutation` variant to its byte-splice + whole-document reparse (via
//! `splice_byte_range`). Out-of-subset constructs reject as `Unsupported`.

mod block;
mod convert;
mod flow;
mod mutations;
mod resolve;

// Re-exported so external callers (yaml/doc.rs, model/convert.rs,
// yaml/project.rs) keep their existing crate::model::yaml::edit::X paths --
// pure code motion, the split shouldn't ripple into unrelated files.
pub(crate) use convert::decode_double;
pub use mutations::serialize_fragment;

use crate::model::document::{MutateError, Mutation};
use crate::model::node::Seg;
use crate::model::yaml::project::walk;
use crate::model::yaml::syntax::SyntaxNode;
use block::{delete, insert, replace};
use convert::{convert_kind};
use mutations::{edit_comment, insert_comment, move_nodes, remark, rename, set_trailing_comment};
use resolve::is_opaque;

/// Backstop after a splice: re-parse and reject duplicate mapping keys
/// (Collision) or structural breakage (Illegal). Mirrors json/edit.rs's DOM
/// check using YAML re-parse + walk-based duplicate-key detection.
pub(crate) fn validate_semantics(tree: &SyntaxNode) -> Result<(), MutateError> {
    let text = tree.to_string();
    let green = crate::model::yaml::parse::parse(&text).map_err(MutateError::Illegal)?;
    let reparsed = SyntaxNode::new_root(green);
    // Re-walk and check for duplicate keys at every mapping level.
    let (node_tree, _idx) = walk(&reparsed, "");
    check_duplicate_keys(&node_tree.root.children)?;
    Ok(())
}

/// Recursively check for duplicate key names among siblings at each level.
pub(crate) fn check_duplicate_keys(nodes: &[crate::model::node::Node]) -> Result<(), MutateError> {
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

/// Extract the primary path(s) from a mutation for the opaque pre-check.
pub(crate) fn mutation_paths(m: &Mutation) -> Vec<&Vec<Seg>> {
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
        Mutation::SetTrailingComment { path, .. } => vec![path],
    }
}

pub fn apply(syntax: &SyntaxNode, m: Mutation) -> Result<SyntaxNode, MutateError> {
    // One projection walk shared by the opaque pre-check and every variant's
    // initial (pre-mutation) resolve. Built on the clone so `Target`s point into
    // the tree the splices mutate. Post-splice lookups still re-resolve — the
    // index is stale once the tree changes.
    let tree = syntax.clone_for_update();
    let (_, idx) = walk(&tree, "");

    // Opaque pre-check: any target path inside (or equal to) an opaque span → Unsupported.
    for path in mutation_paths(&m) {
        if !path.is_empty() && is_opaque(&idx, path) {
            return Err(MutateError::Unsupported);
        }
    }

    match m {
        Mutation::Replace { path, fragment } => replace(&tree, &idx, &path, &fragment)?,
        Mutation::Delete { path } => delete(&tree, &idx, &path)?,
        Mutation::Insert {
            target,
            fragment,
            on_collision,
        } => insert(&tree, &target, &fragment, on_collision)?,
        Mutation::Rename { path, new_key } => rename(&idx, &path, &new_key)?,
        Mutation::Remark { path } => remark(&tree, &idx, &path)?,
        Mutation::EditComment { path, text } => edit_comment(&tree, &idx, &path, &text)?,
        Mutation::InsertComment { target, text } => insert_comment(&tree, &target, &text)?,
        Mutation::Move {
            sources,
            target,
            on_collision,
        } => move_nodes(&tree, &idx, &sources, &target, on_collision)?,
        Mutation::ConvertKind { path, target } => convert_kind(&tree, &idx, &path, target)?,
        Mutation::SetTrailingComment { path, comment } => {
            set_trailing_comment(&tree, &idx, &path, comment.as_deref())?
        }
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
    use super::block::*;
    
    
    
    use super::resolve::*;
    use crate::model::document::{MutateError, Mutation, OnCollision};
    use crate::model::node::Seg;

    #[test]
    fn move_node_past_root_leading_comment_lands_at_projection_slot() {
        // A leading ROOT-level `#` comment is projected as a root child but lives
        // OUTSIDE the top mapping, so the projection index space is one ahead of
        // the mapping's slot space. Moving `multiline_literal` (proj 1) to just
        // before the inter-entry `# section` (proj index 3) must land it there,
        // not after the comment (the offset must be applied).
        let src = "# 1\nmultiline_literal: \"x\"\nempty_string: \"\"\n# section\ndecimal: 42\n";
        let t = SyntaxNode::new_root(crate::model::yaml::parse::parse(src).unwrap());
        let out = super::apply(
            &t,
            Mutation::Move {
                sources: vec![vec![Seg::Key("multiline_literal".into())]],
                target: crate::model::document::Target {
                    parent: vec![],
                    index: 3,
                },
                on_collision: OnCollision::Cancel,
            },
        )
        .unwrap()
        .to_string();
        assert_eq!(
            out,
            "# 1\nempty_string: \"\"\nmultiline_literal: \"x\"\n# section\ndecimal: 42\n"
        );
    }

    #[test]
    fn delete_removes_whole_merged_comment_block() {
        // A merged multi-line `#` block is ONE node — deleting it removes every
        // line, not just the first.
        let src = "# 1\n# 2\n# 3\nk: 1\n";
        let t = SyntaxNode::new_root(crate::model::yaml::parse::parse(src).unwrap());
        let out = super::apply(
            &t,
            Mutation::Delete {
                path: vec![Seg::Index(0)],
            },
        )
        .unwrap()
        .to_string();
        assert_eq!(out, "k: 1\n");
    }

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

    // ── key_colon / item_key_name (depth-aware `: `) ──────────────────────────

    #[test]
    fn key_colon_ignores_quoted_colon_space() {
        // Real keyed entries.
        assert_eq!(key_colon("a: 1"), Some(1));
        assert_eq!(key_colon("flags:"), Some(5)); // trailing-colon block value
                                                  // A quoted key holding `: ` keys on the whole quoted span.
        assert_eq!(key_colon(r#""a: b": v"#), Some(6));
        // A bare quoted scalar holding `: ` is NOT keyed.
        assert_eq!(key_colon(r#""a: b""#), None);
        assert_eq!(key_colon("'a: b'"), None);
        // A double-quoted value with `: ` after a real key still keys on the key.
        assert_eq!(key_colon(r#"k: "a: b""#), Some(1));
    }

    #[test]
    fn item_key_name_uses_whole_quoted_key() {
        assert_eq!(item_key_name("a: 1").as_deref(), Some("a"));
        assert_eq!(item_key_name(r#""a: b": v"#).as_deref(), Some("a: b"));
        assert_eq!(item_key_name(r#""a: b""#), None); // bare quoted scalar, no key
    }

    #[test]
    fn parse_map_entry_fragment_rejects_bare_quoted_scalar() {
        // A bare quoted scalar holding `: ` must not parse as a keyed entry.
        assert!(parse_map_entry_fragment(r#""a: b""#).is_none());
        // A real entry whose value holds `: ` still parses.
        assert!(parse_map_entry_fragment(r#"k: "a: b""#).is_some());
    }

    // ── Quoted-key resolution (insert/delete/collision through quoted keys) ───

    #[test]
    fn insert_under_quoted_key_parent() {
        // `find_container` must descend through a quoted parent key.
        let src = "\"a b\":\n  x: 1\n";
        let out = apply_str(
            src,
            Mutation::Insert {
                target: crate::model::document::Target {
                    parent: vec![Seg::Key("a b".into())],
                    index: 1,
                },
                fragment: "y: 2".into(),
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("insert under quoted-key parent");
        assert_eq!(out, "\"a b\":\n  x: 1\n  y: 2\n");
    }

    #[test]
    fn insert_colliding_quoted_key_is_collision() {
        // `existing_map_keys` must see the decoded form of a quoted sibling key.
        let src = "\"a b\": 1\n";
        let res = apply_str(
            src,
            Mutation::Insert {
                target: crate::model::document::Target {
                    parent: vec![],
                    index: 1,
                },
                fragment: "\"a b\": 2".into(),
                on_collision: OnCollision::Cancel,
            },
        );
        assert!(matches!(res, Err(MutateError::Collision(_))));
    }

    #[test]
    fn rename_onto_quoted_sibling_is_collision() {
        // The rename sibling check must compare decoded keys.
        let src = "\"a b\": 1\nc: 2\n";
        let res = apply_str(
            src,
            Mutation::Rename {
                path: vec![Seg::Key("c".into())],
                new_key: "a b".into(),
            },
        );
        assert!(matches!(res, Err(MutateError::Collision(_))));
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

    // ── 5f: Rename ───────────────────────────────────────────────────────────

    #[test]
    fn rename_key_token_in_place() {
        let out = apply_str(
            "a: 1\n",
            Mutation::Rename {
                path: vec![Seg::Key("a".into())],
                new_key: "c".into(),
            },
        )
        .expect("rename should succeed");
        assert_eq!(out, "c: 1\n");
    }

    #[test]
    fn rename_onto_existing_sibling_is_collision() {
        let r = apply_str(
            "a: 1\nb: 2\n",
            Mutation::Rename {
                path: vec![Seg::Key("a".into())],
                new_key: "b".into(),
            },
        );
        assert!(
            matches!(r, Err(MutateError::Collision(_))),
            "rename onto sibling expected Collision, got {r:?}"
        );
    }

    #[test]
    fn rename_non_key_is_illegal() {
        let r = apply_str(
            "- 1\n- 2\n",
            Mutation::Rename {
                path: vec![Seg::Index(0)],
                new_key: "x".into(),
            },
        );
        assert!(
            matches!(r, Err(MutateError::Illegal(_))),
            "rename of seq element expected Illegal, got {r:?}"
        );
    }

    // ── 5g: Remark ───────────────────────────────────────────────────────────

    #[test]
    fn remark_entry_to_comment() {
        let out = apply_str(
            "a: 1\n",
            Mutation::Remark {
                path: vec![Seg::Key("a".into())],
            },
        )
        .expect("remark entry should succeed");
        assert_eq!(out, "# a: 1\n");
    }

    #[test]
    fn remark_comment_to_entry() {
        let out = apply_str(
            "# a: 1\n",
            Mutation::Remark {
                path: vec![Seg::Index(0)],
            },
        )
        .expect("remark comment should succeed");
        assert_eq!(out, "a: 1\n");
    }

    #[test]
    fn remark_nested_entry_preserves_indent() {
        let src = "srv:\n  host: a\n  port: 80\n";
        let out = apply_str(
            src,
            Mutation::Remark {
                path: vec![Seg::Key("srv".into()), Seg::Key("host".into())],
            },
        )
        .expect("remark nested entry");
        assert_eq!(out, "srv:\n  # host: a\n  port: 80\n");
    }

    #[test]
    fn remark_duplicate_sequence_element_targets_the_right_one() {
        // Two identical elements: remarking index 2 must comment the THIRD,
        // not the first (identity-based position, not text match).
        let src = "- x\n- y\n- x\n";
        let out = apply_str(
            src,
            Mutation::Remark {
                path: vec![Seg::Index(2)],
            },
        )
        .expect("remark dup element");
        assert_eq!(out, "- x\n- y\n# - x\n");
    }

    #[test]
    fn edit_comment_duplicate_first_line_targets_the_right_block() {
        // Two comment blocks share the first line `# TODO`; a blank line breaks
        // them into two projected Comment nodes (Index 0 and 1). Editing the
        // SECOND must rewrite that block, leaving the first untouched.
        let src = "# TODO\n# a\n\n# TODO\n# b\nk: 1\n";
        let out = apply_str(
            src,
            Mutation::EditComment {
                path: vec![Seg::Index(1)],
                text: "# DONE".into(),
            },
        )
        .expect("edit second dup-first-line block");
        assert!(
            out.starts_with("# TODO\n# a\n") && out.contains("# DONE") && !out.contains("# b"),
            "expected first block intact and second→# DONE, got {out:?}"
        );
    }

    // ── 5h: EditComment ──────────────────────────────────────────────────────

    #[test]
    fn edit_comment_rewrites_block() {
        let out = apply_str(
            "# old\n",
            Mutation::EditComment {
                path: vec![Seg::Index(0)],
                text: "# new".into(),
            },
        )
        .expect("edit comment should succeed");
        assert_eq!(out, "# new\n");
    }

    #[test]
    fn edit_leading_comment_preserves_body() {
        // Regression: a leading comment is a direct ROOT child sitting beside the
        // top MAPPING. The old slot-item rebuild overwrote ROOT's whole span and
        // dropped the mapping (total data loss). The body must survive.
        let src = "# header\ntitle: demo\nport: 8080\n";
        let out = apply_str(
            src,
            Mutation::EditComment {
                path: vec![Seg::Index(0)],
                text: "# header EDITED".into(),
            },
        )
        .expect("edit leading comment should succeed");
        assert_eq!(out, "# header EDITED\ntitle: demo\nport: 8080\n");
    }

    #[test]
    fn remark_leading_comment_preserves_body() {
        // Same ROOT-sibling hazard for the remark (comment→entry) path.
        let src = "# title: old\nport: 8080\n";
        let out = apply_str(
            src,
            Mutation::Remark {
                path: vec![Seg::Index(0)],
            },
        )
        .expect("remark leading comment should succeed");
        assert_eq!(out, "title: old\nport: 8080\n");
    }

    #[test]
    fn edit_comment_non_hash_rejected() {
        let r = apply_str(
            "# old\n",
            Mutation::EditComment {
                path: vec![Seg::Index(0)],
                text: "not a comment".into(),
            },
        );
        assert!(
            matches!(r, Err(MutateError::Fragment(_))),
            "non-# text expected Fragment, got {r:?}"
        );
    }

    // ── 5i: InsertComment ────────────────────────────────────────────────────

    #[test]
    fn insert_comment_at_front() {
        use crate::model::document::Target;
        let out = apply_str(
            "a: 1\n",
            Mutation::InsertComment {
                target: Target {
                    parent: vec![],
                    index: 0,
                },
                text: "# note".into(),
            },
        )
        .expect("insert comment should succeed");
        assert_eq!(out, "# note\na: 1\n");
    }

    #[test]
    fn insert_member_after_leading_comment_is_not_mangled() {
        // A pre-existing standalone comment must keep its newline through the
        // collect_items/rebuild round-trip (regression for run-together lines).
        use crate::model::document::{OnCollision, Target};
        let out = apply_str(
            "# header\na: 1\n",
            Mutation::Insert {
                target: Target {
                    parent: vec![],
                    index: 2,
                },
                fragment: "b: 2\n".into(),
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("insert after leading comment");
        assert_eq!(out, "# header\na: 1\nb: 2\n");
    }

    #[test]
    fn insert_comment_non_hash_rejected() {
        use crate::model::document::Target;
        let r = apply_str(
            "a: 1\n",
            Mutation::InsertComment {
                target: Target {
                    parent: vec![],
                    index: 0,
                },
                text: "nope".into(),
            },
        );
        assert!(
            matches!(r, Err(MutateError::Fragment(_))),
            "non-# text expected Fragment, got {r:?}"
        );
    }

    // ── 5c: Replace ─────────────────────────────────────────────────────────

    #[test]
    fn replace_inline_scalar_value() {
        let out = apply_str(
            "k: 1\n",
            Mutation::Replace {
                path: vec![Seg::Key("k".into())],
                fragment: "k: 2".into(),
            },
        )
        .expect("replace should succeed");
        assert_eq!(out, "k: 2\n");
    }

    #[test]
    fn replace_block_mapping_value() {
        // Replace `host: a` inside `srv:` with `host: b`.
        let src = "srv:\n  host: a\n  port: 80\n";
        let out = apply_str(
            src,
            Mutation::Replace {
                path: vec![Seg::Key("srv".into()), Seg::Key("host".into())],
                fragment: "host: b".into(),
            },
        )
        .expect("replace should succeed");
        assert_eq!(out, "srv:\n  host: b\n  port: 80\n");
    }

    #[test]
    fn replace_block_seq_entry_with_own_fragment() {
        // Regression ($EDITOR on a block sequence): the entry's serialized
        // fragment is `flags:\n  - a\n  - b` — its first line ends with `:`
        // (no `: `), which the old keyed-fragment guard missed, sending it down
        // the bare-value path and failing to reparse. Re-applying it must be a
        // lossless no-op and keep the trailing sibling.
        let src = "flags:\n  - a\n  - b\nafter: 1\n";
        let out = apply_str(
            src,
            Mutation::Replace {
                path: vec![Seg::Key("flags".into())],
                fragment: "flags:\n  - a\n  - b".into(),
            },
        )
        .expect("block-seq replace should succeed");
        assert_eq!(out, src);
    }

    #[test]
    fn replace_block_map_entry_with_own_fragment() {
        let src = "server:\n  host: a\n  port: 1\nafter: 1\n";
        let out = apply_str(
            src,
            Mutation::Replace {
                path: vec![Seg::Key("server".into())],
                fragment: "server:\n  host: a\n  port: 1".into(),
            },
        )
        .expect("block-map replace should succeed");
        assert_eq!(out, src);
    }

    #[test]
    fn replace_whole_document_valid() {
        let out = apply_str(
            "a: 1\n",
            Mutation::Replace {
                path: vec![],
                fragment: "b: 2\n".into(),
            },
        )
        .expect("whole-doc replace should succeed");
        assert_eq!(out, "b: 2\n");
    }

    #[test]
    fn replace_whole_document_multi_doc_rejected() {
        let r = apply_str(
            "a: 1\n",
            Mutation::Replace {
                path: vec![],
                fragment: "---\na: 1\n---\nb: 2\n".into(),
            },
        );
        assert!(
            matches!(r, Err(MutateError::Fragment(_))),
            "multi-doc replace should be Fragment error, got {r:?}"
        );
    }

    #[test]
    fn replace_over_opaque_value_is_unsupported() {
        // `ref: *anchor` projects as a read-only MapEntry (opaque value);
        // Replace must reject it, leaving the doc untouched.
        let r = apply_str(
            "ref: *anchor\nk: 1\n",
            Mutation::Replace {
                path: vec![Seg::Key("ref".into())],
                fragment: "ref: 5".into(),
            },
        );
        assert!(
            matches!(r, Err(MutateError::Unsupported)),
            "replace over opaque value expected Unsupported, got {r:?}"
        );
    }

    // ── 5d: Delete ─────────────────────────────────────────────────────────

    #[test]
    fn delete_middle_element_of_sequence() {
        let src = "- 10\n- 20\n- 30\n";
        let out = apply_str(
            src,
            Mutation::Delete {
                path: vec![Seg::Index(1)],
            },
        )
        .expect("delete middle element should succeed");
        assert_eq!(out, "- 10\n- 30\n");
    }

    #[test]
    fn delete_flow_seq_element_keeps_siblings() {
        // Regression (#2): deleting one element of a `[A/F]` flow seq must keep the
        // other elements and the key, not wipe the whole sequence to null.
        let out = apply_str(
            "a: [1, 2, 3]\nb: 9\n",
            Mutation::Delete {
                path: vec![Seg::Key("a".into()), Seg::Index(1)],
            },
        )
        .expect("delete flow-seq element should succeed");
        assert_eq!(out, "a: [1, 3]\nb: 9\n");
    }

    #[test]
    fn delete_only_flow_seq_element_leaves_empty_seq() {
        let out = apply_str(
            "a: [1]\n",
            Mutation::Delete {
                path: vec![Seg::Key("a".into()), Seg::Index(0)],
            },
        )
        .expect("delete last flow-seq element should succeed");
        assert_eq!(out, "a: []\n");
    }

    #[test]
    fn insert_member_into_flow_map_nested_in_flow_seq() {
        // Item 2d.2: a `[T/F]` inside an `[A/F]` is addressable; inserting a member
        // descends through the flow seq by index and rebuilds the `{…}` inline.
        use crate::model::document::{OnCollision, Target};
        let out = apply_str(
            "a: [{x: 1}, {y: 2}]\n",
            Mutation::Insert {
                target: Target {
                    parent: vec![Seg::Key("a".into()), Seg::Index(0)],
                    index: 1,
                },
                fragment: "z: 9\n".into(),
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("insert into flow-map nested in flow-seq should succeed");
        assert_eq!(out, "a: [{x: 1, z: 9}, {y: 2}]\n");
    }

    #[test]
    fn replace_flow_seq_element_inline() {
        let out = apply_str(
            "a: [1, 2, 3]\n",
            Mutation::Replace {
                path: vec![Seg::Key("a".into()), Seg::Index(1)],
                fragment: "20".into(),
            },
        )
        .expect("replace flow-seq element should succeed");
        assert_eq!(out, "a: [1, 20, 3]\n");
    }

    #[test]
    fn delete_map_entry_with_nested_children() {
        let src = "srv:\n  host: a\n  port: 80\nother: x\n";
        let out = apply_str(
            src,
            Mutation::Delete {
                path: vec![Seg::Key("srv".into())],
            },
        )
        .expect("delete entry with nested children");
        assert_eq!(out, "other: x\n");
    }

    #[test]
    fn delete_standalone_comment() {
        let src = "# hello\na: 1\n";
        let out = apply_str(
            src,
            Mutation::Delete {
                path: vec![Seg::Index(0)],
            },
        )
        .expect("delete comment should succeed");
        assert_eq!(out, "a: 1\n");
    }

    // ── 5e: Insert ─────────────────────────────────────────────────────────

    #[test]
    fn insert_member_at_end_of_mapping() {
        use crate::model::document::{OnCollision, Target};
        let out = apply_str(
            "a: 1\n",
            Mutation::Insert {
                target: Target {
                    parent: vec![],
                    index: 1,
                },
                fragment: "b: 2\n".into(),
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("insert member at end");
        assert_eq!(out, "a: 1\nb: 2\n");
    }

    #[test]
    fn insert_member_into_truly_empty_document_synthesizes_a_root_mapping() {
        // No top-level MAPPING/SEQUENCE exists yet for `find_container` to find
        // (regression: this used to fail the whole insert with `NotFound`,
        // surfacing as "path not found" on Add for a blank YAML file).
        use crate::model::document::{OnCollision, Target};
        let out = apply_str(
            "",
            Mutation::Insert {
                target: Target {
                    parent: vec![],
                    index: 0,
                },
                fragment: "new_field: \"\"\n".into(),
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("insert into an empty document");
        assert_eq!(out, "new_field: \"\"\n");
    }

    #[test]
    fn insert_member_into_comment_only_document_keeps_the_comment() {
        use crate::model::document::{OnCollision, Target};
        let out = apply_str(
            "# just a comment\n",
            Mutation::Insert {
                target: Target {
                    parent: vec![],
                    index: 0,
                },
                fragment: "new_field: \"\"\n".into(),
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("insert into a comment-only document");
        assert_eq!(out, "# just a comment\nnew_field: \"\"\n");
    }

    #[test]
    fn insert_sequence_element_into_empty_document_synthesizes_a_root_sequence() {
        use crate::model::document::{OnCollision, Target};
        let out = apply_str(
            "",
            Mutation::Insert {
                target: Target {
                    parent: vec![],
                    index: 0,
                },
                fragment: "- 1\n".into(),
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("insert seq element into an empty document");
        assert_eq!(out, "- 1\n");
    }

    #[test]
    fn insert_at_a_nonexistent_nested_parent_in_an_empty_document_still_errors() {
        // Only the *root* parent gets the empty-document synthesis fallback —
        // a deeper path with nothing to walk into must still report NotFound.
        use crate::model::document::{OnCollision, Target};
        let res = apply_str(
            "",
            Mutation::Insert {
                target: Target {
                    parent: vec![Seg::Key("missing".into())],
                    index: 0,
                },
                fragment: "x: 1\n".into(),
                on_collision: OnCollision::Cancel,
            },
        );
        assert!(matches!(res, Err(MutateError::NotFound)));
    }


    #[test]
    fn insert_keyed_fragment_into_sequence() {
        use crate::model::document::{OnCollision, Target};
        let out = apply_str(
            "- 1\n- 2\n",
            Mutation::Insert {
                target: Target {
                    parent: vec![],
                    index: 1,
                },
                fragment: "b: 2\n".into(),
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("insert keyed fragment into sequence");
        // Keyed fragment into a sequence → becomes a sequence element `- b: 2`
        assert!(
            out.contains("- b: 2"),
            "expected '- b: 2' in output: {out:?}"
        );
    }

    #[test]
    fn insert_bare_value_into_mapping_gets_placeholder_key() {
        use crate::model::document::{OnCollision, Target};
        let out = apply_str(
            "a: 1\n",
            Mutation::Insert {
                target: Target {
                    parent: vec![],
                    index: 1,
                },
                fragment: "5".into(),
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("insert bare value into mapping");
        assert!(
            out.contains("placeholder"),
            "expected 'placeholder' key in output: {out:?}"
        );
    }

    // ── 5j: Move ─────────────────────────────────────────────────────────────

    #[test]
    fn move_block_entry_between_mappings_reindents() {
        // Move `a.x` into `b` (depth 2). It should reindent under `b` and be
        // gone from `a`.
        let src = "a:\n  x: 1\nb:\n  y: 2\n";
        let out = apply_str(
            src,
            Mutation::Move {
                sources: vec![vec![Seg::Key("a".into()), Seg::Key("x".into())]],
                target: crate::model::document::Target {
                    parent: vec![Seg::Key("b".into())],
                    index: 1,
                },
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("move block entry between mappings");
        assert_eq!(out, "a:\nb:\n  y: 2\n  x: 1\n");
    }

    #[test]
    fn move_entry_down_before_trailing_comment() {
        // `a` moved to just after `b` must land BEFORE the trailing `# c`
        // comment (slot index 2), not after it.
        let src = "a: 1\nb: 2\n# c\n";
        let out = apply_str(
            src,
            Mutation::Move {
                sources: vec![vec![Seg::Key("a".into())]],
                target: crate::model::document::Target {
                    parent: vec![],
                    index: 2,
                },
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("move entry before trailing comment");
        assert_eq!(out, "b: 2\na: 1\n# c\n");
    }

    #[test]
    fn move_sequence_element() {
        // Move index 0 to the end. target.index is a pre-deletion ordinal, so
        // "after the last (index-2) element" is ordinal 3; after deleting index
        // 0 the decrement + min(len) clamp lands it at the tail.
        let src = "- 1\n- 2\n- 3\n";
        let out = apply_str(
            src,
            Mutation::Move {
                sources: vec![vec![Seg::Index(0)]],
                target: crate::model::document::Target {
                    parent: vec![],
                    index: 3,
                },
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("move sequence element to end");
        assert_eq!(out, "- 2\n- 3\n- 1\n");
    }

    #[test]
    fn move_sequence_element_low_to_middle_ordinal() {
        // target.index is a pre-deletion ordinal: move index 0 to ordinal 2
        // ("after the original index-1 element"). After deleting index 0 the
        // slots shift down, so the moved element must land at post-deletion
        // index 1 — between the survivors, not appended.
        let src = "- 10\n- 20\n- 30\n- 40\n";
        let out = apply_str(
            src,
            Mutation::Move {
                sources: vec![vec![Seg::Index(0)]],
                target: crate::model::document::Target {
                    parent: vec![],
                    index: 2,
                },
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("move low index to middle ordinal");
        assert_eq!(out, "- 20\n- 10\n- 30\n- 40\n");
    }

    #[test]
    fn move_source_opaque_is_unsupported() {
        // `ref: *anchor` is a read-only (opaque-valued) entry; moving it rejects
        // and leaves the doc untouched.
        let r = apply_str(
            "ref: *anchor\nk: 1\n",
            Mutation::Move {
                sources: vec![vec![Seg::Key("ref".into())]],
                target: crate::model::document::Target {
                    parent: vec![],
                    index: 2,
                },
                on_collision: OnCollision::Cancel,
            },
        );
        assert!(
            matches!(r, Err(MutateError::Unsupported)),
            "move of opaque-valued entry expected Unsupported, got {r:?}"
        );
    }

    #[test]
    fn move_empty_sources_is_noop() {
        let out = apply_str(
            "a: 1\n",
            Mutation::Move {
                sources: vec![],
                target: crate::model::document::Target {
                    parent: vec![],
                    index: 0,
                },
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("empty move is a no-op");
        assert_eq!(out, "a: 1\n");
    }

    // ── 6: ConvertKind ───────────────────────────────────────────────────────

    use crate::model::document::KindTarget;

    fn convert(src: &str, path: Vec<Seg>, target: KindTarget) -> Result<String, MutateError> {
        apply_str(src, Mutation::ConvertKind { path, target })
    }

    #[test]
    fn convert_block_map_to_flow() {
        let out = convert(
            "a:\n  x: 1\n  y: 2\n",
            vec![Seg::Key("a".into())],
            KindTarget::Flow,
        )
        .expect("block→flow");
        assert_eq!(out, "a: {x: 1, y: 2}\n");
    }

    // A plain block-style value containing a flow indicator (`,{}[]`) can't be
    // silently collapsed into `{…}`/`[…]` — unquoted, YAML's flow grammar
    // treats the comma as a member separator, which would silently truncate
    // the value and spawn a bogus sibling key from the remainder on reparse.
    // Reject the whole conversion instead of quietly reformatting the member
    // to a quoted style behind the user's back; they quote it first and retry.
    #[test]
    fn convert_block_map_to_flow_rejects_plain_value_with_comma() {
        let err = convert(
            "about:\n  name: confy\n  pitch: Three dialects, one tree\n",
            vec![Seg::Key("about".into())],
            KindTarget::Flow,
        )
        .expect_err("block→flow with an unquoted comma must be rejected");
        assert!(
            matches!(&err, MutateError::Illegal(msg) if msg.contains("pitch") && msg.contains("quote it first")),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn convert_block_seq_to_flow_rejects_plain_element_with_comma() {
        let err = convert(
            "a:\n  - one, two\n  - three\n",
            vec![Seg::Key("a".into())],
            KindTarget::Flow,
        )
        .expect_err("block seq→flow with an unquoted comma must be rejected");
        assert!(
            matches!(&err, MutateError::Illegal(msg) if msg.contains("quote it first")),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn convert_block_map_to_flow_rejects_plain_value_with_braces() {
        // A leading `{` is real flow-map syntax even in block context (handled
        // by the existing nested-flow-collection path); this covers a brace
        // *inside* an otherwise-plain string, which block context allows but
        // flow context does not.
        let err = convert(
            "a:\n  x: has {curly} braces\n",
            vec![Seg::Key("a".into())],
            KindTarget::Flow,
        )
        .expect_err("block→flow with an unquoted brace must be rejected");
        assert!(
            matches!(&err, MutateError::Illegal(msg) if msg.contains("quote it first")),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn convert_block_map_to_flow_leaves_already_quoted_value_untouched() {
        let out = convert(
            "about:\n  pitch: \"Three dialects, one tree\"\n  ok: 1\n",
            vec![Seg::Key("about".into())],
            KindTarget::Flow,
        )
        .expect("block→flow: an already-quoted value doesn't need rejecting");
        assert_eq!(out, "about: {pitch: \"Three dialects, one tree\", ok: 1}\n");
    }

    #[test]
    fn convert_block_to_flow_rejects_literal_block_scalar_with_specific_message() {
        let err = convert(
            "a:\n  x: |\n    line1\n    line2\n",
            vec![Seg::Key("a".into())],
            KindTarget::Flow,
        )
        .expect_err("literal block scalar can't collapse to flow");
        assert!(
            matches!(&err, MutateError::Illegal(msg) if msg.contains("literal") && msg.contains("folded")),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn convert_block_to_flow_rejects_folded_block_scalar_with_specific_message() {
        let err = convert(
            "a:\n  x: >\n    line1\n    line2\n",
            vec![Seg::Key("a".into())],
            KindTarget::Flow,
        )
        .expect_err("folded block scalar can't collapse to flow");
        assert!(
            matches!(&err, MutateError::Illegal(msg) if msg.contains("literal") && msg.contains("folded")),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn convert_block_to_flow_rejects_comment_with_specific_message() {
        let err = convert(
            "a:\n  x: 1\n  # note\n  y: 2\n",
            vec![Seg::Key("a".into())],
            KindTarget::Flow,
        )
        .expect_err("a comment can't collapse to flow");
        assert!(
            matches!(&err, MutateError::Illegal(msg) if msg.contains("comment") && !msg.contains("block scalar")),
            "unexpected error: {err:?}"
        );
    }

    // Item 2: a quoted string *inside* a flow collection that contains a flow
    // indicator can't become Plain either — same corruption, reached via the
    // per-scalar `K` switch instead of the container-level Block→Flow switch.
    #[test]
    fn convert_string_plain_rejects_flow_member_with_comma() {
        let err = convert(
            "about: {pitch: \"Three dialects, one tree\"}\n",
            vec![Seg::Key("about".into()), Seg::Key("pitch".into())],
            KindTarget::StringPlain,
        )
        .expect_err("flow member with a comma can't become plain");
        assert!(matches!(err, MutateError::Illegal(_)));
    }

    #[test]
    fn kind_options_hides_plain_for_flow_member_with_comma() {
        use crate::model::document::ConfigDocument;
        let doc = crate::model::yaml::doc::YamlDocument::from_str(
            "about: {pitch: \"Three dialects, one tree\", ok: \"fine\"}\n",
        )
        .unwrap();
        let unsafe_opts = doc.kind_options(&[Seg::Key("about".into()), Seg::Key("pitch".into())]);
        assert!(
            !unsafe_opts
                .iter()
                .any(|(_, t)| *t == KindTarget::StringPlain),
            "Plain must not be offered for a flow member containing a comma: {unsafe_opts:?}"
        );
        // A flow member without a flow-unsafe character still offers Plain.
        let safe_opts = doc.kind_options(&[Seg::Key("about".into()), Seg::Key("ok".into())]);
        assert!(
            safe_opts.iter().any(|(_, t)| *t == KindTarget::StringPlain),
            "Plain should still be offered for a flow-safe quoted member: {safe_opts:?}"
        );
    }

    #[test]
    fn convert_flow_map_to_block() {
        let out = convert(
            "a: {x: 1, y: 2}\n",
            vec![Seg::Key("a".into())],
            KindTarget::Block,
        )
        .expect("flow→block");
        assert_eq!(out, "a:\n  x: 1\n  y: 2\n");
    }

    #[test]
    fn convert_block_seq_to_flow() {
        let out = convert(
            "a:\n  - 1\n  - 2\n",
            vec![Seg::Key("a".into())],
            KindTarget::Flow,
        )
        .expect("block seq→flow");
        assert_eq!(out, "a: [1, 2]\n");
    }

    #[test]
    fn convert_flow_seq_to_block() {
        let out = convert("a: [1, 2]\n", vec![Seg::Key("a".into())], KindTarget::Block)
            .expect("flow seq→block");
        assert_eq!(out, "a:\n  - 1\n  - 2\n");
    }

    #[test]
    fn convert_flow_seq_of_flow_maps_to_block() {
        // Item 2d.1: an `[A/F]` nested with `[T/F]` elements expands to block,
        // keeping each inner flow map verbatim (symmetric with the forward path).
        let out = convert(
            "a: [{x: 1}, {y: 2}]\n",
            vec![Seg::Key("a".into())],
            KindTarget::Block,
        )
        .expect("flow seq of flow maps → block");
        assert_eq!(out, "a:\n  - {x: 1}\n  - {y: 2}\n");
    }

    #[test]
    fn convert_block_seq_of_flow_maps_to_flow_roundtrips() {
        // The inverse direction already worked; together they round-trip.
        let out = convert(
            "a:\n  - {x: 1}\n  - {y: 2}\n",
            vec![Seg::Key("a".into())],
            KindTarget::Flow,
        )
        .expect("block seq of flow maps → flow");
        assert_eq!(out, "a: [{x: 1}, {y: 2}]\n");
    }

    #[test]
    fn set_trailing_comment_add_change_clear() {
        let p = || vec![Seg::Key("host".into())];
        let set = |c: Option<&str>| Mutation::SetTrailingComment {
            path: p(),
            comment: c.map(str::to_string),
        };
        // add
        assert_eq!(
            apply_str("host: x\n", set(Some("# bind"))).unwrap(),
            "host: x  # bind\n"
        );
        // change
        assert_eq!(
            apply_str("host: x  # old\n", set(Some("# new"))).unwrap(),
            "host: x  # new\n"
        );
        // clear
        assert_eq!(
            apply_str("host: x  # old\n", set(None)).unwrap(),
            "host: x\n"
        );
        // a `#` inside a quoted string is not the trailing comment
        assert_eq!(
            apply_str("host: \"a # b\"\n", set(Some("# note"))).unwrap(),
            "host: \"a # b\"  # note\n"
        );
    }

    #[test]
    fn set_trailing_comment_on_block_seq_scalar_element() {
        // A block-sequence scalar element can gain/clear a trailing comment.
        let set = |src: &str, c: Option<&str>| {
            apply_str(
                src,
                Mutation::SetTrailingComment {
                    path: vec![Seg::Key("flags".into()), Seg::Index(1)],
                    comment: c.map(str::to_string),
                },
            )
        };
        assert_eq!(
            set("flags:\n  - a\n  - b\n", Some("# second")).unwrap(),
            "flags:\n  - a\n  - b  # second\n"
        );
        assert_eq!(
            set("flags:\n  - a\n  - b  # old\n", None).unwrap(),
            "flags:\n  - a\n  - b\n"
        );
    }

    #[test]
    fn set_trailing_comment_on_block_map_parent() {
        // A branch (block-map parent key) gains/changes/clears a trailing comment
        // on its own `key:` line, leaving the nested block untouched.
        let set = |src: &str, c: Option<&str>| {
            apply_str(
                src,
                Mutation::SetTrailingComment {
                    path: vec![Seg::Key("host".into())],
                    comment: c.map(str::to_string),
                },
            )
        };
        assert_eq!(
            set("host:\n  x: 1\n", Some("# the host")).unwrap(),
            "host:  # the host\n  x: 1\n"
        );
        assert_eq!(
            set("host:  # old\n  x: 1\n", Some("# new")).unwrap(),
            "host:  # new\n  x: 1\n"
        );
        assert_eq!(
            set("host:  # old\n  x: 1\n", None).unwrap(),
            "host:\n  x: 1\n"
        );
        // A block sequence parent works too.
        assert_eq!(
            apply_str(
                "host:\n  - a\n",
                Mutation::SetTrailingComment {
                    path: vec![Seg::Key("host".into())],
                    comment: Some("# list".into()),
                },
            )
            .unwrap(),
            "host:  # list\n  - a\n"
        );
    }

    #[test]
    fn set_trailing_comment_rejects_block_scalar() {
        // A multi-line block scalar (`|`) has no first-line comment slot.
        let r = apply_str(
            "doc: |\n  hello\n  world\n",
            Mutation::SetTrailingComment {
                path: vec![Seg::Key("doc".into())],
                comment: Some("# c".into()),
            },
        );
        assert!(matches!(r, Err(MutateError::Unsupported)), "got {r:?}");
    }

    #[test]
    fn convert_flow_map_seq_element_to_block_is_compact() {
        // A seq element holding a flow map expands to the compact `- key: v`
        // form (first member on the dash line), not `-\n  key: v` which reads
        // as a stray blank line. (R5)
        let out = convert(
            "items:\n  - {name: a, age: 5}\n",
            vec![Seg::Key("items".into()), Seg::Index(0)],
            KindTarget::Block,
        )
        .expect("flow-map seq element→block");
        assert_eq!(out, "items:\n  - name: a\n    age: 5\n");
    }

    #[test]
    fn convert_block_to_flow_with_comment_rejected() {
        let r = convert(
            "a:\n  # note\n  x: 1\n",
            vec![Seg::Key("a".into())],
            KindTarget::Flow,
        );
        assert!(
            matches!(r, Err(MutateError::Illegal(_))),
            "block→flow with comment expected Illegal, got {r:?}"
        );
    }

    #[test]
    fn convert_string_single_to_double() {
        let out = convert(
            "k: 'hi'\n",
            vec![Seg::Key("k".into())],
            KindTarget::StringDouble,
        )
        .expect("single→double");
        assert_eq!(out, "k: \"hi\"\n");
    }

    #[test]
    fn convert_string_double_to_single() {
        let out = convert(
            "k: \"hi\"\n",
            vec![Seg::Key("k".into())],
            KindTarget::StringSingle,
        )
        .expect("double→single");
        assert_eq!(out, "k: 'hi'\n");
    }

    #[test]
    fn convert_string_single_to_plain() {
        let out = convert(
            "k: 'hi'\n",
            vec![Seg::Key("k".into())],
            KindTarget::StringPlain,
        )
        .expect("single→plain");
        assert_eq!(out, "k: hi\n");
    }

    #[test]
    fn convert_string_plain_target_rejects_unsafe_content() {
        let r = convert(
            "k: ': bad'\n",
            vec![Seg::Key("k".into())],
            KindTarget::StringPlain,
        );
        assert!(
            matches!(r, Err(MutateError::Illegal(_))),
            "unsafe plain target expected Illegal, got {r:?}"
        );
    }

    #[test]
    fn convert_string_single_quote_doubling() {
        let out = convert(
            "k: \"it's\"\n",
            vec![Seg::Key("k".into())],
            KindTarget::StringSingle,
        )
        .expect("double→single with apostrophe");
        assert_eq!(out, "k: 'it''s'\n");
    }

    #[test]
    fn convert_string_to_literal_block() {
        let out = convert(
            "k: 'hi'\n",
            vec![Seg::Key("k".into())],
            KindTarget::StringLiteralBlock,
        )
        .expect("→literal block");
        assert_eq!(out, "k: |\n  hi\n");
    }

    #[test]
    fn convert_literal_block_to_double() {
        let out = convert(
            "k: |\n  hi\n",
            vec![Seg::Key("k".into())],
            KindTarget::StringDouble,
        )
        .expect("literal→double");
        assert_eq!(out, "k: \"hi\"\n");
    }

    #[test]
    fn convert_int_dec_to_hex() {
        let out =
            convert("k: 255\n", vec![Seg::Key("k".into())], KindTarget::IntHex).expect("dec→hex");
        assert_eq!(out, "k: 0xff\n");
    }

    #[test]
    fn convert_int_hex_to_dec() {
        let out = convert(
            "k: 0xff\n",
            vec![Seg::Key("k".into())],
            KindTarget::IntDecimal,
        )
        .expect("hex→dec");
        assert_eq!(out, "k: 255\n");
    }

    #[test]
    fn convert_int_dec_to_octal() {
        let out =
            convert("k: 8\n", vec![Seg::Key("k".into())], KindTarget::IntOctal).expect("dec→oct");
        assert_eq!(out, "k: 0o10\n");
    }

    #[test]
    fn convert_int_negative_to_hex_rejected() {
        let r = convert("k: -5\n", vec![Seg::Key("k".into())], KindTarget::IntHex);
        assert!(
            matches!(r, Err(MutateError::Illegal(_))),
            "negative→hex expected Illegal, got {r:?}"
        );
    }

    #[test]
    fn convert_float_plain_to_exponent_and_back() {
        let out = convert(
            "k: 1500.0\n",
            vec![Seg::Key("k".into())],
            KindTarget::FloatExponent,
        )
        .expect("plain→exp");
        assert!(out.contains("e3"), "expected exponent form, got {out:?}");
        let back =
            convert(&out, vec![Seg::Key("k".into())], KindTarget::FloatPlain).expect("exp→plain");
        // Must stay a float: a whole value keeps its `.0` so it doesn't
        // re-classify as an integer.
        assert_eq!(back, "k: 1500.0\n");
    }

    #[test]
    fn convert_float_plain_target_keeps_float_type() {
        // 2.0 → plain must not collapse to `2` (which YAML reads as Integer).
        let out = convert(
            "k: 2.0\n",
            vec![Seg::Key("k".into())],
            KindTarget::FloatPlain,
        );
        // already plain → no conversion offered; exercise exp→plain instead.
        let _ = out;
        let exp = convert(
            "k: 2.0\n",
            vec![Seg::Key("k".into())],
            KindTarget::FloatExponent,
        )
        .expect("plain→exp");
        let plain =
            convert(&exp, vec![Seg::Key("k".into())], KindTarget::FloatPlain).expect("exp→plain");
        assert_eq!(plain, "k: 2.0\n");
    }

    #[test]
    fn convert_seq_element_int_radix() {
        let out = convert("- 255\n- 2\n", vec![Seg::Index(0)], KindTarget::IntHex)
            .expect("seq element dec→hex");
        assert_eq!(out, "- 0xff\n- 2\n");
    }

    #[test]
    fn convert_kind_on_opaque_unsupported() {
        let r = convert(
            "ref: *anchor\n",
            vec![Seg::Key("ref".into())],
            KindTarget::StringDouble,
        );
        assert!(
            matches!(r, Err(MutateError::Unsupported)),
            "convert on opaque expected Unsupported, got {r:?}"
        );
    }

    // ── Flow-collection member edits (R4) ─────────────────────────────────────

    #[test]
    fn replace_flow_map_member_stays_inline() {
        // R4: editing a flow-map member must keep the `{…}` on one line.
        let out = apply_str(
            "ratio: {x: 1.5, y: 2.5}\n",
            Mutation::Replace {
                path: vec![Seg::Key("ratio".into()), Seg::Key("x".into())],
                fragment: "x: 9.9".into(),
            },
        )
        .expect("replace flow member");
        assert_eq!(out, "ratio: {x: 9.9, y: 2.5}\n");
    }

    #[test]
    fn convert_flow_map_member_int_radix() {
        // R4: kind-switch on a flow-map member (the reported `ratio: {x: …}` case).
        let out = convert(
            "n: {a: 255, b: 2}\n",
            vec![Seg::Key("n".into()), Seg::Key("a".into())],
            KindTarget::IntHex,
        )
        .expect("convert flow member dec→hex");
        assert_eq!(out, "n: {a: 0xff, b: 2}\n");
    }

    #[test]
    fn add_child_into_flow_map() {
        use crate::model::document::{OnCollision, Target};
        let out = apply_str(
            "pt: {x: 1, y: 2}\n",
            Mutation::Insert {
                target: Target {
                    parent: vec![Seg::Key("pt".into())],
                    index: 2,
                },
                fragment: "z: 3\n".into(),
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("insert into flow map");
        assert_eq!(out, "pt: {x: 1, y: 2, z: 3}\n");
    }

    #[test]
    fn add_child_into_flow_map_front() {
        use crate::model::document::{OnCollision, Target};
        let out = apply_str(
            "pt: {y: 2}\n",
            Mutation::Insert {
                target: Target {
                    parent: vec![Seg::Key("pt".into())],
                    index: 0,
                },
                fragment: "x: 1\n".into(),
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("insert at front of flow map");
        assert_eq!(out, "pt: {x: 1, y: 2}\n");
    }

    #[test]
    fn add_child_into_flow_map_collision_cancels() {
        use crate::model::document::{OnCollision, Target};
        let r = apply_str(
            "pt: {x: 1}\n",
            Mutation::Insert {
                target: Target {
                    parent: vec![Seg::Key("pt".into())],
                    index: 1,
                },
                fragment: "x: 9\n".into(),
                on_collision: OnCollision::Cancel,
            },
        );
        assert!(
            matches!(r, Err(MutateError::Collision(_))),
            "duplicate flow key expected Collision, got {r:?}"
        );
    }

    #[test]
    fn delete_flow_map_member_fixes_separator() {
        let out = apply_str(
            "pt: {x: 1, y: 2, z: 3}\n",
            Mutation::Delete {
                path: vec![Seg::Key("pt".into()), Seg::Key("y".into())],
            },
        )
        .expect("delete flow member");
        assert_eq!(out, "pt: {x: 1, z: 3}\n");
    }

    #[test]
    fn rename_flow_map_member() {
        let out = apply_str(
            "pt: {x: 1, y: 2}\n",
            Mutation::Rename {
                path: vec![Seg::Key("pt".into()), Seg::Key("x".into())],
                new_key: "w".into(),
            },
        )
        .expect("rename flow member");
        assert_eq!(out, "pt: {w: 1, y: 2}\n");
    }

    #[test]
    fn convert_flow_member_to_block_scalar_rejected() {
        let r = convert(
            "s: {a: hi}\n",
            vec![Seg::Key("s".into()), Seg::Key("a".into())],
            KindTarget::StringLiteralBlock,
        );
        assert!(
            matches!(r, Err(MutateError::Illegal(_))),
            "block scalar inside flow expected Illegal, got {r:?}"
        );
    }

    #[test]
    fn add_element_into_flow_seq() {
        use crate::model::document::{OnCollision, Target};
        let out = apply_str(
            "ls: [a, b]\n",
            Mutation::Insert {
                target: Target {
                    parent: vec![Seg::Key("ls".into())],
                    index: 2,
                },
                fragment: "c\n".into(),
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("insert into flow seq");
        assert_eq!(out, "ls: [a, b, c]\n");
    }

    #[test]
    fn insert_into_nested_block_mapping() {
        use crate::model::document::{OnCollision, Target};
        let src = "srv:\n  host: a\n";
        let out = apply_str(
            src,
            Mutation::Insert {
                target: Target {
                    parent: vec![Seg::Key("srv".into())],
                    index: 1,
                },
                fragment: "port: 80\n".into(),
                on_collision: OnCollision::Cancel,
            },
        )
        .expect("insert into nested block mapping");
        assert_eq!(out, "srv:\n  host: a\n  port: 80\n");
    }
}
