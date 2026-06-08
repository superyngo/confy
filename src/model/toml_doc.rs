use crate::model::document::{ConfigDocument, MutateError, Mutation, Target};
use crate::model::node::{NodeTree, Seg};
use anyhow::Context;
use std::path::{Path, PathBuf};
use toml_edit::{Array, DocumentMut, Item, TableLike};

/// Split a decor string into (leading blank lines, rest). The first element is the
/// maximal run of whitespace-only lines (the `\n` separators) at the start; the
/// second begins at the first line carrying non-whitespace content (e.g. a
/// comment, or the value/header itself). Used to keep a structured node's leading
/// blank separator(s) out of the `$EDITOR` view while re-attaching them on
/// write-back, so file spacing round-trips unchanged.
pub(crate) fn split_leading_blank_lines(s: &str) -> (&str, &str) {
    let mut idx = 0;
    loop {
        let rest = &s[idx..];
        match rest.find('\n') {
            Some(rel) => {
                if s[idx..idx + rel].trim().is_empty() {
                    idx += rel + 1;
                } else {
                    break;
                }
            }
            None => {
                if rest.trim().is_empty() {
                    idx = s.len();
                }
                break;
            }
        }
    }
    s.split_at(idx)
}

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

/// Order-preserving, decor-preserving key rename inside a standard `[table]`:
/// re-insert every entry in its original order, swapping only the target key (a
/// fresh `Key` carrying the old leaf decor — the comments/blanks above it).
fn rename_in_table(
    tbl: &mut toml_edit::Table,
    old: &str,
    new_base: &toml_edit::Key,
) -> Result<(), MutateError> {
    if !tbl.contains_key(old) {
        return Err(MutateError::NotFound);
    }
    if tbl.contains_key(new_base.get()) {
        return Err(MutateError::Collision(new_base.get().to_string()));
    }
    let order: Vec<String> = tbl.iter().map(|(k, _)| k.to_string()).collect();
    for k in order {
        let (key_obj, item) = tbl.remove_entry(&k).expect("key listed from iter");
        if k == old {
            let mut nk = new_base.clone();
            *nk.leaf_decor_mut() = key_obj.leaf_decor().clone();
            tbl.insert_formatted(&nk, item);
        } else {
            tbl.insert_formatted(&key_obj, item);
        }
    }
    Ok(())
}

/// Insert `(key, item)` into `tbl` immediately before the entry named `anchor`
/// (or at the end when `anchor` is `None` or not present). Rebuilds via the
/// order-preserving remove/`insert_formatted` technique used by `rename_in_table`,
/// so existing keys keep their decor and order.
fn insert_before(
    tbl: &mut toml_edit::Table,
    key: toml_edit::Key,
    item: Item,
    anchor: Option<&str>,
) {
    let order: Vec<String> = tbl.iter().map(|(k, _)| k.to_string()).collect();
    let mut new_entry = Some((key, item));
    for k in &order {
        let (ko, it) = tbl.remove_entry(k).expect("key listed from iter");
        if anchor == Some(k.as_str()) {
            if let Some((nk, ni)) = new_entry.take() {
                tbl.insert_formatted(&nk, ni);
            }
        }
        tbl.insert_formatted(&ko, it);
    }
    if let Some((nk, ni)) = new_entry.take() {
        tbl.insert_formatted(&nk, ni);
    }
}

/// Inline-table counterpart of [`rename_in_table`], using `InlineTable`'s
/// value-typed `remove_entry`/`insert_formatted`.
fn rename_in_inline_table(
    it: &mut toml_edit::InlineTable,
    old: &str,
    new_base: &toml_edit::Key,
) -> Result<(), MutateError> {
    if !it.contains_key(old) {
        return Err(MutateError::NotFound);
    }
    if it.contains_key(new_base.get()) {
        return Err(MutateError::Collision(new_base.get().to_string()));
    }
    let order: Vec<String> = it.iter().map(|(k, _)| k.to_string()).collect();
    for k in order {
        let (key_obj, value) = it.remove_entry(&k).expect("key listed from iter");
        if k == old {
            let mut nk = new_base.clone();
            *nk.leaf_decor_mut() = key_obj.leaf_decor().clone();
            it.insert_formatted(&nk, value);
        } else {
            it.insert_formatted(&key_obj, value);
        }
    }
    Ok(())
}

/// Apply `transform` to each entry's leading decor prefix in an array-of-tables,
/// stopping at (and reporting) the first slot it changes. A comment before/between
/// `[[key]]` entries lives in an entry's `decor().prefix()`; the projector surfaces
/// it as a child of the AoT, so an edit/delete must rewrite whichever entry prefix
/// holds it. The text-matching transform is a no-op on the slots that don't hold it.
fn transform_aot_entry_prefixes(
    aot: &mut toml_edit::ArrayOfTables,
    transform: &dyn Fn(&str) -> String,
) -> bool {
    for i in 0..aot.len() {
        if let Some(entry) = aot.get_mut(i) {
            let existing = entry
                .decor()
                .prefix()
                .and_then(|r| r.as_str())
                .unwrap_or("")
                .to_string();
            let new_prefix = transform(&existing);
            if new_prefix != existing {
                entry.decor_mut().set_prefix(new_prefix);
                return true;
            }
        }
    }
    false
}

/// Sweep every comment-bearing decor slot of a table — each key's `leaf_decor`
/// prefix (comments before a leaf/inline key), each child `[table]`'s header decor
/// prefix, and each child array-of-tables' entry prefixes — applying `transform`
/// and stopping at the first slot it changes. The projector reads a standalone
/// comment from whichever of these slots precedes the commented item, so a sweep
/// reaches it regardless of the item's position (the old first-key-only locator
/// missed comments before any non-first item). Returns whether a slot changed.
fn sweep_table_comment_slots(tbl: &mut dyn TableLike, transform: &dyn Fn(&str) -> String) -> bool {
    let keys: Vec<String> = tbl.iter().map(|(k, _)| k.to_string()).collect();
    for k in keys {
        // Comment before a leaf/inline key: the key's leaf_decor prefix.
        let leaf_existing = tbl
            .key(&k)
            .and_then(|key| key.leaf_decor().prefix().and_then(|r| r.as_str()))
            .unwrap_or("")
            .to_string();
        let leaf_new = transform(&leaf_existing);
        if leaf_new != leaf_existing {
            if let Some(mut km) = tbl.key_mut(&k) {
                km.leaf_decor_mut().set_prefix(leaf_new);
            }
            return true;
        }
        // Comment before a `[table]` header or `[[aot]]` entries: the item decor.
        match tbl.get_mut(&k) {
            Some(Item::Table(t)) => {
                let existing = t
                    .decor()
                    .prefix()
                    .and_then(|r| r.as_str())
                    .unwrap_or("")
                    .to_string();
                let new_prefix = transform(&existing);
                if new_prefix != existing {
                    t.decor_mut().set_prefix(new_prefix);
                    return true;
                }
            }
            Some(Item::ArrayOfTables(aot)) => {
                if transform_aot_entry_prefixes(aot, transform) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
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
        // A comment node has no real key in the table; its synthetic `#comment:N`
        // key only exists in the projection. Delete it by stripping its block from
        // the decor slot the projector read it from (same locate-and-rewrite path
        // `uncomment` uses), not via `Table::remove`.
        if let Seg::Key(k) = last {
            if k.starts_with("#comment:") {
                let comment_text = {
                    let projected = self.project();
                    find_node_by_path(&projected.root, path)
                        .and_then(|n| match &n.kind {
                            crate::model::node::NodeKind::Comment(t) => Some(t.clone()),
                            _ => None,
                        })
                        .ok_or(MutateError::NotFound)?
                };
                self.remove_comment_from_decor(parent, &comment_text);
                return Ok(());
            }
        }
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
        // When the target parent is an array, append the fragment's values as bare
        // elements (arrays hold values, not key/value pairs). Collision options do
        // not apply — array positions never collide.
        if self.array_at_mut(&target.parent).is_some() {
            let values: Vec<toml_edit::Value> = frag
                .iter()
                .map(|(_, item)| {
                    item.as_value().cloned().ok_or_else(|| {
                        MutateError::Fragment("array elements must be scalar values".into())
                    })
                })
                .collect::<Result<_, _>>()?;
            let arr = self.array_at_mut(&target.parent).expect("array present");
            let idx = target.index.min(arr.len());
            for (offset, v) in values.into_iter().enumerate() {
                arr.insert(idx + offset, v);
            }
            return Ok(());
        }
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
        // Apply only after the whole fragment passed the collision check. For an
        // existing key (Overwrite), replace only the value via `get_mut` so the
        // key's decor — which holds any standalone comment above it — survives;
        // `Table::insert` would drop that decor and the comment with it.
        for (key, item) in insertions {
            match dest.get_mut(&key) {
                Some(slot) => *slot = item,
                None => {
                    dest.insert(&key, item);
                }
            }
        }
        Ok(())
    }

    /// Resolve a `Key+ Index*` path (a run of table keys, then the array key, then
    /// zero or more array-index descents for nested arrays) to the mutable `Array`
    /// it names, or `None` if it does not name an array. Used both to append bare
    /// elements and to address an element for replacement, including in `[[…]]`-free
    /// nested arrays (`[[1,2],[3,4]]`).
    fn array_at_mut(&mut self, path: &[Seg]) -> Option<&mut Array> {
        // The array key is the last `Key`; everything after it descends arrays.
        let key_pos = path.iter().rposition(|s| matches!(s, Seg::Key(_)))?;
        let key = match &path[key_pos] {
            Seg::Key(k) => k.as_str(),
            _ => return None,
        };
        let tbl = self.parent_table_mut(&path[..key_pos]).ok()?;
        let mut arr = tbl.get_mut(key).and_then(Item::as_array_mut)?;
        for seg in &path[key_pos + 1..] {
            let i = match seg {
                Seg::Index(i) => *i,
                Seg::Key(_) => return None,
            };
            arr = arr.get_mut(i).and_then(|v| v.as_array_mut())?;
        }
        Some(arr)
    }

    /// Walk to the mutable table that directly contains the final segment.
    /// Returns `&mut dyn TableLike` so paths can traverse both regular `[table]`
    /// nodes and inline tables (`pt = { x = 1 }`) — the projector emits paths
    /// through both, and `Item::as_table_mut` alone would not match inline tables.
    fn parent_table_mut(&mut self, parent: &[Seg]) -> Result<&mut dyn TableLike, MutateError> {
        let mut tbl: &mut dyn TableLike = self.doc.as_table_mut();
        let mut i = 0;
        while i < parent.len() {
            match &parent[i] {
                Seg::Key(k) => {
                    let item = tbl.get_mut(k).ok_or(MutateError::NotFound)?;
                    // `[[aot]]` entry: a Key naming an array-of-tables followed by an
                    // Index selecting the entry table (which is `TableLike`). The AoT
                    // itself is not table-like, so this descent must be explicit.
                    if let Item::ArrayOfTables(aot) = item {
                        let idx = match parent.get(i + 1) {
                            Some(Seg::Index(n)) => *n,
                            _ => return Err(MutateError::Unsupported),
                        };
                        tbl = aot.get_mut(idx).ok_or(MutateError::NotFound)?;
                        i += 2;
                        continue;
                    }
                    tbl = item.as_table_like_mut().ok_or(MutateError::NotFound)?;
                    i += 1;
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
    fn replace(&mut self, path: &[Seg], toml: &str, sync_decor: bool) -> Result<(), MutateError> {
        // An empty path targets the whole document (external `E` on the root/file
        // node): reparse the edited text as a full document, validating it so a
        // bad edit leaves the doc untouched and surfaces as `Fragment`.
        if path.is_empty() {
            let parsed = toml
                .parse::<DocumentMut>()
                .map_err(|e| MutateError::Fragment(e.to_string()))?;
            self.doc = parsed;
            return Ok(());
        }
        let (parent, last) = path.split_at(path.len().saturating_sub(1));
        let expected_key = match last.first() {
            Some(Seg::Key(k)) => k.as_str(),
            // A trailing `Index` is either a scalar element of a standard array
            // (`parent` names an array) or an array-of-tables entry (`parent` names
            // an AoT) — they round-trip through different write-backs.
            Some(Seg::Index(idx)) => {
                if self.array_at_mut(parent).is_some() {
                    return self.replace_array_element(parent, *idx, toml);
                }
                return self.replace_aot_entry(parent, *idx, toml);
            }
            None => return Err(MutateError::Unsupported),
        };
        let frag = crate::model::fragment::parse_fragment(toml)?;
        if frag.iter().any(|(k, _)| k != expected_key) {
            return Err(MutateError::Fragment(format!(
                "Replace cannot rename key '{expected_key}'; fragment must keep the same key"
            )));
        }
        // When `sync_decor` is set (a full `$EDITOR` node fragment) the edited
        // fragment's key decor is authoritative: it carries any adjacent leading
        // comment, which lives in the key's `leaf_decor` (array/inline table/scalar)
        // or the item decor (`[table]`). The value-only Overwrite below would leave
        // that stale, so capture the edited fragment's key decor and the *original*
        // node's leading blank separator(s) and sync both back afterwards — letting
        // comment edits round-trip while preserving the blank lines the `$EDITOR`
        // view intentionally hid. Inline edits (`sync_decor == false`) skip this
        // entirely, leaving the existing key decor (and its comment) untouched.
        let frag_key_prefix = frag
            .key(expected_key)
            .and_then(|k| k.leaf_decor().prefix().and_then(|r| r.as_str()))
            .unwrap_or("")
            .to_string();
        // Leading blank separator(s) of the node as it exists now, taken before the
        // Overwrite, to re-attach to the trimmed fragment decor on write-back.
        let orig_lead = if sync_decor {
            let mut pfx = String::new();
            if let Ok(tbl) = self.parent_table_mut(parent) {
                pfx = match tbl.get(expected_key) {
                    Some(Item::Table(t)) => t
                        .decor()
                        .prefix()
                        .and_then(|r| r.as_str())
                        .unwrap_or("")
                        .to_string(),
                    _ => tbl
                        .key(expected_key)
                        .and_then(|k| k.leaf_decor().prefix().and_then(|r| r.as_str()))
                        .unwrap_or("")
                        .to_string(),
                };
            }
            split_leading_blank_lines(&pfx).0.to_string()
        } else {
            String::new()
        };
        self.insert_fragment(
            &Target {
                parent: parent.to_vec(),
                index: 0,
            },
            toml,
            crate::model::document::OnCollision::Overwrite,
        )?;
        if sync_decor {
            if let Ok(tbl) = self.parent_table_mut(parent) {
                match tbl.get_mut(expected_key) {
                    // `[table]`: its leading comment/blank lives in the item decor.
                    Some(Item::Table(t)) => {
                        let cur = t
                            .decor()
                            .prefix()
                            .and_then(|r| r.as_str())
                            .unwrap_or("")
                            .to_string();
                        t.decor_mut().set_prefix(format!("{orig_lead}{cur}"));
                    }
                    // array / inline table: comment lives in the key's leaf_decor.
                    _ => {
                        if let Some(mut km) = tbl.key_mut(expected_key) {
                            km.leaf_decor_mut()
                                .set_prefix(format!("{orig_lead}{frag_key_prefix}"));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Replace a single scalar element at `idx` inside the array addressed by the
    /// `Key+ Index*` path `array_path` (supports nested arrays). `toml` is the
    /// validated `<key> = <value>` fragment from the inline editor; the key is a
    /// placeholder and is ignored — only the value is written, preserving every
    /// other element and its format.
    fn replace_array_element(
        &mut self,
        array_path: &[Seg],
        idx: usize,
        toml: &str,
    ) -> Result<(), MutateError> {
        let frag = crate::model::fragment::parse_fragment(toml)?;
        let mut value = frag
            .iter()
            .next()
            .and_then(|(_, item)| item.as_value())
            .ok_or_else(|| MutateError::Fragment("expected a scalar value".into()))?
            .clone();
        let arr = self.array_at_mut(array_path).ok_or(MutateError::NotFound)?;
        if idx >= arr.len() {
            return Err(MutateError::NotFound);
        }
        // Carry over the old element's surrounding whitespace (prefix/suffix) so a
        // multiline array keeps its per-element indentation/newlines after the edit.
        if let Some(old) = arr.get(idx) {
            let decor = old.decor().clone();
            *value.decor_mut() = decor;
        }
        arr.replace(idx, value);
        Ok(())
    }

    /// Replace a single array-of-tables entry (`product[0]`) with the edited
    /// `[[<key>]]` fragment. `aot_path` ends in the AoT key. The fragment must be
    /// exactly one `[[<key>]]` table; its decor (the entry's leading comment/blank
    /// lines) is taken as-is so an `$EDITOR` round-trip preserves spacing, and the
    /// sibling entries are untouched.
    fn replace_aot_entry(
        &mut self,
        aot_path: &[Seg],
        idx: usize,
        toml: &str,
    ) -> Result<(), MutateError> {
        let (head, key_seg) = aot_path.split_at(aot_path.len().saturating_sub(1));
        let aot_key = match key_seg.first() {
            Some(Seg::Key(k)) => k.clone(),
            _ => return Err(MutateError::Unsupported),
        };
        let parsed: DocumentMut = toml
            .parse()
            .map_err(|e: toml_edit::TomlError| MutateError::Fragment(e.to_string()))?;
        let new_entry = match parsed.as_table().get(&aot_key) {
            Some(Item::ArrayOfTables(a)) if a.len() == 1 => a.get(0).expect("len == 1").clone(),
            _ => {
                return Err(MutateError::Fragment(format!(
                    "fragment must be a single [[{aot_key}]] table"
                )))
            }
        };
        let mut new_entry = new_entry;
        let tbl = self.parent_table_mut(head)?;
        let aot = match tbl.get_mut(&aot_key) {
            Some(Item::ArrayOfTables(a)) => a,
            _ => return Err(MutateError::NotFound),
        };
        let slot = aot.get_mut(idx).ok_or(MutateError::NotFound)?;
        // Re-attach the original entry's leading blank separator(s), which the
        // `$EDITOR` view trimmed, so spacing between entries round-trips unchanged.
        let orig_lead = slot
            .decor()
            .prefix()
            .and_then(|r| r.as_str())
            .map(|p| split_leading_blank_lines(p).0.to_string())
            .unwrap_or_default();
        let cur = new_entry
            .decor()
            .prefix()
            .and_then(|r| r.as_str())
            .unwrap_or("")
            .to_string();
        new_entry
            .decor_mut()
            .set_prefix(format!("{orig_lead}{cur}"));
        *slot = new_entry;
        Ok(())
    }

    /// Walk to the concrete `&mut Table` that directly contains the final segment,
    /// following only `Key` segments through standard `[table]`s (not inline
    /// tables). Used by `rename`, which needs `Table`-only APIs (`remove_entry`,
    /// `insert_formatted`) that the `TableLike` trait does not expose.
    fn concrete_table_mut(&mut self, parent: &[Seg]) -> Option<&mut toml_edit::Table> {
        let mut tbl = self.doc.as_table_mut();
        let mut i = 0;
        while i < parent.len() {
            match &parent[i] {
                Seg::Key(k) => {
                    let item = tbl.get_mut(k)?;
                    // `[[aot]]` entry: Key naming the AoT, then an Index into it.
                    if let Item::ArrayOfTables(aot) = item {
                        let idx = match parent.get(i + 1) {
                            Some(Seg::Index(n)) => *n,
                            _ => return None,
                        };
                        tbl = aot.get_mut(idx)?;
                        i += 2;
                        continue;
                    }
                    tbl = item.as_table_mut()?;
                    i += 1;
                }
                Seg::Index(_) => return None,
            }
        }
        Some(tbl)
    }

    /// Name of the first real (non-comment) child key at or after projected `index`
    /// under `parent`, from the current projection. `None` means "append" (index is
    /// at/after the last real key). Resolves a stable anchor so a moved/inserted
    /// entry positions correctly even though `move_inner` deletes sources first.
    fn anchor_key_at(&self, parent: &[Seg], index: usize) -> Option<String> {
        let projected = self.project();
        let children = find_node_by_path(&projected.root, parent)
            .map(|n| n.children.as_slice())
            .unwrap_or(&[]);
        children
            .iter()
            .skip(index)
            .find(|c| !matches!(c.kind, crate::model::node::NodeKind::Comment(_)))
            .and_then(|c| match c.path.last() {
                Some(Seg::Key(k)) => Some(k.clone()),
                _ => None,
            })
    }

    /// Rename the key at `path` to `new_key`, preserving its position, decor (incl.
    /// any standalone comment above it), and every other entry byte-for-byte. The
    /// whole table is re-inserted in order: unchanged keys keep their exact `Key`
    /// object, and the target gets a fresh `Key` carrying the old leaf decor.
    fn rename(&mut self, path: &[Seg], new_key: &str) -> Result<(), MutateError> {
        let (parent, last) = path.split_at(path.len().saturating_sub(1));
        let old = match last.first() {
            Some(Seg::Key(k)) => k.clone(),
            _ => return Err(MutateError::Unsupported),
        };
        if new_key == old {
            return Ok(());
        }
        let new_base: toml_edit::Key = new_key
            .parse()
            .map_err(|e: toml_edit::TomlError| MutateError::Fragment(e.to_string()))?;
        // Standard `[table]` parent: re-insert in order with `Table` APIs.
        if let Some(tbl) = self.concrete_table_mut(parent) {
            return rename_in_table(tbl, &old, &new_base);
        }
        // Inline-table parent (`pt = { x = 1 }`): same order-preserving re-insert
        // with `InlineTable`'s value-typed APIs, so an inline scalar's key renames
        // from the inline editor too.
        if let Some(it) = self.inline_table_mut(parent) {
            return rename_in_inline_table(it, &old, &new_base);
        }
        Err(MutateError::NotFound)
    }

    /// Walk to the concrete `&mut InlineTable` named by the final `Key` of `parent`
    /// (`pt = { x = 1 }`). Returns `None` if the path does not name an inline table.
    fn inline_table_mut(&mut self, parent: &[Seg]) -> Option<&mut toml_edit::InlineTable> {
        let (head, last) = parent.split_at(parent.len().saturating_sub(1));
        let key = match last.first() {
            Some(Seg::Key(k)) => k.as_str(),
            _ => return None,
        };
        let tbl = self.parent_table_mut(head).ok()?;
        tbl.get_mut(key)
            .and_then(Item::as_value_mut)
            .and_then(|v| v.as_inline_table_mut())
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
        // Array destination: arrays hold bare values with no key or decor to
        // preserve, so keep the value-extraction path (string fragments routed
        // through insert_fragment, which descends into arrays).
        if self.array_at_mut(&target.parent).is_some() {
            return self.move_inner_array(sources, target, oc);
        }

        // Table destination: capture each source's (Key, Item) rather than
        // re-serializing through a fresh document. The Key carries leaf_decor —
        // the leading comments and blank lines above a leaf — so they travel
        // with the moved node instead of being dropped.

        // Resolve the anchor key *before* deleting sources (the projection is
        // still intact here). For a same-parent move where a source precedes the
        // target index, deleting sources first would shift the numeric index, so
        // we anchor by *key name* instead of by position.
        let anchor = self.anchor_key_at(&target.parent, target.index);

        let mut captured: Vec<(toml_edit::Key, Item)> = Vec::new();
        for src_path in sources {
            let (parent, last) = src_path.split_at(src_path.len().saturating_sub(1));
            let last = last.first().ok_or(MutateError::NotFound)?;
            let key_name = match last {
                Seg::Key(k) => k.as_str(),
                Seg::Index(_) => return Err(MutateError::Unsupported),
            };
            let table = self.parent_table_mut(parent)?;
            let (key, item) = table.get_key_value(key_name).ok_or(MutateError::NotFound)?;
            captured.push((key.clone(), item.clone()));
        }

        // Delete sources in reverse path order to keep paths valid.
        for src_path in sources.iter().rev() {
            self.remove_at(src_path)?;
        }

        // Re-insert at the destination, preserving decor and honoring the target
        // position. For a concrete standard table use anchor-based positional
        // insert; for inline tables (or non-concrete targets) fall back to append.
        use crate::model::document::OnCollision::*;
        if self.concrete_table_mut(&target.parent).is_some() {
            let tbl = self.concrete_table_mut(&target.parent).expect("checked");
            for (key, item) in captured {
                let name = key.get().to_string();
                if tbl.contains_key(&name) {
                    match oc {
                        Cancel => return Err(MutateError::Collision(name)),
                        Overwrite => {
                            if let Some(slot) = tbl.get_mut(&name) {
                                *slot = item;
                            }
                            continue;
                        }
                        Rename => {
                            let mut n = 2;
                            while tbl.contains_key(&format!("{name}_{n}")) {
                                n += 1;
                            }
                            let mut nk: toml_edit::Key = format!("{name}_{n}").parse().map_err(
                                |e: toml_edit::TomlError| MutateError::Fragment(e.to_string()),
                            )?;
                            *nk.leaf_decor_mut() = key.leaf_decor().clone();
                            insert_before(tbl, nk, item, anchor.as_deref());
                            continue;
                        }
                    }
                }
                insert_before(tbl, key, item, anchor.as_deref());
            }
        } else {
            // Inline table (or non-concrete) destination: keep append behaviour.
            let dest = self.parent_table_mut(&target.parent)?;
            for (key, item) in captured {
                let name = key.get().to_string();
                if dest.contains_key(&name) {
                    match oc {
                        Cancel => return Err(MutateError::Collision(name)),
                        Overwrite => {
                            if let Some(slot) = dest.get_mut(&name) {
                                *slot = item;
                            }
                            continue;
                        }
                        Rename => {
                            let mut n = 2;
                            while dest.contains_key(&format!("{name}_{n}")) {
                                n += 1;
                            }
                            let mut nk: toml_edit::Key = format!("{name}_{n}").parse().map_err(
                                |e: toml_edit::TomlError| MutateError::Fragment(e.to_string()),
                            )?;
                            *nk.leaf_decor_mut() = key.leaf_decor().clone();
                            dest.entry_format(&nk).or_insert(item);
                            continue;
                        }
                    }
                }
                dest.entry_format(&key).or_insert(item);
            }
        }

        Ok(())
    }

    /// Array-destination move: capture each source as a `key = value` fragment
    /// string and route it through `insert_fragment`, which extracts the bare
    /// value and appends it into the array. Arrays carry no key or decor, so the
    /// (Key, Item)-preserving table path does not apply here.
    fn move_inner_array(
        &mut self,
        sources: &[crate::model::node::Path],
        target: &Target,
        oc: crate::model::document::OnCollision,
    ) -> Result<(), MutateError> {
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

        for src_path in sources.iter().rev() {
            self.remove_at(src_path)?;
        }

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
        self.transform_comment_in_decor(parent, &|s| {
            s.replace(&format!("{comment_text}\n"), "")
                .replace(comment_text, "")
        });
    }

    /// Rewrite the decor slot holding `parent`'s standalone comment(s) through
    /// `transform`, by sweeping every candidate slot and stopping at the first it
    /// changes. Shared by comment removal (uncomment / delete) and in-place comment
    /// editing. The text-matching transform only mutates the slot that actually
    /// holds the comment, so a sweep reaches comments before any item (not just the
    /// first) — and an array-of-tables parent, whose comments live in its entry
    /// prefixes rather than any contained table.
    fn transform_comment_in_decor(&mut self, parent: &[Seg], transform: &dyn Fn(&str) -> String) {
        if parent.is_empty() {
            if sweep_table_comment_slots(self.doc.as_table_mut(), transform) {
                return;
            }
            // End-of-document comments (comment-only files / after the last item).
            let trailing = self.doc.trailing().as_str().unwrap_or("").to_string();
            let new_trailing = transform(&trailing);
            if new_trailing != trailing {
                self.doc.set_trailing(new_trailing);
            }
            return;
        }
        // An array-of-tables parent is not table-like; its comments live in the
        // entry prefixes (`parent_table_mut` cannot descend into the AoT key).
        let (head, last) = parent.split_at(parent.len().saturating_sub(1));
        if let Some(Seg::Key(aot_key)) = last.first() {
            let is_aot = matches!(
                self.parent_table_mut(head)
                    .ok()
                    .and_then(|t| t.get(aot_key)),
                Some(Item::ArrayOfTables(_))
            );
            if is_aot {
                if let Ok(t) = self.parent_table_mut(head) {
                    if let Some(Item::ArrayOfTables(aot)) = t.get_mut(aot_key) {
                        transform_aot_entry_prefixes(aot, transform);
                    }
                }
                return;
            }
        }
        if let Ok(tbl) = self.parent_table_mut(parent) {
            sweep_table_comment_slots(tbl, transform);
        }
    }

    /// Edit a (multi-line) comment node in place: replace its current text in the
    /// owning decor slot with `new_text`. The edited text must be comment lines —
    /// every non-blank line must start with `#`; otherwise the document is left
    /// untouched and `Fragment` is returned.
    fn edit_comment(&mut self, path: &[Seg], new_text: &str) -> Result<(), MutateError> {
        let (parent, last) = path.split_at(path.len().saturating_sub(1));
        match last.first() {
            Some(Seg::Key(k)) if k.starts_with("#comment:") => {}
            _ => return Err(MutateError::NotFound),
        }
        let new_text = new_text.trim_end_matches('\n').to_string();
        if new_text
            .lines()
            .any(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        {
            return Err(MutateError::Fragment(
                "comment lines must start with '#'".into(),
            ));
        }
        // Read the current comment text from the projection (handles nesting).
        let old_text = {
            let projected = self.project();
            find_node_by_path(&projected.root, path)
                .and_then(|n| match &n.kind {
                    crate::model::node::NodeKind::Comment(t) => Some(t.clone()),
                    _ => None,
                })
                .ok_or(MutateError::NotFound)?
        };
        if old_text == new_text {
            return Ok(());
        }
        self.transform_comment_in_decor(parent, &|s| s.replacen(&old_text, &new_text, 1));
        Ok(())
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
            Mutation::Replace {
                path,
                toml,
                sync_decor,
            } => self.replace(&path, &toml, sync_decor),
            Mutation::Rename { path, new_key } => self.rename(&path, &new_key),
            Mutation::Remark { path } => self.remark(&path),
            Mutation::EditComment { path, text } => self.edit_comment(&path, &text),
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
    fn replace_empty_path_reparses_whole_document() {
        let mut doc = doc_from_str("a = 1\nb = 2\n");
        doc.apply(Mutation::Replace {
            path: vec![],
            toml: "a = 10\nc = 3\n".to_string(),
            sync_decor: true,
        })
        .unwrap();
        assert_eq!(doc.serialize(), "a = 10\nc = 3\n");
    }

    #[test]
    fn replace_empty_path_rejects_invalid_and_leaves_doc_intact() {
        let mut doc = doc_from_str("a = 1\n");
        let err = doc
            .apply(Mutation::Replace {
                path: vec![],
                toml: "a = = bad".to_string(),
                sync_decor: true,
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Fragment(_)));
        assert_eq!(doc.serialize(), "a = 1\n", "doc must be untouched on error");
    }

    #[test]
    fn split_leading_blank_lines_splits_blanks() {
        assert_eq!(split_leading_blank_lines("\n# c\n"), ("\n", "# c\n"));
        assert_eq!(split_leading_blank_lines("# c\n"), ("", "# c\n"));
        assert_eq!(split_leading_blank_lines("\n\n"), ("\n\n", ""));
        assert_eq!(split_leading_blank_lines(""), ("", ""));
        assert_eq!(split_leading_blank_lines("  \n[t]"), ("  \n", "[t]"));
    }

    #[test]
    fn replace_table_preserves_leading_blank_separator() {
        // The $EDITOR view trims the blank above `[t]`; saving the (unchanged)
        // trimmed fragment must re-attach it so spacing round-trips.
        let mut doc = doc_from_str("a = 1\n\n# c\n[t]\nx = 1\n");
        doc.apply(Mutation::Replace {
            path: vec![Seg::Key("t".into())],
            toml: "# c\n[t]\nx = 1\n".to_string(),
            sync_decor: true,
        })
        .unwrap();
        assert_eq!(doc.serialize(), "a = 1\n\n# c\n[t]\nx = 1\n");
    }

    #[test]
    fn replace_array_preserves_leading_blank_separator() {
        let mut doc = doc_from_str("a = 1\n\n# c\narr = [1, 2]\n");
        doc.apply(Mutation::Replace {
            path: vec![Seg::Key("arr".into())],
            toml: "# c\narr = [1, 2]\n".to_string(),
            sync_decor: true,
        })
        .unwrap();
        assert_eq!(doc.serialize(), "a = 1\n\n# c\narr = [1, 2]\n");
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
            sync_decor: false,
        })
        .unwrap();
        assert!(doc.serialize().contains("port = 9090"));
    }

    #[test]
    fn replace_array_element_preserves_others_and_format() {
        // `e` inline write-back for a scalar array element: only the addressed
        // element changes; the others keep their value and written format.
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str("arr = [0x1, 0o2, 3] # tail\n");
        doc.apply(Mutation::Replace {
            path: vec![Seg::Key("arr".into()), Seg::Index(1)],
            toml: "__elem__ = 99\n".into(),
            sync_decor: false,
        })
        .unwrap();
        assert_eq!(doc.serialize(), "arr = [0x1, 99, 3] # tail\n");
    }

    #[test]
    fn replace_array_element_preserves_multiline_decor() {
        // Editing one element of a multiline array keeps the per-element newline
        // indentation (carried over from the old element's decor).
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str("ml = [\n  \"first\",\n  \"second\",\n]\n");
        doc.apply(Mutation::Replace {
            path: vec![Seg::Key("ml".into()), Seg::Index(0)],
            toml: "__elem__ = \"FIRST\"\n".into(),
            sync_decor: false,
        })
        .unwrap();
        assert_eq!(doc.serialize(), "ml = [\n  \"FIRST\",\n  \"second\",\n]\n");
    }

    #[test]
    fn replace_nested_array_element() {
        // A scalar in an array-of-arrays is addressable by a `Key Index Index` path.
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str("nested = [[1, 2], [3, 4]]\n");
        doc.apply(Mutation::Replace {
            path: vec![Seg::Key("nested".into()), Seg::Index(1), Seg::Index(0)],
            toml: "_ = 99\n".into(),
            sync_decor: false,
        })
        .unwrap();
        assert_eq!(doc.serialize(), "nested = [[1, 2], [99, 4]]\n");
    }

    #[test]
    fn insert_appends_array_element() {
        // `a` on an array inserts a bare value element (not a key/value pair).
        use crate::model::document::{Mutation, OnCollision, Target};
        use crate::model::node::Seg;
        let mut doc = doc_from_str("arr = [1, 2]\nempty = []\n");
        doc.apply(Mutation::Insert {
            target: Target {
                parent: vec![Seg::Key("arr".into())],
                index: 1,
            },
            toml: "_ = 9\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        doc.apply(Mutation::Insert {
            target: Target {
                parent: vec![Seg::Key("empty".into())],
                index: 0,
            },
            toml: "_ = \"x\"\n".into(),
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        assert_eq!(doc.serialize(), "arr = [1, 9, 2]\nempty = [\"x\"]\n");
    }

    #[test]
    fn rename_preserves_order_comment_and_other_keys() {
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str("# lead\na = 1 # ta\nb = 2\nc = 3\n");
        doc.apply(Mutation::Rename {
            path: vec![Seg::Key("b".into())],
            new_key: "beta".into(),
        })
        .unwrap();
        // order preserved, b->beta, every other line byte-identical (incl. comments)
        assert_eq!(doc.serialize(), "# lead\na = 1 # ta\nbeta = 2\nc = 3\n");
    }

    #[test]
    fn rename_preserves_comment_above_renamed_key() {
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str("# hint\nold = 5\n");
        doc.apply(Mutation::Rename {
            path: vec![Seg::Key("old".into())],
            new_key: "fresh".into(),
        })
        .unwrap();
        assert_eq!(doc.serialize(), "# hint\nfresh = 5\n");
    }

    #[test]
    fn rename_rejects_collision_and_invalid_key() {
        use crate::model::document::{MutateError, Mutation};
        use crate::model::node::Seg;
        let mut doc = doc_from_str("a = 1\nb = 2\n");
        let collide = doc.apply(Mutation::Rename {
            path: vec![Seg::Key("a".into())],
            new_key: "b".into(),
        });
        assert!(matches!(collide, Err(MutateError::Collision(_))));
        let invalid = doc.apply(Mutation::Rename {
            path: vec![Seg::Key("a".into())],
            new_key: "bad key".into(),
        });
        assert!(matches!(invalid, Err(MutateError::Fragment(_))));
        // document untouched after both rejections
        assert_eq!(doc.serialize(), "a = 1\nb = 2\n");
    }

    fn first_comment_path(node: &crate::model::node::Node) -> Option<crate::model::node::Path> {
        if matches!(node.kind, crate::model::node::NodeKind::Comment(_)) {
            return Some(node.path.clone());
        }
        node.children.iter().find_map(first_comment_path)
    }

    #[test]
    fn delete_standalone_comment_node() {
        // A comment node's synthetic `#comment:N` key is not a real table entry;
        // Delete must strip it from the decor rather than fail with NotFound.
        use crate::model::document::Mutation;
        // Leading comment before a top-level key.
        let mut doc = doc_from_str("# top\nport = 8080\n");
        let path = first_comment_path(&doc.project().root).expect("top comment");
        doc.apply(Mutation::Delete { path }).unwrap();
        assert_eq!(doc.serialize(), "port = 8080\n");

        // Comment before a `[table]` header.
        let mut doc = doc_from_str("# about\n[server]\nhost = \"x\"\n");
        let path = first_comment_path(&doc.project().root).expect("table comment");
        doc.apply(Mutation::Delete { path }).unwrap();
        assert_eq!(doc.serialize(), "[server]\nhost = \"x\"\n");

        // Comment inside a table, before a leaf.
        let mut doc = doc_from_str("[server]\n# mid\nhost = \"x\"\n");
        let path = first_comment_path(&doc.project().root).expect("nested comment");
        doc.apply(Mutation::Delete { path }).unwrap();
        assert_eq!(doc.serialize(), "[server]\nhost = \"x\"\n");
    }

    #[test]
    fn edit_comment_between_aot_entries() {
        // A comment between successive `[[product]]` entries lives in the next
        // entry's decor prefix and projects as an AoT child; EditComment must
        // rewrite it there (regression: silently no-op'd, "cannot save").
        use crate::model::document::Mutation;
        let mut doc =
            doc_from_str("[[product]]\nname = \"Hammer\"\n# test\n[[product]]\nname = \"Nail\"\n");
        let path = first_comment_path(&doc.project().root).expect("aot comment");
        doc.apply(Mutation::EditComment {
            path,
            text: "# changed".into(),
        })
        .unwrap();
        assert_eq!(
            doc.serialize(),
            "[[product]]\nname = \"Hammer\"\n# changed\n[[product]]\nname = \"Nail\"\n"
        );
    }

    #[test]
    fn delete_comment_between_aot_entries() {
        // Deleting that same AoT-attributed comment strips it from the entry prefix.
        use crate::model::document::Mutation;
        let mut doc =
            doc_from_str("[[product]]\nname = \"Hammer\"\n# test\n[[product]]\nname = \"Nail\"\n");
        let path = first_comment_path(&doc.project().root).expect("aot comment");
        doc.apply(Mutation::Delete { path }).unwrap();
        assert_eq!(
            doc.serialize(),
            "[[product]]\nname = \"Hammer\"\n[[product]]\nname = \"Nail\"\n"
        );
    }

    #[test]
    fn edit_comment_before_aot_in_multi_section_doc() {
        // Regression: a section-separator comment before `[[product]]` when an
        // earlier section precedes it lives in the AoT's first-entry prefix, but the
        // old first-key-only locator looked at the earlier section's slot and missed
        // it ("cannot save" / "cannot delete"). The sweep reaches it now.
        use crate::model::document::Mutation;
        let src = "[meta]\nname = \"x\"\n\n# ── products ──\n[[product]]\nname = \"Hammer\"\n";
        // Find the comment node (not the first node).
        fn nth_comment(node: &crate::model::node::Node, text: &str) -> Option<Vec<Seg>> {
            if matches!(&node.kind, crate::model::node::NodeKind::Comment(t) if t == text) {
                return Some(node.path.clone());
            }
            node.children.iter().find_map(|c| nth_comment(c, text))
        }
        let mut doc = doc_from_str(src);
        let path = nth_comment(&doc.project().root, "# ── products ──").expect("sep comment");
        doc.apply(Mutation::EditComment {
            path: path.clone(),
            text: "# ── PRODUCTS ──".into(),
        })
        .unwrap();
        let s = doc.serialize();
        assert!(
            s.contains("# ── PRODUCTS ──") && !s.contains("# ── products ──"),
            "edit missed: {s:?}"
        );
        // And delete strips it, leaving the meta section intact.
        let mut doc2 = doc_from_str(src);
        let path2 = nth_comment(&doc2.project().root, "# ── products ──").unwrap();
        doc2.apply(Mutation::Delete { path: path2 }).unwrap();
        assert_eq!(
            doc2.serialize(),
            "[meta]\nname = \"x\"\n\n[[product]]\nname = \"Hammer\"\n"
        );
    }

    #[test]
    fn edit_and_delete_comment_inside_aot_entry() {
        // `#123` before a key inside an AoT entry: path carries an `Index` but the
        // comment lives in the key's leaf_decor and is reachable via parent_table_mut.
        use crate::model::document::Mutation;
        let src = "[[product]]\n#123\nname = \"Hammer\"\n";
        let mut doc = doc_from_str(src);
        let path = first_comment_path(&doc.project().root).expect("inner comment");
        assert_eq!(
            path,
            vec![
                Seg::Key("product".into()),
                Seg::Index(0),
                Seg::Key("#comment:0".into())
            ]
        );
        doc.apply(Mutation::EditComment {
            path,
            text: "#321".into(),
        })
        .unwrap();
        assert_eq!(doc.serialize(), "[[product]]\n#321\nname = \"Hammer\"\n");
        let mut doc2 = doc_from_str(src);
        let path2 = first_comment_path(&doc2.project().root).unwrap();
        doc2.apply(Mutation::Delete { path: path2 }).unwrap();
        assert_eq!(doc2.serialize(), "[[product]]\nname = \"Hammer\"\n");
    }

    #[test]
    fn rename_key_inside_inline_table() {
        // `Tab`-rename from the inline editor on an inline-table scalar: the key is
        // renamed in place, preserving order and the other entries.
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str("pt = { x = 1, y = 2 }\n");
        doc.apply(Mutation::Rename {
            path: vec![Seg::Key("pt".into()), Seg::Key("x".into())],
            new_key: "x2".into(),
        })
        .unwrap();
        assert_eq!(doc.serialize(), "pt = { x2 = 1, y = 2 }\n");
    }

    #[test]
    fn replace_scalar_inside_aot_entry() {
        // A scalar member of an array-of-tables entry (`product[0].sku`) is
        // addressable via the `Key→Index` AoT descent: Replace rewrites just that
        // value, leaving every other entry and member untouched.
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str(
            "[[product]]\nname = \"Hammer\"\nsku = 738\n\n[[product]]\nname = \"Nail\"\nsku = 284\n",
        );
        doc.apply(Mutation::Replace {
            path: vec![
                Seg::Key("product".into()),
                Seg::Index(0),
                Seg::Key("sku".into()),
            ],
            toml: "sku = 999\n".into(),
            sync_decor: false,
        })
        .unwrap();
        assert_eq!(
            doc.serialize(),
            "[[product]]\nname = \"Hammer\"\nsku = 999\n\n[[product]]\nname = \"Nail\"\nsku = 284\n"
        );
    }

    #[test]
    fn replace_aot_entry_rewrites_one_entry() {
        // Editing a single `[[product]]` entry rewrites only that entry; the other
        // entry and the between-entries comment are untouched.
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str(
            "[[product]]\nname = \"Hammer\"\nsku = 738\n\n# mid\n[[product]]\nname = \"Nail\"\nsku = 284\n",
        );
        doc.apply(Mutation::Replace {
            path: vec![Seg::Key("product".into()), Seg::Index(0)],
            toml: "[[product]]\nname = \"Mallet\"\nsku = 738\n".into(),
            sync_decor: true,
        })
        .unwrap();
        assert_eq!(
            doc.serialize(),
            "[[product]]\nname = \"Mallet\"\nsku = 738\n\n# mid\n[[product]]\nname = \"Nail\"\nsku = 284\n"
        );
    }

    #[test]
    fn replace_aot_entry_rejects_wrong_key() {
        // A fragment that renames the header (`[[other]]`) is rejected; the doc is
        // left untouched.
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str("[[product]]\nname = \"Hammer\"\n");
        let before = doc.serialize();
        let err = doc
            .apply(Mutation::Replace {
                path: vec![Seg::Key("product".into()), Seg::Index(0)],
                toml: "[[other]]\nname = \"Hammer\"\n".into(),
                sync_decor: true,
            })
            .unwrap_err();
        assert!(matches!(err, MutateError::Fragment(_)));
        assert_eq!(doc.serialize(), before);
    }

    #[test]
    fn rename_key_inside_aot_entry() {
        // `Tab`-rename on an AoT-entry scalar renames the key in place, preserving
        // order, the other members, and the sibling entries.
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str("[[product]]\nname = \"Hammer\"\nsku = 738\n");
        doc.apply(Mutation::Rename {
            path: vec![
                Seg::Key("product".into()),
                Seg::Index(0),
                Seg::Key("sku".into()),
            ],
            new_key: "id".into(),
        })
        .unwrap();
        assert_eq!(
            doc.serialize(),
            "[[product]]\nname = \"Hammer\"\nid = 738\n"
        );
    }

    #[test]
    fn replace_array_roundtrips_edited_leading_comment() {
        // External edit of a structured node (array) carries its leading comment in
        // the key's leaf_decor; an edit to that comment must round-trip on write-back.
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str("# old\nnums = [1, 2]\n");
        doc.apply(Mutation::Replace {
            path: vec![Seg::Key("nums".into())],
            toml: "# new\nnums = [1, 2, 3]\n".into(),
            sync_decor: true,
        })
        .unwrap();
        assert_eq!(doc.serialize(), "# new\nnums = [1, 2, 3]\n");
    }

    #[test]
    fn replace_keeps_standalone_comment_above_key() {
        // Editing the value below a standalone comment must not drop the comment
        // (regression: `Table::insert` over an existing key wiped its decor).
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str("# leading\nport = 8080\n");
        doc.apply(Mutation::Replace {
            path: vec![Seg::Key("port".into())],
            toml: "port = 9090\n".into(),
            sync_decor: false,
        })
        .unwrap();
        let s = doc.serialize();
        assert!(s.contains("# leading"), "comment dropped: {s:?}");
        assert!(s.contains("port = 9090"), "value not updated: {s:?}");
    }

    #[test]
    fn replace_scalar_roundtrips_edited_leading_comment() {
        // External (`$EDITOR`) edit of a *scalar* carries its leading comment in the
        // key's leaf_decor (same slot as an array); with `sync_decor` an edit to that
        // comment — and the value — round-trips on write-back, blanks preserved.
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str("\n# old\nport = 8080\n");
        doc.apply(Mutation::Replace {
            path: vec![Seg::Key("port".into())],
            toml: "# new\nport = 9090\n".into(),
            sync_decor: true,
        })
        .unwrap();
        assert_eq!(doc.serialize(), "\n# new\nport = 9090\n");
    }

    #[test]
    fn replace_scalar_deletes_leading_comment_with_sync_decor() {
        // Removing the comment in `$EDITOR` (fragment carries no prefix) drops it on
        // write-back; the node's leading blank separator is preserved.
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str("\n# gone\nport = 8080\n");
        doc.apply(Mutation::Replace {
            path: vec![Seg::Key("port".into())],
            toml: "port = 9090\n".into(),
            sync_decor: true,
        })
        .unwrap();
        assert_eq!(doc.serialize(), "\nport = 9090\n");
    }

    #[test]
    fn replace_scalar_without_sync_decor_keeps_comment() {
        // The inline value-only path (`sync_decor == false`) must never disturb the
        // comment even though the fragment carries none.
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str("# keep\nport = 8080\n");
        doc.apply(Mutation::Replace {
            path: vec![Seg::Key("port".into())],
            toml: "port = 9090\n".into(),
            sync_decor: false,
        })
        .unwrap();
        assert_eq!(doc.serialize(), "# keep\nport = 9090\n");
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
            sync_decor: false,
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
    fn edit_comment_rewrites_single_line_in_place() {
        use crate::model::document::Mutation;
        let mut doc = doc_from_str("# old note\nx = 1\n");
        let cpath = doc.project().root.children[0].path.clone();
        doc.apply(Mutation::EditComment {
            path: cpath,
            text: "# new note\n".into(),
        })
        .unwrap();
        let s = doc.serialize();
        assert!(s.contains("# new note"), "edited text missing: {s:?}");
        assert!(!s.contains("# old note"), "old text remains: {s:?}");
        assert!(s.contains("x = 1"), "sibling key disturbed: {s:?}");
    }

    #[test]
    fn edit_comment_rewrites_merged_multiline_block() {
        use crate::model::document::Mutation;
        let mut doc = doc_from_str("# a\n# b\nx = 1\n");
        // The two consecutive lines project as one merged comment node.
        let node = &doc.project().root.children[0];
        assert_eq!(
            node.kind,
            crate::model::node::NodeKind::Comment("# a\n# b".into())
        );
        let cpath = node.path.clone();
        doc.apply(Mutation::EditComment {
            path: cpath,
            text: "# a\n# b changed\n# c\n".into(),
        })
        .unwrap();
        let s = doc.serialize();
        assert!(s.contains("# b changed"), "edit missing: {s:?}");
        assert!(s.contains("# c"), "added line missing: {s:?}");
        assert!(s.contains("x = 1"), "sibling key disturbed: {s:?}");
        // Re-projecting yields the new merged block.
        assert_eq!(
            doc.project().root.children[0].kind,
            crate::model::node::NodeKind::Comment("# a\n# b changed\n# c".into())
        );
    }

    #[test]
    fn edit_comment_rejects_non_comment_text() {
        use crate::model::document::{MutateError, Mutation};
        let mut doc = doc_from_str("# note\nx = 1\n");
        let cpath = doc.project().root.children[0].path.clone();
        let err = doc.apply(Mutation::EditComment {
            path: cpath,
            text: "not a comment\n".into(),
        });
        assert!(matches!(err, Err(MutateError::Fragment(_))));
        assert_eq!(doc.serialize(), "# note\nx = 1\n", "document changed");
    }

    #[test]
    fn replace_preserves_key_order() {
        use crate::model::document::Mutation;
        use crate::model::node::Seg;
        let mut doc = doc_from_str("a = 1\nb = 2\nc = 3\n");
        doc.apply(Mutation::Replace {
            path: vec![Seg::Key("b".into())],
            toml: "b = 99\n".into(),
            sync_decor: false,
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
    fn move_preserves_leading_comment_and_blank() {
        // Regression: the moved leaf's leading comment + blank line lived in the
        // key's leaf_decor; the old re-serialize-through-fresh-doc path dropped them.
        use crate::model::document::{Mutation, OnCollision, Target};
        use crate::model::node::Seg;
        let mut doc = doc_from_str("# lead\na = 1\n\n[dest]\n");
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
        assert!(
            s.contains("# lead"),
            "leading comment dropped on move: {s:?}"
        );
        // a is gone from top level and present under dest, with its comment.
        let tree = doc.project();
        let dest = tree.root.children.iter().find(|n| n.key == "dest").unwrap();
        assert!(dest.children.iter().any(|n| n.key == "a"));
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
    fn move_into_table_honors_target_index() {
        // dest has x, y, z; move `a` (top-level) to index 1 (between x and y).
        use crate::model::document::{Mutation, OnCollision, Target};
        use crate::model::node::Seg;
        let mut doc = doc_from_str("a = 1\n[dest]\nx = 1\ny = 2\nz = 3\n");
        doc.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("a".into())]],
            target: Target {
                parent: vec![Seg::Key("dest".into())],
                index: 1,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        let out = doc.serialize();
        let xi = out.find("x = 1").unwrap();
        let ai = out.find("a = 1").unwrap();
        let yi = out.find("y = 2").unwrap();
        assert!(xi < ai && ai < yi, "expected x < a < y, got:\n{out}");
    }

    #[test]
    fn move_within_table_after_anchor_handles_shift() {
        // [a, b, c]; move `a` to just-after `b` (target.index = 2). `a` is deleted
        // before re-insert, so a naive index would land it after `c`. Anchor-based
        // insert must place it between b and c.
        use crate::model::document::{Mutation, OnCollision, Target};
        use crate::model::node::Seg;
        let mut doc = doc_from_str("a = 1\nb = 2\nc = 3\n");
        doc.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("a".into())]],
            target: Target {
                parent: vec![],
                index: 2,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        let out = doc.serialize();
        let bi = out.find("b = 2").unwrap();
        let ai = out.find("a = 1").unwrap();
        let ci = out.find("c = 3").unwrap();
        assert!(bi < ai && ai < ci, "expected b < a < c, got:\n{out}");
    }

    #[test]
    fn move_multi_source_preserves_relative_order() {
        // Move [a, b] (in that order) to the front of [dest] which has y.
        // Expect a, then b, then y.
        use crate::model::document::{Mutation, OnCollision, Target};
        use crate::model::node::Seg;
        let mut doc = doc_from_str("a = 1\nb = 2\n[dest]\ny = 9\n");
        doc.apply(Mutation::Move {
            sources: vec![vec![Seg::Key("a".into())], vec![Seg::Key("b".into())]],
            target: Target {
                parent: vec![Seg::Key("dest".into())],
                index: 0,
            },
            on_collision: OnCollision::Cancel,
        })
        .unwrap();
        let out = doc.serialize();
        let ai = out.find("a = 1").unwrap();
        let bi = out.find("b = 2").unwrap();
        let yi = out.find("y = 9").unwrap();
        assert!(ai < bi && bi < yi, "expected a < b < y, got:\n{out}");
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
