use crate::model::document::{ConfigDocument, Mutation, MutateError};
use crate::model::node::{NodeTree, Seg};
use anyhow::Context;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, TableLike};

pub struct TomlDocument {
    pub(crate) doc: DocumentMut,
    pub(crate) path: PathBuf,
    pub(crate) original: String,
    pub(crate) filename: String,
}

impl TomlDocument {
    /// Remove the item addressed by `path`.
    fn remove_at(&mut self, path: &[Seg]) -> Result<(), MutateError> {
        let (parent, last) = path.split_at(path.len().saturating_sub(1));
        let last = last.first().ok_or(MutateError::NotFound)?;
        let table = self.parent_table_mut(parent)?;
        match last {
            Seg::Key(k) => { table.remove(k).ok_or(MutateError::NotFound)?; Ok(()) }
            Seg::Index(_) => Err(MutateError::Unsupported),
        }
    }

    /// Walk to the mutable table that directly contains the final segment.
    /// Returns `&mut dyn TableLike` so paths can traverse both regular `[table]`
    /// nodes and inline tables (`pt = { x = 1 }`) — the projector emits paths
    /// through both, and `Item::as_table_mut` alone would not match inline tables.
    fn parent_table_mut(&mut self, parent: &[Seg]) -> Result<&mut dyn TableLike, MutateError> {
        let mut tbl: &mut dyn TableLike = self.doc.as_table_mut();
        for seg in parent {
            match seg {
                Seg::Key(k) => {
                    tbl = tbl
                        .get_mut(k)
                        .and_then(Item::as_table_like_mut)
                        .ok_or(MutateError::NotFound)?;
                }
                Seg::Index(_) => return Err(MutateError::Unsupported),
            }
        }
        Ok(tbl)
    }
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

    fn apply(&mut self, m: Mutation) -> Result<(), MutateError> {
        match m {
            Mutation::Delete { path } => self.remove_at(&path),
            _ => Err(MutateError::Unsupported),
        }
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

    #[test]
    fn delete_leaf_and_branch() {
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str("a = 1\n[server]\nport = 8080\nhost = \"x\"\n");
        doc.apply(Mutation::Delete { path: vec![Seg::Key("a".into())] }).unwrap();
        assert!(!doc.serialize().contains("a = 1"));
        // delete a whole table (branch) removes its subtree
        doc.apply(Mutation::Delete { path: vec![Seg::Key("server".into())] }).unwrap();
        assert_eq!(doc.serialize().trim(), "");
        assert!(doc.is_dirty());
    }

    #[test]
    fn delete_dotted_key_navigates_implicit_tables() {
        // The projector emits multi-segment paths for dotted keys; the resolver
        // must walk the implicit tables (get_mut + as_table_mut) to reach the leaf.
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str("a.b.c = 1\na.b.d = 2\n");
        doc.apply(Mutation::Delete {
            path: vec![
                Seg::Key("a".into()),
                Seg::Key("b".into()),
                Seg::Key("c".into()),
            ],
        })
        .unwrap();
        assert!(!doc.serialize().contains("a.b.c"));
        // sibling dotted key under the same implicit table survives
        assert!(doc.serialize().contains("a.b.d = 2"));
    }

    #[test]
    fn delete_key_inside_inline_table() {
        // The projector emits paths through inline tables (pt = { x = 1 } ->
        // [Key("pt"), Key("x")]); the resolver must traverse them via TableLike.
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str("pt = { x = 1, y = 2 }\n");
        doc.apply(Mutation::Delete {
            path: vec![Seg::Key("pt".into()), Seg::Key("x".into())],
        })
        .unwrap();
        assert!(!doc.serialize().contains("x = 1"));
        assert!(doc.serialize().contains("y = 2"));
    }
}
