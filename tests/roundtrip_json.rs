use confy::model::document::ConfigDocument;
use confy::model::json::JsonDocument;
use std::path::Path;

#[test]
fn json_fixtures_roundtrip_byte_identical() {
    let fx = Path::new("tests/fixtures");
    let mut checked = 0;
    for entry in std::fs::read_dir(fx).unwrap() {
        let p = entry.unwrap().path();
        let ext = p.extension().and_then(|e| e.to_str());
        if !matches!(ext, Some("json") | Some("jsonc")) {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap();
        let doc = JsonDocument::load(&p).unwrap();
        assert_eq!(doc.serialize(), text, "roundtrip mismatch for {p:?}");
        checked += 1;
    }
    assert!(
        checked >= 5,
        "expected to check at least 5 json/jsonc fixtures, got {checked}"
    );
}

#[test]
fn mutation_then_reparse_is_lossless() {
    use confy::model::document::Mutation;
    use confy::model::node::Seg;

    let p = Path::new("tests/fixtures/sample.json");
    let mut doc = JsonDocument::load(p).unwrap();
    // replace a scalar value
    doc.apply(Mutation::Replace {
        path: vec![Seg::Key("version".into())],
        fragment: "6".into(),
    })
    .unwrap();
    let after = doc.serialize();
    assert!(
        after.contains("\"version\": 6"),
        "version not updated in:\n{after}"
    );
    // re-load the serialized text and serialize again — must be stable (lossless apply)
    let tmp = tempfile::Builder::new()
        .suffix(".json")
        .tempfile()
        .unwrap();
    std::fs::write(tmp.path(), &after).unwrap();
    let doc2 = JsonDocument::load(tmp.path()).unwrap();
    assert_eq!(doc2.serialize(), after, "reload of mutated doc not stable");
    // everything ELSE in the file is unchanged
    assert!(
        after.contains("\"host\": \"localhost\""),
        "host missing in:\n{after}"
    );
    assert!(
        after.contains("\"matrix\": [[1, 2], [3, 4]]"),
        "matrix missing in:\n{after}"
    );
}

#[test]
fn delete_preserves_unrelated_comment() {
    use confy::model::document::Mutation;
    use confy::model::node::Seg;

    let p = Path::new("tests/fixtures/comments.jsonc");
    let mut doc = JsonDocument::load(p).unwrap();
    // delete top-level member "a"; the header comments and block comment must remain
    doc.apply(Mutation::Delete {
        path: vec![Seg::Key("a".into())],
    })
    .unwrap();
    let after = doc.serialize();
    assert!(
        after.contains("// header comment"),
        "header comment missing in:\n{after}"
    );
    assert!(
        after.contains("/* block comment (read-only) */"),
        "block comment missing in:\n{after}"
    );
    assert!(
        !after.contains("\"a\": 1"),
        "deleted key still present in:\n{after}"
    );
}
