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

/// A `Violation` plus its violating node's resolved source-text byte range —
/// the native-editor Diagnostics data source (`Session::schema_violations`).
/// `text_range: None` only if `path` no longer resolves against the current
/// tree (defensive: in practice this is only ever read against the same
/// tree revision the violations were computed from).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ViolationView {
    pub path: Path,
    pub pointer: String,
    pub keyword: String,
    pub message: String,
    pub category: Category,
    pub text_range: Option<(u32, u32)>,
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

impl EditHint {
    /// Format as a standalone advisory sentence — "Valid values: a, b, c" /
    /// "Must be between X and Y, a multiple of Z" — for surfaces that show
    /// the constraint proactively (not tied to a violation message). Mirrors
    /// `web/ui.ts`'s `schemaHintTooltip` wording exactly, so the desktop
    /// hover tooltip and any other host-side rendering read the same. `None`
    /// when unconstrained or nothing resolvable to say.
    pub fn describe(&self) -> Option<String> {
        match self {
            EditHint::None => None,
            EditHint::Enum(options) => {
                let labels: Vec<&str> = options.iter().map(|(l, _)| l.as_str()).collect();
                if labels.is_empty() {
                    None
                } else {
                    Some(format!("Valid values: {}", labels.join(", ")))
                }
            }
            EditHint::Bounded {
                minimum,
                maximum,
                multiple_of,
            } => {
                let mut parts = Vec::new();
                match (minimum, maximum) {
                    (Some(min), Some(max)) => parts.push(format!("between {min} and {max}")),
                    (Some(min), None) => parts.push(format!("at least {min}")),
                    (None, Some(max)) => parts.push(format!("at most {max}")),
                    (None, None) => {}
                }
                if let Some(m) = multiple_of {
                    parts.push(format!("a multiple of {m}"));
                }
                if parts.is_empty() {
                    None
                } else {
                    Some(format!("Must be {}", parts.join(", ")))
                }
            }
        }
    }
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
    /// Whether the whole schema document uses only same-document `$ref`/
    /// `properties`/`items` composition (no remote `$ref`, `allOf`/`not`/
    /// `if`/`then`/`else`, or `oneOf`/`anyOf` beyond the bare-`const`
    /// carve-out) — computed once in `apply_schema_text`. Gates the Task 14
    /// per-mutation dirty-check (`schema::dirty_check::path_is_constrained`):
    /// only `true` here lets `on_mutation_success` skip a full
    /// `revalidate_schema()` walk. `false` for any schema that failed to
    /// compile (`raw: None`) — there's no document to walk.
    pub fully_analyzable: bool,
    pub violations: Vec<Violation>,
    /// Every strict ancestor path of every current violation, including the
    /// root (`vec![]`) — lets a collapsed branch row show a "warning inside"
    /// marker without walking the whole subtree per render. Rebuilt in
    /// lockstep with `violations` by `Session::revalidate_schema`.
    pub warning_ancestors: std::collections::HashSet<Path>,
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
