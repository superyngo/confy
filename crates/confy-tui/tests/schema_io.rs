use confy_core::schema::SchemaSource;
use confy_tui::tui::schema_io::resolve_schema_source;
use std::fs;

#[test]
fn resolves_a_local_relative_path_against_the_open_files_directory() {
    let dir = std::env::temp_dir().join("confy_schema_io_test");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("s.json"), r#"{"type":"object"}"#).unwrap();
    let source = SchemaSource::Local("./s.json".into());
    let result = resolve_schema_source(&source, &dir);
    assert_eq!(result, Ok(r#"{"type":"object"}"#.to_string()));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_missing_local_file_is_a_soft_error_not_a_panic() {
    let dir = std::env::temp_dir().join("confy_schema_io_test_missing");
    let source = SchemaSource::Local("./nope.json".into());
    let result = resolve_schema_source(&source, &dir);
    assert!(result.is_err());
}
