use crate::model::document::ConfigDocument;
use crate::model::node::{NodeTree, Path};
use crate::tui::selection::Selection;
use crate::tui::state::History;
use std::collections::HashSet;

pub struct App {
    pub tree: NodeTree,
    pub expanded: HashSet<Path>,
    pub cursor: usize,
    pub rows: Vec<RowSnapshot>,
    pub selection: Selection,
    /// Present when the app was constructed with a real document (interactive mode).
    pub doc: Option<crate::model::toml_doc::TomlDocument>,
    pub history: Option<History>,
    /// Status message shown in the bottom bar (errors, info).
    pub status: Option<String>,
}

#[derive(Clone)]
pub struct RowSnapshot {
    pub key: String,
    pub path: Path,
    pub depth: usize,
    pub is_branch: bool,
}

impl App {
    /// Construct an App backed by a real TomlDocument (interactive mode).
    pub fn new(doc: crate::model::toml_doc::TomlDocument) -> Self {
        let tree = doc.project();
        let initial_snapshot = doc.serialize();
        let history = History::new(initial_snapshot);
        let mut app = App {
            tree,
            expanded: HashSet::new(),
            cursor: 0,
            rows: Vec::new(),
            selection: Selection::new(),
            doc: Some(doc),
            history: Some(history),
            status: None,
        };
        app.rebuild_rows();
        app
    }

    /// Construct a headless App from a pre-built NodeTree (used in unit tests).
    pub fn from_tree(tree: NodeTree) -> Self {
        App {
            tree,
            expanded: HashSet::new(),
            cursor: 0,
            rows: Vec::new(),
            selection: Selection::new(),
            doc: None,
            history: None,
            status: None,
        }
    }
    pub fn rebuild_rows(&mut self) {
        let expanded = &self.expanded;
        self.rows = self.tree
            .flatten(&|p| expanded.contains(p))
            .into_iter()
            .map(|r| RowSnapshot {
                key: r.node.key.clone(), path: r.node.path.clone(),
                depth: r.depth, is_branch: r.node.is_branch(),
            })
            .collect();
        if self.cursor >= self.rows.len() { self.cursor = self.rows.len().saturating_sub(1); }
        // Selection is keyed by row index; any structural change (expand/collapse
        // or a mutation) invalidates those indices, so clear it rather than let it
        // silently point at the wrong rows. Operations read selected_paths() before
        // rebuilding, so the selection is still live when an op consumes it.
        self.selection.clear();
    }
    pub fn visible_keys(&self) -> Vec<String> { self.rows.iter().map(|r| r.key.clone()).collect() }
    pub fn cursor_down(&mut self) { if self.cursor + 1 < self.rows.len() { self.cursor += 1; } }
    pub fn cursor_up(&mut self) { self.cursor = self.cursor.saturating_sub(1); }
    pub fn toggle_expand(&mut self) {
        if let Some(r) = self.rows.get(self.cursor) {
            if r.is_branch {
                if !self.expanded.remove(&r.path) { self.expanded.insert(r.path.clone()); }
            }
        }
    }
    pub fn collapse_all(&mut self) { self.expanded.clear(); }
    pub fn expand_all(&mut self) {
        let mut all = HashSet::new();
        fn walk(n: &crate::model::node::Node, all: &mut HashSet<Path>) {
            if n.is_branch() { all.insert(n.path.clone()); for c in &n.children { walk(c, all); } }
        }
        walk(&self.tree.root, &mut all);
        self.expanded = all;
    }
    pub fn page_up(&mut self, page_size: usize) {
        let step = page_size.max(1);
        self.cursor = self.cursor.saturating_sub(step);
    }
    pub fn page_down(&mut self, page_size: usize) {
        let step = page_size.max(1);
        let max = self.rows.len().saturating_sub(1);
        self.cursor = (self.cursor + step).min(max);
    }
    pub fn cursor_home(&mut self) { self.cursor = 0; }
    pub fn cursor_end(&mut self) { self.cursor = self.rows.len().saturating_sub(1); }
    pub fn is_expanded(&self, path: &Path) -> bool { self.expanded.contains(path) }

    /// Toggle selection at the current cursor row (bound to `s`).
    pub fn toggle_select(&mut self) {
        self.selection.toggle(self.cursor);
    }

    /// Extend range selection upward (Shift+Up).
    pub fn extend_select_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.selection.extend_to(self.cursor);
        }
    }

    /// Extend range selection downward (Shift+Down).
    pub fn extend_select_down(&mut self) {
        if self.cursor + 1 < self.rows.len() {
            self.cursor += 1;
            self.selection.extend_to(self.cursor);
        }
    }

    /// Return normalized selected paths (§6.2). Falls back to cursor path if nothing selected.
    pub fn selected_paths(&self) -> Vec<Path> {
        if self.selection.indices.is_empty() {
            return self.rows.get(self.cursor)
                .map(|r| vec![r.path.clone()])
                .unwrap_or_default();
        }
        let paths: Vec<Path> = self.selection.indices.iter()
            .filter_map(|&i| self.rows.get(i).map(|r| r.path.clone()))
            .collect();
        crate::tui::selection::normalize(paths)
    }

    /// `e` — edit the cursor node's fragment in $EDITOR and apply Replace.
    /// On MutateError::Fragment: show error in status line, leave doc unchanged.
    pub fn edit_node(&mut self) {
        let doc = match self.doc.as_mut() { Some(d) => d, None => return };
        let cursor_row = match self.rows.get(self.cursor) { Some(r) => r.clone(), None => return };
        let path = cursor_row.path.clone();
        // Serialize just the cursor node's own fragment.
        let fragment = serialize_node_fragment(doc, &path);
        let edited = match crate::tui::editor::edit_text(&fragment) {
            Ok(t) => t,
            Err(e) => { self.status = Some(format!("editor error: {e}")); return; }
        };
        self.apply_replace(path, edited);
    }

    /// Apply edited text as a Replace at `path` (the post-editor half of `e`,
    /// split out so it is unit-testable without spawning $EDITOR). On error the
    /// status line is set and the document is left unchanged.
    pub(crate) fn apply_replace(&mut self, path: Path, edited: String) {
        let doc = match self.doc.as_mut() { Some(d) => d, None => return };
        match doc.apply(crate::model::document::Mutation::Replace { path, toml: edited }) {
            Ok(()) => self.on_mutation_success(),
            Err(crate::model::document::MutateError::Fragment(msg)) => {
                self.status = Some(format!("invalid TOML: {msg}"));
            }
            Err(e) => self.status = Some(format!("error: {e}")),
        }
    }

    /// `n` — open $EDITOR with empty buffer, resolve insertion target, apply Insert.
    /// On Collision: set status (Task 17 will wire the prompt).
    pub fn new_node(&mut self) {
        if self.doc.is_none() { return; }
        let edited = match crate::tui::editor::edit_text("") {
            Ok(t) => t,
            Err(e) => { self.status = Some(format!("editor error: {e}")); return; }
        };
        let cursor_row = self.rows.get(self.cursor).cloned();
        let target = match &cursor_row {
            Some(r) => {
                let expanded = self.expanded.contains(&r.path);
                let sibling_index = sibling_index_of(r, &self.rows);
                crate::tui::insertion::resolve_target(r, expanded, sibling_index)
            }
            None => crate::model::document::Target { parent: vec![], index: 0 },
        };
        self.apply_insert(target, edited);
    }

    /// Apply edited text as an Insert at `target` (the post-editor half of `n`,
    /// split out so it is unit-testable without spawning $EDITOR). On collision or
    /// error the status line is set and the document is left unchanged.
    pub(crate) fn apply_insert(&mut self, target: crate::model::document::Target, edited: String) {
        let doc = match self.doc.as_mut() { Some(d) => d, None => return };
        match doc.apply(crate::model::document::Mutation::Insert {
            target,
            toml: edited,
            on_collision: crate::model::document::OnCollision::Cancel,
        }) {
            Ok(()) => self.on_mutation_success(),
            Err(crate::model::document::MutateError::Collision(key)) => {
                self.status = Some(format!("key collision: {key} (rename/overwrite not yet prompted)"));
            }
            Err(e) => self.status = Some(format!("error: {e}")),
        }
    }

    /// Shared post-mutation bookkeeping: snapshot for undo, re-project, rebuild
    /// rows, clear the status line.
    fn on_mutation_success(&mut self) {
        if let Some(doc) = self.doc.as_ref() {
            let snapshot = doc.serialize();
            let tree = doc.project();
            if let Some(h) = self.history.as_mut() { h.push(snapshot); }
            self.tree = tree;
        }
        self.rebuild_rows();
        self.status = None;
    }
}

/// Serialize a single node at `path` as a TOML fragment string.
fn serialize_node_fragment(doc: &crate::model::toml_doc::TomlDocument, path: &[crate::model::node::Seg]) -> String {
    use crate::model::node::Seg;
    use toml_edit::{DocumentMut, Item};
    if path.is_empty() { return doc.serialize(); }
    let (parent_segs, last) = path.split_at(path.len().saturating_sub(1));
    let key = match last.first() {
        Some(Seg::Key(k)) => k.as_str(),
        _ => return String::new(),
    };
    // Walk to the parent table
    let mut tbl: &dyn toml_edit::TableLike = doc.doc.as_table();
    for seg in parent_segs {
        match seg {
            Seg::Key(k) => {
                tbl = match tbl.get(k).and_then(Item::as_table_like) {
                    Some(t) => t,
                    None => return String::new(),
                };
            }
            _ => return String::new(),
        }
    }
    let item = match tbl.get(key) {
        Some(i) => i.clone(),
        None => return String::new(),
    };
    let mut tmp = DocumentMut::new();
    tmp.as_table_mut().insert(key, item);
    tmp.to_string()
}

/// Compute the 0-based index of `row` within its parent's visible children.
fn sibling_index_of(row: &RowSnapshot, rows: &[RowSnapshot]) -> usize {
    let parent_depth = row.depth.saturating_sub(1);
    // Locate the cursor row in the flattened list by path (paths are unique).
    let row_pos = rows.iter().position(|r| r.path == row.path).unwrap_or(0);
    // Count siblings (same depth) before this row within the same parent
    let mut count = 0usize;
    for r in rows[..row_pos].iter().rev() {
        if r.depth == row.depth { count += 1; }
        else if r.depth <= parent_depth { break; }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::*;

    fn sample() -> App {
        // build a tree: root > [a(branch: x), b(leaf)]
        let mut x = Node::leaf("x", NodeKind::Scalar(ScalarType::Integer));
        x.path = vec![Seg::Key("a".into()), Seg::Key("x".into())];
        let mut a = Node::branch("a", NodeKind::Table);
        a.path = vec![Seg::Key("a".into())];
        a.children = vec![x];
        let mut b = Node::leaf("b", NodeKind::Scalar(ScalarType::Integer));
        b.path = vec![Seg::Key("b".into())];
        let mut root = Node::branch("f.toml", NodeKind::Root);
        root.children = vec![a, b];
        App::from_tree(NodeTree { root })
    }

    #[test]
    fn cursor_moves_and_expand_reveals_children() {
        let mut app = sample();
        app.rebuild_rows();
        // collapsed: root, a, b
        assert_eq!(app.visible_keys(), vec!["f.toml", "a", "b"]);
        app.cursor_down(); // on `a`
        app.toggle_expand(); // expand a
        app.rebuild_rows();
        assert_eq!(app.visible_keys(), vec!["f.toml", "a", "x", "b"]);
        app.collapse_all();
        app.rebuild_rows();
        assert_eq!(app.visible_keys(), vec!["f.toml", "a", "b"]);
    }

    #[test]
    fn rebuild_clears_stale_selection() {
        // Selecting rows then changing structure (expand) must not leave stale
        // row-index selections pointing at the wrong rows.
        let mut app = sample();
        app.rebuild_rows();
        app.cursor_down(); // on `a`
        app.toggle_select(); // select `a`
        assert!(!app.selection.indices.is_empty());
        app.toggle_expand();
        app.rebuild_rows(); // structure changed
        assert!(app.selection.indices.is_empty(), "selection must clear on rebuild");
    }

    #[test]
    fn expand_all_reveals_all_descendants() {
        // `9` expands every branch at all depths; `0` collapses back.
        let mut app = sample();
        app.expand_all();
        app.rebuild_rows();
        assert_eq!(app.visible_keys(), vec!["f.toml", "a", "x", "b"]);
        // round-trip symmetry: collapse_all then expand_all returns to full view
        app.collapse_all();
        app.rebuild_rows();
        assert_eq!(app.visible_keys(), vec!["f.toml", "a", "b"]);
        app.expand_all();
        app.rebuild_rows();
        assert_eq!(app.visible_keys(), vec!["f.toml", "a", "x", "b"]);
    }

    // --- e/n apply-path tests (post-editor logic, no $EDITOR spawned) ---

    fn app_with(src: &str) -> App {
        use crate::model::document::ConfigDocument;
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(src.as_bytes()).unwrap();
        let doc = crate::model::toml_doc::TomlDocument::load(f.path()).unwrap();
        App::new(doc)
    }

    #[test]
    fn apply_replace_invalid_toml_sets_status_and_leaves_doc() {
        let mut app = app_with("port = 8080\n");
        let before = app.doc.as_ref().unwrap().serialize();
        app.apply_replace(vec![Seg::Key("port".into())], "port = = nope".into());
        assert!(app.status.is_some(), "invalid TOML must surface in status");
        assert_eq!(app.doc.as_ref().unwrap().serialize(), before, "doc unchanged");
    }

    #[test]
    fn apply_replace_valid_pushes_history_and_rebuilds() {
        let mut app = app_with("port = 8080\n");
        app.apply_replace(vec![Seg::Key("port".into())], "port = 9090\n".into());
        assert!(app.status.is_none());
        assert!(app.doc.as_ref().unwrap().serialize().contains("9090"));
        // history advanced: undo restores the pre-edit snapshot
        let restored = app.history.as_mut().unwrap().undo().unwrap();
        assert!(restored.contains("8080"));
    }

    #[test]
    fn apply_insert_collision_sets_status_and_leaves_doc() {
        let mut app = app_with("port = 8080\n");
        let before = app.doc.as_ref().unwrap().serialize();
        app.apply_insert(
            crate::model::document::Target { parent: vec![], index: 1 },
            "port = 1\n".into(),
        );
        assert!(app.status.is_some(), "collision must surface in status");
        assert_eq!(app.doc.as_ref().unwrap().serialize(), before, "doc unchanged");
    }

    #[test]
    fn apply_insert_valid_pushes_history_and_rebuilds() {
        let mut app = app_with("port = 8080\n");
        app.apply_insert(
            crate::model::document::Target { parent: vec![], index: 1 },
            "host = \"x\"\n".into(),
        );
        assert!(app.status.is_none());
        assert!(app.doc.as_ref().unwrap().serialize().contains("host = \"x\""));
        // reproject + rebuild surfaced the new key as a visible row
        assert!(app.visible_keys().contains(&"host".to_string()));
        let restored = app.history.as_mut().unwrap().undo().unwrap();
        assert!(!restored.contains("host"));
    }
}
