use confy_core::model::cst_doc::CstDocument;
use confy_core::model::document::{ConfigDocument, Mutation, OnCollision, Target};
use confy_core::model::node::Seg;

fn insert_elem(src: &str, index: usize) -> String {
    let mut doc = CstDocument::from_str(src).unwrap();
    doc.apply(Mutation::Insert {
        target: Target {
            parent: vec![Seg::Key("a".into())],
            index,
        },
        fragment: "9".into(),
        on_collision: OnCollision::Cancel,
        suggested_key: None,
    })
    .unwrap();
    doc.serialize()
}

fn delete_elem(src: &str, index: usize) -> String {
    let mut doc = CstDocument::from_str(src).unwrap();
    doc.apply(Mutation::Delete {
        path: vec![Seg::Key("a".into()), Seg::Index(index)],
    })
    .unwrap();
    doc.serialize()
}

/// A multiline array keeps its own layout across an element insert: the new
/// element gets its own line with the array's *authored* indent, whatever that
/// indent is. Before this was measured, `array_insert` always spliced the
/// single-line `, ` separator, so every add landed on a neighbour's line and
/// repeated adds collapsed the array to one line.
#[test]
fn inserting_into_a_multiline_array_keeps_one_element_per_line() {
    // 2-space, front / middle / append / past-the-end (append).
    let src = "a = [\n  1,\n  2,\n]\n";
    assert_eq!(insert_elem(src, 0), "a = [\n  9,\n  1,\n  2,\n]\n");
    assert_eq!(insert_elem(src, 1), "a = [\n  1,\n  9,\n  2,\n]\n");
    assert_eq!(insert_elem(src, 2), "a = [\n  1,\n  2,\n  9,\n]\n");
    assert_eq!(insert_elem(src, 9), "a = [\n  1,\n  2,\n  9,\n]\n");

    // The indent is the author's, not a constant.
    assert_eq!(
        insert_elem("a = [\n    1,\n    2,\n]\n", 2),
        "a = [\n    1,\n    2,\n    9,\n]\n"
    );
    assert_eq!(
        insert_elem("a = [\n\t1,\n\t2,\n]\n", 2),
        "a = [\n\t1,\n\t2,\n\t9,\n]\n"
    );

    // No trailing comma: one is added for the new element, and the last element
    // still ends without one.
    assert_eq!(
        insert_elem("a = [\n  1,\n  2\n]\n", 2),
        "a = [\n  1,\n  2,\n  9\n]\n"
    );

    // A per-element EOL comment stays on its own element's line.
    assert_eq!(
        insert_elem("a = [\n  1, # one\n  2, # two\n]\n", 1),
        "a = [\n  1, # one\n  9,\n  2, # two\n]\n"
    );

    // A nested array element is one element, not a layout signal.
    assert_eq!(
        insert_elem("a = [\n  [1, 2],\n  3,\n]\n", 1),
        "a = [\n  [1, 2],\n  9,\n  3,\n]\n"
    );

    // A single-line array is untouched by the rule: still `, `.
    assert_eq!(insert_elem("a = [1, 2]\n", 1), "a = [1, 9, 2]\n");
    assert_eq!(insert_elem("a = [1, 2]\n", 2), "a = [1, 2, 9]\n");
    assert_eq!(insert_elem("a = []\n", 0), "a = [9]\n");
}

/// Deleting the **last** element of a trailing-comma multiline array no longer
/// strands that element's indent in front of the `]` (`[\n  1,\n  ]`).
#[test]
fn deleting_the_last_multiline_element_leaves_no_stray_indent() {
    assert_eq!(delete_elem("a = [\n  1,\n  2,\n]\n", 1), "a = [\n  1,\n]\n");
    assert_eq!(
        delete_elem("a = [\n    1,\n    2,\n]\n", 1),
        "a = [\n    1,\n]\n"
    );
    assert_eq!(delete_elem("a = [\n\t1,\n\t2,\n]\n", 1), "a = [\n\t1,\n]\n");
    assert_eq!(delete_elem("a = [\n  1,\n]\n", 0), "a = [\n]\n");

    // A non-last element is unaffected, and so is a no-trailing-comma array
    // (whose last element already took the comma *before* it).
    assert_eq!(delete_elem("a = [\n  1,\n  2,\n]\n", 0), "a = [\n  2,\n]\n");
    assert_eq!(delete_elem("a = [\n  1,\n  2\n]\n", 1), "a = [\n  1\n]\n");

    // The retract stops at anything that still owns the indent: the deleted
    // element's own EOL comment keeps its column.
    assert_eq!(
        delete_elem("a = [\n  1, # one\n  2, # two\n]\n", 1),
        "a = [\n  1, # one\n  # two\n]\n"
    );

    // Single-line arrays and inline tables keep their existing behaviour.
    assert_eq!(delete_elem("a = [1, 2]\n", 1), "a = [1]\n");
    assert_eq!(delete_elem("a = [1, 2,]\n", 1), "a = [1,]\n");
}

#[test]
fn untouched_file_roundtrips_byte_identical() {
    let src = include_str!("fixtures/sample.toml");
    let doc = CstDocument::from_str(src).unwrap();
    assert_eq!(doc.serialize(), src);
}

#[test]
fn edit_one_value_leaves_other_bytes_untouched() {
    let src = include_str!("fixtures/sample.toml");
    let mut doc = CstDocument::from_str(src).unwrap();
    doc.apply(Mutation::Replace {
        path: vec![Seg::Key("server".into()), Seg::Key("port".into())],
        fragment: "port = 9090\n".into(),
    })
    .unwrap();
    let expected = include_str!("fixtures/expected_after_edit.toml");
    assert_eq!(doc.serialize(), expected);
}
