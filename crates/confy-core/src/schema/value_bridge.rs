//! Node+Value → **JSON projection** bridging (`CONTEXT.md` § Schema).
//!
//! Both the `Node` tree (paths, no decoded scalars) and the `Value` tree from
//! `ConfigDocument::to_value()` (decoded scalars, no paths) are order-preserving
//! 1:1 walks of the *same* backing document at every nesting level — every
//! child, including Comment nodes/`Item::Comment`, in document order (see
//! `CONTEXT.md` § Projection: "the backing document — not the Node tree — is
//! the single source of truth"). `bridge()` walks them together by position,
//! skipping Comment/`Item::Comment` pairs, to attach a `Path` to every JSON
//! projection node without reimplementing per-format scalar decoding (already
//! correctly done by `to_value()`).

use crate::model::node::{Node, NodeKind, Path};
use crate::model::value::{Item, Value};
use serde_json::{Map, Number, Value as Json};
use std::collections::HashMap;

/// JSON Pointer (RFC 6901 string, e.g. `/server/port`; `""` = document root)
/// → the Node `Path` it came from.
#[derive(Default)]
pub struct PointerMap(HashMap<String, Path>);

impl PointerMap {
    fn insert(&mut self, pointer: String, path: Path) {
        self.0.insert(pointer, path);
    }

    /// Resolve a violation's JSON Pointer to a Node Path. Falls back to the
    /// nearest ancestor pointer (strips one trailing `/segment` at a time)
    /// for any pointer the walk didn't visit directly — a defensive default,
    /// not the primary path: a `required` failure's pointer *is* the parent
    /// object, which the walk always visits and maps.
    pub fn resolve(&self, pointer: &str) -> Option<&Path> {
        let mut p = pointer;
        loop {
            if let Some(path) = self.0.get(p) {
                return Some(path);
            }
            match p.rfind('/') {
                Some(i) => p = &p[..i],
                None => return self.0.get(""),
            }
        }
    }
}

/// Lower `root`/`root_value` into a JSON projection, building the pointer map
/// as it goes.
pub fn bridge(root: &Node, root_value: &Value) -> (Json, PointerMap) {
    let mut map = PointerMap::default();
    let json = walk(root, root_value, "", &mut map);
    (json, map)
}

/// Lower a bare `Value` (no `Node`/`Path` pairing) into a JSON projection —
/// used where only the decoded shape is needed (schema-hint detection,
/// parsing a fetched schema document itself), not per-node violation
/// pointers. Mirrors `walk`'s scalar/container mapping minus the pointer
/// bookkeeping; `Item::Comment` entries are dropped.
pub fn value_to_json(value: &Value) -> Json {
    match value {
        Value::Null => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Int(i) => Json::Number(Number::from(*i)),
        Value::Float(f) if f.is_finite() => {
            Number::from_f64(*f).map(Json::Number).unwrap_or(Json::Null)
        }
        Value::Float(f) => Json::String(if f.is_nan() {
            "nan".to_string()
        } else if *f > 0.0 {
            "inf".to_string()
        } else {
            "-inf".to_string()
        }),
        Value::Str(s) | Value::Datetime(s) => Json::String(s.clone()),
        Value::Seq(items) => Json::Array(
            items
                .iter()
                .filter_map(|it| match it {
                    Item::Node { value, .. } => Some(value_to_json(value)),
                    Item::Comment(_) => None,
                })
                .collect(),
        ),
        Value::Map(items) => {
            let mut obj = Map::new();
            for it in items {
                if let Item::Node {
                    key: Some(k),
                    value,
                    ..
                } = it
                {
                    obj.insert(k.clone(), value_to_json(value));
                }
            }
            Json::Object(obj)
        }
    }
}

fn walk(node: &Node, value: &Value, pointer: &str, map: &mut PointerMap) -> Json {
    map.insert(pointer.to_string(), node.path.clone());
    match value {
        Value::Null => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Int(i) => Json::Number(Number::from(*i)),
        Value::Float(f) if f.is_finite() => {
            Number::from_f64(*f).map(Json::Number).unwrap_or(Json::Null)
        }
        Value::Float(f) => Json::String(if f.is_nan() {
            "nan".to_string()
        } else if *f > 0.0 {
            "inf".to_string()
        } else {
            "-inf".to_string()
        }),
        // TOML datetimes have no JSON Schema-native type: bridged as a string
        // (RFC3339-shaped source text passes `format: date-time`/`date`/`time`
        // checks as-is; a schema requiring `type: null` against a TOML node
        // and other representation gaps are flagged by `validate.rs`, not here).
        Value::Str(s) | Value::Datetime(s) => Json::String(s.clone()),
        Value::Seq(items) => {
            let mut arr = Vec::new();
            let mut idx = 0usize;
            let mut child_nodes = node
                .children
                .iter()
                .filter(|c| !matches!(c.kind, NodeKind::Comment(_)));
            for it in items {
                let Item::Node { value, .. } = it else {
                    continue;
                };
                if let Some(child) = child_nodes.next() {
                    let child_pointer = format!("{pointer}/{idx}");
                    arr.push(walk(child, value, &child_pointer, map));
                    idx += 1;
                }
            }
            Json::Array(arr)
        }
        Value::Map(items) => {
            let mut obj = Map::new();
            let mut child_nodes = node
                .children
                .iter()
                .filter(|c| !matches!(c.kind, NodeKind::Comment(_)));
            for it in items {
                let Item::Node {
                    key: Some(k),
                    value,
                    ..
                } = it
                else {
                    continue;
                };
                if let Some(child) = child_nodes.next() {
                    let child_pointer = format!("{pointer}/{}", escape_pointer_segment(k));
                    obj.insert(k.clone(), walk(child, value, &child_pointer, map));
                }
            }
            Json::Object(obj)
        }
    }
}

/// RFC 6901 pointer-segment escaping (`~` → `~0`, `/` → `~1`).
fn escape_pointer_segment(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}
