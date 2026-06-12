//! `CstDocument` — the lossless-CST backend (migration in progress; see
//! `docs/superpowers/plans/2026-06-08-cst-backend-migration.md`).
//!
//! The source of truth is a [`taplo`] rowan syntax tree. Unlike `toml_edit`, every
//! byte — including `# comment`, whitespace and newlines — is a real token with a
//! position, so a standalone comment is a *first-class, independently-positioned
//! node* (not decor glued to the following item) and `serialize()` is a lossless
//! token concatenation.
//!
//! Phase 1 (this file) implements `load`/`serialize` with a byte-identical
//! round-trip guarantee. `project` (Phase 2) and `apply` (Phase 3) are stubs until
//! ported; `CstDocument` is **not** wired into the TUI until it reaches parity with
//! `TomlDocument` (Phase 5).

use std::path::{Path, PathBuf};

use anyhow::Context;
use taplo::syntax::SyntaxNode;

use crate::model::document::{ConfigDocument, DocFormat, MutateError, Mutation};
use crate::model::node::NodeTree;

pub struct CstDocument {
    /// The rowan syntax tree root — the single source of truth.
    pub(crate) syntax: SyntaxNode,
    /// Read in Phase 5 (TUI wiring) as the save target.
    #[allow(dead_code)]
    pub(crate) path: PathBuf,
    pub(crate) original: String,
    pub(crate) filename: String,
}

impl ConfigDocument for CstDocument {
    fn load(path: &Path) -> anyhow::Result<Self> {
        let original =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let parse = taplo::parser::parse(&original);
        if let Some(err) = parse.errors.first() {
            anyhow::bail!("parsing {} as TOML: {}", path.display(), err);
        }
        let syntax = parse.into_syntax();
        let filename = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        Ok(CstDocument {
            syntax,
            path: path.to_path_buf(),
            original,
            filename,
        })
    }

    fn project(&self) -> NodeTree {
        crate::model::cst_project::project(&self.syntax, &self.filename)
    }

    fn serialize(&self) -> String {
        self.syntax.to_string()
    }

    fn is_dirty(&self) -> bool {
        self.serialize() != self.original
    }

    fn serialize_fragment(&self, path: &[crate::model::node::Seg]) -> String {
        if path.is_empty() {
            return self.serialize();
        }
        crate::model::cst_edit::serialize_fragment(&self.syntax, path)
    }

    fn serialize_fragment_relative(&self, path: &[crate::model::node::Seg]) -> String {
        if path.is_empty() {
            return self.serialize();
        }
        crate::model::cst_edit::serialize_fragment_relative(&self.syntax, path)
    }

    fn apply(&mut self, m: Mutation) -> Result<(), MutateError> {
        // Mutate a copy and commit only on success (free atomic rollback). The edit
        // works on a `clone_for_update` (mutable) tree; normalize the result back to
        // an immutable tree (re-parse is byte-identical) so the next `apply` can
        // `clone_for_update` again.
        let new = crate::model::cst_edit::apply(&self.syntax, m)?;
        self.syntax = taplo::parser::parse(&new.to_string()).into_syntax();
        Ok(())
    }

    fn format(&self) -> DocFormat {
        DocFormat::Toml
    }
    fn comment_prefix(&self) -> &'static str {
        "#"
    }
    fn supports_comments(&self) -> bool {
        true
    }
}

impl CstDocument {
    /// Write the current document to its source path.
    pub fn save(&self) -> std::io::Result<()> {
        std::fs::write(&self.path, self.serialize())
    }

    /// Reset the dirty baseline so `is_dirty()` returns false.
    pub fn mark_saved(&mut self) {
        self.original = self.serialize();
    }

    /// Re-parse the document from a serialized snapshot string (undo/redo restore).
    /// Propagates a parse error rather than silently no-op'ing.
    pub fn replace_from_str(&mut self, s: &str) -> Result<(), MutateError> {
        let parse = taplo::parser::parse(s);
        if let Some(e) = parse.errors.first() {
            return Err(MutateError::Fragment(e.to_string()));
        }
        self.syntax = parse.into_syntax();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn cst_from_str(s: &str) -> CstDocument {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(s.as_bytes()).unwrap();
        CstDocument::load(f.path()).unwrap()
    }

    /// The Phase 1 contract: load → serialize is byte-identical for every fixture
    /// and the project's own `test.toml`. This is the go/no-go gate for the backend.
    #[test]
    fn roundtrip_is_byte_identical() {
        let mut files: Vec<PathBuf> = vec![PathBuf::from("test.toml")];
        let fx = Path::new("tests/fixtures");
        if fx.is_dir() {
            for e in std::fs::read_dir(fx).unwrap() {
                let p = e.unwrap().path();
                if p.extension().map(|x| x == "toml").unwrap_or(false) {
                    files.push(p);
                }
            }
        }
        for f in &files {
            let text = std::fs::read_to_string(f).unwrap();
            let doc = CstDocument::load(f).unwrap();
            assert_eq!(
                doc.serialize(),
                text,
                "round-trip not byte-identical for {f:?}"
            );
        }
    }

    #[test]
    fn load_rejects_invalid_toml() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"this is = = not toml").unwrap();
        assert!(CstDocument::load(f.path()).is_err());
    }

    #[test]
    fn toml_format_facets() {
        let doc = cst_from_str("a = 1\n");
        assert_eq!(doc.format(), DocFormat::Toml);
        assert_eq!(doc.comment_prefix(), "#");
        assert!(doc.supports_comments());
    }

    #[test]
    fn serialize_roundtrips_a_small_doc() {
        let src = "# top\nbasic = \"x\"\n\n# sec\n[srv]\nport = 8080\n";
        let doc = cst_from_str(src);
        assert_eq!(doc.serialize(), src);
    }
}
