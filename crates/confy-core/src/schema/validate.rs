//! `jsonschema`-backed validation over a JSON projection. Full draft 2020-12
//! semantics (composition, `$ref` to the schema's own `$defs`) apply
//! uniformly across TOML/JSON/YAML since this operates on the projection,
//! never on source syntax.

use super::types::{Category, Violation};
use super::value_bridge::PointerMap;
use jsonschema::error::{TypeKind, ValidationErrorKind};
use jsonschema::{JsonType, Validator};
use serde_json::Value as Json;

/// Validate `projection` against `compiled`, returning every Violation.
/// Infallible: `Validator::iter_errors` only panics on malformed schemas,
/// which `Validator::new` already rejects at compile time (surfaced as
/// `SchemaState.load_error`, never reaching this function).
pub fn validate(projection: &Json, compiled: &Validator, map: &PointerMap) -> Vec<Violation> {
    compiled
        .iter_errors(projection)
        .map(|err| {
            let pointer = err.instance_path.to_string();
            let schema_path = err.schema_path.to_string();
            let keyword = schema_path.rsplit('/').next().unwrap_or("").to_string();
            let path = map.resolve(&pointer).cloned().unwrap_or_default();
            let message = err.to_string();
            // A `type: null` mismatch against a TOML-sourced document is a
            // structural representation gap (TOML has no null literal — the
            // bridge never emits `Json::Null` for a TOML scalar), not an
            // ordinary value error the user can fix by editing. Matched on
            // the error's structured kind (not the rendered message text)
            // so a string value that merely contains "null", or a nullable
            // `type` union that includes `"null"` as one alternative, are
            // not misclassified.
            let category = match &err.kind {
                ValidationErrorKind::Type {
                    kind: TypeKind::Single(JsonType::Null),
                } => Category::Representation,
                _ => Category::Value,
            };
            Violation {
                path,
                pointer,
                keyword,
                message,
                category,
            }
        })
        .collect()
}
