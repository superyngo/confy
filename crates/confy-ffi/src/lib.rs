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

    /// Current document text — the host writes/downloads this on save.
    pub fn serialize(&self) -> String {
        self.session.serialize().unwrap_or_default()
    }

    pub fn is_dirty(&self) -> bool {
        self.session.is_dirty()
    }

    pub fn doc_format(&self) -> String {
        format_name(self.session.doc_format())
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

    /// Convenience accessor: the current external-edit request (if any), as
    /// `{ initial, kind }`. The host opens its async modal with `initial`.
    pub fn external_edit(&self) -> Result<JsValue, JsValue> {
        match self.session.snapshot().external_edit {
            Some(e) => to_value(&e).map_err(js_serde_error),
            None => Ok(JsValue::UNDEFINED),
        }
    }
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
