use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::{ConfigDocument, DocFormat};
use proptest::prelude::*;

fn toml_fixtures() -> Vec<&'static str> {
    vec![
        include_str!("fixtures/sample.toml"),
        include_str!("fixtures/test.toml"),
    ]
}
fn json_fixtures() -> Vec<&'static str> {
    vec![
        include_str!("fixtures/sample.json"),
        include_str!("fixtures/test.json"),
        include_str!("fixtures/edgecases.json"),
        include_str!("fixtures/root_array.json"),
        include_str!("fixtures/sample.jsonc"),
        include_str!("fixtures/comments.jsonc"),
    ]
}
fn yaml_fixtures() -> Vec<&'static str> {
    // multi-doc.yaml is intentionally excluded, same as
    // roundtrip_yaml.rs: multi-document YAML is rejected at parse, not a
    // round-trip candidate.
    vec![
        include_str!("fixtures/sample.yaml"),
        include_str!("fixtures/test.yaml"),
        include_str!("fixtures/yaml/flow-style.yaml"),
        include_str!("fixtures/yaml/github-actions.yaml"),
        include_str!("fixtures/yaml/helm-values.yaml"),
        include_str!("fixtures/yaml/prometheus.yaml"),
        include_str!("fixtures/yaml/scalars.yaml"),
        include_str!("fixtures/yaml/simple-config.yaml"),
        include_str!("fixtures/yaml/tags-and-anchors.yaml"),
        include_str!("fixtures/yaml/comments.yaml"),
        include_str!("fixtures/yaml/deployment.yaml"),
        include_str!("fixtures/yaml/docker-compose.yaml"),
    ]
}

fn assert_roundtrip(src: &str, fmt: DocFormat) {
    let doc = AnyDocument::from_str_as(src, fmt).unwrap();
    assert_eq!(doc.serialize(), src);
}

proptest! {
    #[test]
    fn toml_fixture_roundtrips(src in prop::sample::select(toml_fixtures())) {
        assert_roundtrip(src, DocFormat::Toml);
    }
    #[test]
    fn json_fixture_roundtrips(src in prop::sample::select(json_fixtures())) {
        assert_roundtrip(src, DocFormat::Json);
    }
    #[test]
    fn yaml_fixture_roundtrips(src in prop::sample::select(yaml_fixtures())) {
        assert_roundtrip(src, DocFormat::Yaml);
    }
}
