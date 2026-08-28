use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::DocFormat;
use confy_core::model::node::Seg;
use confy_core::session::{Intent, Session};

fn add_sibling_after(src: &str, fmt: DocFormat, path: Vec<Seg>) -> String {
    let doc = AnyDocument::from_str_as(src, fmt).unwrap();
    let mut s = Session::new(doc);
    s.dispatch(Intent::SetCursor(path));
    s.dispatch(Intent::AddSibling);
    s.dispatch(Intent::CommitEdit {
        value: Some("2".into()),
        name: Some("b".into()),
    });
    s.serialize().unwrap()
}

#[test]
fn json_add_sibling_keeps_trailing_comment_attached() {
    // Regression test for comment-advisory follow-up issue #3: adding a
    // sibling right after a node with a same-line trailing comment must not
    // detach that comment (JSON previously gave it its own item/slot in the
    // CST rebuild, so the new sibling landed between the value and its
    // comment). TOML/YAML never had this bug (see the two tests below) —
    // this pins the JSON-specific fix.
    let out = add_sibling_after(
        "{\n  \"a\": 1  // c\n}\n",
        DocFormat::Json,
        vec![Seg::Key("a".into())],
    );
    assert_eq!(
        out, "{\n  \"a\": 1,  // c\n  \"b\": \"\"\n}\n",
        "full output: {out:?}"
    );
}

#[test]
fn toml_keeps_trailing_comment_attached() {
    let out = add_sibling_after("a = 1  # c\n", DocFormat::Toml, vec![Seg::Key("a".into())]);
    eprintln!("TOML result:\n{out}");
    let a_line = out.lines().find(|l| l.starts_with("a ")).unwrap();
    assert!(
        a_line.contains("# c"),
        "TOML: comment stays attached: {a_line:?}"
    );
}

#[test]
fn yaml_keeps_trailing_comment_attached() {
    let out = add_sibling_after("a: 1  # c\n", DocFormat::Yaml, vec![Seg::Key("a".into())]);
    eprintln!("YAML result:\n{out}");
    let a_line = out.lines().find(|l| l.starts_with("a:")).unwrap();
    assert!(
        a_line.contains("# c"),
        "YAML: comment stays attached: {a_line:?}"
    );
}
