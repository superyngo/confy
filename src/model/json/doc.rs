//! `JsonDocument` — the lossless JSON/JSONC backend (mirrors `cst_doc.rs`).

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::model::document::{ConfigDocument, DocFormat, KindTarget, MutateError, Mutation};
use crate::model::json::syntax::SyntaxNode;
use crate::model::node::{NodeTree, Seg};

pub struct JsonDocument {
    pub(crate) syntax: SyntaxNode,
    pub(crate) path: PathBuf,
    pub(crate) original: String,
    pub(crate) filename: String,
    /// True once authored comments are legal: the file already contained a `//`
    /// or `/* */` at load, OR the extension is `.jsonc`, OR the user accepted the
    /// JSONC upgrade this session. A pure `.json` with no comments starts false.
    pub(crate) comments_enabled: bool,
}

impl ConfigDocument for JsonDocument {
    fn load(path: &Path) -> anyhow::Result<Self> {
        let original =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let green = crate::model::json::parse::parse(&original)
            .map_err(|e| anyhow::anyhow!("parsing {} as JSON: {}", path.display(), e))?;
        let syntax = SyntaxNode::new_root(green);
        let filename = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let is_jsonc_ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("jsonc"))
            .unwrap_or(false);
        let has_comment = original.contains("//") || original.contains("/*");
        let comments_enabled = is_jsonc_ext || has_comment;
        Ok(JsonDocument {
            syntax,
            path: path.to_path_buf(),
            original,
            filename,
            comments_enabled,
        })
    }

    fn project(&self) -> NodeTree {
        crate::model::json::project::project(&self.syntax, &self.filename)
    }

    fn serialize(&self) -> String {
        self.syntax.to_string()
    }

    fn is_dirty(&self) -> bool {
        self.serialize() != self.original
    }

    fn serialize_fragment(&self, path: &[Seg]) -> String {
        if path.is_empty() {
            return self.serialize();
        }
        crate::model::json::edit::serialize_fragment(&self.syntax, path)
    }

    fn serialize_fragment_relative(&self, path: &[Seg]) -> String {
        // JSON has no dotted scope tables, so relative == absolute fragment.
        self.serialize_fragment(path)
    }

    fn apply(&mut self, m: Mutation) -> Result<(), MutateError> {
        let new = crate::model::json::edit::apply(&self.syntax, m)?;
        let text = new.to_string();
        let green = crate::model::json::parse::parse(&text).map_err(MutateError::Fragment)?;
        self.syntax = SyntaxNode::new_root(green);
        Ok(())
    }

    fn format(&self) -> DocFormat {
        DocFormat::Json
    }

    fn comment_prefix(&self) -> &'static str {
        "//"
    }

    fn supports_comments(&self) -> bool {
        self.comments_enabled
    }

    fn kind_options(&self, path: &[Seg]) -> Vec<(String, KindTarget)> {
        kind_options(&self.project(), path)
    }
}

impl JsonDocument {
    // wired into AnyDocument in a later task
    #[allow(dead_code)]
    pub fn save(&self) -> std::io::Result<()> {
        std::fs::write(&self.path, self.serialize())
    }

    // wired into AnyDocument in a later task
    #[allow(dead_code)]
    pub fn mark_saved(&mut self) {
        self.original = self.serialize();
    }

    // wired into AnyDocument in a later task
    #[allow(dead_code)]
    pub fn replace_from_str(&mut self, s: &str) -> Result<(), MutateError> {
        let green = crate::model::json::parse::parse(s).map_err(MutateError::Fragment)?;
        self.syntax = SyntaxNode::new_root(green);
        Ok(())
    }

    /// Accept the JSONC upgrade: authored comments become legal for this session.
    pub fn enable_comments(&mut self) {
        self.comments_enabled = true;
    }
}

/// Per-node convertible-kind list. Filled in by a later task; empty for now.
pub(crate) fn kind_options(_tree: &NodeTree, _path: &[Seg]) -> Vec<(String, KindTarget)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::document::{ConfigDocument, DocFormat};
    use std::io::Write;

    /// Create a temp file with the given extension, write `s`, load it. The
    /// NamedTempFile drops at end of this fn — fine, load already read everything.
    fn json_from_str(ext: &str, s: &str) -> JsonDocument {
        let mut f = tempfile::Builder::new().suffix(ext).tempfile().unwrap();
        f.write_all(s.as_bytes()).unwrap();
        JsonDocument::load(f.path()).unwrap()
    }

    #[test]
    fn roundtrip_and_facets() {
        let src = "{\n  \"a\": 1\n}\n";
        let doc = json_from_str(".json", src);
        assert_eq!(doc.serialize(), src);
        assert!(!doc.is_dirty());
        assert_eq!(doc.format(), DocFormat::Json);
        assert_eq!(doc.comment_prefix(), "//");
    }

    #[test]
    fn pure_json_starts_without_comment_support() {
        let doc = json_from_str(".json", "{}\n");
        assert!(!doc.supports_comments());
    }

    #[test]
    fn jsonc_extension_supports_comments() {
        let doc = json_from_str(".jsonc", "{}\n");
        assert!(doc.supports_comments());
    }

    #[test]
    fn existing_comment_enables_support() {
        let doc = json_from_str(".json", "// hi\n{}\n");
        assert!(doc.supports_comments());
    }

    #[test]
    fn load_rejects_invalid() {
        let mut f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        f.write_all(b"{ \"a\": }").unwrap();
        assert!(JsonDocument::load(f.path()).is_err());
    }

    #[test]
    fn enable_comments_then_supports() {
        let mut doc = json_from_str(".json", "{}\n");
        assert!(!doc.supports_comments());
        doc.enable_comments();
        assert!(doc.supports_comments());
    }
}
