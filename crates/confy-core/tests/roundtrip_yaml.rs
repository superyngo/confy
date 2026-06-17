use confy_core::model::document::ConfigDocument;
use confy_core::model::yaml::YamlDocument;
use std::path::Path;

#[test]
fn yaml_fixtures_roundtrip_byte_identical() {
    // multi-doc.yaml is intentionally excluded: it is rejected at load.
    for name in [
        "docker-compose",
        "github-actions",
        "deployment",
        "helm-values",
        "prometheus",
        "simple-config",
        "flow-style",
        "scalars",
        "comments",
        "tags-and-anchors",
    ] {
        let path = format!("tests/fixtures/yaml/{name}.yaml");
        let src = std::fs::read_to_string(&path).unwrap();
        let doc = YamlDocument::load(Path::new(&path)).unwrap();
        assert_eq!(doc.serialize(), src, "roundtrip mismatch for {name}");
    }
}

#[test]
fn multi_document_is_rejected_at_load() {
    let p = Path::new("tests/fixtures/yaml/multi-doc.yaml");
    assert!(
        YamlDocument::load(p).is_err(),
        "multi-document YAML must be rejected at load"
    );
}

#[test]
fn mutation_then_reparse_is_lossless() {
    use confy_core::model::document::Mutation;
    use confy_core::model::node::Seg;

    let p = Path::new("tests/fixtures/yaml/simple-config.yaml");
    let mut doc = YamlDocument::load(p).unwrap();
    // replace a top-level scalar value via its `key: value` line.
    doc.apply(Mutation::Replace {
        path: vec![Seg::Key("port".into())],
        fragment: "port: 9090".into(),
    })
    .unwrap();
    let after = doc.serialize();
    assert!(
        after.contains("port: 9090"),
        "port not updated in:\n{after}"
    );
    // re-load the serialized text and serialize again — must be stable.
    let tmp = tempfile::Builder::new().suffix(".yaml").tempfile().unwrap();
    std::fs::write(tmp.path(), &after).unwrap();
    let doc2 = YamlDocument::load(tmp.path()).unwrap();
    assert_eq!(doc2.serialize(), after, "reload of mutated doc not stable");
    // everything ELSE in the file is unchanged.
    assert!(after.contains("name: my-app"), "name missing in:\n{after}");
    assert!(
        after.contains("host: localhost"),
        "nested host missing in:\n{after}"
    );
    assert!(
        after.contains("  - reporting"),
        "sequence item missing in:\n{after}"
    );
}

#[test]
fn opaque_node_mutation_is_unsupported() {
    use confy_core::model::document::{MutateError, Mutation};
    use confy_core::model::node::Seg;

    let p = Path::new("tests/fixtures/yaml/tags-and-anchors.yaml");
    let mut doc = YamlDocument::load(p).unwrap();
    let before = doc.serialize();
    // `defaults: &defaults …` has an out-of-subset (anchor) value → opaque.
    let err = doc
        .apply(Mutation::Delete {
            path: vec![Seg::Key("defaults".into())],
        })
        .unwrap_err();
    assert!(
        matches!(err, MutateError::Unsupported),
        "expected Unsupported, got {err:?}"
    );
    // The document is untouched (atomic-commit: failure leaves it as-is).
    assert_eq!(doc.serialize(), before, "doc changed after failed mutation");
}
