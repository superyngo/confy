use confy::model::cst_doc::CstDocument;
use confy::model::document::{ConfigDocument, Mutation};
use confy::model::node::Seg;

#[test]
fn untouched_file_roundtrips_byte_identical() {
    let src = include_str!("fixtures/sample.toml");
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("sample.toml");
    std::fs::write(&p, src).unwrap();
    let doc = CstDocument::load(&p).unwrap();
    assert_eq!(doc.serialize(), src);
}

#[test]
fn edit_one_value_leaves_other_bytes_untouched() {
    let src = include_str!("fixtures/sample.toml");
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("sample.toml");
    std::fs::write(&p, src).unwrap();
    let mut doc = CstDocument::load(&p).unwrap();
    doc.apply(Mutation::Replace {
        path: vec![Seg::Key("server".into()), Seg::Key("port".into())],
        toml: "port = 9090\n".into(),
    })
    .unwrap();
    let expected = include_str!("fixtures/expected_after_edit.toml");
    assert_eq!(doc.serialize(), expected);
}
