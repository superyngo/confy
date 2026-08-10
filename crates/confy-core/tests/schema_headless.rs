//! Headless schema-engine tests — no TUI/host dependency, matches the
//! `session_headless.rs` convention (crate-root `#[test]` fns, tiny local
//! helpers, no test framework macros).
use confy_core::schema::types::{Category, SchemaSource};

#[test]
fn schema_source_variants_are_distinguishable() {
    let local = SchemaSource::Local("./schema.json".into());
    let url = SchemaSource::Url("https://example.com/s.json".into());
    assert_ne!(local, url);
    assert_eq!(Category::Value, Category::Value);
    assert_ne!(Category::Value, Category::Representation);
}

use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::{ConfigDocument, DocFormat};
use confy_core::schema::value_bridge::bridge;
use serde_json::json;

fn toml_doc(src: &str) -> AnyDocument {
    AnyDocument::from_str_as(src, DocFormat::Toml).unwrap()
}

#[test]
fn bridge_projects_scalars_and_nesting() {
    let doc = toml_doc("name = \"svc\"\nport = 8080\n[db]\nhost = \"local\"\n");
    let tree = doc.project();
    let (value, _warnings) = doc.to_value().unwrap();
    let (json, _map) = bridge(&tree.root, &value);
    assert_eq!(
        json,
        json!({ "name": "svc", "port": 8080, "db": { "host": "local" } })
    );
}

#[test]
fn bridge_maps_pointers_to_paths_including_nested_and_required_parent() {
    use confy_core::model::node::Seg;
    let doc = toml_doc("[server]\nport = 8080\n");
    let tree = doc.project();
    let (value, _warnings) = doc.to_value().unwrap();
    let (_json, map) = bridge(&tree.root, &value);
    // Nested leaf resolves exactly.
    let leaf_path = map.resolve("/server/port").expect("leaf pointer mapped");
    assert_eq!(
        leaf_path,
        &vec![Seg::Key("server".into()), Seg::Key("port".into())]
    );
    // The parent object (a `required` failure's pointer) resolves too.
    let parent_path = map.resolve("/server").expect("parent pointer mapped");
    assert_eq!(parent_path, &vec![Seg::Key("server".into())]);
    // The document root resolves to the empty path.
    let root_path = map.resolve("").expect("root pointer mapped");
    assert_eq!(root_path, &Vec::<Seg>::new());
}

#[test]
fn bridge_skips_comments_and_keeps_array_order() {
    let doc = toml_doc("# a comment\nvals = [1, 2, 3]\n");
    let tree = doc.project();
    let (value, _warnings) = doc.to_value().unwrap();
    let (json, map) = bridge(&tree.root, &value);
    assert_eq!(json, json!({ "vals": [1, 2, 3] }));
    assert!(map.resolve("/vals/1").is_some());
}

#[test]
fn bridge_maps_non_finite_floats_to_strings_not_null() {
    let doc = toml_doc("x = nan\ny = inf\nz = -inf\n");
    let tree = doc.project();
    let (value, _warnings) = doc.to_value().unwrap();
    let (json, _map) = bridge(&tree.root, &value);
    assert_eq!(
        json,
        json!({ "x": "nan", "y": "inf", "z": "-inf" })
    );
}


use confy_core::schema::hints::detect_hint;

#[test]
fn detect_hint_json_root_schema_key() {
    let src = r#"{ "$schema": "./app.schema.json", "port": 1 }"#;
    assert_eq!(
        detect_hint(src, DocFormat::Json),
        Some(SchemaSource::Local("./app.schema.json".into()))
    );
}

#[test]
fn detect_hint_json_url_schema_key() {
    let src = r#"{ "$schema": "https://example.com/s.json" }"#;
    assert_eq!(
        detect_hint(src, DocFormat::Json),
        Some(SchemaSource::Url("https://example.com/s.json".into()))
    );
}

#[test]
fn detect_hint_json_none_when_absent() {
    let src = r#"{ "port": 1 }"#;
    assert_eq!(detect_hint(src, DocFormat::Json), None);
}

#[test]
fn detect_hint_yaml_modeline() {
    let src = "# yaml-language-server: $schema=./s.yaml\nport: 1\n";
    assert_eq!(
        detect_hint(src, DocFormat::Yaml),
        Some(SchemaSource::Local("./s.yaml".into()))
    );
}

#[test]
fn detect_hint_yaml_none_when_modeline_not_leading() {
    // The modeline must be a leading comment — not one that appears after
    // real content.
    let src = "port: 1\n# yaml-language-server: $schema=./s.yaml\n";
    assert_eq!(detect_hint(src, DocFormat::Yaml), None);
}

#[test]
fn detect_hint_toml_first_line_schema_comment() {
    let src = "#:schema ./app.schema.json\nport = 1\n";
    assert_eq!(
        detect_hint(src, DocFormat::Toml),
        Some(SchemaSource::Local("./app.schema.json".into()))
    );
}

#[test]
fn detect_hint_toml_none_when_not_first_line() {
    let src = "port = 1\n#:schema ./app.schema.json\n";
    assert_eq!(detect_hint(src, DocFormat::Toml), None);
}