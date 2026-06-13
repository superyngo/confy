//! Format dispatch: one enum wrapping every backend, so the TUI holds a single
//! concrete type and a new format is one more variant (spec §Phase 1.1).

use crate::model::cst_doc::CstDocument;
use crate::model::document::{ConfigDocument, DocFormat, KindTarget, MutateError, Mutation};
use crate::model::json::JsonDocument;
use crate::model::node::{NodeTree, Seg};
use crate::model::yaml::YamlDocument;
use std::path::Path as FsPath;

pub enum AnyDocument {
    Toml(CstDocument),
    Json(JsonDocument),
    Yaml(YamlDocument),
}

/// Format from the file extension. `None` = unrecognized.
pub fn detect_format(path: &FsPath) -> Option<DocFormat> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("toml") => Some(DocFormat::Toml),
        Some("json") | Some("jsonc") => Some(DocFormat::Json),
        Some("yaml") | Some("yml") => Some(DocFormat::Yaml),
        _ => None,
    }
}

macro_rules! delegate {
    ($self:ident, $d:ident => $body:expr) => {
        match $self {
            AnyDocument::Toml($d) => $body,
            AnyDocument::Json($d) => $body,
            AnyDocument::Yaml($d) => $body,
        }
    };
}

impl AnyDocument {
    /// Load `path` as `format` (caller resolved detection/override).
    pub fn load_as(path: &FsPath, format: DocFormat) -> anyhow::Result<Self> {
        match format {
            DocFormat::Toml => Ok(Self::Toml(CstDocument::load(path)?)),
            DocFormat::Json => Ok(Self::Json(JsonDocument::load(path)?)),
            DocFormat::Yaml => Ok(Self::Yaml(YamlDocument::load(path)?)),
        }
    }

    /// Write the current document to its source path.
    pub fn save(&self) -> std::io::Result<()> {
        delegate!(self, d => d.save())
    }

    /// Reset the dirty baseline so `is_dirty()` returns false.
    pub fn mark_saved(&mut self) {
        delegate!(self, d => d.mark_saved())
    }

    /// Re-parse the document from a serialized snapshot string (undo/redo restore).
    pub fn replace_from_str(&mut self, s: &str) -> Result<(), MutateError> {
        delegate!(self, d => d.replace_from_str(s))
    }

    /// Accept the JSONC upgrade (enables authored comments). No-op for TOML.
    pub fn enable_comments(&mut self) {
        if let AnyDocument::Json(d) = self {
            d.enable_comments();
        }
    }
}

impl ConfigDocument for AnyDocument {
    fn load(path: &FsPath) -> anyhow::Result<Self> {
        let fmt = detect_format(path)
            .ok_or_else(|| anyhow::anyhow!("unrecognized config format: {}", path.display()))?;
        Self::load_as(path, fmt)
    }
    fn project(&self) -> NodeTree {
        delegate!(self, d => d.project())
    }
    fn serialize(&self) -> String {
        delegate!(self, d => d.serialize())
    }
    fn is_dirty(&self) -> bool {
        delegate!(self, d => d.is_dirty())
    }
    fn apply(&mut self, m: Mutation) -> Result<(), MutateError> {
        delegate!(self, d => d.apply(m))
    }
    fn serialize_fragment(&self, path: &[Seg]) -> String {
        delegate!(self, d => d.serialize_fragment(path))
    }
    fn serialize_fragment_relative(&self, path: &[Seg]) -> String {
        delegate!(self, d => d.serialize_fragment_relative(path))
    }
    fn format(&self) -> DocFormat {
        delegate!(self, d => d.format())
    }
    fn comment_prefix(&self) -> &'static str {
        delegate!(self, d => d.comment_prefix())
    }
    fn supports_comments(&self) -> bool {
        delegate!(self, d => d.supports_comments())
    }
    fn kind_options(&self, path: &[Seg]) -> Vec<(String, KindTarget)> {
        delegate!(self, d => d.kind_options(path))
    }
    fn scalar_fragment(&self, key: Option<&str>, value: &str) -> String {
        delegate!(self, d => d.scalar_fragment(key, value))
    }
    fn value_kind(&self, value: &str) -> Result<crate::model::node::NodeKind, String> {
        delegate!(self, d => d.value_kind(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::document::{ConfigDocument, DocFormat};

    #[test]
    fn any_document_delegates_to_toml() {
        let f = tempfile::NamedTempFile::with_suffix(".toml").unwrap();
        std::fs::write(f.path(), "a = 1\n").unwrap();
        let doc = AnyDocument::load(f.path()).unwrap();
        assert_eq!(doc.format(), DocFormat::Toml);
        assert_eq!(doc.serialize(), "a = 1\n");
        assert!(!doc.is_dirty());
    }

    #[test]
    fn load_rejects_unknown_extension() {
        let f = tempfile::NamedTempFile::with_suffix(".ini").unwrap();
        std::fs::write(f.path(), "a = 1\n").unwrap();
        assert!(AnyDocument::load(f.path()).is_err());
    }

    #[test]
    fn any_document_loads_json() {
        let f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        std::fs::write(f.path(), "{ \"a\": 1 }\n").unwrap();
        let doc = AnyDocument::load(f.path()).unwrap();
        assert_eq!(doc.format(), DocFormat::Json);
        assert_eq!(doc.serialize(), "{ \"a\": 1 }\n");
    }

    #[test]
    fn any_document_loads_yaml() {
        let f = tempfile::Builder::new().suffix(".yaml").tempfile().unwrap();
        std::fs::write(f.path(), "a: 1\n").unwrap();
        let doc = AnyDocument::load(f.path()).unwrap();
        assert_eq!(doc.format(), DocFormat::Yaml);
        assert_eq!(doc.serialize(), "a: 1\n");
    }
}
