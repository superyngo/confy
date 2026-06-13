//! `YamlDocument` — the lossless YAML-subset backend (mirrors `json/doc.rs`).

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::model::document::{ConfigDocument, DocFormat, KindTarget, MutateError, Mutation};
use crate::model::node::{NodeKind, NodeTree, Seg};
use crate::model::yaml::syntax::SyntaxNode;

pub struct YamlDocument {
    pub(crate) syntax: SyntaxNode,
    pub(crate) path: PathBuf,
    pub(crate) original: String,
    pub(crate) filename: String,
}

impl ConfigDocument for YamlDocument {
    fn load(path: &Path) -> anyhow::Result<Self> {
        let original =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let green = crate::model::yaml::parse::parse(&original)
            .map_err(|e| anyhow::anyhow!("parsing {} as YAML: {}", path.display(), e))?;
        let syntax = SyntaxNode::new_root(green);
        let filename = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        Ok(YamlDocument {
            syntax,
            path: path.to_path_buf(),
            original,
            filename,
        })
    }

    fn project(&self) -> NodeTree {
        crate::model::yaml::project::project(&self.syntax, &self.filename)
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
        crate::model::yaml::edit::serialize_fragment(&self.syntax, path)
    }

    fn serialize_fragment_relative(&self, path: &[Seg]) -> String {
        // YAML has no dotted scope tables; relative == absolute fragment.
        self.serialize_fragment(path)
    }

    fn apply(&mut self, m: Mutation) -> Result<(), MutateError> {
        // NOTE: edit::apply is a passthrough stub until Tasks 5–6 — mutations
        // are currently no-ops (the re-parse round-trip below is a no-op too).
        let new = crate::model::yaml::edit::apply(&self.syntax, m)?;
        let text = new.to_string();
        let green = crate::model::yaml::parse::parse(&text).map_err(MutateError::Fragment)?;
        self.syntax = SyntaxNode::new_root(green);
        Ok(())
    }

    fn format(&self) -> DocFormat {
        DocFormat::Yaml
    }

    fn comment_prefix(&self) -> &'static str {
        "#"
    }

    fn supports_comments(&self) -> bool {
        true
    }

    fn kind_options(&self, path: &[Seg]) -> Vec<(String, KindTarget)> {
        kind_options(&self.project(), path)
    }

    fn scalar_fragment(&self, key: Option<&str>, value: &str) -> String {
        match key {
            Some(k) => format!("{k}: {value}\n"),
            None => format!("- {value}\n"),
        }
    }

    fn value_kind(&self, value: &str) -> Result<NodeKind, String> {
        // Project the value as the sole member of a mapping and read its kind.
        let green = crate::model::yaml::parse::parse(&format!("__k__: {value}\n"))?;
        crate::model::yaml::project::project(&SyntaxNode::new_root(green), "")
            .root
            .children
            .into_iter()
            .next()
            .map(|n| n.kind)
            .ok_or_else(|| "fragment has no value".into())
    }
}

impl YamlDocument {
    pub fn save(&self) -> std::io::Result<()> {
        std::fs::write(&self.path, self.serialize())
    }

    pub fn mark_saved(&mut self) {
        self.original = self.serialize();
    }

    pub fn replace_from_str(&mut self, s: &str) -> Result<(), MutateError> {
        let green = crate::model::yaml::parse::parse(s).map_err(MutateError::Fragment)?;
        self.syntax = SyntaxNode::new_root(green);
        Ok(())
    }
}

/// Per-node convertible-kind list (current notation excluded). See Task 6 for
/// the body; stubbed empty until then.
pub(crate) fn kind_options(_tree: &NodeTree, _path: &[Seg]) -> Vec<(String, KindTarget)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::document::{ConfigDocument, DocFormat};
    use std::io::Write;

    /// Create a temp file with the given extension, write `s`, load it.
    fn yaml_from_str(ext: &str, s: &str) -> YamlDocument {
        let mut f = tempfile::Builder::new().suffix(ext).tempfile().unwrap();
        f.write_all(s.as_bytes()).unwrap();
        YamlDocument::load(f.path()).unwrap()
    }

    #[test]
    fn roundtrip_and_facets() {
        let src = "a: 1\nb: two\n";
        let doc = yaml_from_str(".yaml", src);
        assert_eq!(doc.serialize(), src);
        assert!(!doc.is_dirty());
        assert_eq!(doc.format(), DocFormat::Yaml);
        assert_eq!(doc.comment_prefix(), "#");
        assert!(doc.supports_comments());
    }

    #[test]
    fn load_rejects_multi_doc() {
        let mut f = tempfile::Builder::new().suffix(".yaml").tempfile().unwrap();
        f.write_all(b"---\na: 1\n---\nb: 2\n").unwrap();
        assert!(YamlDocument::load(f.path()).is_err());
    }

    #[test]
    fn scalar_fragment_uses_yaml_forms() {
        let doc = yaml_from_str(".yaml", "a: 1\n");
        assert_eq!(doc.scalar_fragment(Some("k"), "v"), "k: v\n");
        assert_eq!(doc.scalar_fragment(None, "v"), "- v\n");
    }
}
