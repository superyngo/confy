//! Tests for the JSON/JSONC splice helpers — kept in a sibling file via
//! `#[path = "tests.rs"]`, the same shape `cst_edit`, `yaml/edit` and
//! `tui/app.rs` use (F6, 2026-09-09).

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
fn fragment_of_member_includes_trailing_comment() {
    let t = parse("{\n  \"a\": 1 // c\n}\n");
    assert_eq!(
        serialize_fragment(&t, &[Seg::Key("a".into())]),
        "\"a\": 1  // c"
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
    super::apply(&t, m).unwrap().1
}

#[test]
fn set_trailing_comment_add_change_clear_comma() {
    let set = |c: Option<&str>| Mutation::SetTrailingComment {
        path: vec![Seg::Key("a".into())],
        comment: c.map(str::to_string),
    };
    // add to the last member (no comma)
    assert_eq!(
        apply_str("{\n  \"a\": 1\n}\n", set(Some("// bind"))),
        "{\n  \"a\": 1  // bind\n}\n"
    );
    // add keeps a following comma before the comment
    assert_eq!(
        apply_str("{\n  \"a\": 1,\n  \"b\": 2\n}\n", set(Some("// bind"))),
        "{\n  \"a\": 1,  // bind\n  \"b\": 2\n}\n"
    );
    // change
    assert_eq!(
        apply_str("{\n  \"a\": 1 // old\n}\n", set(Some("// new"))),
        "{\n  \"a\": 1  // new\n}\n"
    );
    // clear
    assert_eq!(
        apply_str("{\n  \"a\": 1 // old\n}\n", set(None)),
        "{\n  \"a\": 1\n}\n"
    );
}

#[test]
fn replace_member_applies_edited_trailing_comment() {
    let out = apply_str(
        "{\n  \"a\": 1 // old\n}\n",
        Mutation::Replace {
            path: vec![Seg::Key("a".into())],
            fragment: "\"a\": 2  // new\n".into(),
        },
    );
    assert!(out.contains("// new"), "edited comment applied: {out}");
    assert!(!out.contains("// old"), "old comment replaced: {out}");
}

#[test]
fn replace_member_without_comment_keeps_old_comment() {
    let out = apply_str(
        "{\n  \"a\": 1 // old\n}\n",
        Mutation::Replace {
            path: vec![Seg::Key("a".into())],
            fragment: "\"a\": 2".into(),
        },
    );
    assert!(
        out.contains("// old"),
        "value-only edit keeps old comment: {out}"
    );
}

#[test]
fn replace_member_applies_edited_trailing_comment_no_source_newline() {
    // The popup editor / `$EDITOR` round-trip: `serialize_fragment` never
    // appends a trailing newline (see `fragment_of`/`with_comment`), so a
    // fragment with an edited `//` comment but no trailing `\n` must still
    // parse — the wrap helpers must not let the `//` comment swallow the
    // synthetic closing `}`.
    let out = apply_str(
        "{\n  \"a\": 1 // old\n}\n",
        Mutation::Replace {
            path: vec![Seg::Key("a".into())],
            fragment: "\"a\": 2  // new".into(),
        },
    );
    assert!(out.contains("// new"), "edited comment applied: {out}");
    assert!(!out.contains("// old"), "old comment replaced: {out}");
}

#[test]
fn set_trailing_comment_on_object_branch() {
    // A branch (object member) carries its trailing comment after the closing
    // brace; the splice keeps a following comma, just like a scalar member.
    let set = |c: Option<&str>| Mutation::SetTrailingComment {
        path: vec![Seg::Key("srv".into())],
        comment: c.map(str::to_string),
    };
    // add to a non-final object member (comma preserved)
    assert_eq!(
        apply_str(
            "{\n  \"srv\": {\n    \"x\": 1\n  },\n  \"z\": 2\n}\n",
            set(Some("// the server"))
        ),
        "{\n  \"srv\": {\n    \"x\": 1\n  },  // the server\n  \"z\": 2\n}\n"
    );
    // clear it again
    assert_eq!(
        apply_str(
            "{\n  \"srv\": {\n    \"x\": 1\n  },  // the server\n  \"z\": 2\n}\n",
            set(None)
        ),
        "{\n  \"srv\": {\n    \"x\": 1\n  },\n  \"z\": 2\n}\n"
    );
}

#[test]
fn edit_multiline_comment_block() {
    // A merged multi-line `//` block (one node, one slot) edits via EditComment
    // without a "path not found" — item-space matches the projection slot-space.
    let out = apply_str(
        "{\n  // l1\n  // l2\n  \"a\": 1\n}\n",
        Mutation::EditComment {
            path: vec![Seg::Index(0)],
            text: "// edited 1\n// edited 2".into(),
        },
    );
    assert_eq!(out, "{\n  // edited 1\n  // edited 2\n  \"a\": 1\n}\n");
}

#[test]
fn insert_after_multiline_comment_no_offset() {
    // A 3-line leading `//` block is ONE slot, so inserting at projected index
    // (after both members) lands at the end — not shifted up by the comment lines.
    let out = apply_str(
        "{\n  // c1\n  // c2\n  // c3\n  \"a\": 1,\n  \"b\": 2\n}\n",
        Mutation::Insert {
            target: MTarget {
                parent: vec![],
                index: 3, // [comment@0, a@1, b@2] → after b
            },
            fragment: "\"c\": 3".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        },
    );
    assert_eq!(
        out,
        "{\n  // c1\n  // c2\n  // c3\n  \"a\": 1,\n  \"b\": 2,\n  \"c\": 3\n}\n"
    );
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
    assert!(matches!(
        r,
        Err(MutateError::Fragment(_)) | Err(MutateError::Illegal(_))
    ));
}

#[test]
fn stubbed_mutations_unsupported() {
    let t = parse("{ \"a\": 1 }\n");
    // ConvertKind TableInline on an integer value -> not an object -> Unsupported.
    let r = apply(
        &t,
        Mutation::ConvertKind {
            path: vec![Seg::Key("a".into())],
            target: crate::model::document::KindTarget::TableInline,
        },
    );
    assert!(matches!(r, Err(MutateError::Unsupported)));
}

#[test]
fn delete_middle_member() {
    let out = apply_str(
        "{\n  \"a\": 1,\n  \"b\": 2,\n  \"c\": 3\n}\n",
        Mutation::Delete {
            path: vec![Seg::Key("b".into())],
        },
    );
    assert_eq!(out, "{\n  \"a\": 1,\n  \"c\": 3\n}\n");
}

#[test]
fn delete_last_member_fixes_comma() {
    let out = apply_str(
        "{\n  \"a\": 1,\n  \"b\": 2\n}\n",
        Mutation::Delete {
            path: vec![Seg::Key("b".into())],
        },
    );
    assert_eq!(out, "{\n  \"a\": 1\n}\n");
}

#[test]
fn delete_only_member() {
    let out = apply_str(
        "{\n  \"a\": 1\n}\n",
        Mutation::Delete {
            path: vec![Seg::Key("a".into())],
        },
    );
    // The splice removes NEWLINE + WHITESPACE before "a" and the MEMBER node.
    // No comma exists, so the result is "{\n}" — valid JSON.
    assert_eq!(out, "{\n}\n");
}

#[test]
fn delete_middle_element() {
    let out = apply_str(
        "[1, 2, 3]\n",
        Mutation::Delete {
            path: vec![Seg::Index(1)],
        },
    );
    assert_eq!(out, "[1, 3]\n");
}

#[test]
fn delete_last_element() {
    let out = apply_str(
        "[1, 2]\n",
        Mutation::Delete {
            path: vec![Seg::Index(1)],
        },
    );
    assert_eq!(out, "[1]\n");
}

#[test]
fn delete_comment() {
    let out = apply_str(
        "{\n  // gone\n  \"a\": 1\n}\n",
        Mutation::Delete {
            path: vec![Seg::Index(0)],
        },
    );
    assert_eq!(out, "{\n  \"a\": 1\n}\n");
}

use crate::model::document::{OnCollision, Target as MTarget};

#[test]
fn insert_member_into_object() {
    let out = apply_str(
        "{\n  \"a\": 1\n}\n",
        Mutation::Insert {
            target: MTarget {
                parent: vec![],
                index: 1,
            },
            fragment: "\"b\": 2".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        },
    );
    assert_eq!(out, "{\n  \"a\": 1,\n  \"b\": 2\n}\n");
}

#[test]
fn insert_member_at_front() {
    let out = apply_str(
        "{\n  \"a\": 1\n}\n",
        Mutation::Insert {
            target: MTarget {
                parent: vec![],
                index: 0,
            },
            fragment: "\"b\": 2".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        },
    );
    assert_eq!(out, "{\n  \"b\": 2,\n  \"a\": 1\n}\n");
}

#[test]
fn insert_element_into_array() {
    let out = apply_str(
        "[1, 2]\n",
        Mutation::Insert {
            target: MTarget {
                parent: vec![],
                index: 2,
            },
            fragment: "3".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        },
    );
    assert_eq!(out, "[1, 2, 3]\n");
}

#[test]
fn insert_keyed_into_array_wraps() {
    let out = apply_str(
        "[1]\n",
        Mutation::Insert {
            target: MTarget {
                parent: vec![],
                index: 1,
            },
            fragment: "\"k\": 2".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        },
    );
    assert_eq!(out, "[1, { \"k\": 2 }]\n");
}

#[test]
fn insert_bare_into_object_placeholder() {
    let out = apply_str(
        "{\n  \"a\": 1\n}\n",
        Mutation::Insert {
            target: MTarget {
                parent: vec![],
                index: 1,
            },
            fragment: "42".into(),
            on_collision: OnCollision::Rename,
            suggested_key: None,
        },
    );
    assert_eq!(out, "{\n  \"a\": 1,\n  \"placeholder\": 42\n}\n");
}

#[test]
fn insert_bare_with_suggested_key() {
    // Copy-paste path: the caller knows the scalar came from `arr` index 1
    // and passes the suggestion explicitly; insert() prefers it over the
    // generic "placeholder".
    let out = apply_str(
        "{\n  \"a\": 1\n}\n",
        Mutation::Insert {
            target: MTarget {
                parent: vec![],
                index: 1,
            },
            fragment: "20".into(),
            on_collision: OnCollision::Rename,
            suggested_key: Some("arr_1".into()),
        },
    );
    assert_eq!(out, "{\n  \"a\": 1,\n  \"arr_1\": 20\n}\n");
}

#[test]
fn move_bare_array_element_to_object_suggested_key() {
    // A bare scalar moved out of a keyed array into an object needs a member
    // key synthesized: `<arrayKey>_<index>` instead of "placeholder".
    let out = apply_str(
        "{\n  \"arr\": [10, 20, 30],\n  \"o\": {}\n}\n",
        Mutation::Move {
            sources: vec![vec![Seg::Key("arr".into()), Seg::Index(1)]],
            target: MTarget {
                parent: vec![Seg::Key("o".into())],
                index: 0,
            },
            on_collision: OnCollision::Rename,
        },
    );
    assert_eq!(
        out,
        "{\n  \"arr\": [10, 30],\n  \"o\": { \"arr_1\": 20 }\n}\n"
    );
}

#[test]
fn move_bare_array_element_suggested_key_collision_renames() {
    // Synthesized key colliding with an existing member follows the same
    // Rename policy as "placeholder": arr_1 → arr_1_2.
    let out = apply_str(
        "{\n  \"arr\": [10, 20],\n  \"o\": { \"arr_1\": 99 }\n}\n",
        Mutation::Move {
            sources: vec![vec![Seg::Key("arr".into()), Seg::Index(1)]],
            target: MTarget {
                parent: vec![Seg::Key("o".into())],
                index: 1,
            },
            on_collision: OnCollision::Rename,
        },
    );
    assert_eq!(
        out,
        "{\n  \"arr\": [10],\n  \"o\": { \"arr_1\": 99, \"arr_1_2\": 20 }\n}\n"
    );
}

#[test]
fn move_nested_unkeyed_array_element_still_placeholder() {
    // An element of a nested array (`matrix[0][1]`) has no Key segment
    // before the final Index, so no suggestion exists and the generic
    // placeholder key is kept.
    let out = apply_str(
        "{\n  \"matrix\": [[1, 2], [3, 4]],\n  \"o\": {}\n}\n",
        Mutation::Move {
            sources: vec![vec![
                Seg::Key("matrix".into()),
                Seg::Index(0),
                Seg::Index(1),
            ]],
            target: MTarget {
                parent: vec![Seg::Key("o".into())],
                index: 0,
            },
            on_collision: OnCollision::Rename,
        },
    );
    assert_eq!(
        out,
        "{\n  \"matrix\": [[1], [3, 4]],\n  \"o\": { \"placeholder\": 2 }\n}\n"
    );
}

#[test]
fn move_root_unkeyed_array_element_still_placeholder() {
    // Same fallback at document root: a scalar lifted from a bare top-level
    // array into the root array's object element has no keyed parent.
    let out = apply_str(
        "[[1, 2], { \"a\": 1 }]\n",
        Mutation::Move {
            sources: vec![vec![Seg::Index(0), Seg::Index(1)]],
            target: MTarget {
                parent: vec![Seg::Index(1)],
                index: 1,
            },
            on_collision: OnCollision::Rename,
        },
    );
    assert_eq!(out, "[[1], { \"a\": 1, \"placeholder\": 2 }]\n");
}

#[test]
fn move_bare_array_element_to_array_stays_bare() {
    // Regression: array → array moves keep the element bare — no key
    // synthesized, no `{ ... }` wrapping; suggested_key is never consulted
    // for array destinations.
    let out = apply_str(
        "{\n  \"src\": [1, 2],\n  \"dst\": []\n}\n",
        Mutation::Move {
            sources: vec![vec![Seg::Key("src".into()), Seg::Index(1)]],
            target: MTarget {
                parent: vec![Seg::Key("dst".into())],
                index: 0,
            },
            on_collision: OnCollision::Rename,
        },
    );
    assert_eq!(out, "{\n  \"src\": [1],\n  \"dst\": [2]\n}\n");
}

#[test]
fn move_object_array_element_to_object_value_kept_intact() {
    // Regression: an object element moved into an object is a bare *value*
    // fragment (not a member), so it nests under the synthesized key with
    // its members kept intact — now named after the source array + index.
    let out = apply_str(
        "{\n  \"src\": [{ \"a\": 1 }],\n  \"dst\": {}\n}\n",
        Mutation::Move {
            sources: vec![vec![Seg::Key("src".into()), Seg::Index(0)]],
            target: MTarget {
                parent: vec![Seg::Key("dst".into())],
                index: 0,
            },
            on_collision: OnCollision::Rename,
        },
    );
    assert_eq!(
        out,
        "{\n  \"src\": [],\n  \"dst\": { \"src_0\": { \"a\": 1 } }\n}\n"
    );
}

#[test]
fn insert_collision_cancels() {
    let t = parse("{ \"a\": 1 }\n");
    let r = super::apply(
        &t,
        Mutation::Insert {
            target: MTarget {
                parent: vec![],
                index: 1,
            },
            fragment: "\"a\": 2".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        },
    );
    assert!(matches!(r, Err(MutateError::Collision(_))));
}

#[test]
fn insert_into_nested_multiline_object() {
    let out = apply_str(
        "{\n  \"o\": {\n    \"a\": 1\n  }\n}\n",
        Mutation::Insert {
            target: MTarget {
                parent: vec![Seg::Key("o".into())],
                index: 1,
            },
            fragment: "\"b\": 2".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        },
    );
    assert_eq!(out, "{\n  \"o\": {\n    \"a\": 1,\n    \"b\": 2\n  }\n}\n");
}

#[test]
fn rename_member_key() {
    let out = apply_str(
        "{ \"a\": 1 }\n",
        Mutation::Rename {
            path: vec![Seg::Key("a".into())],
            new_key: "b".into(),
        },
    );
    assert_eq!(out, "{ \"b\": 1 }\n");
}

#[test]
fn rename_collision() {
    let t = parse("{ \"a\": 1, \"b\": 2 }\n");
    let r = super::apply(
        &t,
        Mutation::Rename {
            path: vec![Seg::Key("a".into())],
            new_key: "b".into(),
        },
    );
    assert!(matches!(r, Err(MutateError::Collision(_))));
}

#[test]
fn insert_member_into_empty_document() {
    let out = apply_str(
        "",
        Mutation::Insert {
            target: MTarget {
                parent: vec![],
                index: 0,
            },
            fragment: "\"a\": 1".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        },
    );
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v, serde_json::json!({ "a": 1 }));
}

#[test]
fn insert_member_into_comment_only_document() {
    let out = apply_str(
        "// just a comment\n",
        Mutation::Insert {
            target: MTarget {
                parent: vec![],
                index: 0,
            },
            fragment: "\"a\": 1".into(),
            on_collision: OnCollision::Cancel,
            suggested_key: None,
        },
    );
    // The whole-document splice used to synthesize the root replaces ALL
    // existing ROOT children, so the standalone leading comment is
    // dropped — not part of the audited drift or the product decision to
    // fix empty-document insert, so this documents actual behavior
    // rather than attempting to also preserve the comment.
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v, serde_json::json!({ "a": 1 }));
    assert!(!out.contains("just a comment"));
}

#[test]
fn rename_with_internal_quote_roundtrips() {
    let out = apply_str(
        "{ \"a\": 1 }\n",
        Mutation::Rename {
            path: vec![Seg::Key("a".into())],
            new_key: "foo\"bar".into(),
        },
    );
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert!(
        v.get("foo\"bar").is_some(),
        "expected decoded key foo\"bar in {v:?}"
    );
}

#[test]
fn rename_collision_compares_decoded_keys() {
    // new_key arrives pre-quoted (e.g. round-tripped from Node::key_literal);
    // its DECODED form ("b") collides with the existing sibling "b" even
    // though the raw strings "b" and "\"b\"" differ — this is exactly the
    // raw-vs-decoded mismatch the fix addresses.
    let t = parse("{ \"a\": 1, \"b\": 2 }\n");
    let r = super::apply(
        &t,
        Mutation::Rename {
            path: vec![Seg::Key("a".into())],
            new_key: "\"b\"".into(),
        },
    );
    assert!(matches!(r, Err(MutateError::Collision(_))));
}

#[test]
fn remark_member_to_comment() {
    let out = apply_str(
        "{\n  \"a\": 1\n}\n",
        Mutation::Remark {
            path: vec![Seg::Key("a".into())],
        },
    );
    assert_eq!(out, "{\n  // \"a\": 1\n}\n");
}

#[test]
fn remark_member_keeps_trailing_comment() {
    let out = apply_str(
        "{\n  \"a\": 1, // t\n  \"b\": 2\n}\n",
        Mutation::Remark {
            path: vec![Seg::Key("a".into())],
        },
    );
    assert_eq!(out, "{\n  // \"a\": 1  // t\n  \"b\": 2\n}\n");
}

#[test]
fn remark_comment_with_trailing_restores_member() {
    let out = apply_str(
        "{\n  // \"a\": 1  // t\n  \"b\": 2\n}\n",
        Mutation::Remark {
            path: vec![Seg::Index(0)],
        },
    );
    assert_eq!(out, "{\n  \"a\": 1,  // t\n  \"b\": 2\n}\n");
}

#[test]
fn remark_multiline_member_with_trailing_roundtrips() {
    let out = apply_str(
        "{\n  \"a\": {\n    \"x\": 1\n  }, // t\n  \"b\": 2\n}\n",
        Mutation::Remark {
            path: vec![Seg::Key("a".into())],
        },
    );
    assert_eq!(
        out,
        "{\n  // \"a\": {\n  //     \"x\": 1\n  //   }  // t\n  \"b\": 2\n}\n"
    );
    // …and back: comma re-forms before the restored trailing comment.
    let back = apply_str(
        &out,
        Mutation::Remark {
            path: vec![Seg::Index(0)],
        },
    );
    assert_eq!(
        back,
        "{\n  \"a\": {\n    \"x\": 1\n  },  // t\n  \"b\": 2\n}\n"
    );
}

#[test]
fn remark_two_members_merge_then_uncomment_restores_both() {
    // Remarking consecutive members merges their `//` lines into ONE
    // Comment node; un-remarking that node must restore BOTH members.
    let out = apply_str(
        "{\n  \"a\": 1,\n  \"b\": 2,\n  \"c\": 3\n}\n",
        Mutation::Remark {
            path: vec![Seg::Key("a".into())],
        },
    );
    assert_eq!(out, "{\n  // \"a\": 1\n  \"b\": 2,\n  \"c\": 3\n}\n");
    let out = apply_str(
        &out,
        Mutation::Remark {
            path: vec![Seg::Key("b".into())],
        },
    );
    assert_eq!(out, "{\n  // \"a\": 1\n  // \"b\": 2\n  \"c\": 3\n}\n");
    let back = apply_str(
        &out,
        Mutation::Remark {
            path: vec![Seg::Index(0)],
        },
    );
    assert_eq!(back, "{\n  \"a\": 1,\n  \"b\": 2,\n  \"c\": 3\n}\n");
}

#[test]
fn remark_multiline_member_and_member_merge_then_uncomment() {
    // A multi-line member spans several `//` lines; the greedy reverse
    // split must reassemble it before restoring, then restore the next
    // single-line member as its own item.
    let out = apply_str(
        "{\n  \"a\": {\n    \"x\": 1\n  },\n  \"b\": 2,\n  \"c\": 3\n}\n",
        Mutation::Remark {
            path: vec![Seg::Key("a".into())],
        },
    );
    assert_eq!(
        out,
        "{\n  // \"a\": {\n  //     \"x\": 1\n  //   }\n  \"b\": 2,\n  \"c\": 3\n}\n"
    );
    let out = apply_str(
        &out,
        Mutation::Remark {
            path: vec![Seg::Key("b".into())],
        },
    );
    assert_eq!(
        out,
        "{\n  // \"a\": {\n  //     \"x\": 1\n  //   }\n  // \"b\": 2\n  \"c\": 3\n}\n"
    );
    let back = apply_str(
        &out,
        Mutation::Remark {
            path: vec![Seg::Index(0)],
        },
    );
    assert_eq!(
        back,
        "{\n  \"a\": {\n    \"x\": 1\n  },\n  \"b\": 2,\n  \"c\": 3\n}\n"
    );
}

#[test]
fn remark_comment_to_member() {
    let out = apply_str(
        "{\n  // \"a\": 1\n}\n",
        Mutation::Remark {
            path: vec![Seg::Index(0)],
        },
    );
    assert_eq!(out, "{\n  \"a\": 1\n}\n");
}

#[test]
fn edit_comment_text() {
    let out = apply_str(
        "{\n  // old\n  \"a\": 1\n}\n",
        Mutation::EditComment {
            path: vec![Seg::Index(0)],
            text: "// new".into(),
        },
    );
    assert_eq!(out, "{\n  // new\n  \"a\": 1\n}\n");
}

#[test]
fn edit_comment_duplicate_text_edits_second_block() {
    // Two identical comment blocks: editing the SECOND must not touch the first.
    let out = apply_str(
        "{\n  // dup\n  \"a\": 1,\n  // dup\n  \"b\": 2\n}\n",
        Mutation::EditComment {
            path: vec![Seg::Index(2)],
            text: "// new".into(),
        },
    );
    assert_eq!(out, "{\n  // dup\n  \"a\": 1,\n  // new\n  \"b\": 2\n}\n");
}

#[test]
fn edit_comment_rejects_non_comment() {
    let t = parse("{\n  // old\n  \"a\": 1\n}\n");
    let r = super::apply(
        &t,
        Mutation::EditComment {
            path: vec![Seg::Index(0)],
            text: "not a comment".into(),
        },
    );
    assert!(matches!(r, Err(MutateError::Fragment(_))));
}

#[test]
fn move_member_within_object() {
    let out = apply_str(
        "{\n  \"a\": 1,\n  \"b\": 2\n}\n",
        Mutation::Move {
            sources: vec![vec![Seg::Key("a".into())]],
            target: crate::model::document::Target {
                parent: vec![],
                index: 2,
            },
            on_collision: OnCollision::Cancel,
        },
    );
    assert_eq!(out, "{\n  \"b\": 2,\n  \"a\": 1\n}\n");
}

#[test]
fn move_member_down_before_trailing_comment() {
    // `a` moved to just after `b` must land BEFORE the trailing `// c`
    // comment (slot index 2), not after it.
    let out = apply_str(
        "{\n  \"a\": 1,\n  \"b\": 2,\n  // c\n}\n",
        Mutation::Move {
            sources: vec![vec![Seg::Key("a".into())]],
            target: crate::model::document::Target {
                parent: vec![],
                index: 2,
            },
            on_collision: OnCollision::Cancel,
        },
    );
    // `a` is now the last member, so the splice drops its trailing comma
    // (project rule: never emit trailing commas); it still lands before `// c`.
    assert_eq!(out, "{\n  \"b\": 2,\n  \"a\": 1\n  // c\n}\n");
}

#[test]
fn move_member_into_nested_array_wraps() {
    let out = apply_str(
        "{\n  \"a\": 1,\n  \"arr\": []\n}\n",
        Mutation::Move {
            sources: vec![vec![Seg::Key("a".into())]],
            target: crate::model::document::Target {
                parent: vec![Seg::Key("arr".into())],
                index: 0,
            },
            on_collision: OnCollision::Cancel,
        },
    );
    assert_eq!(out, "{\n  \"arr\": [{ \"a\": 1 }]\n}\n");
}

#[test]
fn insert_comment_block() {
    let out = apply_str(
        "{\n  \"a\": 1\n}\n",
        Mutation::InsertComment {
            target: crate::model::document::Target {
                parent: vec![],
                index: 0,
            },
            text: "// note".into(),
        },
    );
    assert_eq!(out, "{\n  // note\n  \"a\": 1\n}\n");
}

use crate::model::document::KindTarget;

#[test]
fn array_multiline_to_inline() {
    let out = apply_str(
        "{\n  \"a\": [\n    1,\n    2\n  ]\n}\n",
        Mutation::ConvertKind {
            path: vec![Seg::Key("a".into())],
            target: KindTarget::ArrayInline,
        },
    );
    assert_eq!(out, "{\n  \"a\": [1, 2]\n}\n");
}

#[test]
fn array_inline_to_multiline() {
    let out = apply_str(
        "{\n  \"a\": [1, 2]\n}\n",
        Mutation::ConvertKind {
            path: vec![Seg::Key("a".into())],
            target: KindTarget::ArrayMultiline,
        },
    );
    // "a" member is on a line with 2-space indent, so items at 4 spaces, close at 2 spaces.
    assert_eq!(out, "{\n  \"a\": [\n    1,\n    2\n  ]\n}\n");
}

#[test]
fn float_plain_to_exponent() {
    let out = apply_str(
        "{ \"f\": 1500.0 }\n",
        Mutation::ConvertKind {
            path: vec![Seg::Key("f".into())],
            target: KindTarget::FloatExponent,
        },
    );
    assert_eq!(out, "{ \"f\": 1.5e3 }\n");
}

#[test]
fn float_exponent_to_plain() {
    let out = apply_str(
        "{ \"f\": 1.5e3 }\n",
        Mutation::ConvertKind {
            path: vec![Seg::Key("f".into())],
            target: KindTarget::FloatPlain,
        },
    );
    assert_eq!(out, "{ \"f\": 1500.0 }\n");
}

#[test]
fn inline_collapse_rejects_comment() {
    let t = parse("{\n  \"a\": [\n    1, // c\n    2\n  ]\n}\n");
    let r = super::apply(
        &t,
        Mutation::ConvertKind {
            path: vec![Seg::Key("a".into())],
            target: KindTarget::ArrayInline,
        },
    );
    assert!(matches!(r, Err(MutateError::Illegal(_))));
}

#[test]
fn inline_collapse_allows_string_value_containing_comment_chars() {
    // Regression: the comment guard text-scanned the source, so a string
    // VALUE containing "//" or "/*" wrongly blocked the collapse.
    let out = apply_str(
        "{\n  \"o\": {\n    \"a\": \"// not a comment\",\n    \"b\": \"/* nor this */\"\n  }\n}\n",
        Mutation::ConvertKind {
            path: vec![Seg::Key("o".into())],
            target: KindTarget::TableInline,
        },
    );
    assert_eq!(
        out,
        "{\n  \"o\": {\"a\": \"// not a comment\", \"b\": \"/* nor this */\"}\n}\n"
    );
}

#[test]
fn object_inline_to_multiline() {
    let out = apply_str(
        "{ \"o\": { \"a\": 1, \"b\": 2 } }\n",
        Mutation::ConvertKind {
            path: vec![Seg::Key("o".into())],
            target: KindTarget::TableMultiline,
        },
    );
    // "o" member has no own-line indent (inline ancestor) — fallback: items at 2 spaces, close at 0.
    assert_eq!(out, "{ \"o\": {\n  \"a\": 1,\n  \"b\": 2\n} }\n");
}

#[test]
fn convert_string_unsupported() {
    let t = parse("{ \"s\": \"x\" }\n");
    let r = super::apply(
        &t,
        Mutation::ConvertKind {
            path: vec![Seg::Key("s".into())],
            target: KindTarget::StringBasic,
        },
    );
    assert!(matches!(r, Err(MutateError::Unsupported)));
}

#[test]
fn set_trailing_blank_lines_on_a_member_and_an_object() {
    let set = |src: &str, path: Vec<Seg>, n: usize| {
        apply_str(src, Mutation::SetTrailingBlankLines { path, n })
    };
    assert_eq!(
        set(
            "{\n  \"a\": 1,\n  \"b\": 2\n}\n",
            vec![Seg::Key("a".into())],
            1
        ),
        "{\n  \"a\": 1,\n\n  \"b\": 2\n}\n"
    );
    // Clearing an existing run.
    assert_eq!(
        set(
            "{\n  \"a\": 1,\n\n\n  \"b\": 2\n}\n",
            vec![Seg::Key("a".into())],
            0
        ),
        "{\n  \"a\": 1,\n  \"b\": 2\n}\n"
    );
    // A nested object's run sits after its closing brace.
    assert_eq!(
        set(
            "{\n  \"o\": {\n    \"x\": 1\n  },\n  \"b\": 2\n}\n",
            vec![Seg::Key("o".into())],
            1
        ),
        "{\n  \"o\": {\n    \"x\": 1\n  },\n\n  \"b\": 2\n}\n"
    );
}

#[test]
fn set_trailing_blank_lines_keeps_the_separator_comma() {
    // The comma belongs to the member's line, so it must stay *before* the
    // blank run — a comma stranded after a blank line is still legal JSON
    // but is not what the user asked for.
    let out = apply_str(
        "{\n  \"a\": 1,\n  \"b\": 2\n}\n",
        Mutation::SetTrailingBlankLines {
            path: vec![Seg::Key("a".into())],
            n: 1,
        },
    );
    assert!(out.contains("\"a\": 1,\n\n"), "{out}");
}
