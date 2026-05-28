use crate::model::document::{ConfigDocument, Mutation, MutateError, Target};
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

    fn insert_fragment(
        &mut self,
        target: &crate::model::document::Target,
        toml: &str,
        oc: crate::model::document::OnCollision,
    ) -> Result<(), MutateError> {
        use crate::model::document::OnCollision::*;
        let frag = crate::model::fragment::parse_fragment(toml)?;
        let dest = self.parent_table_mut(&target.parent)?;
        // Pre-pass: resolve every final key and detect a Cancel collision BEFORE
        // mutating, so a multi-entry fragment that collides part-way leaves the
        // document untouched (Cancel must be all-or-nothing).
        let mut insertions: Vec<(String, Item)> = Vec::new();
        for (k, item) in frag.iter() {
            let mut key = k.to_string();
            if dest.contains_key(&key) {
                match oc {
                    Cancel => return Err(MutateError::Collision(key)),
                    Overwrite => {} // keep key; the apply pass removes the old value
                    Rename => {
                        let mut n = 2;
                        while dest.contains_key(&format!("{key}_{n}")) {
                            n += 1;
                        }
                        key = format!("{key}_{n}");
                    }
                }
            }
            insertions.push((key, item.clone()));
        }
        // Apply only after the whole fragment passed the collision check.
        for (key, item) in insertions {
            dest.remove(&key); // no-op unless Overwrite replacing an existing key
            dest.insert(&key, item);
        }
        Ok(())
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

    /// Replace = delete at `path`, then insert the fragment at the same parent.
    fn replace(&mut self, path: &[Seg], toml: &str) -> Result<(), MutateError> {
        let parent = path.split_at(path.len().saturating_sub(1)).0.to_vec();
        self.remove_at(path)?;
        self.insert_fragment(
            &Target { parent, index: 0 },
            toml,
            crate::model::document::OnCollision::Overwrite,
        )
    }

    fn remark(&mut self, path: &[Seg]) -> Result<(), MutateError> {
        let is_comment = matches!(path.last(), Some(Seg::Key(k)) if k.starts_with("#comment:"));
        if is_comment {
            self.uncomment(path)
        } else {
            self.comment_out(path)
        }
    }

    /// Comment-out a live key: serialize the item, prefix each line with `# `,
    /// delete the live key, write the commented text into the parent table's decor
    /// at the same position.
    fn comment_out(&mut self, path: &[Seg]) -> Result<(), MutateError> {
        let (parent, last) = path.split_at(path.len().saturating_sub(1));
        let last = last.first().ok_or(MutateError::NotFound)?;
        let key_name = match last {
            Seg::Key(k) => k.to_string(),
            Seg::Index(_) => return Err(MutateError::Unsupported),
        };
        // Serialize the item to text before removing it
        let rendered = {
            let table = self.parent_table_mut(parent)?;
            let item = table.get(&key_name).ok_or(MutateError::NotFound)?;
            format!("{} = {}", key_name, item)
        };
        let commented = rendered
            .lines()
            .map(|l| format!("# {l}"))
            .collect::<Vec<_>>()
            .join("\n");
        // Delete the live key
        {
            let table = self.parent_table_mut(parent)?;
            table.remove(&key_name).ok_or(MutateError::NotFound)?;
        }
        // Write the commented text into the appropriate decor slot.
        if parent.is_empty() && self.doc.as_table().is_empty() {
            // Only item at top level — write to document trailing
            let trailing = self.doc.trailing().as_str().unwrap_or("");
            let new_trailing = if trailing.is_empty() {
                format!("{commented}\n")
            } else {
                format!("{commented}\n{trailing}")
            };
            self.doc.set_trailing(new_trailing);
        } else {
            // Collect first remaining key name before mutable borrow
            let first_key = self
                .parent_table_mut(parent)
                .ok()
                .and_then(|t| t.iter().next().map(|(k, _)| k.to_string()));
            if let Some(fk) = first_key {
                let table = self.parent_table_mut(parent)?;
                let existing = table
                    .key(&fk)
                    .and_then(|k| k.leaf_decor().prefix().and_then(|r| r.as_str()))
                    .unwrap_or("")
                    .to_string();
                let table = self.parent_table_mut(parent)?;
                if let Some(mut km) = table.key_mut(&fk) {
                    km.leaf_decor_mut()
                        .set_prefix(format!("{commented}\n{existing}"));
                }
            } else {
                // Table is now empty (nested) — use the table header's decor.
                // Walk the path on concrete Table types only (no TableLike).
                self.write_comment_to_table_decor(parent, &format!("{}\n", commented));
            }
        }
        Ok(())
    }

    /// Uncomment: take the comment text from the projector's synthetic path,
    /// strip `# ` from each line, parse as TOML fragment, and insert at that
    /// position. On parse failure return Fragment and leave the document untouched.
    fn uncomment(&mut self, path: &[Seg]) -> Result<(), MutateError> {
        let (parent, last) = path.split_at(path.len().saturating_sub(1));
        let last_seg = last.first().ok_or(MutateError::NotFound)?;
        let marker = match last_seg {
            Seg::Key(k) if k.starts_with("#comment:") => k.as_str(),
            _ => return Err(MutateError::NotFound),
        };
        // Read the comment text from the projection
        let comment_text = {
            let projected = self.project();
            projected
                .root
                .children
                .iter()
                .find(|n| n.path.last() == Some(&Seg::Key(marker.to_string())))
                .and_then(|n| match &n.kind {
                    crate::model::node::NodeKind::Comment(t) => Some(t.clone()),
                    _ => None,
                })
                .ok_or(MutateError::NotFound)?
        };
        // Strip leading "# " from each line
        let stripped = comment_text
            .lines()
            .map(|l| l.strip_prefix("# ").unwrap_or(l.strip_prefix('#').unwrap_or(l)))
            .collect::<Vec<_>>()
            .join("\n");
        let fragment = format!("{stripped}\n");
        // Validate: parse the fragment. On failure, document is untouched.
        crate::model::fragment::parse_fragment(&fragment)?;
        // Remove the comment text from decor BEFORE inserting the live items,
        // so that the "is table empty?" check in remove_comment_from_decor
        // correctly identifies whether the comment lives in trailing or leaf_decor.
        self.remove_comment_from_decor(parent, &comment_text);
        // Parse succeeded — insert the live items
        self.insert_fragment(
            &Target {
                parent: parent.to_vec(),
                index: 0,
            },
            &fragment,
            crate::model::document::OnCollision::Overwrite,
        )?;
        Ok(())
    }

    /// Walk to a concrete `&mut toml_edit::Table` and set its header decor prefix.
    fn write_comment_to_table_decor(&mut self, parent: &[Seg], comment: &str) {
        let mut table = self.doc.as_table_mut();
        for seg in parent {
            match seg {
                Seg::Key(k) => {
                    table = match table.get_mut(k).and_then(Item::as_table_mut) {
                        Some(t) => t,
                        None => return,
                    };
                }
                Seg::Index(_) => return,
            }
        }
        table.decor_mut().set_prefix(comment);
    }

    /// Remove a comment line from the decor slot where the projector would have read it.
    fn remove_comment_from_decor(&mut self, parent: &[Seg], comment_text: &str) {
        if parent.is_empty() {
            // Read first key name from the immutable view
            let first_key = self.doc.as_table().iter().next().map(|(k, _)| k.to_string());
            if let Some(fk) = first_key {
                let existing = self
                    .doc
                    .as_table()
                    .key(&fk)
                    .and_then(|k| k.leaf_decor().prefix().and_then(|r| r.as_str()))
                    .unwrap_or("")
                    .to_string();
                let new_prefix = existing
                    .lines()
                    .filter(|l| l.trim() != comment_text)
                    .collect::<Vec<_>>()
                    .join("\n");
                if let Some(mut km) = self.doc.as_table_mut().key_mut(&fk) {
                    km.leaf_decor_mut().set_prefix(new_prefix);
                }
            } else {
                // No keys — comment was in document trailing
                let trailing = self.doc.trailing().as_str().unwrap_or("");
                let new_trailing = trailing
                    .lines()
                    .filter(|l| l.trim() != comment_text)
                    .collect::<Vec<_>>()
                    .join("\n");
                self.doc.set_trailing(new_trailing);
            }
        } else {
            let first_key = self
                .parent_table_mut(parent)
                .ok()
                .and_then(|t| t.iter().next().map(|(k, _)| k.to_string()));
            if let Some(fk) = first_key {
                let existing = {
                    let table = self.parent_table_mut(parent).unwrap();
                    table
                        .key(&fk)
                        .and_then(|k| k.leaf_decor().prefix().and_then(|r| r.as_str()))
                        .unwrap_or("")
                        .to_string()
                };
                let new_prefix = existing
                    .lines()
                    .filter(|l| l.trim() != comment_text)
                    .collect::<Vec<_>>()
                    .join("\n");
                let table = self.parent_table_mut(parent).unwrap();
                if let Some(mut km) = table.key_mut(&fk) {
                    km.leaf_decor_mut().set_prefix(new_prefix);
                }
            }
        }
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
            Mutation::Insert {
                target,
                toml,
                on_collision,
            } => self.insert_fragment(&target, &toml, on_collision),
            Mutation::Replace { path, toml } => self.replace(&path, &toml),
            Mutation::Remark { path } => self.remark(&path),
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
        // must walk the implicit tables (get_mut + as_table_like_mut) to reach the leaf.
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
    fn insert_into_table_and_collision() {
        use crate::model::document::{Mutation, OnCollision, Target};
        use crate::model::node::Seg;

        let mut doc = doc_from_str("[server]\nport = 8080\n");
        let target = Target { parent: vec![Seg::Key("server".into())], index: 1 };

        // Insert new key — no collision
        doc.apply(Mutation::Insert {
            target: target.clone(),
            toml: "host = \"x\"\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert!(doc.serialize().contains("host = \"x\""));

        // Collision with Cancel → error
        let err = doc.apply(Mutation::Insert {
            target: target.clone(),
            toml: "port = 1\n".into(),
            on_collision: OnCollision::Cancel,
        });
        assert!(matches!(
            err,
            Err(crate::model::document::MutateError::Collision(_))
        ));
        // Cancel must leave the document unchanged: port keeps its original value.
        assert!(doc.serialize().contains("port = 8080"));
        assert!(!doc.serialize().contains("port = 1"));

        // Overwrite replaces
        doc.apply(Mutation::Insert {
            target: target.clone(),
            toml: "port = 1\n".into(),
            on_collision: OnCollision::Overwrite,
        })
        .unwrap();
        assert!(doc.serialize().contains("port = 1"));

        // Rename appends _2
        doc.apply(Mutation::Insert {
            target,
            toml: "port = 2\n".into(),
            on_collision: OnCollision::Rename,
        })
        .unwrap();
        assert!(doc.serialize().contains("port_2 = 2"));
    }

    #[test]
    fn insert_cancel_is_atomic_for_multi_entry_fragment() {
        // A multi-entry fragment whose later key collides under Cancel must NOT
        // insert the earlier keys — Cancel is all-or-nothing.
        use crate::model::document::{Mutation, OnCollision, Target};
        use crate::model::node::Seg;
        let mut doc = doc_from_str("[server]\nport = 8080\n");
        let err = doc.apply(Mutation::Insert {
            target: Target { parent: vec![Seg::Key("server".into())], index: 1 },
            // `host` is new, `port` collides — Cancel must reject the whole fragment.
            toml: "host = \"x\"\nport = 1\n".into(),
            on_collision: OnCollision::Cancel,
        });
        assert!(matches!(
            err,
            Err(crate::model::document::MutateError::Collision(_))
        ));
        // The non-colliding earlier key must NOT have been inserted.
        assert!(!doc.serialize().contains("host = \"x\""));
        assert!(doc.serialize().contains("port = 8080"));
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

    #[test]
    fn replace_node_fragment() {
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str("port = 8080\n");
        doc.apply(Mutation::Replace {
            path: vec![Seg::Key("port".into())],
            toml: "port = 9090\n".into(),
        })
        .unwrap();
        assert!(doc.serialize().contains("port = 9090"));
    }

    #[test]
    fn remark_toggles_leaf() {
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str("port = 8080\n");
        // live -> comment
        doc.apply(Mutation::Remark {
            path: vec![Seg::Key("port".into())],
        })
        .unwrap();
        let s = doc.serialize();
        assert!(s.contains("# port = "), "commented output should contain '# port =': {:?}", s);
        assert!(s.contains("8080"), "commented output should contain '8080': {:?}", s);
        // comment -> live: address the comment via its synthetic path
        let cpath = doc.project().root.children[0].path.clone();
        doc.apply(Mutation::Remark { path: cpath }).unwrap();
        assert!(doc.serialize().contains("port"), "uncommented output should contain 'port': {:?}", doc.serialize());
        assert!(doc.serialize().contains("8080"), "uncommented output should contain '8080': {:?}", doc.serialize());
    }

    #[test]
    fn remark_rejects_non_toml_comment() {
        use crate::model::document::{Mutation, MutateError};
        let mut doc = doc_from_str("# just prose\n");
        let cpath = doc.project().root.children[0].path.clone();
        let err = doc.apply(Mutation::Remark { path: cpath });
        assert!(matches!(err, Err(MutateError::Fragment(_))));
        // document unchanged
        assert_eq!(doc.serialize(), "# just prose\n");
    }
}
