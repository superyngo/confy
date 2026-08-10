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

#[test]
fn detect_hint_toml_none_when_no_separator_after_schema() {
    let src = "#:schemaless\nport = 1\n";
    assert_eq!(detect_hint(src, DocFormat::Toml), None);
}

#[test]
fn detect_hint_json_none_when_schema_value_empty() {
    let src = r#"{ "$schema": "" }"#;
    assert_eq!(detect_hint(src, DocFormat::Json), None);
}

#[test]
fn detect_hint_yaml_none_when_modeline_schema_value_empty() {
    let src = "# yaml-language-server: $schema=\nport: 1\n";
    assert_eq!(detect_hint(src, DocFormat::Yaml), None);
}

use confy_core::schema::validate::validate;
use jsonschema::Validator;

fn compiled(schema: serde_json::Value) -> Validator {
    Validator::new(&schema).expect("valid test schema")
}

#[test]
fn validate_reports_no_violations_for_a_conforming_document() {
    let doc = toml_doc("port = 8080\n");
    let tree = doc.project();
    let (value, _w) = doc.to_value().unwrap();
    let (json, map) = bridge(&tree.root, &value);
    let v = compiled(json!({
        "type": "object",
        "properties": { "port": { "type": "integer" } }
    }));
    assert!(validate(&json, &v, &map).is_empty());
}

#[test]
fn validate_reports_a_type_violation_with_the_leaf_path() {
    use confy_core::model::node::Seg;
    let doc = toml_doc("port = \"not-a-number\"\n");
    let tree = doc.project();
    let (value, _w) = doc.to_value().unwrap();
    let (json, map) = bridge(&tree.root, &value);
    let v = compiled(json!({
        "type": "object",
        "properties": { "port": { "type": "integer" } }
    }));
    let violations = validate(&json, &v, &map);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].path, vec![Seg::Key("port".into())]);
    assert_eq!(violations[0].keyword, "type");
    assert_eq!(violations[0].category, Category::Value);
}

#[test]
fn validate_reports_a_required_violation_against_the_parent_path() {
    use confy_core::model::node::Seg;
    let doc = toml_doc("[server]\nhost = \"local\"\n");
    let tree = doc.project();
    let (value, _w) = doc.to_value().unwrap();
    let (json, map) = bridge(&tree.root, &value);
    let v = compiled(json!({
        "type": "object",
        "properties": {
            "server": {
                "type": "object",
                "required": ["port"]
            }
        }
    }));
    let violations = validate(&json, &v, &map);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].keyword, "required");
    assert_eq!(violations[0].path, vec![Seg::Key("server".into())]);
    assert!(violations[0].message.contains("port"));
}

#[test]
fn validate_flags_null_type_against_toml_as_representation_category() {
    let doc = toml_doc("port = 8080\n");
    let tree = doc.project();
    let (value, _w) = doc.to_value().unwrap();
    let (json, map) = bridge(&tree.root, &value);
    let v = compiled(json!({
        "type": "object",
        "properties": { "port": { "type": "null" } }
    }));
    let violations = validate(&json, &v, &map);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].category, Category::Representation);
}

#[test]
fn validate_does_not_misclassify_a_literal_string_null_value_as_representation() {
    let doc = toml_doc("port = \"null\"\n");
    let tree = doc.project();
    let (value, _w) = doc.to_value().unwrap();
    let (json, map) = bridge(&tree.root, &value);
    let v = compiled(json!({
        "type": "object",
        "properties": { "port": { "type": "integer" } }
    }));
    let violations = validate(&json, &v, &map);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].category, Category::Value);
}

#[test]
fn validate_does_not_misclassify_a_nullable_type_union_mismatch_as_representation() {
    let doc = toml_doc("port = 8080\n");
    let tree = doc.project();
    let (value, _w) = doc.to_value().unwrap();
    let (json, map) = bridge(&tree.root, &value);
    let v = compiled(json!({
        "type": "object",
        "properties": { "port": { "type": ["string", "null"] } }
    }));
    let violations = validate(&json, &v, &map);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].category, Category::Value);
}
use confy_core::model::node::Seg;
use confy_core::schema::hints_edit::resolve_edit_hint;
use confy_core::schema::types::EditHint;

#[test]
fn resolve_edit_hint_finds_enum_via_properties() {
    let schema = json!({
        "type": "object",
        "properties": {
            "level": { "enum": ["debug", "info", "warn"] }
        }
    });
    let hint = resolve_edit_hint(&schema, &vec![Seg::Key("level".into())]);
    match hint {
        EditHint::Enum(opts) => {
            let labels: Vec<_> = opts.iter().map(|(l, _)| l.clone()).collect();
            assert_eq!(labels, vec!["debug", "info", "warn"]);
        }
        other => panic!("expected Enum, got {other:?}"),
    }
}

#[test]
fn resolve_edit_hint_finds_bounded_numeric() {
    let schema = json!({
        "type": "object",
        "properties": {
            "port": { "type": "integer", "minimum": 1, "maximum": 65535 }
        }
    });
    let hint = resolve_edit_hint(&schema, &vec![Seg::Key("port".into())]);
    assert_eq!(
        hint,
        EditHint::Bounded { minimum: Some(1.0), maximum: Some(65535.0), multiple_of: None }
    );
}

#[test]
fn resolve_edit_hint_carves_out_oneof_of_const() {
    let schema = json!({
        "type": "object",
        "properties": {
            "level": {
                "oneOf": [
                    { "const": "debug", "title": "Debug" },
                    { "const": "info", "title": "Info" }
                ]
            }
        }
    });
    let hint = resolve_edit_hint(&schema, &vec![Seg::Key("level".into())]);
    match hint {
        EditHint::Enum(opts) => {
            assert_eq!(
                opts,
                vec![
                    ("Debug".to_string(), json!("debug")),
                    ("Info".to_string(), json!("info")),
                ]
            );
        }
        other => panic!("expected Enum via oneOf carve-out, got {other:?}"),
    }
}

#[test]
fn resolve_edit_hint_declines_true_composition() {
    let schema = json!({
        "type": "object",
        "properties": {
            "value": {
                "oneOf": [
                    { "type": "string", "minLength": 1 },
                    { "type": "integer" }
                ]
            }
        }
    });
    let hint = resolve_edit_hint(&schema, &vec![Seg::Key("value".into())]);
    assert_eq!(hint, EditHint::None);
}

#[test]
fn resolve_edit_hint_resolves_array_items_and_local_ref() {
    let schema = json!({
        "type": "object",
        "properties": {
            "tags": { "type": "array", "items": { "$ref": "#/$defs/tag" } }
        },
        "$defs": { "tag": { "enum": ["a", "b"] } }
    });
    let hint = resolve_edit_hint(&schema, &vec![Seg::Key("tags".into()), Seg::Index(0)]);
    match hint {
        EditHint::Enum(opts) => {
            assert_eq!(opts.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
        }
        other => panic!("expected Enum via items+$ref, got {other:?}"),
    }
}

#[test]
fn resolve_edit_hint_none_for_unresolvable_path() {
    let schema = json!({ "type": "object", "properties": {} });
    let hint = resolve_edit_hint(&schema, &vec![Seg::Key("missing".into())]);
    assert_eq!(hint, EditHint::None);
}

use confy_core::session::Session;

fn session_from(src: &str, format: DocFormat) -> Session {
    let doc = AnyDocument::from_str_as(src, format).unwrap();
    Session::new(doc)
}

#[test]
fn session_detects_toml_schema_hint_on_construction() {
    let s = session_from("#:schema ./s.json\nport = 1\n", DocFormat::Toml);
    // Detection itself doesn't load — schema stays None until the host
    // resolves the fetch request and dispatches the text back.
    assert!(s.schema.is_none());
}

#[test]
fn session_detect_and_request_schema_returns_the_hint() {
    let mut s = session_from("#:schema ./s.json\nport = 1\n", DocFormat::Toml);
    let source = s.detect_and_request_schema();
    assert_eq!(source, Some(SchemaSource::Local("./s.json".into())));
}

#[test]
fn session_detect_and_request_schema_none_without_a_hint() {
    let mut s = session_from("port = 1\n", DocFormat::Toml);
    assert_eq!(s.detect_and_request_schema(), None);
}

#[test]
fn session_apply_schema_text_compiles_and_revalidates() {
    let mut s = session_from("port = \"nope\"\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "port": { "type": "integer" } }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    let state = s.schema.as_ref().expect("schema loaded");
    assert!(state.load_error.is_none());
    assert_eq!(state.violations.len(), 1);
    assert_eq!(state.violations[0].keyword, "type");
}

#[test]
fn session_apply_schema_text_load_error_is_soft() {
    let mut s = session_from("port = 1\n", DocFormat::Toml);
    s.apply_schema_text(
        SchemaSource::Local("./missing.json".into()),
        Err("file not found".into()),
    );
    let state = s.schema.as_ref().expect("schema state present even on load error");
    assert!(state.load_error.is_some());
    assert!(state.compiled.is_none());
    // The document is still fully editable — no error on the session itself.
    assert!(s.error.is_none());
}

#[test]
fn session_revalidates_after_a_mutation_commit() {
    let mut s = session_from("port = 1\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "port": { "type": "string" } }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    assert_eq!(s.schema.as_ref().unwrap().violations.len(), 1);
    // Fix the value via the same Replace mutation path CommitEdit/Nudge use.
    let path = vec![Seg::Key("port".into())];
    let doc = s.doc.as_mut().unwrap();
    let fragment = doc.scalar_fragment(Some("port"), "\"eighty\"");
    doc.apply(confy_core::model::document::Mutation::Replace { path, fragment })
        .unwrap();
    s.tree = doc.project();
    s.revalidate_schema();
    assert!(s.schema.as_ref().unwrap().violations.is_empty());
}

#[test]
fn session_begin_inline_edit_sets_schema_enum_mode_for_an_enum_constrained_node() {
    use confy_core::session::state::Mode;
    let mut s = session_from("level = \"debug\"\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "level": { "enum": ["debug", "info"] } }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    s.cursor = vec![Seg::Key("level".into())];
    s.begin_inline_edit();
    assert!(matches!(s.mode, Mode::SchemaEnum(_)));
}

#[test]
fn session_schema_enum_commit_writes_the_chosen_value() {
    use confy_core::session::state::Mode;
    let mut s = session_from("level = \"debug\"\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "level": { "enum": ["debug", "info"] } }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    s.cursor = vec![Seg::Key("level".into())];
    s.begin_inline_edit();
    s.schema_enum_move(1); // move to "info"
    s.schema_enum_commit();
    assert!(matches!(s.mode, Mode::Normal));
    let node = s.tree.node_at(&[Seg::Key("level".into())]).unwrap();
    assert_eq!(node.value.as_deref(), Some("\"info\""));
}

#[test]
fn dispatch_nudge_clamps_to_schema_maximum() {
    let mut s = session_from("port = 65534\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "port": { "type": "integer", "minimum": 1, "maximum": 65535 } }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    s.cursor = vec![Seg::Key("port".into())];
    // 65534 -> 65535 lands exactly at the maximum: allowed.
    let snap = s.dispatch(confy_core::session::Intent::Nudge(1));
    let row = snap.rows.iter().find(|r| r.key == "port").unwrap();
    assert_eq!(row.value.as_deref(), Some("65535"));
    // 65535 -> 65536 would exceed the maximum: clamped, silently a no-op.
    let snap = s.dispatch(confy_core::session::Intent::Nudge(1));
    let row = snap.rows.iter().find(|r| r.key == "port").unwrap();
    assert_eq!(row.value.as_deref(), Some("65535"));
}
