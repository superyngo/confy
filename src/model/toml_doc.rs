use crate::model::document::ConfigDocument;
use crate::model::node::{Node, NodeKind, NodeTree};
use anyhow::Context;
use std::path::{Path, PathBuf};
use toml_edit::DocumentMut;

pub struct TomlDocument {
    pub(crate) doc: DocumentMut,
    pub(crate) path: PathBuf,
    pub(crate) original: String,
    pub(crate) filename: String,
}

impl ConfigDocument for TomlDocument {
    fn load(path: &Path) -> anyhow::Result<Self> {
        let original = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let doc = original
            .parse::<DocumentMut>()
            .with_context(|| format!("parsing {} as TOML", path.display()))?;
        let filename = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        Ok(TomlDocument { doc, path: path.to_path_buf(), original, filename })
    }

    fn project(&self) -> NodeTree {
        crate::model::project::project(&self.doc, &self.filename)
    }

    fn serialize(&self) -> String {
        self.doc.to_string()
    }

    fn is_dirty(&self) -> bool {
        self.serialize() != self.original
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn doc_from_str(s: &str) -> TomlDocument {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(s.as_bytes()).unwrap();
        TomlDocument::load(f.path()).unwrap()
    }

    #[test]
    fn roundtrip_byte_identical_with_comments_and_blanks() {
        let src = "# header comment\n\n[server]\nhost = \"0.0.0.0\"  # bind\nport = 8080\n";
        let doc = doc_from_str(src);
        assert_eq!(doc.serialize(), src, "untouched file must serialize byte-identically");
        assert!(!doc.is_dirty());
    }

    #[test]
    fn load_rejects_invalid_toml() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"this is = = not toml").unwrap();
        assert!(TomlDocument::load(f.path()).is_err());
    }
}
