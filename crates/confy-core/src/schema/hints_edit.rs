//! Best-effort resolution of the applicable sub-schema at one target `Path`,
//! for constrained inline editing (spec §3). Deliberately simpler than
//! `validate.rs`: resolves through `properties`/`items`/local `$defs` +
//! same-document `$ref`, plus a narrow `oneOf`/`anyOf`-of-`const` carve-out
//! (the single most common real-world enum-with-descriptions idiom). Any
//! other composition (`allOf`/`not`/`if-then-else`, a `oneOf`/`anyOf` branch
//! carrying more than `const`/`title`/`description`) or a remote `$ref`
//! declines to `EditHint::None` — `validate.rs` still enforces those fully,
//! only the editing *widget* stays plain text.

use super::types::EditHint;
use crate::model::node::{Path, Seg};
use serde_json::Value as Json;

pub fn resolve_edit_hint(schema: &Json, path: &Path) -> EditHint {
    let Some(sub) = resolve_subschema(schema, schema, path) else {
        return EditHint::None;
    };
    hint_from_subschema(schema, sub)
}

/// Walk `path` from the schema root, following `properties`/`items` and
/// resolving same-document `$ref`s along the way.
fn resolve_subschema<'a>(root: &'a Json, current: &'a Json, path: &[Seg]) -> Option<&'a Json> {
    let current = deref(root, current)?;
    match path.split_first() {
        None => Some(current),
        Some((Seg::Key(k), rest)) => {
            let next = current.get("properties")?.get(k)?;
            resolve_subschema(root, next, rest)
        }
        Some((Seg::Index(_), rest)) => {
            let next = current.get("items")?;
            resolve_subschema(root, next, rest)
        }
    }
}

/// Resolve a single `$ref` hop if present — same-document only (`#/...`).
/// A remote `$ref` (no leading `#`) returns `None` unresolved, which
/// `resolve_subschema` propagates as "no hint" (spec: "remote `$ref`
/// resolution" is out of scope for editing hints).
pub(crate) fn deref<'a>(root: &'a Json, schema: &'a Json) -> Option<&'a Json> {
    let Some(r) = schema.get("$ref").and_then(Json::as_str) else {
        return Some(schema);
    };
    let pointer = r.strip_prefix('#')?;
    root.pointer(pointer)
}

fn hint_from_subschema(root: &Json, sub: &Json) -> EditHint {
    let Some(sub) = deref(root, sub) else {
        return EditHint::None;
    };
    if let Some(values) = sub.get("enum").and_then(Json::as_array) {
        return EditHint::Enum(
            values
                .iter()
                .map(|v| (display_label(v), v.clone()))
                .collect(),
        );
    }
    if let Some(v) = sub.get("const") {
        return EditHint::Enum(vec![(display_label(v), v.clone())]);
    }
    if let Some(opts) = oneof_of_const(root, sub) {
        return EditHint::Enum(opts);
    }
    let minimum = sub.get("minimum").and_then(Json::as_f64);
    let maximum = sub.get("maximum").and_then(Json::as_f64);
    let multiple_of = sub.get("multipleOf").and_then(Json::as_f64);
    if minimum.is_some() || maximum.is_some() || multiple_of.is_some() {
        return EditHint::Bounded { minimum, maximum, multiple_of };
    }
    EditHint::None
}

/// The `oneOf`/`anyOf`-of-`const` carve-out: every branch must be a bare
/// `{const, title?, description?}` object (no other keywords) for this to
/// fire — any richer branch (e.g. carrying its own `type`/`properties`) is
/// true composition and declines to `None`.
fn oneof_of_const(root: &Json, sub: &Json) -> Option<Vec<(String, Json)>> {
    let branches = sub
        .get("oneOf")
        .or_else(|| sub.get("anyOf"))
        .and_then(Json::as_array)?;
    let allowed_keys = ["const", "title", "description"];
    let mut opts = Vec::with_capacity(branches.len());
    for branch in branches {
        let branch = deref(root, branch)?;
        let obj = branch.as_object()?;
        if obj.keys().any(|k| !allowed_keys.contains(&k.as_str())) {
            return None; // richer branch — true composition, decline
        }
        let value = obj.get("const")?;
        let label = obj
            .get("title")
            .and_then(Json::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| display_label(value));
        opts.push((label, value.clone()));
    }
    Some(opts)
}

fn display_label(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        other => other.to_string(),
    }
}
