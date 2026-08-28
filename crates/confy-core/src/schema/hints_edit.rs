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

/// A short, non-widget descriptive line for the node's schema constraint —
/// `description`, `type`, `format`, and/or `pattern` — independent of
/// `resolve_edit_hint`'s widget-selection job (that only models `enum`/
/// `const`/numeric bounds). Surfaced by hosts alongside `EditHint::describe()`
/// so a plain `{"type": "string"}` field (the common, non-enum/bounded case)
/// still has *something* to show in a persistent schema-info surface, not
/// only on violation. `None` when the path is unresolvable or the resolved
/// subschema carries none of the four keywords.
pub fn resolve_schema_info(schema: &Json, path: &Path) -> Option<String> {
    let sub = resolve_subschema(schema, schema, path)?;
    let sub = deref(schema, sub)?;
    let mut lines = Vec::new();
    if let Some(d) = sub.get("description").and_then(Json::as_str) {
        lines.push(d.to_string());
    }
    if let Some(t) = schema_type_line(sub) {
        lines.push(t);
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

/// "Type: string" / "Type: string | null" / "Type: string · Format: email" /
/// "Format: email · Pattern: ^[a-z]+$" — `None` when the subschema has none
/// of `type`/`format`/`pattern`.
fn schema_type_line(sub: &Json) -> Option<String> {
    let type_str = match sub.get("type") {
        Some(Json::String(s)) => Some(s.clone()),
        Some(Json::Array(arr)) => {
            let parts: Vec<&str> = arr.iter().filter_map(Json::as_str).collect();
            (!parts.is_empty()).then(|| parts.join(" | "))
        }
        _ => None,
    };
    let mut parts = Vec::new();
    if let Some(t) = type_str {
        parts.push(format!("Type: {t}"));
    }
    if let Some(f) = sub.get("format").and_then(Json::as_str) {
        parts.push(format!("Format: {f}"));
    }
    if let Some(p) = sub.get("pattern").and_then(Json::as_str) {
        parts.push(format!("Pattern: {p}"));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// Walk `path` from the schema root, following `properties`/`patternProperties`/
/// `items` and resolving same-document `$ref`s along the way.
fn resolve_subschema<'a>(root: &'a Json, current: &'a Json, path: &[Seg]) -> Option<&'a Json> {
    let current = deref(root, current)?;
    match path.split_first() {
        None => Some(current),
        Some((Seg::Key(k), rest)) => {
            let next = current
                .get("properties")
                .and_then(|p| p.get(k))
                .or_else(|| pattern_property_match(current, k))?;
            resolve_subschema(root, next, rest)
        }
        Some((Seg::Index(_), rest)) => {
            let next = current.get("items")?;
            resolve_subschema(root, next, rest)
        }
    }
}

/// First `patternProperties` entry whose key (an ECMA-style regex) matches
/// `key` — the dictionary-of-named-objects idiom (e.g. a schema keyed by
/// arbitrary task/host names via `"^[a-zA-Z0-9_]+$"`, with no `properties`
/// at all). An unparsable pattern is skipped, not fatal — same
/// safe-if-unsure-say-no-hint polarity as the rest of this module.
fn pattern_property_match<'a>(schema: &'a Json, key: &str) -> Option<&'a Json> {
    let patterns = schema.get("patternProperties")?.as_object()?;
    patterns.iter().find_map(|(pattern, sub)| {
        regex::Regex::new(pattern)
            .ok()
            .filter(|re| re.is_match(key))
            .map(|_| sub)
    })
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
        return EditHint::Bounded {
            minimum,
            maximum,
            multiple_of,
        };
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
