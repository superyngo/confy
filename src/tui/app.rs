use crate::model::document::{ConfigDocument, Mutation, OnCollision, Target};
use crate::model::node::{NodeTree, Path, Seg};
use crate::tui::search::{fuzzy_match, haystack};
use crate::tui::selection::Selection;
use crate::tui::state::{Clipboard, History, Mode, PromptKind};
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
    pub mode: Mode,
    pub clipboard: Option<Clipboard>,
    /// Saved sources + target for a move that entered MoveCollision prompt.
    pub pending_move: Option<(Vec<Path>, Target)>,
    /// Filter state: current filter string. When non-empty, rows are filtered.
    pub filter: String,
    /// Set of node paths that match the current filter (including ancestors kept for context).
    pub filtered_paths: Option<HashSet<Path>>,
    /// Read-only detail text for the current detail popup.
    pub detail_text: Option<String>,
}

#[derive(Clone)]
pub struct RowSnapshot {
    pub key: String,
    pub path: Path,
    pub depth: usize,
    pub is_branch: bool,
    pub value: Option<String>,
    pub scalar_type: Option<String>,
    pub trailing_comment: Option<String>,
}

pub enum PromptOutcome {
    Consumed,
    Quit,
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
            mode: Mode::Normal,
            clipboard: None,
            pending_move: None,
            filter: String::new(),
            filtered_paths: None,
            detail_text: None,
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
            mode: Mode::Normal,
            clipboard: None,
            pending_move: None,
            filter: String::new(),
            filtered_paths: None,
            detail_text: None,
        }
    }
    pub fn rebuild_rows(&mut self) {
        let expanded = &self.expanded;
        let rows = self
            .tree
            .flatten(&|p| expanded.contains(p))
            .into_iter()
            .map(|r| {
                let scalar_type = match &r.node.kind {
                    crate::model::node::NodeKind::Scalar(st) => {
                        Some(format!("{st:?}").to_lowercase())
                    }
                    _ => None,
                };
                RowSnapshot {
                    key: r.node.key.clone(),
                    path: r.node.path.clone(),
                    depth: r.depth,
                    is_branch: r.node.is_branch(),
                    value: r.node.value.clone(),
                    scalar_type,
                    trailing_comment: r.node.trailing_comment.clone(),
                }
            })
            .collect::<Vec<_>>();
        // Apply filter if active: keep rows whose path is in filtered_paths.
        self.rows = if let Some(ref fp) = self.filtered_paths {
            rows.into_iter().filter(|r| fp.contains(&r.path)).collect()
        } else {
            rows
        };
        if self.cursor >= self.rows.len() {
            self.cursor = self.rows.len().saturating_sub(1);
        }
        // Selection is keyed by row index; any structural change (expand/collapse
        // or a mutation) invalidates those indices, so clear it rather than let it
        // silently point at the wrong rows. Operations read selected_paths() before
        // rebuilding, so the selection is still live when an op consumes it.
        self.selection.clear();
    }
    pub fn visible_keys(&self) -> Vec<String> {
        self.rows.iter().map(|r| r.key.clone()).collect()
    }
    pub fn cursor_down(&mut self) {
        if self.cursor + 1 < self.rows.len() {
            self.cursor += 1;
        }
    }
    pub fn cursor_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }
    pub fn toggle_expand(&mut self) {
        if let Some(r) = self.rows.get(self.cursor) {
            if r.is_branch && !self.expanded.remove(&r.path) {
                self.expanded.insert(r.path.clone());
            }
        }
    }
    pub fn collapse_all(&mut self) {
        self.expanded.clear();
    }
    pub fn expand_all(&mut self) {
        let mut all = HashSet::new();
        fn walk(n: &crate::model::node::Node, all: &mut HashSet<Path>) {
            if n.is_branch() {
                all.insert(n.path.clone());
                for c in &n.children {
                    walk(c, all);
                }
            }
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
    pub fn cursor_home(&mut self) {
        self.cursor = 0;
    }
    pub fn cursor_end(&mut self) {
        self.cursor = self.rows.len().saturating_sub(1);
    }
    pub fn is_expanded(&self, path: &Path) -> bool {
        self.expanded.contains(path)
    }

    // ---- Filter (/) ----

    /// `/` — enter filter mode.
    pub fn enter_filter(&mut self) {
        self.filter.clear();
        self.filtered_paths = None;
        self.mode = Mode::Filter;
        self.rebuild_rows();
    }

    /// Feed a character into the active filter.
    pub fn filter_char(&mut self, c: char) {
        self.filter.push(c);
        self.recompute_filter();
        self.rebuild_rows();
    }

    /// Backspace in filter mode.
    pub fn filter_backspace(&mut self) {
        self.filter.pop();
        self.recompute_filter();
        self.rebuild_rows();
    }

    /// Compute which paths match the current filter string. A node is visible
    /// if it matches OR is an ancestor of a match (keep context).
    fn recompute_filter(&mut self) {
        if self.filter.is_empty() {
            self.filtered_paths = None;
            return;
        }
        let mut matching: HashSet<Path> = HashSet::new();
        let mut ancestors: HashSet<Path> = HashSet::new();
        fn walk(
            n: &crate::model::node::Node,
            ancestor_paths: &mut Vec<Path>,
            matching: &mut HashSet<Path>,
            ancestors: &mut HashSet<Path>,
            needle: &str,
        ) {
            let path_keys: Vec<&str> = n
                .path
                .iter()
                .filter_map(|s| match s {
                    Seg::Key(k) => Some(k.as_str()),
                    _ => None,
                })
                .collect();
            let leaf_value = if n.is_leaf() {
                n.value.as_deref()
            } else {
                None
            };
            let comment = match &n.kind {
                crate::model::node::NodeKind::Comment(c) => Some(c.as_str()),
                _ => None,
            };
            let h = haystack(&path_keys, leaf_value, comment);
            if fuzzy_match(&h, needle) {
                matching.insert(n.path.clone());
                for anc in ancestor_paths.iter() {
                    ancestors.insert(anc.clone());
                }
            }
            ancestor_paths.push(n.path.clone());
            for c in &n.children {
                walk(c, ancestor_paths, matching, ancestors, needle);
            }
            ancestor_paths.pop();
        }
        walk(
            &self.tree.root,
            &mut Vec::new(),
            &mut matching,
            &mut ancestors,
            &self.filter,
        );
        matching.extend(ancestors);
        self.filtered_paths = Some(matching);
    }

    /// Esc from filter mode clears and restores full view.
    pub fn exit_filter(&mut self) {
        self.filter.clear();
        self.filtered_paths = None;
        self.mode = Mode::Normal;
        self.rebuild_rows();
    }

    // ---- Detail view (Leaf Enter/Space) ----

    /// Enter/Space on a Leaf opens a read-only detail popup.
    pub fn open_detail(&mut self) {
        let row = match self.rows.get(self.cursor) {
            Some(r) => r.clone(),
            None => return,
        };
        debug_assert!(!row.is_branch, "open_detail called on a branch row");
        let path_keys: Vec<String> = row
            .path
            .iter()
            .filter_map(|s| match s {
                Seg::Key(k) => Some(k.clone()),
                _ => None,
            })
            .collect();
        let dotted = path_keys.join(".");
        let type_str = row.scalar_type.as_deref().unwrap_or("unknown");
        let val_str = row.value.as_deref().unwrap_or("");
        let mut detail = format!("Path:    {dotted}\nType:    {type_str}\nValue:   {val_str}");
        if let Some(tc) = &row.trailing_comment {
            detail.push_str(&format!("\nComment: {tc}"));
        }
        self.detail_text = Some(detail);
        self.mode = Mode::Detail;
    }

    /// Esc from detail view.
    pub fn exit_detail(&mut self) {
        self.detail_text = None;
        self.mode = Mode::Normal;
    }

    // ---- Help (?) ----

    /// `?` — show help overlay.
    pub fn enter_help(&mut self) {
        self.mode = Mode::Help;
    }

    /// Esc from help overlay.
    pub fn exit_help(&mut self) {
        self.mode = Mode::Normal;
    }

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
            return self
                .rows
                .get(self.cursor)
                .map(|r| vec![r.path.clone()])
                .unwrap_or_default();
        }
        let paths: Vec<Path> = self
            .selection
            .indices
            .iter()
            .filter_map(|&i| self.rows.get(i).map(|r| r.path.clone()))
            .collect();
        crate::tui::selection::normalize(paths)
    }

    /// `e` — edit the cursor node's fragment in $EDITOR and apply Replace.
    /// On MutateError::Fragment: show error in status line, leave doc unchanged.
    pub fn edit_node(&mut self) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        let cursor_row = match self.rows.get(self.cursor) {
            Some(r) => r.clone(),
            None => return,
        };
        let path = cursor_row.path.clone();
        // Serialize just the cursor node's own fragment.
        let fragment = serialize_node_fragment(doc, &path);
        let edited = match crate::tui::editor::edit_text(&fragment) {
            Ok(t) => t,
            Err(e) => {
                self.status = Some(format!("editor error: {e}"));
                return;
            }
        };
        self.apply_replace(path, edited);
    }

    /// Apply edited text as a Replace at `path` (the post-editor half of `e`,
    /// split out so it is unit-testable without spawning $EDITOR). On error the
    /// status line is set and the document is left unchanged.
    pub(crate) fn apply_replace(&mut self, path: Path, edited: String) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
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
        if self.doc.is_none() {
            return;
        }
        let edited = match crate::tui::editor::edit_text("") {
            Ok(t) => t,
            Err(e) => {
                self.status = Some(format!("editor error: {e}"));
                return;
            }
        };
        let cursor_row = self.rows.get(self.cursor).cloned();
        let target = match &cursor_row {
            Some(r) => {
                let expanded = self.expanded.contains(&r.path);
                let sibling_index = sibling_index_of(r, &self.rows);
                crate::tui::insertion::resolve_target(r, expanded, sibling_index)
            }
            None => crate::model::document::Target {
                parent: vec![],
                index: 0,
            },
        };
        self.apply_insert(target, edited);
    }

    /// Apply edited text as an Insert at `target` (the post-editor half of `n`,
    /// split out so it is unit-testable without spawning $EDITOR). On collision or
    /// error the status line is set and the document is left unchanged.
    pub(crate) fn apply_insert(&mut self, target: crate::model::document::Target, edited: String) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        match doc.apply(crate::model::document::Mutation::Insert {
            target,
            toml: edited,
            on_collision: crate::model::document::OnCollision::Cancel,
        }) {
            Ok(()) => self.on_mutation_success(),
            Err(crate::model::document::MutateError::Collision(key)) => {
                self.status = Some(format!(
                    "key collision: {key} (rename/overwrite not yet prompted)"
                ));
            }
            Err(crate::model::document::MutateError::Fragment(msg)) => {
                self.status = Some(format!("invalid TOML: {msg}"));
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
            if let Some(h) = self.history.as_mut() {
                h.push(snapshot);
            }
            self.tree = tree;
        }
        self.rebuild_rows();
        self.status = None;
    }

    // ---- §6 operations: d/x/c/v/m/r/z/y ----

    /// `d` — delete selected or cursor node(s).
    pub fn delete_selected(&mut self) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            return;
        }
        let mut paths = paths;
        // Reverse path order (longer first) so deletions don't invalidate later paths.
        paths.sort_by_key(|b| std::cmp::Reverse(b.len()));
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        for p in &paths {
            if let Err(e) = doc.apply(Mutation::Delete { path: p.clone() }) {
                self.status = Some(format!("delete error: {e}"));
                return;
            }
        }
        self.on_mutation_success();
    }

    /// `c` — copy selected nodes' fragments into clipboard.
    pub fn copy_selected(&mut self) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            return;
        }
        let doc = match self.doc.as_ref() {
            Some(d) => d,
            None => return,
        };
        let mut fragments = Vec::new();
        for p in &paths {
            fragments.push(serialize_node_fragment(doc, p));
        }
        self.clipboard = Some(Clipboard {
            fragments,
            cut: false,
            sources: Vec::new(),
        });
        self.status = Some(format!(
            "copied {} node(s)",
            self.clipboard.as_ref().unwrap().fragments.len()
        ));
    }

    /// `x` — cut: copy fragments + remember sources. Deletion deferred to paste (wenv-style).
    pub fn cut_selected(&mut self) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            return;
        }
        let doc = match self.doc.as_ref() {
            Some(d) => d,
            None => return,
        };
        let mut fragments = Vec::new();
        for p in &paths {
            fragments.push(serialize_node_fragment(doc, p));
        }
        self.clipboard = Some(Clipboard {
            fragments,
            cut: true,
            sources: paths,
        });
        self.status = Some(format!(
            "cut {} node(s)",
            self.clipboard.as_ref().unwrap().fragments.len()
        ));
    }

    /// `v` — paste clipboard fragments at insertion target.
    /// On Collision: enters Mode::Prompt(Collision{key}).
    /// If clipboard was cut, deletes sources after successful paste.
    pub fn paste(&mut self) {
        let (fragments, is_cut, sources) = match self.clipboard.take() {
            Some(cb) => (cb.fragments, cb.cut, cb.sources),
            None => {
                self.status = Some("clipboard empty".into());
                return;
            }
        };
        let cursor_row = match self.rows.get(self.cursor) {
            Some(r) => r.clone(),
            None => return,
        };
        let expanded = self.expanded.contains(&cursor_row.path);
        let sibling_index = sibling_index_of(&cursor_row, &self.rows);
        let target = crate::tui::insertion::resolve_target(&cursor_row, expanded, sibling_index);
        self.do_paste(fragments, is_cut, sources, target, OnCollision::Cancel);
    }

    /// Core paste logic, split out so it can be re-issued after a collision prompt.
    pub(crate) fn do_paste(
        &mut self,
        fragments: Vec<String>,
        is_cut: bool,
        sources: Vec<Path>,
        target: Target,
        on_collision: OnCollision,
    ) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        for (i, frag) in fragments.iter().enumerate() {
            match doc.apply(Mutation::Insert {
                target: target.clone(),
                toml: frag.clone(),
                on_collision,
            }) {
                Ok(()) => {}
                Err(crate::model::document::MutateError::Collision(key)) => {
                    // Put only the remaining unprocessed fragments back so retry
                    // with Rename doesn't re-insert already-inserted fragments.
                    self.clipboard = Some(Clipboard {
                        fragments: fragments[i..].to_vec(),
                        cut: is_cut,
                        sources,
                    });
                    self.status = Some(format!("collision on key '{key}' — o/r/c"));
                    self.mode = Mode::Prompt(PromptKind::Collision { key });
                    return;
                }
                Err(e) => {
                    self.status = Some(format!("paste error: {e}"));
                    return;
                }
            }
        }
        // If cut, delete source nodes after successful paste.
        if is_cut {
            let mut sorted_sources = sources;
            sorted_sources.sort_by_key(|b| std::cmp::Reverse(b.len()));
            for src in &sorted_sources {
                if let Err(e) = doc.apply(Mutation::Delete { path: src.clone() }) {
                    self.status = Some(format!("cut-delete error: {e}"));
                    return;
                }
            }
        }
        self.on_mutation_success();
    }

    /// `m` — move: first press enters MovePending, second press executes.
    pub fn move_pressed(&mut self) {
        match &self.mode {
            Mode::Normal => {
                let sources = self.selected_paths();
                if sources.is_empty() {
                    return;
                }
                self.mode = Mode::MovePending { sources };
                self.status =
                    Some("move-pending: navigate then press m to drop, Esc to cancel".into());
            }
            Mode::MovePending { .. } => {
                let sources = match std::mem::replace(&mut self.mode, Mode::Normal) {
                    Mode::MovePending { sources } => sources,
                    _ => unreachable!(),
                };
                let cursor_row = match self.rows.get(self.cursor) {
                    Some(r) => r.clone(),
                    None => return,
                };
                let expanded = self.expanded.contains(&cursor_row.path);
                let sibling_index = sibling_index_of(&cursor_row, &self.rows);
                let target =
                    crate::tui::insertion::resolve_target(&cursor_row, expanded, sibling_index);
                let doc = match self.doc.as_mut() {
                    Some(d) => d,
                    None => return,
                };
                match doc.apply(Mutation::Move {
                    sources: sources.clone(),
                    target: target.clone(),
                    on_collision: OnCollision::Cancel,
                }) {
                    Ok(()) => {
                        self.on_mutation_success();
                    }
                    Err(crate::model::document::MutateError::Collision(key)) => {
                        // Enter a prompt so user can choose o/r/c; preserve sources+target.
                        self.pending_move = Some((sources, target));
                        self.status = Some(format!(
                            "move collision on '{key}' — o:overwrite  r:rename  c:cancel"
                        ));
                        self.mode = Mode::Prompt(PromptKind::MoveCollision { key });
                    }
                    Err(e) => {
                        self.status = Some(format!("move error: {e}"));
                    }
                }
            }
            Mode::Prompt(_) | Mode::Filter | Mode::Detail | Mode::Help => {}
        }
    }
    pub fn escape(&mut self) {
        match &self.mode {
            Mode::MovePending { .. } => {
                self.mode = Mode::Normal;
                self.status = Some("move cancelled".into());
            }
            Mode::Prompt(_) => {
                self.mode = Mode::Normal;
                self.clipboard = None;
                self.pending_move = None;
                self.status = None;
            }
            Mode::Filter => self.exit_filter(),
            Mode::Detail => self.exit_detail(),
            Mode::Help => self.exit_help(),
            Mode::Normal => {}
        }
    }

    /// `r` — toggle remark on cursor node.
    pub fn remark(&mut self) {
        let path = match self.rows.get(self.cursor) {
            Some(r) => r.path.clone(),
            None => return,
        };
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        match doc.apply(Mutation::Remark { path }) {
            Ok(()) => self.on_mutation_success(),
            Err(crate::model::document::MutateError::Fragment(_)) => {
                self.status = Some("not valid TOML, kept as comment".into());
            }
            Err(e) => self.status = Some(format!("remark error: {e}")),
        }
    }

    /// `z` — undo.
    pub fn undo(&mut self) {
        let snapshot = match self.history.as_mut().and_then(|h| h.undo()) {
            Some(s) => s,
            None => {
                self.status = Some("nothing to undo".into());
                return;
            }
        };
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        match doc.replace_from_str(&snapshot) {
            Ok(()) => {
                self.tree = doc.project();
                self.rebuild_rows();
                self.status = None;
            }
            Err(e) => self.status = Some(format!("undo restore error: {e}")),
        }
    }

    /// `y` — redo.
    pub fn redo(&mut self) {
        let snapshot = match self.history.as_mut().and_then(|h| h.redo()) {
            Some(s) => s,
            None => {
                self.status = Some("nothing to redo".into());
                return;
            }
        };
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        match doc.replace_from_str(&snapshot) {
            Ok(()) => {
                self.tree = doc.project();
                self.rebuild_rows();
                self.status = None;
            }
            Err(e) => self.status = Some(format!("redo restore error: {e}")),
        }
    }

    /// Handle a key press while in a prompt mode. Returns true if consumed.
    pub fn handle_prompt_key(&mut self, c: char) -> PromptOutcome {
        match &self.mode {
            Mode::Prompt(PromptKind::Collision { key: _ }) => {
                let oc = match c {
                    'o' => OnCollision::Overwrite,
                    'r' => OnCollision::Rename,
                    // 'c' or any other key cancels.
                    _ => OnCollision::Cancel,
                };
                if !matches!(c, 'o' | 'r') {
                    // Cancel
                    self.mode = Mode::Normal;
                    self.clipboard = None;
                    self.status = None;
                    return PromptOutcome::Consumed;
                }
                let cb = self.clipboard.take();
                let (fragments, is_cut, sources) = match cb {
                    Some(cb) => (cb.fragments, cb.cut, cb.sources),
                    None => {
                        self.mode = Mode::Normal;
                        return PromptOutcome::Consumed;
                    }
                };
                let cursor_row = match self.rows.get(self.cursor) {
                    Some(r) => r.clone(),
                    None => {
                        self.mode = Mode::Normal;
                        return PromptOutcome::Consumed;
                    }
                };
                let expanded = self.expanded.contains(&cursor_row.path);
                let sibling_index = sibling_index_of(&cursor_row, &self.rows);
                let target =
                    crate::tui::insertion::resolve_target(&cursor_row, expanded, sibling_index);
                self.mode = Mode::Normal;
                self.do_paste(fragments, is_cut, sources, target, oc);
                PromptOutcome::Consumed
            }
            Mode::Prompt(PromptKind::ConfirmQuit) => match c {
                'y' => {
                    self.mode = Mode::Normal;
                    self.clipboard = None;
                    self.status = None;
                    PromptOutcome::Quit
                }
                'n' => {
                    self.mode = Mode::Normal;
                    self.clipboard = None;
                    self.status = None;
                    PromptOutcome::Consumed
                }
                _ => PromptOutcome::Consumed,
            },
            Mode::Prompt(PromptKind::MoveCollision { .. }) => {
                let on_collision = match c {
                    'o' => OnCollision::Overwrite,
                    'r' => OnCollision::Rename,
                    _ => {
                        self.mode = Mode::Normal;
                        self.pending_move = None;
                        self.status = None;
                        return PromptOutcome::Consumed;
                    }
                };
                let (sources, target) = match self.pending_move.take() {
                    Some(pm) => pm,
                    None => {
                        self.mode = Mode::Normal;
                        return PromptOutcome::Consumed;
                    }
                };
                self.mode = Mode::Normal;
                let doc = match self.doc.as_mut() {
                    Some(d) => d,
                    None => return PromptOutcome::Consumed,
                };
                match doc.apply(Mutation::Move {
                    sources,
                    target,
                    on_collision,
                }) {
                    Ok(()) => self.on_mutation_success(),
                    Err(crate::model::document::MutateError::Collision(key)) => {
                        self.status = Some(format!("move collision: {key}"));
                    }
                    Err(e) => self.status = Some(format!("move error: {e}")),
                }
                PromptOutcome::Consumed
            }
            _ => PromptOutcome::Consumed,
        }
    }

    /// Check if mode is ConfirmQuit and user confirmed.
    pub fn confirm_quit(&self) -> bool {
        matches!(&self.mode, Mode::Prompt(PromptKind::ConfirmQuit))
    }

    /// Enter quit-confirm prompt if dirty.
    pub fn quit_requested(&mut self) -> bool {
        let dirty = self.doc.as_ref().map(|d| d.is_dirty()).unwrap_or(false);
        if dirty {
            self.mode = Mode::Prompt(PromptKind::ConfirmQuit);
            self.status = Some("unsaved changes — quit? y/n".into());
            false
        } else {
            true
        }
    }
}

/// Serialize a single node at `path` as a TOML fragment string.
fn serialize_node_fragment(
    doc: &crate::model::toml_doc::TomlDocument,
    path: &[crate::model::node::Seg],
) -> String {
    use crate::model::node::Seg;
    use toml_edit::{DocumentMut, Item};
    if path.is_empty() {
        return doc.serialize();
    }
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
        if r.depth == row.depth {
            count += 1;
        } else if r.depth <= parent_depth {
            break;
        }
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
        assert!(
            app.selection.indices.is_empty(),
            "selection must clear on rebuild"
        );
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
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            before,
            "doc unchanged"
        );
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
            crate::model::document::Target {
                parent: vec![],
                index: 1,
            },
            "port = 1\n".into(),
        );
        assert!(app.status.is_some(), "collision must surface in status");
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            before,
            "doc unchanged"
        );
    }

    #[test]
    fn apply_insert_invalid_toml_sets_status_and_leaves_doc() {
        // §10 rejection path for `n`: invalid fragment -> Fragment -> status, no change.
        let mut app = app_with("port = 8080\n");
        let before = app.doc.as_ref().unwrap().serialize();
        app.apply_insert(
            crate::model::document::Target {
                parent: vec![],
                index: 1,
            },
            "= = nope".into(),
        );
        assert!(app.status.is_some(), "invalid TOML must surface in status");
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            before,
            "doc unchanged"
        );
    }

    #[test]
    fn apply_insert_valid_pushes_history_and_rebuilds() {
        let mut app = app_with("port = 8080\n");
        app.apply_insert(
            crate::model::document::Target {
                parent: vec![],
                index: 1,
            },
            "host = \"x\"\n".into(),
        );
        assert!(app.status.is_none());
        assert!(app
            .doc
            .as_ref()
            .unwrap()
            .serialize()
            .contains("host = \"x\""));
        // reproject + rebuild surfaced the new key as a visible row
        assert!(app.visible_keys().contains(&"host".to_string()));
        let restored = app.history.as_mut().unwrap().undo().unwrap();
        assert!(!restored.contains("host"));
    }

    #[test]
    fn cut_then_paste_moves_node() {
        let mut app = app_with("a = 1\n[dest]\n");
        // cursor on `a` (row 1, after root)
        app.cursor = 1;
        // cut
        app.cut_selected();
        assert!(app.clipboard.is_some());
        assert!(app.clipboard.as_ref().unwrap().cut);
        let s_before_paste = app.doc.as_ref().unwrap().serialize();
        assert!(
            s_before_paste.contains("a = 1"),
            "cut defers deletion until paste"
        );

        // navigate into [dest] — expand root + dest, cursor on dest
        app.expand_all();
        app.rebuild_rows();
        let dest_idx = app.rows.iter().position(|r| r.key == "dest").unwrap();
        app.cursor = dest_idx;

        // paste
        app.paste();
        let s = app.doc.as_ref().unwrap().serialize();
        assert!(s.contains("[dest]"), "dest table still present");
        assert!(s.contains("a = 1"), "a should be under dest");
        assert_eq!(
            s.matches("a = 1").count(),
            1,
            "a only under dest, not at top level"
        );
    }

    #[test]
    fn delete_selected_removes_node() {
        let mut app = app_with("a = 1\nb = 2\n");
        app.cursor = 1; // on `a`
        app.delete_selected();
        let s = app.doc.as_ref().unwrap().serialize();
        assert!(!s.contains("a = 1"));
        assert!(s.contains("b = 2"));
    }

    #[test]
    fn undo_restores_after_delete() {
        let mut app = app_with("a = 1\n");
        app.cursor = 1;
        app.delete_selected();
        assert!(!app.doc.as_ref().unwrap().serialize().contains("a = 1"));
        app.undo();
        assert!(
            app.doc.as_ref().unwrap().serialize().contains("a = 1"),
            "undo restores deleted node"
        );
    }

    #[test]
    fn redo_reapplies_after_undo() {
        let mut app = app_with("a = 1\n");
        app.cursor = 1;
        app.delete_selected();
        app.undo();
        assert!(app.doc.as_ref().unwrap().serialize().contains("a = 1"));
        app.redo();
        assert!(
            !app.doc.as_ref().unwrap().serialize().contains("a = 1"),
            "redo re-applies deletion"
        );
    }

    #[test]
    fn remark_toggles_comment() {
        let mut app = app_with("port = 8080\n");
        app.cursor = 1; // on port
        app.remark();
        let s = app.doc.as_ref().unwrap().serialize();
        assert!(
            s.contains("# port = 8080"),
            "remark should comment out: {s:?}"
        );
    }

    // --- Tests for TDD: issues from review ---

    #[test]
    fn multi_fragment_paste_collision_stores_only_remaining_fragments() {
        // When pasting [frag_a, frag_b] and frag_b collides, clipboard should only
        // hold [frag_b] (the remaining unprocessed fragment), not [frag_a, frag_b].
        let mut app = app_with("b = 99\n");
        app.rebuild_rows();
        app.cursor = 0; // root
        let target = crate::model::document::Target {
            parent: vec![],
            index: 0,
        };
        app.do_paste(
            vec!["a = 1\n".into(), "b = 2\n".into()],
            false,
            vec![],
            target,
            OnCollision::Cancel,
        );
        assert!(matches!(
            app.mode,
            Mode::Prompt(PromptKind::Collision { .. })
        ));
        let cb = app.clipboard.as_ref().expect("clipboard must be set");
        assert_eq!(
            cb.fragments.len(),
            1,
            "only remaining (b) fragment should be stored, got: {:?}",
            cb.fragments
        );
        assert_eq!(cb.fragments[0], "b = 2\n");
    }

    #[test]
    fn move_collision_enters_prompt_with_sources_preserved() {
        // When the second move-press hits a collision, app must enter MoveCollision
        // prompt so the user can resolve with o/r/c.
        let mut app = app_with("a = 1\n[dest]\na = 999\n");
        app.expand_all();
        app.rebuild_rows();
        let a_idx = app
            .rows
            .iter()
            .position(|r| r.key == "a" && r.path.len() == 1)
            .unwrap();
        app.cursor = a_idx;
        app.move_pressed(); // first press: MovePending
        assert!(matches!(&app.mode, Mode::MovePending { .. }));
        let dest_idx = app.rows.iter().position(|r| r.key == "dest").unwrap();
        app.cursor = dest_idx;
        app.move_pressed(); // second press: collision
        assert!(
            matches!(&app.mode, Mode::Prompt(PromptKind::MoveCollision { .. })),
            "expected MoveCollision prompt, got mode is something else"
        );
    }

    #[test]
    fn move_collision_resolve_overwrite() {
        // After a MoveCollision prompt, pressing 'o' should complete the move.
        let mut app = app_with("a = 1\n[dest]\na = 999\n");
        app.expand_all();
        app.rebuild_rows();
        let a_idx = app
            .rows
            .iter()
            .position(|r| r.key == "a" && r.path.len() == 1)
            .unwrap();
        app.cursor = a_idx;
        app.move_pressed();
        let dest_idx = app.rows.iter().position(|r| r.key == "dest").unwrap();
        app.cursor = dest_idx;
        app.move_pressed(); // collision
        assert!(matches!(
            &app.mode,
            Mode::Prompt(PromptKind::MoveCollision { .. })
        ));
        app.handle_prompt_key('o');
        assert!(
            matches!(app.mode, Mode::Normal),
            "mode should be Normal after resolving"
        );
        let s = app.doc.as_ref().unwrap().serialize();
        assert_eq!(
            s.matches("a = ").count(),
            1,
            "only one 'a' should exist after overwrite move: {s}"
        );
    }

    #[test]
    fn confirm_quit_y_returns_quit() {
        let mut app = app_with("a = 1\n");
        app.mode = Mode::Prompt(PromptKind::ConfirmQuit);
        let outcome = app.handle_prompt_key('y');
        assert!(matches!(outcome, PromptOutcome::Quit));
        assert!(matches!(app.mode, Mode::Normal));
    }

    #[test]
    fn confirm_quit_n_returns_consumed() {
        let mut app = app_with("a = 1\n");
        app.mode = Mode::Prompt(PromptKind::ConfirmQuit);
        let outcome = app.handle_prompt_key('n');
        assert!(matches!(outcome, PromptOutcome::Consumed));
        assert!(matches!(app.mode, Mode::Normal));
    }

    #[test]
    fn move_pressed_two_step() {
        let mut app = app_with("a = 1\n[dest]\n");
        app.cursor = 1; // on `a`
        app.move_pressed(); // first m: enters MovePending
        assert!(matches!(
            &app.mode,
            crate::tui::state::Mode::MovePending { .. }
        ));
        // navigate to dest
        app.expand_all();
        app.rebuild_rows();
        let dest_idx = app.rows.iter().position(|r| r.key == "dest").unwrap();
        app.cursor = dest_idx;
        app.move_pressed(); // second m: executes move
        assert!(matches!(app.mode, crate::tui::state::Mode::Normal));
        let s = app.doc.as_ref().unwrap().serialize();
        assert_eq!(s.matches("a = 1").count(), 1, "a moved under dest");
    }

    // --- Blocker 1: filter must match by scalar VALUE ---

    #[test]
    fn filter_matches_by_scalar_value() {
        let mut app = app_with("port = 8080\nhost = \"localhost\"\n");
        app.expand_all();
        app.rebuild_rows();
        app.enter_filter();
        for c in "8080".chars() {
            app.filter_char(c);
        }
        let keys = app.visible_keys();
        assert!(
            keys.iter().any(|k| k == "port"),
            "port (value=8080) should be visible after filtering for '8080', got: {keys:?}"
        );
    }

    // --- Blocker 2: detail must show type and value ---

    #[test]
    fn detail_view_shows_type_and_value() {
        let mut app = app_with("port = 8080\n");
        app.cursor = 1; // on port (row 0 is root)
        app.open_detail();
        let detail = app.detail_text.as_ref().expect("detail should be set");
        assert!(
            detail.contains("integer"),
            "detail should contain ScalarType, got: {detail}"
        );
        assert!(
            detail.contains("8080"),
            "detail should contain value, got: {detail}"
        );
        assert!(
            detail.contains("server") || detail.lines().next().is_some_and(|l| l.contains("port")),
            "detail should contain dotted path, got: {detail}"
        );
    }
}
