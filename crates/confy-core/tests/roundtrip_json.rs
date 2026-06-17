use confy_core::model::document::ConfigDocument;
use confy_core::model::json::JsonDocument;
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
        let doc = JsonDocument::from_str(&text).unwrap();
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
    use confy_core::model::document::Mutation;
    use confy_core::model::node::Seg;

    let text = std::fs::read_to_string("tests/fixtures/sample.json").unwrap();
    let mut doc = JsonDocument::from_str(&text).unwrap();
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
    // re-parse the serialized text and serialize again — must be stable (lossless apply)
    let doc2 = JsonDocument::from_str(&after).unwrap();
    assert_eq!(doc2.serialize(), after, "reparse of mutated doc not stable");
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
    use confy_core::model::document::Mutation;
    use confy_core::model::node::Seg;

    let text = std::fs::read_to_string("tests/fixtures/comments.jsonc").unwrap();
    let mut doc = JsonDocument::from_str(&text).unwrap();
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
