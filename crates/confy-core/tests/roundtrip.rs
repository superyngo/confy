use confy_core::model::cst_doc::CstDocument;
use confy_core::model::document::{ConfigDocument, Mutation};
use confy_core::model::node::Seg;

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
