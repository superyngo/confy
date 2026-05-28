use crate::model::document::{ConfigDocument, MutateError, Mutation, Target};
use crate::model::node::{NodeTree, Seg};
use anyhow::Context;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, TableLike};

fn find_node_by_path<'a>(
    node: &'a crate::model::node::Node,
    path: &[Seg],
) -> Option<&'a crate::model::node::Node> {
    if node.path == path {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|c| find_node_by_path(c, path))
}

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
            Seg::Key(k) => {
                table.remove(k).ok_or(MutateError::NotFound)?;
                Ok(())
            }
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

    /// Replace the item at `path` with the fragment content, preserving key position.
    ///
    /// Replace is the write-back path for `e` (the user edits the node's own
    /// fragment), so the fragment must keep the same key. A renamed key would, via
    /// `Overwrite`, leave the original key in place and add the new one alongside —
    /// silent double entry. We therefore require every fragment key to match the
    /// path's final segment and reject a rename with `Fragment`. (Position-preserving
    /// rename is out of scope for the MVP.)
    fn replace(&mut self, path: &[Seg], toml: &str) -> Result<(), MutateError> {
        let (parent, last) = path.split_at(path.len().saturating_sub(1));
        let expected_key = match last.first() {
            Some(Seg::Key(k)) => k.as_str(),
            _ => return Err(MutateError::Unsupported),
        };
        let frag = crate::model::fragment::parse_fragment(toml)?;
        if frag.iter().any(|(k, _)| k != expected_key) {
            return Err(MutateError::Fragment(format!(
                "Replace cannot rename key '{expected_key}'; fragment must keep the same key"
            )));
        }
        self.insert_fragment(
            &Target {
                parent: parent.to_vec(),
                index: 0,
            },
            toml,
            crate::model::document::OnCollision::Overwrite,
        )
    }

    /// Move `sources` under `target`. Atomic on any error: a snapshot is taken
    /// up front and restored if any step fails, so a partial move (e.g. a Cancel
    /// collision after some sources were already deleted) never corrupts or loses
    /// data. The snapshot round-trips byte-identically (see Task 3 round-trip).
    fn r#move(
        &mut self,
        sources: &[crate::model::node::Path],
        target: &Target,
        oc: crate::model::document::OnCollision,
    ) -> Result<(), MutateError> {
        let snapshot = self.doc.to_string();
        match self.move_inner(sources, target, oc) {
            Ok(()) => Ok(()),
            Err(e) => {
                // Restore the pre-move state. The snapshot came from to_string(),
                // so it always re-parses; the expect guards against a logic bug.
                self.doc = snapshot
                    .parse::<DocumentMut>()
                    .expect("move snapshot must re-parse");
                Err(e)
            }
        }
    }

    fn move_inner(
        &mut self,
        sources: &[crate::model::node::Path],
        target: &Target,
        oc: crate::model::document::OnCollision,
    ) -> Result<(), MutateError> {
        // 1. Capture each source's item as a TOML fragment string.
        let mut fragments: Vec<String> = Vec::new();
        for src_path in sources {
            let (parent, last) = src_path.split_at(src_path.len().saturating_sub(1));
            let last = last.first().ok_or(MutateError::NotFound)?;
            let key_name = match last {
                Seg::Key(k) => k.as_str(),
                Seg::Index(_) => return Err(MutateError::Unsupported),
            };
            let table = self.parent_table_mut(parent)?;
            let item = table.get(key_name).ok_or(MutateError::NotFound)?.clone();
            let mut tmp = DocumentMut::new();
            tmp.as_table_mut().insert(key_name, item);
            fragments.push(tmp.to_string());
        }

        // 2. Delete sources in reverse path order to keep paths valid.
        for src_path in sources.iter().rev() {
            self.remove_at(src_path)?;
        }

        // 3. Insert collected fragments at the target.
        for frag in fragments {
            self.insert_fragment(target, &frag, oc)?;
        }

        Ok(())
    }

    /// Write the current serialized content to `self.path`.
    pub fn save(&self) -> std::io::Result<()> {
        std::fs::write(&self.path, self.serialize())
    }

    /// Reset the dirty baseline so `is_dirty()` returns false.
    pub fn mark_saved(&mut self) {
        self.original = self.serialize();
    }

    /// Re-parse the document from a serialized snapshot string (for undo/redo
    /// restore). Propagates a parse error rather than silently no-op'ing, so a
    /// caller is never told a restore succeeded when the document is unchanged.
    pub fn replace_from_str(&mut self, s: &str) -> Result<(), MutateError> {
        self.doc = s
            .parse::<DocumentMut>()
            .map_err(|e| MutateError::Fragment(e.to_string()))?;
        Ok(())
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
            let item = table.get(&key_name).ok_or(MutateError::NotFound)?.clone();
            let mut tmp = DocumentMut::new();
            tmp.as_table_mut().insert(&key_name, item);
            tmp.to_string().trim_end_matches('\n').to_string()
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
                // Check if the first remaining item is a Table — comments before
                // [table] headers live in Table::decor(), not in key.leaf_decor().
                let is_table = self
                    .parent_table_mut(parent)
                    .ok()
                    .and_then(|t| t.get(&fk))
                    .map(|item| matches!(item, Item::Table(_)))
                    .unwrap_or(false);
                if is_table {
                    let existing = {
                        let table = self.parent_table_mut(parent)?;
                        match table.get_mut(&fk) {
                            Some(Item::Table(t)) => t
                                .decor()
                                .prefix()
                                .and_then(|r| r.as_str())
                                .unwrap_or("")
                                .to_string(),
                            _ => String::new(),
                        }
                    };
                    let new_prefix = format!("{commented}\n{existing}");
                    let table = self.parent_table_mut(parent)?;
                    if let Some(Item::Table(t)) = table.get_mut(&fk) {
                        t.decor_mut().set_prefix(new_prefix);
                    }
                } else {
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
                }
            } else {
                // Table is now empty (nested) — use the table header's decor.
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
        match last_seg {
            Seg::Key(k) if k.starts_with("#comment:") => {}
            _ => return Err(MutateError::NotFound),
        }
        // Read the comment text from the projection (recursive descent to handle nested tables)
        let comment_text = {
            let projected = self.project();
            find_node_by_path(&projected.root, path)
                .and_then(|n| match &n.kind {
                    crate::model::node::NodeKind::Comment(t) => Some(t.clone()),
                    _ => None,
                })
                .ok_or(MutateError::NotFound)?
        };
        // Strip leading "# " from each line
        let stripped = comment_text
            .lines()
            .map(|l| {
                l.strip_prefix("# ")
                    .unwrap_or(l.strip_prefix('#').unwrap_or(l))
            })
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

    /// Remove a comment block from the decor slot where the projector would have read it.
    fn remove_comment_from_decor(&mut self, parent: &[Seg], comment_text: &str) {
        let remove_block = |s: &str| -> String {
            s.replace(&format!("{comment_text}\n"), "")
                .replace(comment_text, "")
        };
        if parent.is_empty() {
            let first_key = self
                .doc
                .as_table()
                .iter()
                .next()
                .map(|(k, _)| k.to_string());
            if let Some(fk) = first_key {
                // Comments before [table] headers live in Table::decor(), not leaf_decor.
                if let Some(Item::Table(t)) = self.doc.as_table().get(&fk) {
                    let existing = t
                        .decor()
                        .prefix()
                        .and_then(|r| r.as_str())
                        .unwrap_or("")
                        .to_string();
                    let new_prefix = remove_block(&existing);
                    if let Some(Item::Table(t)) = self.doc.as_table_mut().get_mut(&fk) {
                        t.decor_mut().set_prefix(new_prefix);
                    }
                } else {
                    let existing = self
                        .doc
                        .as_table()
                        .key(&fk)
                        .and_then(|k| k.leaf_decor().prefix().and_then(|r| r.as_str()))
                        .unwrap_or("")
                        .to_string();
                    let new_prefix = remove_block(&existing);
                    if let Some(mut km) = self.doc.as_table_mut().key_mut(&fk) {
                        km.leaf_decor_mut().set_prefix(new_prefix);
                    }
                }
            } else {
                let trailing = self.doc.trailing().as_str().unwrap_or("");
                let new_trailing = remove_block(trailing);
                self.doc.set_trailing(new_trailing);
            }
        } else {
            let first_key = self
                .parent_table_mut(parent)
                .ok()
                .and_then(|t| t.iter().next().map(|(k, _)| k.to_string()));
            if let Some(fk) = first_key {
                let is_table = self
                    .parent_table_mut(parent)
                    .ok()
                    .and_then(|t| t.get(&fk))
                    .map(|item| matches!(item, Item::Table(_)))
                    .unwrap_or(false);
                if is_table {
                    let existing = {
                        let table = self.parent_table_mut(parent).unwrap();
                        match table.get_mut(&fk) {
                            Some(Item::Table(t)) => t
                                .decor()
                                .prefix()
                                .and_then(|r| r.as_str())
                                .unwrap_or("")
                                .to_string(),
                            _ => String::new(),
                        }
                    };
                    let new_prefix = remove_block(&existing);
                    let table = self.parent_table_mut(parent).unwrap();
                    if let Some(Item::Table(t)) = table.get_mut(&fk) {
                        t.decor_mut().set_prefix(new_prefix);
                    }
                } else {
                    let existing = {
                        let table = self.parent_table_mut(parent).unwrap();
                        table
                            .key(&fk)
                            .and_then(|k| k.leaf_decor().prefix().and_then(|r| r.as_str()))
                            .unwrap_or("")
                            .to_string()
                    };
                    let new_prefix = remove_block(&existing);
                    let table = self.parent_table_mut(parent).unwrap();
                    if let Some(mut km) = table.key_mut(&fk) {
                        km.leaf_decor_mut().set_prefix(new_prefix);
                    }
                }
            }
        }
    }
}

impl ConfigDocument for TomlDocument {
    fn load(path: &Path) -> anyhow::Result<Self> {
        let original =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let doc = original
            .parse::<DocumentMut>()
            .with_context(|| format!("parsing {} as TOML", path.display()))?;
        let filename = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        Ok(TomlDocument {
            doc,
            path: path.to_path_buf(),
            original,
            filename,
        })
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
            Mutation::Move {
                sources,
                target,
                on_collision,
            } => self.r#move(&sources, &target, on_collision),
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
        assert_eq!(
            doc.serialize(),
            src,
            "untouched file must serialize byte-identically"
        );
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
        doc.apply(Mutation::Delete {
            path: vec![Seg::Key("a".into())],
        })
        .unwrap();
        assert!(!doc.serialize().contains("a = 1"));
        // delete a whole table (branch) removes its subtree
        doc.apply(Mutation::Delete {
            path: vec![Seg::Key("server".into())],
        })
        .unwrap();
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
        let target = Target {
            parent: vec![Seg::Key("server".into())],
            index: 1,
        };

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
            target: Target {
                parent: vec![Seg::Key("server".into())],
                index: 1,
            },
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
    fn replace_rejects_key_rename() {
        // Replace is the write-back for `e`; a renamed key would leave the old key
        // alongside the new one (silent double entry). Reject it, leave doc untouched.
        use crate::model::document::{MutateError, Mutation};
        use crate::model::node::Seg;
        let mut doc = doc_from_str("port = 8080\n");
        let err = doc.apply(Mutation::Replace {
            path: vec![Seg::Key("port".into())],
            toml: "Port = 9090\n".into(),
        });
        assert!(matches!(err, Err(MutateError::Fragment(_))));
        // document unchanged: original key/value intact, no stray "Port"
        assert!(doc.serialize().contains("port = 8080"));
        assert!(!doc.serialize().contains("Port = 9090"));
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
        assert!(
            s.contains("# port = "),
            "commented output should contain '# port =': {:?}",
            s
        );
        assert!(
            s.contains("8080"),
            "commented output should contain '8080': {:?}",
            s
        );
        // comment -> live: address the comment via its synthetic path
        let cpath = doc.project().root.children[0].path.clone();
        doc.apply(Mutation::Remark { path: cpath }).unwrap();
        assert!(
            doc.serialize().contains("port"),
            "uncommented output should contain 'port': {:?}",
            doc.serialize()
        );
        assert!(
            doc.serialize().contains("8080"),
            "uncommented output should contain '8080': {:?}",
            doc.serialize()
        );
    }

    #[test]
    fn remark_rejects_non_toml_comment() {
        use crate::model::document::{MutateError, Mutation};
        let mut doc = doc_from_str("# just prose\n");
        let cpath = doc.project().root.children[0].path.clone();
        let err = doc.apply(Mutation::Remark { path: cpath });
        assert!(matches!(err, Err(MutateError::Fragment(_))));
        // document unchanged
        assert_eq!(doc.serialize(), "# just prose\n");
    }

    #[test]
    fn replace_preserves_key_order() {
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str("a = 1\nb = 2\nc = 3\n");
        doc.apply(Mutation::Replace {
            path: vec![Seg::Key("b".into())],
            toml: "b = 99\n".into(),
        })
        .unwrap();
        let keys: Vec<&str> = doc.doc.as_table().iter().map(|(k, _)| k).collect();
        assert_eq!(
            keys,
            vec!["a", "b", "c"],
            "Replace must preserve key position"
        );
        assert!(doc.serialize().contains("b = 99"));
    }

    #[test]
    fn comment_out_produces_canonical_toml_no_double_space() {
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str("port = 8080\n");
        doc.apply(Mutation::Remark {
            path: vec![Seg::Key("port".into())],
        })
        .unwrap();
        let s = doc.serialize();
        // Must NOT contain double-space between = and value
        assert!(
            !s.contains("=  "),
            "commented output must be canonical (no double-space): {s:?}"
        );
        assert!(
            s.contains("# port = 8080"),
            "expected '# port = 8080', got: {s:?}"
        );
    }

    #[test]
    fn remark_roundtrip_nested_key_with_sibling() {
        use crate::model::document::Mutation;
        use crate::model::node::{NodeKind, Seg};
        let mut doc = doc_from_str("[server]\nport = 8080\nhost = \"x\"\n");
        // comment out nested key
        doc.apply(Mutation::Remark {
            path: vec![Seg::Key("server".into()), Seg::Key("port".into())],
        })
        .unwrap();
        let s = doc.serialize();
        assert!(s.contains("# port = 8080"), "commented: {s:?}");
        assert!(s.contains("host = \"x\""), "sibling preserved: {s:?}");

        // find the comment node inside server's children
        let projected = doc.project();
        let server = projected
            .root
            .children
            .iter()
            .find(|n| n.key == "server")
            .unwrap();
        let comment_node = server
            .children
            .iter()
            .find(|n| matches!(&n.kind, NodeKind::Comment(_)))
            .unwrap();
        // uncomment via the comment's synthetic path
        doc.apply(Mutation::Remark {
            path: comment_node.path.clone(),
        })
        .unwrap();
        let s2 = doc.serialize();
        assert!(s2.contains("port = 8080"), "uncommented: {s2:?}");
        assert!(s2.contains("host = \"x\""), "sibling still present: {s2:?}");
    }

    #[test]
    fn remark_roundtrip_nested_table_subtree() {
        // comment_out a [table] entry produces multi-line commented output;
        // uncomment must strip the entire block, not leave ghost comments.
        use crate::model::document::Mutation;
        use crate::model::node::{NodeKind, Seg};
        let mut doc = doc_from_str("[server]\nport = 8080\nhost = \"x\"\n[db]\nname = \"test\"\n");
        // comment out the entire [server] table
        doc.apply(Mutation::Remark {
            path: vec![Seg::Key("server".into())],
        })
        .unwrap();
        let s = doc.serialize();
        assert!(s.contains("# [server]"), "table header commented: {s:?}");
        assert!(s.contains("[db]"), "other table preserved: {s:?}");

        // find the comment node at top level
        let projected = doc.project();
        let comment_node = projected
            .root
            .children
            .iter()
            .find(|n| matches!(&n.kind, NodeKind::Comment(_)))
            .unwrap();
        doc.apply(Mutation::Remark {
            path: comment_node.path.clone(),
        })
        .unwrap();
        let s2 = doc.serialize();
        assert!(s2.contains("[server]"), "server table restored: {s2:?}");
        assert!(
            s2.contains("port = 8080"),
            "server children restored: {s2:?}"
        );
        assert!(s2.contains("[db]"), "db table still present: {s2:?}");
    }

    #[test]
    fn move_reparents_node() {
        use crate::model::document::{Mutation, OnCollision, Target};
        use crate::model::node::Seg;
        let mut doc = doc_from_str("a = 1\n[dest]\n");
        doc.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("a".into())]],
            target: Target {
                parent: vec![Seg::Key("dest".into())],
                index: 0,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        let s = doc.serialize();
        assert!(s.contains("[dest]"));
        assert!(s.contains("a = 1"));
        // `a` no longer at top level (only under dest)
        assert_eq!(s.matches("a = 1").count(), 1);
    }

    #[test]
    fn move_multi_source_success() {
        use crate::model::document::{Mutation, OnCollision, Target};
        use crate::model::node::Seg;
        let mut doc = doc_from_str("a = 1\nb = 2\n[dest]\n");
        doc.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("a".into())], vec![Seg::Key("b".into())]],
            target: Target {
                parent: vec![Seg::Key("dest".into())],
                index: 0,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        let s = doc.serialize();
        // both landed under [dest], neither remains at top level
        assert_eq!(s.matches("a = 1").count(), 1);
        assert_eq!(s.matches("b = 2").count(), 1);
        let tree = doc.project();
        let dest = &tree
            .root
            .children
            .iter()
            .find(|n| n.key == "dest")
            .unwrap()
            .children;
        let keys: Vec<String> = dest.iter().map(|n| n.key.clone()).collect();
        assert!(keys.contains(&"a".to_string()) && keys.contains(&"b".to_string()));
    }

    #[test]
    fn move_multi_source_cancel_is_atomic() {
        // Second source `b` collides at the destination under Cancel. The whole
        // move must roll back: NOTHING deleted, NOTHING inserted (no data loss).
        use crate::model::document::{MutateError, Mutation, OnCollision, Target};
        use crate::model::node::Seg;
        let mut doc = doc_from_str("a = 1\nb = 2\n[dest]\nb = 99\n");
        let before = doc.serialize();
        let err = doc.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("a".into())], vec![Seg::Key("b".into())]],
            target: Target {
                parent: vec![Seg::Key("dest".into())],
                index: 0,
            },
            on_collision: OnCollision::Cancel,
        });
        assert!(matches!(err, Err(MutateError::Collision(_))));
        // Atomic rollback: document byte-identical to the pre-move state.
        assert_eq!(doc.serialize(), before);
    }

    #[test]
    fn remark_roundtrip_top_level_sole_key() {
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str("port = 8080\n");
        doc.apply(Mutation::Remark {
            path: vec![Seg::Key("port".into())],
        })
        .unwrap();
        assert!(
            doc.serialize().contains("# port = 8080"),
            "commented: {:?}",
            doc.serialize()
        );
        // uncomment via the comment's synthetic path
        let cpath = doc.project().root.children[0].path.clone();
        doc.apply(Mutation::Remark { path: cpath }).unwrap();
        assert!(
            doc.serialize().contains("port = 8080"),
            "uncommented: {:?}",
            doc.serialize()
        );
    }
}
