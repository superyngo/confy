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
    assert!(s.snapshot().error_text().is_none());
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
fn schema_enum_jump_clamps_instead_of_wrapping() {
    // PageUp/PageDown/Home/End land exactly on the ends and never wrap,
    // unlike the ±1 arrow-key `schema_enum_move` step.
    use confy_core::session::state::Mode;
    let mut s = session_from("level = \"debug\"\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "level": { "enum": ["debug", "info", "warn", "error"] } }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    s.cursor = vec![Seg::Key("level".into())];
    s.begin_inline_edit();
    let cursor_of = |s: &confy_core::session::Session| match &s.mode {
        Mode::SchemaEnum(st) => st.cursor,
        _ => panic!("expected SchemaEnum mode"),
    };
    assert_eq!(cursor_of(&s), 0);

    s.schema_enum_jump(-4); // End-style oversized delta from cursor 0 stays clamped at 0
    assert_eq!(cursor_of(&s), 0);

    s.schema_enum_jump(4); // End: oversized positive delta clamps to the last option
    assert_eq!(cursor_of(&s), 3);

    s.schema_enum_jump(4); // already at the end - stays put, no wrap to 0
    assert_eq!(cursor_of(&s), 3);

    s.schema_enum_jump(-2); // PageUp by 2
    assert_eq!(cursor_of(&s), 1);

    s.schema_enum_jump(-4); // Home: oversized negative delta clamps to 0
    assert_eq!(cursor_of(&s), 0);
}

#[test]
fn schema_enum_commit_defers_to_type_change_prompt_when_the_picked_value_changes_type() {
    // A mixed-type enum ("debug" string, 42 integer) — picking the
    // differently-typed option must go through the same `Mode::Prompt(
    // TypeChange)` confirmation gate as any other value commit (previously
    // `schema_enum_commit` bypassed it entirely and wrote the value
    // unconditionally).
    use confy_core::session::{Mode, PromptKind};
    let mut s = session_from("level = \"debug\"\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "level": { "enum": ["debug", 42] } }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    s.cursor = vec![Seg::Key("level".into())];
    s.begin_inline_edit();
    s.schema_enum_move(1); // move to 42
    s.schema_enum_commit();
    assert!(
        matches!(s.mode, Mode::Prompt(PromptKind::TypeChange { .. })),
        "expected a TypeChange prompt"
    );
    let node = s.tree.node_at(&[Seg::Key("level".into())]).unwrap();
    assert_eq!(node.value.as_deref(), Some("\"debug\""), "not yet committed");

    // 'y' applies the value and settles back on Normal (no live editor / no
    // Detail panel to fall back into — matches the pre-existing "always
    // resolves to Normal on success" contract of schema_enum_commit).
    assert!(!s.handle_prompt_key('y'));
    assert!(matches!(s.mode, Mode::Normal));
    let node = s.tree.node_at(&[Seg::Key("level".into())]).unwrap();
    assert_eq!(node.value.as_deref(), Some("42"));
}

#[test]
fn schema_enum_commit_type_change_prompt_declined_leaves_the_document_unchanged() {
    use confy_core::session::{Mode, PromptKind};
    let mut s = session_from("level = \"debug\"\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "level": { "enum": ["debug", 42] } }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    s.cursor = vec![Seg::Key("level".into())];
    s.begin_inline_edit();
    s.schema_enum_move(1);
    s.schema_enum_commit();
    assert!(matches!(s.mode, Mode::Prompt(PromptKind::TypeChange { .. })));
    assert!(!s.handle_prompt_key('n'));
    assert!(
        matches!(s.mode, Mode::Normal),
        "n → Normal, not a resurrected live editor (one-shot, no editor to show)"
    );
    let node = s.tree.node_at(&[Seg::Key("level".into())]).unwrap();
    assert_eq!(node.value.as_deref(), Some("\"debug\""), "unchanged");
}

#[test]
fn schema_enum_commit_preserves_the_existing_trailing_comment() {
    // Same-type pick (no prompt) with a pre-existing trailing comment on the
    // line — must round-trip untouched, not get silently dropped by the
    // buffer-based comment-diff logic `edit_commit` now applies underneath.
    use confy_core::session::Mode;
    let mut s = session_from("level = \"debug\" # pick one\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "level": { "enum": ["debug", "info"] } }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    s.cursor = vec![Seg::Key("level".into())];
    s.begin_inline_edit();
    s.schema_enum_move(1);
    s.schema_enum_commit();
    assert!(matches!(s.mode, Mode::Normal));
    assert_eq!(
        s.serialize().unwrap(),
        "level = \"info\" # pick one\n",
        "trailing comment preserved across the enum-picker commit"
    );
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
    // 65535 -> 65536 would exceed the maximum: clamped back down to 65535.
    let snap = s.dispatch(confy_core::session::Intent::Nudge(1));
    let row = snap.rows.iter().find(|r| r.key == "port").unwrap();
    assert_eq!(row.value.as_deref(), Some("65535"));
}

#[test]
fn commit_edit_bypasses_schema_enum_diversion_and_writes_the_value() {
    let mut s = session_from("level = \"debug\"\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "level": { "enum": ["debug", "info"] } }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    s.cursor = vec![Seg::Key("level".into())];
    // The Web one-shot CommitEdit carries the final value directly — it must
    // NOT be diverted to the interactive SchemaEnum picker (which would make
    // commit_edit's `Mode::Edit` requirement silently drop the edit).
    s.dispatch(confy_core::session::Intent::CommitEdit {
        value: Some("\"info\"".into()),
        name: None,
    });
    assert!(
        s.serialize().unwrap().contains("level = \"info\""),
        "value written despite the enum hint: {}",
        s.serialize().unwrap()
    );
}

#[test]
fn add_node_resolving_enum_hint_is_cancellable_via_escape() {
    use confy_core::session::state::Mode;
    let mut s = session_from("port = 1\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "new_field": { "enum": ["a", "b"] } }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    s.cursor = vec![Seg::Key("port".into())];
    // Adding a sibling generates a `new_field` key, which resolves the enum
    // hint → the picker opens with created_on_add = true.
    s.dispatch(confy_core::session::Intent::AddNode);
    assert!(
        matches!(&s.mode, Mode::SchemaEnum(st) if st.created_on_add),
        "picker opened for the freshly-added enum node"
    );
    // Escape must cancel the picker AND remove the placeholder (mirrors the
    // Mode::Edit created_on_add → edit_cancel → cancel_added_node safety net).
    s.dispatch(confy_core::session::Intent::Escape);
    assert!(matches!(s.mode, Mode::Normal));
    let text = s.serialize().unwrap();
    assert!(!text.contains("new_field"), "placeholder removed on cancel: {text}");
    assert!(text.contains("port = 1"), "pre-existing node intact: {text}");
}

use confy_core::session::Intent;

#[test]
fn dispatch_schema_loaded_populates_snapshot_status_and_row_warnings() {
    let mut s = session_from("port = \"nope\"\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "port": { "type": "integer" } }
    })
    .to_string();
    let snap = s.dispatch(Intent::SchemaLoaded {
        source: SchemaSource::Local("./s.json".into()),
        text: Ok(schema_text),
    });
    let status = snap.schema_status.expect("schema_status set");
    assert_eq!(status.violation_count, 1);
    let port_row = snap.rows.iter().find(|r| r.key == "port").unwrap();
    assert!(port_row.violations.is_some());
    assert!(port_row.violations.as_ref().unwrap()[0].contains("type"));
}

#[test]
fn revalidate_schema_marks_ancestors_of_violating_paths() {
    use confy_core::model::node::{Path, Seg};
    let mut s = session_from("[server]\nport = \"nope\"\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": {
            "server": {
                "type": "object",
                "properties": { "port": { "type": "integer" } }
            }
        }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("/tmp/s.json".into()), Ok(schema_text));
    let state = s.schema.as_ref().unwrap();
    assert!(!state.violations.is_empty(), "port must violate (string vs integer)");
    let server_path: Path = vec![Seg::Key("server".into())];
    assert!(
        state.warning_ancestors.contains(&server_path),
        "server (ancestor of violating port) must be marked"
    );
    assert!(state.warning_ancestors.contains(&Vec::new()));
}

#[test]
fn collapsed_ancestor_row_reports_has_descendant_warning() {
    use confy_core::model::node::{Path, Seg};
    let mut s = session_from("[server]\nport = \"nope\"\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": {
            "server": {
                "type": "object",
                "properties": { "port": { "type": "integer" } }
            }
        }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("/tmp/s.json".into()), Ok(schema_text));
    let server_path: Path = vec![Seg::Key("server".into())];
    s.expanded.remove(&server_path);
    let rows = s.visible_rows();
    let server_row = rows.iter().find(|r| r.key == "server").unwrap();
    assert!(server_row.is_branch);
    assert!(server_row.has_descendant_warning);
}

#[test]
fn begin_edit_external_forces_the_popup_editor_for_an_enum_constrained_scalar() {
    use confy_core::session::state::Mode;
    let mut s = session_from("level = \"debug\"\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "level": { "enum": ["debug", "info"] } }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    s.cursor = vec![Seg::Key("level".into())];
    // Plain BeginEdit is diverted into the picker (req 2's precedent, already
    // covered above) — BeginEditExternal must NOT be, for the same node.
    s.dispatch(Intent::BeginEditExternal);
    assert!(
        s.pending_external_edit.is_some(),
        "BeginEditExternal always routes to the external-edit handshake, schema or not"
    );
    assert!(!matches!(s.mode, Mode::SchemaEnum(_)));
}

#[test]
fn edit_hint_exposes_enum_and_bounded_constraints_without_entering_edit_mode() {
    let mut s = session_from("level = \"debug\"\nport = 1\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": {
            "level": { "enum": ["debug", "info"] },
            "port": { "type": "integer", "minimum": 1, "maximum": 65535 }
        }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    assert_eq!(
        s.edit_hint(&vec![Seg::Key("level".into())]),
        EditHint::Enum(vec![
            ("debug".into(), json!("debug")),
            ("info".into(), json!("info")),
        ])
    );
    assert_eq!(
        s.edit_hint(&vec![Seg::Key("port".into())]),
        EditHint::Bounded { minimum: Some(1.0), maximum: Some(65535.0), multiple_of: None }
    );
    // Mode untouched — this is a read-only query, not an edit-mode entry.
    assert!(matches!(s.mode, confy_core::session::state::Mode::Normal));
}

#[test]
fn committing_a_schema_violating_value_sets_an_advisory_status_with_suggested_values() {
    let mut s = session_from("level = \"debug\"\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "level": { "enum": ["debug", "info"] } }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    s.cursor = vec![Seg::Key("level".into())];
    // commit_edit's one-shot path bypasses the picker (existing precedent
    // above) — write an out-of-enum value directly, as the free-form popup
    // editor (BeginEditExternal) would.
    s.dispatch(Intent::CommitEdit { value: Some("\"trace\"".into()), name: None });
    // Soft constraint: the write still succeeds.
    assert!(s.serialize().unwrap().contains("level = \"trace\""));
    assert_eq!(s.schema.as_ref().unwrap().violations.len(), 1);
    let notice = s.notice.as_ref().expect("advisory notice set on violation");
    assert!(notice.text.contains("debug"), "notice suggests valid values: {}", notice.text);
    assert!(notice.text.contains("info"), "notice suggests valid values: {}", notice.text);
}

#[test]
fn committing_a_schema_compliant_value_leaves_notice_untouched() {
    let mut s = session_from("level = \"debug\"\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "level": { "enum": ["debug", "info"] } }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    s.cursor = vec![Seg::Key("level".into())];
    s.dispatch(Intent::CommitEdit { value: Some("\"info\"".into()), name: None });
    assert!(s.schema.as_ref().unwrap().violations.is_empty());
    assert!(s.notice.is_none());
}

// ---- Task 14 dirty-check: skip revalidate_schema() when the mutated path
// carries no constraint on a fully_analyzable schema (2026-08-11 audit
// remediation). Verified behaviorally, not by pointer identity (an empty or
// freshly-freed-then-reused Vec allocation can share an address by
// coincidence — flaky): seed `violations` with a sentinel entry no real
// `validate()` run would ever produce, then check whether it survived.

fn sentinel_violation() -> confy_core::schema::Violation {
    confy_core::schema::Violation {
        path: vec![],
        pointer: "/__sentinel__".into(),
        keyword: "__sentinel__".into(),
        message: "planted by the dirty-check test — must survive a skipped revalidate".into(),
        category: Category::Value,
    }
}

#[test]
fn dirty_check_skips_revalidate_for_an_unconstrained_path_on_a_fully_analyzable_schema() {
    let mut s = session_from("level = 5\nname = \"x\"\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "level": { "type": "string" } }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    assert!(
        s.schema.as_ref().unwrap().fully_analyzable,
        "plain properties-only schema must be fully_analyzable"
    );
    // Plant the sentinel, replacing whatever real (level-type-mismatch)
    // violation apply_schema_text's own revalidate produced.
    s.schema.as_mut().unwrap().violations = vec![sentinel_violation()];
    s.cursor = vec![Seg::Key("name".into())];
    // "name" carries no schema constraint (not in `properties`) — the
    // dirty-check should conclude "unconstrained" and skip revalidate_schema().
    s.dispatch(Intent::CommitEdit {
        value: Some("\"y\"".into()),
        name: None,
    });
    let violations = &s.schema.as_ref().unwrap().violations;
    assert_eq!(
        violations.len(),
        1,
        "sentinel must survive untouched: {violations:?}"
    );
    assert_eq!(violations[0].keyword, "__sentinel__");
}

#[test]
fn dirty_check_always_revalidates_when_the_schema_is_not_fully_analyzable() {
    let mut s = session_from("level = 5\nname = \"x\"\n", DocFormat::Toml);
    // `allOf` disqualifies the whole document from the dirty-check's simple
    // properties/items model — the conservative fallback must always engage.
    let schema_text = json!({
        "type": "object",
        "allOf": [{ "properties": { "level": { "type": "string" } } }]
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    assert!(
        !s.schema.as_ref().unwrap().fully_analyzable,
        "a schema using allOf must not be fully_analyzable"
    );
    s.schema.as_mut().unwrap().violations = vec![sentinel_violation()];
    s.cursor = vec![Seg::Key("name".into())];
    // Even a mutation to a path with no *direct* properties entry must
    // revalidate — the conservative fallback ignores the dirty-check
    // entirely once fully_analyzable is false.
    s.dispatch(Intent::CommitEdit {
        value: Some("\"y\"".into()),
        name: None,
    });
    let violations = &s.schema.as_ref().unwrap().violations;
    assert!(
        violations.iter().all(|v| v.keyword != "__sentinel__"),
        "sentinel must be overwritten by a real revalidate: {violations:?}"
    );
}

#[test]
fn dirty_check_revalidates_and_surfaces_a_new_violation_for_a_constrained_path() {
    let mut s = session_from("port = 8080\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "port": { "type": "integer", "maximum": 65535 } }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    assert!(s.schema.as_ref().unwrap().fully_analyzable);
    assert!(s.schema.as_ref().unwrap().violations.is_empty(), "8080 conforms");
    s.schema.as_mut().unwrap().violations = vec![sentinel_violation()];
    s.cursor = vec![Seg::Key("port".into())];
    // "port" IS in `properties` with a `maximum` — a constrained path, so the
    // dirty-check must say "constrained" and let revalidate_schema() run,
    // which both clears the sentinel and surfaces the real new violation.
    s.dispatch(Intent::CommitEdit {
        value: Some("99999".into()),
        name: None,
    });
    let violations = &s.schema.as_ref().unwrap().violations;
    assert!(
        violations.iter().all(|v| v.keyword != "__sentinel__"),
        "sentinel must be overwritten: {violations:?}"
    );
    assert_eq!(violations.len(), 1, "the maximum violation: {violations:?}");
    assert_eq!(violations[0].keyword, "maximum");
}

use confy_core::model::node::Seg as HeadlessSeg;
use confy_core::session::{Intent as HeadlessIntent, Session as HeadlessSession};

fn session_from_toml(src: &str) -> HeadlessSession {
    HeadlessSession::new(AnyDocument::from_str_as(src, DocFormat::Toml).unwrap())
}

#[test]
fn detect_schema_intent_sets_pending_schema_fetch() {
    let mut s = session_from_toml("port = 1\n");
    // No hint yet: dispatch is a no-op.
    let _ = s.dispatch(HeadlessIntent::DetectSchema);
    assert_eq!(s.pending_schema_fetch, None);

    let mut s = session_from_toml("#:schema ./s.json\nport = 1\n");
    // `Session::new` already ran detection once (session.rs:72) — drain that first.
    let _ = s.pending_schema_fetch.take();
    let snap = s.dispatch(HeadlessIntent::DetectSchema);
    assert_eq!(
        snap.schema_fetch_request,
        Some(SchemaSource::Local("./s.json".into()))
    );
}

#[test]
fn schema_violations_is_empty_without_a_loaded_schema() {
    let s = session_from_toml("port = 1\n");
    assert!(s.schema_violations().is_empty());
}

#[test]
fn schema_violations_carries_the_violating_node_text_range() {
    let mut s = session_from_toml("port = \"not-a-number\"\n");
    s.apply_schema_text(
        SchemaSource::Local("./s.json".into()),
        Ok(r#"{"type":"object","properties":{"port":{"type":"integer"}}}"#.to_string()),
    );
    let violations = s.schema_violations();
    assert_eq!(violations.len(), 1);
    let v = &violations[0];
    assert_eq!(v.path, vec![HeadlessSeg::Key("port".into())]);
    assert_eq!(v.keyword, "type");
    let (start, end) = v.text_range.expect("port node resolves");
    // `Node.text_range` for a TOML entry spans the whole `key = value` (ADR
    // 0006 / Outline-anchoring policy, `model/node.rs`'s `Node::text_range`
    // doc comment) — not just the value token. `schema_violations` reuses
    // that same node range rather than inventing a narrower one.
    assert_eq!(
        &"port = \"not-a-number\"\n"[start as usize..end as usize],
        "port = \"not-a-number\""
    );
}
