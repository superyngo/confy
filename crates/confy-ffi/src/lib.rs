//! `confy-ffi` — the WebAssembly FFI wrapper over `confy-core`.
//!
//! Exposes the `Session` state machine to JavaScript via `wasm-bindgen` +
//! `serde-wasm-bindgen`. One command channel: [`ConfySession::dispatch`] takes an
//! [`Intent`] (a plain JS object) and returns a [`SessionSnapshot`] (full-state
//! transport — PORTING §8.3). See `WEBUI.md` for the contract.
//!
//! The wire types (`Intent`, `SessionSnapshot`, `ViewRow`, `Seg`, …) are the
//! `serde` representations of the `confy-core` types; `serde-wasm-bindgen`
//! marshals them, so adding a Rust field is the only change needed (no per-field
//! FFI plumbing). The hand-written `web/types.ts` is the canonical TS view.

use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::{ConfigDocument, DocFormat, KindTarget};
use confy_core::model::node::Path;
use confy_core::session::{Intent, Session, ViewRow};
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::{from_value, to_value};
use wasm_bindgen::prelude::*;

/// Options for a kind-switch popup entry, mirrored in TS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KindOption {
    pub label: String,
    pub target: KindTarget,
}

/// The JS-facing handle on a confy editing session. Construct via [`from_text`],
/// then drive with [`dispatch`]. The host owns all file I/O (load bytes in, write
/// `serialize()` out); this type never touches the filesystem.
#[wasm_bindgen]
pub struct ConfySession {
    session: Session,
}

#[wasm_bindgen]
impl ConfySession {
    /// Parse `text` as `format` and open a session. Throws a JS `Error` on a
    /// parse failure (the host catches and reports).
    #[wasm_bindgen(constructor)]
    pub fn from_text(text: &str, format: &str) -> Result<ConfySession, JsValue> {
        let format = parse_format(format)?;
        let doc = AnyDocument::from_str_as(text, format)
            .map_err(|e| js_error(&format!("parse error: {e}")))?;
        Ok(ConfySession {
            session: Session::new(doc),
        })
    }

    /// The single command channel: send an `Intent` (JS object matching the
    /// `Intent` serde shape), receive a full `SessionSnapshot`. The UI re-renders
    /// from the snapshot (full-state transport, no diff).
    pub fn dispatch(&mut self, intent: JsValue) -> Result<JsValue, JsValue> {
        let intent: Intent = from_value(intent).map_err(js_serde_error)?;
        let snap = self.session.dispatch(intent);
        to_value(&snap).map_err(js_serde_error)
    }

    /// Re-pull the full renderable state without mutating.
    pub fn snapshot(&self) -> Result<JsValue, JsValue> {
        to_value(&self.session.snapshot()).map_err(js_serde_error)
    }

    /// Convenience: just the visible rows.
    pub fn visible_rows(&self) -> Result<JsValue, JsValue> {
        let rows: Vec<ViewRow> = self.session.visible_rows();
        to_value(&rows).map_err(js_serde_error)
    }

    /// Diagnostics log (oldest-first sequence of `DiagEvent`s recorded by core).
    pub fn diag_log(&self) -> Result<JsValue, JsValue> {
        let events: Vec<_> = self.session.diag.iter().collect();
        to_value(&events).map_err(js_serde_error)
    }

    /// Current document text — the host writes/downloads this on save.
    pub fn serialize(&self) -> String {
        self.session.serialize().unwrap_or_default()
    }

    pub fn is_dirty(&self) -> bool {
        self.session.is_dirty()
    }

    /// Host-supplied: true iff the open document's real file extension is
    /// plain `.json` (not `.jsonc`) — confy-core is extension-blind
    /// (`DocFormat::Json` covers both), so only the host knows this. Drives
    /// the per-row `comment_advisory` decoration (`ViewRow.comment_advisory`).
    /// The host calls this once right after `from_text`, before the first
    /// `snapshot()`/`visible_rows()`.
    pub fn set_strict_json(&mut self, v: bool) {
        self.session.strict_json = v;
    }

    /// Whether the open document already contained a comment when it was
    /// loaded — drives the host's one-shot "file already had comments" toast.
    /// `false` for a non-JSON document.
    pub fn had_comments_at_open(&self) -> bool {
        self.session
            .doc
            .as_ref()
            .is_some_and(|d| d.had_comments_at_open())
    }

    pub fn doc_format(&self) -> String {
        format_name(self.session.doc_format())
    }

    /// About-tab body text for the session's current language — the single
    /// source of truth (`confy_core::session::state::about_text`), so the web
    /// host never hand-mirrors it.
    pub fn about_text(&self) -> String {
        confy_core::session::state::about_text(self.session.lang).to_string()
    }

    /// Per-node convertible kinds for the `K` popup.
    pub fn kind_options(&self, path: JsValue) -> Result<JsValue, JsValue> {
        let path: Path = from_value(path).map_err(js_serde_error)?;
        let opts: Vec<KindOption> = self
            .session
            .doc
            .as_ref()
            .map(|d| {
                d.kind_options(&path)
                    .into_iter()
                    .map(|(label, target)| KindOption { label, target })
                    .collect()
            })
            .unwrap_or_default();
        to_value(&opts).map_err(js_serde_error)
    }

    /// Schema-driven editing hint for the node at `path` (enum/const options
    /// or numeric bounds, `EditHint::None` when unconstrained) — read-only,
    /// does not enter edit mode. Used for the desktop hover tooltip and to
    /// decide whether the detail panel should render a schema-select widget
    /// before dispatching `BeginEdit`.
    pub fn schema_hint(&self, path: JsValue) -> Result<JsValue, JsValue> {
        let path: Path = from_value(path).map_err(js_serde_error)?;
        to_value(&self.session.edit_hint(&path)).map_err(js_serde_error)
    }

    /// Non-widget descriptive schema info for the node at `path` —
    /// `description`/`type`/`format`/`pattern` from the resolved subschema,
    /// `undefined` when unresolvable or none of those keywords are present.
    /// Orthogonal to `schema_hint` (that only models `enum`/`const`/numeric
    /// bounds); this covers the common plain-typed field `schema_hint`
    /// leaves at `None`. Used for the shared web/touch/VS Code detail panel.
    pub fn schema_info(&self, path: JsValue) -> Result<JsValue, JsValue> {
        let path: Path = from_value(path).map_err(js_serde_error)?;
        to_value(&self.session.schema_info(&path)).map_err(js_serde_error)
    }

    /// Immediate children of the node at `path` (breadcrumb mini-tree), as
    /// `ChildView[]` — independent of expansion state.
    pub fn children(&self, path: JsValue) -> Result<JsValue, JsValue> {
        let path: Path = from_value(path).map_err(js_serde_error)?;
        to_value(&self.session.children_of(&path)).map_err(js_serde_error)
    }

    /// Read-only symbol tree for editor Outline/breadcrumb integrations
    /// (`OutlineNode[]`), independent of cursor/expansion state.
    pub fn outline(&self) -> Result<JsValue, JsValue> {
        to_value(&self.session.outline()).map_err(js_serde_error)
    }

    /// Current schema violations with resolved `text_range`s — the
    /// native-editor Diagnostics data source (VS Code schema-hints design).
    pub fn schema_violations(&self) -> Result<JsValue, JsValue> {
        to_value(&self.session.schema_violations()).map_err(js_serde_error)
    }

    /// Pointer-drop classification (Web mouse / touch): "this row, this
    /// relative vertical position" -> the `PasteSlot` it represents, or
    /// `undefined` if the row is no longer visible. Every pointer surface
    /// (click-to-target while armed, drag-drop into/before/after
    /// eligibility) calls this instead of hand-rolling the classification
    /// (ADR 0004 §1).
    pub fn pointer_slot(&self, path: JsValue, rel_y: f32) -> Result<JsValue, JsValue> {
        let path: Path = from_value(path).map_err(js_serde_error)?;
        match self.session.pointer_slot(&path, rel_y) {
            Some(slot) => to_value(&slot).map_err(js_serde_error),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    /// Convenience accessor: the current external-edit request (if any), as
    /// `{ initial, kind }`. The host opens its async modal with `initial`.
    pub fn external_edit(&self) -> Result<JsValue, JsValue> {
        match self.session.snapshot().external_edit {
            Some(e) => to_value(&e).map_err(js_serde_error),
            None => Ok(JsValue::UNDEFINED),
        }
    }
}

/// Char positions in `haystack` that the fuzzy `needle` matched, or `undefined`
/// when it doesn't match (or `needle` is empty).
///
/// A free function, not a `ConfySession` method: it's pure and stateless, and
/// hosts call it per rendered cell while a filter is active to mark the matched
/// characters — the same `SkimMatcherV2` the TUI highlights with, so web and TUI
/// can't drift apart. Indices are **char** offsets (the matcher works on chars),
/// so JS must index via `Array.from(text)`, never `text[i]`. `u32` (not `usize`)
/// because that's what wasm-bindgen marshals to a `Uint32Array`.
#[wasm_bindgen]
pub fn fuzzy_indices(haystack: &str, needle: &str) -> Option<Vec<u32>> {
    confy_core::session::search::fuzzy_indices(haystack, needle)
        .map(|idx| idx.into_iter().map(|i| i as u32).collect())
}

// ---- helpers ----

fn parse_format(s: &str) -> Result<DocFormat, JsValue> {
    match s.to_ascii_lowercase().as_str() {
        "toml" => Ok(DocFormat::Toml),
        "json" | "jsonc" => Ok(DocFormat::Json),
        "yaml" | "yml" => Ok(DocFormat::Yaml),
        other => Err(js_error(&format!(
            "unknown format '{other}' (expected toml/json/yaml)"
        ))),
    }
}

fn format_name(f: DocFormat) -> String {
    match f {
        DocFormat::Toml => "toml",
        DocFormat::Json => "json",
        DocFormat::Yaml => "yaml",
    }
    .to_string()
}

fn js_error(msg: &str) -> JsValue {
    js_sys::Error::new(msg).into()
}

fn js_serde_error(e: serde_wasm_bindgen::Error) -> JsValue {
    js_error(&format!("serde error: {e}"))
}
