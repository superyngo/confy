//! Core types for JSON Schema support. See `CONTEXT.md` § Schema for the
//! canonical vocabulary (JSON projection, Violation, Soft constraint).

use crate::model::node::Path;
use serde::{Deserialize, Serialize};

/// Where a schema came from — a relative/absolute local path, or a URL.
/// Never resolved to bytes by confy-core itself; hosts do the I/O (see
/// `Session::apply_schema_text`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchemaSource {
    Local(String),
    Url(String),
}

/// Whether a Violation is an ordinary value mismatch, or a case where the
/// document's *source format* cannot represent what the schema requires
/// (e.g. `type: null` against a TOML-sourced node, which has no null
/// literal). Both are soft — see `CONTEXT.md` § Schema "Soft constraint".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Category {
    Value,
    Representation,
}

/// A single JSON Schema constraint failure. Purely informational: never
/// blocks a Mutation, never appears in a `MutateError`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Violation {
    /// The Node this violation is reported against — the failing node itself,
    /// or (for a `required` failure, whose JSON Pointer targets the parent
    /// object that's missing a child) the parent's Path.
    pub path: Path,
    /// The raw JSON Pointer `jsonschema` reported (RFC 6901).
    pub pointer: String,
    /// The failing schema keyword (`"type"`, `"enum"`, `"required"`, …).
    pub keyword: String,
    /// Human-readable message, as `jsonschema` renders it.
    pub message: String,
    pub category: Category,
}

/// A resolved editing constraint for one node, used to swap the inline
/// editor's plain text input for a constrained widget (enum/const picker,
/// numeric bounds). Deliberately does not attempt to resolve `allOf`/
/// `oneOf`/`anyOf`/`not`/`if-then-else` (beyond the narrow oneOf/anyOf-of-const
/// carve-out) or remote `$ref` — those fall through to `None`. `validate()`
/// still fully enforces them regardless; only the *widget* stays plain text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EditHint {
    /// `(display_label, value)` pairs — from a schema `enum`, `const`, or a
    /// `oneOf`/`anyOf` where every branch is a bare `{const, title?,
    /// description?}`.
    Enum(Vec<(String, serde_json::Value)>),
    Bounded {
        minimum: Option<f64>,
        maximum: Option<f64>,
        multiple_of: Option<f64>,
    },
    None,
}

/// Per-session schema state. Lives on `Session`, not `Node`/`NodeTree` — the
/// projected tree is rebuilt from the document on every mutation, so
/// per-document state belongs one level up (mirrors `Session.clipboard`,
/// `Session.filter`, etc.).
#[derive(Debug)]
pub struct SchemaState {
    pub source: SchemaSource,
    /// `None` while `load_error` is set (load/compile failed) or before the
    /// host has resolved `schema_fetch_request`.
    pub compiled: Option<jsonschema::Validator>,
    /// The raw (uncompiled) schema JSON — `hints_edit::resolve_edit_hint`
    /// walks this directly (it needs keyword introspection the compiled
    /// `Validator` doesn't expose).
    pub raw: Option<serde_json::Value>,
    pub violations: Vec<Violation>,
    pub load_error: Option<String>,
}

/// Document-level summary surfaced to hosts (status line / toolbar).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SchemaStatus {
    pub source_label: String,
    pub violation_count: usize,
    pub load_error: Option<String>,
}

impl SchemaState {
    pub fn status(&self) -> SchemaStatus {
        let source_label = match &self.source {
            SchemaSource::Local(p) => p.clone(),
            SchemaSource::Url(u) => u.clone(),
        };
        SchemaStatus {
            source_label,
            violation_count: self.violations.len(),
            load_error: self.load_error.clone(),
        }
    }
}
