//! Cross-format parity: one gesture, three backends, one outcome.
//!
//! `docs/reference/BEHAVIOR_MATRIX.md`'s premise is that confy has **one
//! model for three formats**. Nothing mechanically enforced that: the older
//! multi-format tests each loop over two formats and handle the third in a
//! block below the loop, which is exactly the shape drift hides in.
//!
//! Every test here runs the *same* gesture against **all three** `DocFormat`s
//! and asserts they agree. The per-format fixtures are chosen by an
//! **exhaustive `match`**, so adding a fourth backend fails to compile until
//! its expectation is written down — the enforcement is the point.
//!
//! Notation, not semantics, is what differs between formats. So a parity case
//! states the *outcome* (this succeeds and round-trips / this is `Unsupported`)
//! and lets each format spell it its own way.

use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::{ConfigDocument, DocFormat, MutateError, Mutation};
use confy_core::model::node::{NodeKind, Seg};

const ALL: [DocFormat; 3] = [DocFormat::Toml, DocFormat::Json, DocFormat::Yaml];

fn name(f: DocFormat) -> &'static str {
    match f {
        DocFormat::Toml => "toml",
        DocFormat::Json => "json",

        DocFormat::Yaml => "yaml",
    }
}

fn doc(f: DocFormat, src: &str) -> AnyDocument {
    AnyDocument::from_str_as(src, f).unwrap_or_else(|e| panic!("{} parse: {e:?}", name(f)))
}

/// The path of the first `Comment` node anywhere in the tree, or `None`.
fn first_comment(d: &AnyDocument) -> Option<Vec<Seg>> {
    fn walk(n: &confy_core::model::node::Node, prefix: Vec<Seg>) -> Option<Vec<Seg>> {
        for (i, c) in n.children.iter().enumerate() {
            let mut p = prefix.clone();
            // A comment is positional in every backend — addressed by its
            // slot in the parent's full child sequence, never by a key (TOML
            // parks the comment text in `key`, which is display only).
            if matches!(c.kind, NodeKind::Comment(_)) {
                p.push(Seg::Index(i));
                return Some(p);
            }
            if c.key.is_empty() {
                p.push(Seg::Index(i));
            } else {
                p.push(Seg::Key(c.key.clone()));
            }
            if let Some(found) = walk(c, p) {
                return Some(found);
            }
        }
        None
    }
    walk(&d.project().root, vec![])
}

// ---------------------------------------------------------------------------
// Remark
// ---------------------------------------------------------------------------

/// A node that owns its own line can be remarked, and un-remarking restores
/// the source byte-for-byte. Seeded by the three-way split this suite exists
/// to prevent: TOML said `Unsupported`, JSON said `Illegal`, YAML said `Ok`.
#[test]
fn remark_an_array_element_round_trips_in_every_format() {
    for f in ALL {
        let src = match f {
            DocFormat::Toml => "a = [\n  1,\n  2,\n]\n",
            DocFormat::Json => "{\n  \"a\": [\n    1,\n    2\n  ]\n}\n",
            DocFormat::Yaml => "a:\n  - 1\n  - 2\n",
        };
        let mut d = doc(f, src);
        d.apply(Mutation::Remark {
            path: vec![Seg::Key("a".into()), Seg::Index(1)],
        })
        .unwrap_or_else(|e| panic!("{}: remark rejected: {e:?}", name(f)));

        let commented = d.serialize();
        assert_ne!(commented, src, "{}: remark changed nothing", name(f));
        let marker = match f {
            DocFormat::Json => "//",
            _ => "#",
        };
        assert!(
            commented.contains(marker),
            "{}: no comment leader in {commented:?}",
            name(f)
        );

        let back = first_comment(&d)
            .unwrap_or_else(|| panic!("{}: remark produced no Comment node", name(f)));
        d.apply(Mutation::Remark { path: back })
            .unwrap_or_else(|e| panic!("{}: un-remark rejected: {e:?}", name(f)));
        assert_eq!(d.serialize(), src, "{}: un-remark lost bytes", name(f));
    }
}

/// A member of a **single-line** collection has no line of its own, so a
/// comment leader would swallow its siblings. Every backend rejects it with
/// the same variant — `Unsupported`, not `Illegal` (a rule violation) and not
/// `NotFound` (the node is perfectly addressable, see the next test).
#[test]
fn remark_inside_a_single_line_collection_is_unsupported_in_every_format() {
    for f in ALL {
        let cases: &[(&str, Vec<Seg>)] = &match f {
            DocFormat::Toml => [
                ("a = [1, 2]\n", vec![Seg::Key("a".into()), Seg::Index(1)]),
                (
                    "a = { x = 1, y = 2 }\n",
                    vec![Seg::Key("a".into()), Seg::Key("y".into())],
                ),
            ],
            DocFormat::Json => [
                (
                    "{\n  \"a\": [1, 2]\n}\n",
                    vec![Seg::Key("a".into()), Seg::Index(1)],
                ),
                (
                    "{\n  \"a\": {\"x\": 1, \"y\": 2}\n}\n",
                    vec![Seg::Key("a".into()), Seg::Key("y".into())],
                ),
            ],
            DocFormat::Yaml => [
                ("a: [1, 2]\n", vec![Seg::Key("a".into()), Seg::Index(1)]),
                (
                    "a: {x: 1, y: 2}\n",
                    vec![Seg::Key("a".into()), Seg::Key("y".into())],
                ),
            ],
        };
        for (src, path) in cases {
            let mut d = doc(f, src);
            let err = d
                .apply(Mutation::Remark { path: path.clone() })
                .unwrap_err();
            assert!(
                matches!(err, MutateError::Unsupported),
                "{}: {src:?} gave {err:?}, expected Unsupported",
                name(f)
            );
            assert_eq!(
                d.serialize(),
                *src,
                "{}: rejected edit still wrote",
                name(f)
            );
        }
    }
}

/// The companion to the rule above: a rejected *gesture* must not be reported
/// as a missing *node*. Every path the previous test rejects is reachable by
/// another mutation, which is what makes `NotFound` the wrong answer there.
#[test]
fn a_path_rejected_by_remark_is_still_addressable_in_every_format() {
    for f in ALL {
        let (src, path) = match f {
            DocFormat::Toml => ("a = [1, 2]\n", vec![Seg::Key("a".into()), Seg::Index(1)]),
            DocFormat::Json => (
                "{\n  \"a\": [1, 2]\n}\n",
                vec![Seg::Key("a".into()), Seg::Index(1)],
            ),
            DocFormat::Yaml => ("a: [1, 2]\n", vec![Seg::Key("a".into()), Seg::Index(1)]),
        };
        let mut d = doc(f, src);
        d.apply(Mutation::Delete { path })
            .unwrap_or_else(|e| panic!("{}: delete rejected: {e:?}", name(f)));
        assert!(
            !d.serialize().contains('2'),
            "{}: element survived delete",
            name(f)
        );
    }
}

// ---------------------------------------------------------------------------
// The other core gestures
// ---------------------------------------------------------------------------

/// Deleting one member of a multi-member map leaves the siblings intact and
/// the document parseable.
#[test]
fn delete_a_map_member_in_every_format() {
    for f in ALL {
        let src = match f {
            DocFormat::Toml => "x = 1\ny = 2\n",
            DocFormat::Json => "{\n  \"x\": 1,\n  \"y\": 2\n}\n",
            DocFormat::Yaml => "x: 1\ny: 2\n",
        };
        let mut d = doc(f, src);
        d.apply(Mutation::Delete {
            path: vec![Seg::Key("y".into())],
        })
        .unwrap_or_else(|e| panic!("{}: {e:?}", name(f)));
        let out = d.serialize();
        assert!(out.contains('x'), "{}: sibling lost: {out:?}", name(f));
        assert!(!out.contains('y'), "{}: member survived: {out:?}", name(f));
        doc(f, &out); // reparses
    }
}

/// Renaming a key keeps the value and the key's position.
#[test]
fn rename_a_key_in_every_format() {
    for f in ALL {
        let (src, new_key) = match f {
            DocFormat::Toml => ("x = 1\ny = 2\n", "z"),
            DocFormat::Json => ("{\n  \"x\": 1,\n  \"y\": 2\n}\n", "\"z\""),
            DocFormat::Yaml => ("x: 1\ny: 2\n", "z"),
        };
        let mut d = doc(f, src);
        d.apply(Mutation::Rename {
            path: vec![Seg::Key("x".into())],
            new_key: new_key.into(),
        })
        .unwrap_or_else(|e| panic!("{}: {e:?}", name(f)));
        let out = d.serialize();
        assert!(out.contains('z'), "{}: rename missing: {out:?}", name(f));
        assert!(out.contains('1'), "{}: value lost: {out:?}", name(f));
        let t = d.project();
        assert_eq!(t.root.children[0].key, "z", "{}: order moved", name(f));
    }
}

/// A value `Replace` swaps the value and nothing else.
#[test]
fn replace_a_scalar_value_in_every_format() {
    for f in ALL {
        let src = match f {
            DocFormat::Toml => "x = 1\n",
            DocFormat::Json => "{\n  \"x\": 1\n}\n",
            DocFormat::Yaml => "x: 1\n",
        };
        let mut d = doc(f, src);
        let fragment = d.scalar_fragment(Some("x"), "42");
        d.apply(Mutation::Replace {
            path: vec![Seg::Key("x".into())],
            fragment,
        })
        .unwrap_or_else(|e| panic!("{}: {e:?}", name(f)));
        let out = d.serialize();
        assert!(out.contains("42"), "{}: not replaced: {out:?}", name(f));
        assert!(!out.contains('1'), "{}: old value left: {out:?}", name(f));
    }
}

/// A trailing comment is decoration on the value, settable and clearable, and
/// clearing it leaves the source as it began.
#[test]
fn set_and_clear_a_trailing_comment_in_every_format() {
    for f in ALL {
        let src = match f {
            DocFormat::Toml => "x = 1\n",
            DocFormat::Json => "{\n  \"x\": 1\n}\n",
            DocFormat::Yaml => "x: 1\n",
        };
        let mut d = doc(f, src);
        let path = vec![Seg::Key("x".into())];
        // The comment carries its own leader, and each backend spells it
        // differently — that is exactly what `comment_prefix()` is for.
        let note = format!("{} note", d.comment_prefix());
        d.apply(Mutation::SetTrailingComment {
            path: path.clone(),
            comment: Some(note),
        })
        .unwrap_or_else(|e| panic!("{}: set: {e:?}", name(f)));
        assert!(
            d.serialize().contains("note"),
            "{}: comment missing: {:?}",
            name(f),
            d.serialize()
        );
        assert!(
            d.project().root.children[0].trailing_comment.is_some(),
            "{}: not projected",
            name(f)
        );
        d.apply(Mutation::SetTrailingComment {
            path,
            comment: None,
        })
        .unwrap_or_else(|e| panic!("{}: clear: {e:?}", name(f)));
        assert_eq!(d.serialize(), src, "{}: clear left residue", name(f));
    }
}

/// An unmodified document serializes byte-identically — the lossless-CST
/// promise, asserted for all three backends in one place.
#[test]
fn an_untouched_document_round_trips_in_every_format() {
    for f in ALL {
        let src = match f {
            DocFormat::Toml => "# lead\nx = 1  # eol\n\n[t]\ny = [\n  1,\n  # why\n  2,\n]\n",
            DocFormat::Json => {
                "{\n  // lead\n  \"x\": 1, // eol\n  \"t\": {\n    \"y\": [\n      1,\n      // why\n      2\n    ]\n  }\n}\n"
            }
            DocFormat::Yaml => "# lead\nx: 1  # eol\n\nt:\n  y:\n    - 1\n    # why\n    - 2\n",
        };
        assert_eq!(doc(f, src).serialize(), src, "{}: not lossless", name(f));
    }
}

/// A failed mutation leaves the document untouched — the atomic-commit
/// promise. Each backend is handed a fragment its own parser rejects.
#[test]
fn a_rejected_mutation_changes_nothing_in_every_format() {
    for f in ALL {
        let src = match f {
            DocFormat::Toml => "x = 1\n",
            DocFormat::Json => "{\n  \"x\": 1\n}\n",
            DocFormat::Yaml => "x: 1\n",
        };
        let mut d = doc(f, src);
        // Each backend gets a fragment its own grammar rejects. They differ
        // more than they should: TOML and JSON reject an unterminated string,
        // while the YAML backend accepts `"unclosed` (and `[1, `) as a plain
        // scalar and does no fragment validation at all — F14. A tab is the
        // one thing it does reject, so that is YAML's case here.
        let fragment = match f {
            DocFormat::Toml | DocFormat::Json => d.scalar_fragment(Some("x"), "\"unclosed"),
            DocFormat::Yaml => "x:\n\tbad".to_string(),
        };
        let err = d
            .apply(Mutation::Replace {
                path: vec![Seg::Key("x".into())],
                fragment,
            })
            .unwrap_err();
        assert!(
            !matches!(err, MutateError::NotFound),
            "{}: path should resolve, got {err:?}",
            name(f)
        );
        assert_eq!(
            d.serialize(),
            src,
            "{}: document mutated on failure",
            name(f)
        );
    }
}
