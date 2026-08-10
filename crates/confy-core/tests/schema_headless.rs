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
