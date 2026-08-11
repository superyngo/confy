//! Per-mutation "does this path carry any schema constraint" check — the
//! Task 14 (2026-08-11 audit remediation) optimization that lets
//! `Session::on_mutation_success` skip a full `revalidate_schema()` walk
//! when the answer is no.
//!
//! Deliberately a separate module from `hints_edit.rs`, not folded into it:
//! that file's `EditHint::None` return means "safe to fall back to
//! plain-text editing" (conservative-if-unsure-say-no-hint). This module's
//! "unsure" case must mean the OPPOSITE polarity — "assume constrained,
//! revalidate" — since silently skipping a real validation would let a
//! violation go stale. Mixing the two in one file risks a future edit
//! flipping the wrong one's safe direction.
//!
//! `path_is_constrained`'s `false` result is only trustworthy when
//! `is_fully_analyzable(schema)` was already confirmed `true` for the whole
//! document (checked once in `Session::apply_schema_text`, cached on
//! `SchemaState::fully_analyzable`) — this module only understands
//! `properties`/`items` composition plus same-document `$ref`, the same
//! subset `hints_edit.rs` understands.

use super::hints_edit::deref;
use crate::model::node::{Path, Seg};
use serde_json::Value as Json;

/// Keywords that constrain a schema object's own children in ways this
/// module doesn't model (dynamic key sets, extra structural rules beyond
/// plain `properties`/`items`). Their presence anywhere along the walk
/// means "unsure" — bail to `true` per this module's safe-unsure polarity.
const UNMODELED_STRUCTURAL_KEYWORDS: &[&str] = &[
    "required",
    "additionalProperties",
    "patternProperties",
    "propertyNames",
    "unevaluatedProperties",
    "unevaluatedItems",
    "contains",
    "prefixItems",
    "additionalItems",
];

/// Keywords that directly constrain a scalar/value's own shape — presence
/// on the FINAL resolved subschema means the mutated path is constrained.
const VALUE_KEYWORDS: &[&str] = &[
    "type",
    "enum",
    "const",
    "pattern",
    "format",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "multipleOf",
    "minLength",
    "maxLength",
    "minItems",
    "maxItems",
    "uniqueItems",
];

/// Whole-schema-document walk, done once per `SetSchema`/`SchemaLoaded` (not
/// per mutation) — see `SchemaState::fully_analyzable`. `true` iff the
/// schema uses only same-document `$ref`/`properties`/`items` composition:
/// no remote `$ref`, `allOf`/`not`/`if`/`then`/`else`, or `oneOf`/`anyOf`
/// beyond the bare-`const` carve-out `hints_edit.rs` already special-cases.
pub(crate) fn is_fully_analyzable(schema: &Json) -> bool {
    match schema {
        Json::Object(map) => {
            if let Some(r) = map.get("$ref").and_then(Json::as_str) {
                if !r.starts_with('#') {
                    return false;
                }
            }
            if map.contains_key("allOf")
                || map.contains_key("not")
                || map.contains_key("if")
                || map.contains_key("then")
                || map.contains_key("else")
            {
                return false;
            }
            for key in ["oneOf", "anyOf"] {
                if let Some(branches) = map.get(key).and_then(Json::as_array) {
                    if !branches.iter().all(is_bare_const_branch) {
                        return false;
                    }
                }
            }
            map.values().all(is_fully_analyzable)
        }
        Json::Array(items) => items.iter().all(is_fully_analyzable),
        _ => true,
    }
}

/// A `oneOf`/`anyOf` branch that resolves through the carve-out: a bare
/// `{const, title?, description?}` object, no other keywords — mirrors
/// `hints_edit.rs`'s own per-branch shape check (`oneof_of_const`).
fn is_bare_const_branch(branch: &Json) -> bool {
    match branch {
        Json::Object(map) => {
            map.contains_key("const")
                && map
                    .keys()
                    .all(|k| matches!(k.as_str(), "const" | "title" | "description"))
        }
        _ => false,
    }
}

/// Whether `path` carries any schema constraint, given `schema` is already
/// confirmed `is_fully_analyzable`. Walks `properties`/`items` (mirroring
/// `hints_edit::resolve_subschema`'s traversal, sharing its `deref` helper)
/// and bails to `true` — the safe "unsure, assume constrained" answer — the
/// moment it sees a structural keyword it doesn't model. Only when the
/// entire walk resolves cleanly does it check the final subschema for a
/// direct value-constraining keyword.
pub(crate) fn path_is_constrained(schema: &Json, path: &Path) -> bool {
    walk(schema, schema, path)
}

fn walk(root: &Json, current: &Json, path: &[Seg]) -> bool {
    let Some(current) = deref(root, current) else {
        // An unresolved same-document $ref: `is_fully_analyzable` already
        // excludes remote refs, so a miss here means a dangling pointer.
        // Stay safe rather than assume no constraint.
        return true;
    };
    if has_unmodeled_structural_keyword(current) {
        return true;
    }
    match path.split_first() {
        None => has_value_keyword(current),
        Some((Seg::Key(k), rest)) => match current.get("properties").and_then(|p| p.get(k)) {
            Some(next) => walk(root, next, rest),
            // Not explicitly covered by `properties`, and no
            // additionalProperties/patternProperties seen above (already
            // bailed on those) — genuinely unconstrained.
            None => false,
        },
        Some((Seg::Index(_), rest)) => match current.get("items") {
            Some(next) => walk(root, next, rest),
            None => false,
        },
    }
}

fn has_unmodeled_structural_keyword(schema: &Json) -> bool {
    let Json::Object(map) = schema else {
        return false;
    };
    UNMODELED_STRUCTURAL_KEYWORDS
        .iter()
        .any(|k| map.contains_key(*k))
}

fn has_value_keyword(schema: &Json) -> bool {
    let Json::Object(map) = schema else {
        return false;
    };
    VALUE_KEYWORDS.iter().any(|k| map.contains_key(*k))
}
