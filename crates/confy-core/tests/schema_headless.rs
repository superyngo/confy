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
