use crate::model::document::{ConfigDocument, Mutation, OnCollision, Target};
use crate::model::node::{Format, Node, NodeKind, NodeTree, Path, ScalarType, Seg};
use crate::tui::search::{fuzzy_match, haystack};
use crate::tui::selection::Selection;
use crate::tui::state::{Clipboard, EditState, History, Mode, PromptKind};
use std::collections::HashSet;

/// How `e` should edit the cursor node: in-place (single-line scalar directly
/// under a Table/Root) or by spawning $EDITOR (everything nested, non-scalar,
/// or a multiline string).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    Inline,
    External,
}

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
    /// Saved inline-edit + validated fragment awaiting a TypeChange confirmation.
    pub pending_edit: Option<(EditState, String)>,
    /// Vertical scroll offset (in display rows) of the detail popup.
    pub detail_scroll: u16,
    /// Persisted vertical scroll offset (top visible row) of the main tree table.
    /// Kept across frames so the viewport only scrolls when the cursor would
    /// leave it, instead of ratatui re-deriving it (and pinning the cursor to an
    /// edge) every draw. `Cell` so the immutable-`&App` render path can update it.
    pub table_offset: std::cell::Cell<usize>,
}

#[derive(Clone)]
pub struct RowSnapshot {
    pub key: String,
    pub path: Path,
    pub depth: usize,
    pub is_branch: bool,
    pub value: Option<String>,
    pub scalar_type: Option<String>,
    /// Display label for the TYPE column (scalar type, branch kind, or "comment").
    pub type_label: String,
    /// Writing style of a scalar leaf (`Plain` for branches/comments).
    pub format: Format,
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
            pending_edit: None,
            detail_scroll: 0,
            table_offset: std::cell::Cell::new(0),
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
            pending_edit: None,
            detail_scroll: 0,
            table_offset: std::cell::Cell::new(0),
        }
    }
    pub fn rebuild_rows(&mut self) {
        let expanded = &self.expanded;
        let rows = self
            .tree
            .flatten(&|p| expanded.contains(p))
            .into_iter()
            .map(|r| {
                use crate::model::node::NodeKind;
                let scalar_type = match &r.node.kind {
                    NodeKind::Scalar(st) => Some(format!("{st:?}").to_lowercase()),
                    _ => None,
                };
                let type_label = match &r.node.kind {
                    NodeKind::Root => String::new(),
                    NodeKind::Table => "table".into(),
                    NodeKind::ArrayOfTables => "array-of-tables".into(),
                    NodeKind::Array => "array".into(),
                    NodeKind::InlineTable => "inline".into(),
                    NodeKind::Scalar(st) => format!("{st:?}").to_lowercase(),
                    NodeKind::Comment(_) => "comment".into(),
                };
                RowSnapshot {
                    key: r.node.key.clone(),
                    path: r.node.path.clone(),
                    depth: r.depth,
                    is_branch: r.node.is_branch(),
                    value: r.node.value.clone(),
                    scalar_type,
                    type_label,
                    format: r.node.format,
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

    // ---- Detail view (Leaf Enter/Space, `i` for any node) ----

    /// `i` — toggle the detail popup for the cursor node (any node, including
    /// branches). Closes the popup if it is already open.
    pub fn toggle_detail(&mut self) {
        if matches!(self.mode, Mode::Detail) {
            self.exit_detail();
        } else {
            self.open_detail();
        }
    }

    /// Open the read-only detail popup for the cursor node. Leaves show
    /// type/format/value; branches show their kind and child count.
    pub fn open_detail(&mut self) {
        let row = match self.rows.get(self.cursor) {
            Some(r) => r.clone(),
            None => return,
        };
        let path_keys: Vec<String> = row
            .path
            .iter()
            .filter_map(|s| match s {
                Seg::Key(k) => Some(k.clone()),
                _ => None,
            })
            .collect();
        let dotted = if path_keys.is_empty() {
            "(root)".to_string()
        } else {
            path_keys.join(".")
        };
        let mut detail = if row.is_branch {
            // Branch nodes carry their writing style in the kind: a table can be
            // a standard `[table]` or an `{ inline }` table, an array a standard
            // `[...]` or an `[[array-of-tables]]`. Surface both axes — the coarse
            // Type and the concrete Format — plus the child count.
            let node = node_at(&self.tree.root, &row.path);
            let (type_str, fmt_str) = node
                .map(|n| branch_type_format(&n.kind))
                .unwrap_or(("unknown", "-"));
            let children = node.map(|n| n.children.len()).unwrap_or(0);
            format!(
                "Path:     {dotted}\nType:     {type_str}\nFormat:   {fmt_str}\nChildren: {children}"
            )
        } else {
            // A comment node has no scalar_type; fall back to its type_label
            // ("comment") so the popup reads `Type: comment`. Its `value` now
            // carries the full (multi-line) comment text, shown in full below.
            let type_str = row.scalar_type.as_deref().unwrap_or(&row.type_label);
            let val_str = row.value.as_deref().unwrap_or("");
            // Compact format label, matching the TYPE/FORMAT column; single-style
            // scalars (bool/float/datetime) read as "plain".
            let fmt_str = crate::tui::ui::format_label(row.format).unwrap_or("plain");
            format!("Path:     {dotted}\nType:     {type_str}\nFormat:   {fmt_str}\nValue:    {val_str}")
        };
        if let Some(tc) = &row.trailing_comment {
            detail.push_str(&format!("\nComment:  {tc}"));
        }
        self.detail_text = Some(detail);
        self.detail_scroll = 0;
        self.mode = Mode::Detail;
    }

    /// Scroll the detail popup by `delta` rows, clamped to `[0, max]`.
    pub fn detail_scroll_by(&mut self, delta: i32, max: u16) {
        let v = (self.detail_scroll as i32 + delta).clamp(0, max as i32);
        self.detail_scroll = v as u16;
    }

    /// Jump the detail popup to an absolute scroll offset (Home/End).
    pub fn detail_set_scroll(&mut self, v: u16) {
        self.detail_scroll = v;
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
        let cursor_row = match self.rows.get(self.cursor) {
            Some(r) => r.clone(),
            None => return,
        };
        // A comment node has no real item to serialize: open $EDITOR with its raw
        // `#`-prefixed text and write the edit back into the decor. (Comments nested
        // inside an AoT entry carry an `Index` in their path and are not addressable
        // this way — fall through to container editing, as Remark does.)
        if let Some(node) = node_at(&self.tree.root, &cursor_row.path) {
            if let NodeKind::Comment(text) = &node.kind {
                if !cursor_row.path.iter().any(|s| matches!(s, Seg::Index(_))) {
                    let initial = format!("{text}\n");
                    let edited = match crate::tui::editor::edit_text(&initial) {
                        Ok(t) => t,
                        Err(e) => {
                            self.status = Some(format!("editor error: {e}"));
                            return;
                        }
                    };
                    self.apply_edit_comment(cursor_row.path.clone(), edited);
                    return;
                }
            }
        }
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        // `Replace` can only address an all-`Key` path, so for any node inside an
        // array/AoT (path with an `Index`) edit the nearest addressable container:
        // truncate at the first `Index`, yielding the enclosing top-level array/AoT
        // key. (A direct scalar element is handled inline and never reaches here.)
        let first_index = cursor_row
            .path
            .iter()
            .position(|s| matches!(s, Seg::Index(_)));
        let path = match first_index {
            Some(i) => cursor_row.path[..i].to_vec(),
            None => cursor_row.path.clone(),
        };
        // Serialize just the (container) node's own fragment.
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

    /// Apply edited comment text as an `EditComment` at `path` (the post-editor
    /// half of editing a comment node). On error the status line is set and the
    /// document is left unchanged.
    pub(crate) fn apply_edit_comment(&mut self, path: Path, text: String) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        match doc.apply(crate::model::document::Mutation::EditComment { path, text }) {
            Ok(()) => self.on_mutation_success(),
            Err(crate::model::document::MutateError::Fragment(msg)) => {
                self.status = Some(format!("invalid comment: {msg}"));
            }
            Err(e) => self.status = Some(format!("error: {e}")),
        }
    }

    /// Decide how `e` should edit the cursor node. Inline editing applies only to
    /// a Scalar leaf reachable by an all-`Key` path whose immediate parent is a
    /// Table or the Root — i.e. NOT inside an array, inline table, or AoT entry
    /// (those are "nested" and `Replace` cannot address an `Index` segment).
    /// Multiline strings are also routed to $EDITOR: their repr carries real
    /// newlines the single-line inline editor cannot represent.
    pub fn edit_target_kind(&self) -> EditKind {
        let path = match self.rows.get(self.cursor) {
            Some(r) => &r.path,
            None => return EditKind::External,
        };
        if path.is_empty() {
            return EditKind::External; // Root
        }
        let node = match node_at(&self.tree.root, path) {
            Some(n) => n,
            None => return EditKind::External,
        };
        if !matches!(node.kind, NodeKind::Scalar(_)) {
            return EditKind::External;
        }
        // Multiline strings carry real newlines the single-line inline editor
        // cannot represent — route them to $EDITOR. Keyed on Format (not on a raw
        // `\n` scan): an element of a *multiline array* carries indentation-newline
        // decor in its repr yet is itself an ordinary single-line string.
        if matches!(
            node.format,
            Format::MultilineBasic | Format::MultilineLiteral
        ) {
            return EditKind::External;
        }
        let parent_path = &path[..path.len() - 1];
        let parent = node_at(&self.tree.root, parent_path);
        match path.last() {
            // Scalar element of an array: inline when the path is `Key+ Index*`
            // (a run of keys then array-index descents, no `Key` after the first
            // `Index`) so the Replace write-back can address it — covers top-level
            // and array-of-arrays nesting. Arrays inside AoT/inline-table entries
            // (a `Key` after an `Index`) stay External (edit the whole container).
            Some(Seg::Index(_)) => {
                let first_index = path
                    .iter()
                    .position(|s| matches!(s, Seg::Index(_)))
                    .unwrap_or(0);
                let tail_all_index = path[first_index..]
                    .iter()
                    .all(|s| matches!(s, Seg::Index(_)));
                let parent_is_array = parent
                    .map(|p| matches!(p.kind, NodeKind::Array))
                    .unwrap_or(false);
                if tail_all_index && parent_is_array {
                    EditKind::Inline
                } else {
                    EditKind::External
                }
            }
            // Scalar under a key: inline only when no `Index` sits above it (else it
            // lives inside an AoT entry that Replace cannot address) and the parent
            // is a Table or the Root (not an inline table).
            Some(Seg::Key(_)) => {
                let no_index_above = !parent_path.iter().any(|s| matches!(s, Seg::Index(_)));
                let parent_ok = path.len() == 1
                    || parent
                        .map(|p| matches!(p.kind, NodeKind::Table | NodeKind::Root))
                        .unwrap_or(false);
                if no_index_above && parent_ok {
                    EditKind::Inline
                } else {
                    EditKind::External
                }
            }
            None => EditKind::External,
        }
    }

    /// Enter the inline editor for the cursor scalar, pre-filled with its value
    /// repr (quotes/base prefix included, so the user edits the literal form).
    pub fn begin_inline_edit(&mut self) {
        let row = match self.rows.get(self.cursor) {
            Some(r) => r,
            None => return,
        };
        let (key, is_element) = match row.path.last() {
            Some(Seg::Key(k)) => (k.clone(), false),
            // Array element: no key. `edit_commit` builds a bare-value fragment.
            Some(Seg::Index(_)) => (String::new(), true),
            None => return,
        };
        // `value` is `Value::to_string()`, which carries the decor whitespace
        // around the `=` (e.g. " 8080"); trim it so the edited literal doesn't
        // accumulate a leading space on write-back.
        let buffer = row.value.clone().unwrap_or_default().trim().to_string();
        let cursor = buffer.chars().count();
        // The Value field is active first; the Name field (the key) is the saved
        // inactive set, ready for a `Tab` swap.
        let name_cursor = key.chars().count();
        self.mode = Mode::Edit(EditState {
            path: row.path.clone(),
            key: key.clone(),
            field: crate::tui::state::EditField::Value,
            is_element,
            buffer,
            cursor,
            scroll: 0,
            other_buffer: key,
            other_cursor: name_cursor,
            other_scroll: 0,
        });
        self.status = None;
    }

    /// `Tab` in the inline editor: swap focus between the Value and Name fields,
    /// stashing the active working set and loading the other. No-op for array
    /// elements (no name).
    pub fn edit_toggle_field(&mut self) {
        if let Mode::Edit(ref mut e) = self.mode {
            if e.is_element {
                return;
            }
            std::mem::swap(&mut e.buffer, &mut e.other_buffer);
            std::mem::swap(&mut e.cursor, &mut e.other_cursor);
            std::mem::swap(&mut e.scroll, &mut e.other_scroll);
            e.field = match e.field {
                crate::tui::state::EditField::Value => crate::tui::state::EditField::Name,
                crate::tui::state::EditField::Name => crate::tui::state::EditField::Value,
            };
            self.status = None;
        }
    }

    /// Adjust the inline editor's horizontal viewport so the cursor stays visible
    /// in a `width`-wide cell, scrolling by the minimum needed. Called from the
    /// event loop (which knows the terminal width) before each draw.
    pub fn edit_clamp_scroll(&mut self, width: usize) {
        if let Mode::Edit(ref mut e) = self.mode {
            let len = e.buffer.chars().count();
            e.scroll = clamp_scroll(e.scroll, e.cursor.min(len), len, width);
        }
    }

    pub fn edit_input_char(&mut self, c: char) {
        if let Mode::Edit(ref mut e) = self.mode {
            let byte = char_byte_idx(&e.buffer, e.cursor);
            e.buffer.insert(byte, c);
            e.cursor += 1;
            // Clear any prior commit error now the user is revising the value.
            self.status = None;
        }
    }

    pub fn edit_backspace(&mut self) {
        if let Mode::Edit(ref mut e) = self.mode {
            if e.cursor > 0 {
                let prev = char_byte_idx(&e.buffer, e.cursor - 1);
                e.buffer.remove(prev);
                e.cursor -= 1;
                self.status = None;
            }
        }
    }

    pub fn edit_cursor_left(&mut self) {
        if let Mode::Edit(ref mut e) = self.mode {
            e.cursor = e.cursor.saturating_sub(1);
        }
    }

    pub fn edit_cursor_right(&mut self) {
        if let Mode::Edit(ref mut e) = self.mode {
            let len = e.buffer.chars().count();
            if e.cursor < len {
                e.cursor += 1;
            }
        }
    }

    pub fn edit_cursor_home(&mut self) {
        if let Mode::Edit(ref mut e) = self.mode {
            e.cursor = 0;
        }
    }

    pub fn edit_cursor_end(&mut self) {
        if let Mode::Edit(ref mut e) = self.mode {
            e.cursor = e.buffer.chars().count();
        }
    }

    pub fn edit_cancel(&mut self) {
        self.mode = Mode::Normal;
        self.pending_edit = None;
        self.status = None;
    }

    /// Commit the inline edit. First apply a key rename if the Name field changed
    /// (its own undo step, position/decor-preserving), then reconstruct
    /// `key = <value>`, validate it parses, and either apply `Replace` directly or
    /// — if the scalar's displayed type would change — stash it and enter a
    /// TypeChange confirm prompt. On any failure: set status, stay in the editor.
    pub fn edit_commit(&mut self) {
        let mut e = match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Edit(e) => e,
            other => {
                self.mode = other;
                return;
            }
        };
        use crate::tui::state::EditField;
        // The active working set is the focused field; `other_*` holds the rest.
        let (name_str, value_str) = match e.field {
            EditField::Value => (e.other_buffer.clone(), e.buffer.clone()),
            EditField::Name => (e.buffer.clone(), e.other_buffer.clone()),
        };
        // Array elements have no key; validate/label the bare value under a
        // placeholder key. The model's Replace ignores the key for an Index path.
        let is_element = matches!(e.path.last(), Some(Seg::Index(_)));
        let mut frag_key = if is_element {
            "__elem__".to_string()
        } else {
            e.key.clone()
        };
        // 1. Key rename (Name field changed). Applied as its own mutation so it is
        //    independently undoable; on collision/invalid key, stay in the editor.
        if !is_element {
            let new_name = name_str.trim().to_string();
            if new_name != e.key {
                if new_name.is_empty() {
                    self.status = Some("key cannot be empty".into());
                    self.mode = Mode::Edit(e);
                    return;
                }
                let res = match self.doc.as_mut() {
                    Some(doc) => doc.apply(crate::model::document::Mutation::Rename {
                        path: e.path.clone(),
                        new_key: new_name.clone(),
                    }),
                    None => Ok(()),
                };
                match res {
                    Ok(()) => {
                        self.on_mutation_success();
                        if let Some(last) = e.path.last_mut() {
                            *last = Seg::Key(new_name.clone());
                        }
                        e.key = new_name.clone();
                        frag_key = new_name;
                    }
                    Err(err) => {
                        self.status = Some(format!("rename failed: {err}"));
                        self.mode = Mode::Edit(e);
                        return;
                    }
                }
            }
        }
        // 2. Value replace.
        let fragment = format!("{} = {}\n", frag_key, value_str);
        let table = match crate::model::fragment::parse_fragment(&fragment) {
            Ok(t) => t,
            Err(err) => {
                self.status = Some(format!("invalid TOML: {err}"));
                self.mode = Mode::Edit(e); // stay in the editor so the user can fix it
                return;
            }
        };
        let new_label = table
            .get(&frag_key)
            .map(fragment_value_label)
            .unwrap_or_default();
        let old_label = self
            .rows
            .get(self.cursor)
            .map(|r| r.type_label.clone())
            .unwrap_or_default();
        if new_label != old_label {
            self.status = Some(format!("type {old_label} → {new_label}? y/n"));
            self.pending_edit = Some((e, fragment));
            self.mode = Mode::Prompt(PromptKind::TypeChange {
                from: old_label,
                to: new_label,
            });
            return;
        }
        self.apply_replace(e.path, fragment);
    }

    /// `←`/`→` in Normal mode: toggle a bool or step an integer/float by ±1 at
    /// its least-significant digit, preserving the written format. No-op for
    /// strings, datetimes, and anything not an inline-editable scalar.
    pub fn nudge(&mut self, delta: i64) {
        let path = match self.rows.get(self.cursor) {
            Some(r) => r.path.clone(),
            None => return,
        };
        // A scalar reached by a key, or a scalar element of an array whose path is
        // `Key+ Index*` (addressable by Replace, incl. nested arrays).
        let frag_key = match path.last() {
            Some(Seg::Key(k)) => k.clone(),
            Some(Seg::Index(_)) => {
                let fi = path
                    .iter()
                    .position(|s| matches!(s, Seg::Index(_)))
                    .unwrap_or(0);
                if path[fi..].iter().all(|s| matches!(s, Seg::Index(_))) {
                    "__elem__".to_string()
                } else {
                    return;
                }
            }
            _ => return,
        };
        let node = match node_at(&self.tree.root, &path) {
            Some(n) => n,
            None => return,
        };
        let st = match node.kind {
            NodeKind::Scalar(st) => st,
            _ => return,
        };
        let repr = match &node.value {
            Some(v) => v.clone(),
            None => return,
        };
        if let Some(new_repr) = nudge_scalar(st, node.format, &repr, delta) {
            self.apply_replace(path, format!("{frag_key} = {new_repr}\n"));
        }
    }

    /// `a` — insert a new empty-string node (`new_field = ""`) below the cursor
    /// and immediately open the inline editor on it. (TOML has no null, so the
    /// neutral placeholder is an empty string; rename the key later via `E`.)
    pub fn add_node(&mut self) {
        if self.doc.is_none() {
            return;
        }
        let cursor_row = self.rows.get(self.cursor).cloned();
        let target = match &cursor_row {
            Some(r) => {
                let expanded = self.expanded.contains(&r.path);
                let sibling_index = sibling_index_of(r, &self.rows);
                crate::tui::insertion::resolve_target(r, expanded, sibling_index)
            }
            None => Target {
                parent: vec![],
                index: 0,
            },
        };
        // Choose a key unique within the destination so the insert never collides.
        let existing: Vec<String> = node_at(&self.tree.root, &target.parent)
            .map(|p| p.children.iter().map(|c| c.key.clone()).collect())
            .unwrap_or_default();
        let key = unique_key("new_field", &existing);
        // Ensure the destination branch is expanded so the new node is visible.
        if !target.parent.is_empty() {
            self.expanded.insert(target.parent.clone());
        }
        // An array parent takes a bare value element at `target.index`; a table
        // parent takes the seeded `key = ""`. The fragment string is the same
        // (`insert_fragment` ignores the key for an array), but the new node's path
        // ends in an `Index` rather than a `Key`.
        let parent_is_array = node_at(&self.tree.root, &target.parent)
            .map(|n| matches!(n.kind, NodeKind::Array))
            .unwrap_or(false);
        self.apply_insert(target.clone(), format!("{key} = \"\"\n"));
        if self.status.is_some() {
            return; // insert failed; status already set
        }
        // Locate the freshly inserted row, move the cursor to it, and edit inline.
        let mut new_path = target.parent.clone();
        if parent_is_array {
            new_path.push(Seg::Index(target.index));
        } else {
            new_path.push(Seg::Key(key));
        }
        if let Some(idx) = self.rows.iter().position(|r| r.path == new_path) {
            self.cursor = idx;
            self.begin_inline_edit();
        }
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
            Mode::Prompt(_) | Mode::Filter | Mode::Detail | Mode::Help | Mode::Edit(_) => {}
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
                self.pending_edit = None;
                self.status = None;
            }
            Mode::Filter => self.exit_filter(),
            Mode::Detail => self.exit_detail(),
            Mode::Help => self.exit_help(),
            Mode::Edit(_) => self.edit_cancel(),
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

    /// `w`/`Ctrl+s` — save current document to its path.
    pub fn save(&mut self) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        if !doc.is_dirty() {
            self.status = Some("no changes to save".into());
            return;
        }
        match doc.save() {
            Ok(()) => {
                doc.mark_saved();
                self.status = Some("Saved".into());
            }
            Err(e) => {
                self.status = Some(format!("save error: {e}"));
            }
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
            Mode::Prompt(PromptKind::TypeChange { .. }) => {
                match c {
                    'y' => {
                        if let Some((e, fragment)) = self.pending_edit.take() {
                            self.mode = Mode::Normal;
                            self.apply_replace(e.path, fragment);
                        } else {
                            self.mode = Mode::Normal;
                        }
                    }
                    // Any other key returns to the inline editor to revise.
                    _ => {
                        if let Some((e, _)) = self.pending_edit.take() {
                            self.mode = Mode::Edit(e);
                        } else {
                            self.mode = Mode::Normal;
                        }
                    }
                }
                PromptOutcome::Consumed
            }
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

/// Coarse `(type, format)` labels for a branch node: the Type is the conceptual
/// kind and the Format the concrete TOML writing style. Tables split into
/// standard/inline; arrays into standard/array-of-tables.
fn branch_type_format(kind: &NodeKind) -> (&'static str, &'static str) {
    match kind {
        NodeKind::Root => ("root", "-"),
        NodeKind::Table => ("table", "table"),
        NodeKind::InlineTable => ("table", "inline"),
        NodeKind::Array => ("array", "array"),
        NodeKind::ArrayOfTables => ("array", "array-of-tables"),
        NodeKind::Scalar(_) | NodeKind::Comment(_) => ("unknown", "-"),
    }
}

/// Find a node in the projected tree by its exact path (Root has empty path).
fn node_at<'a>(root: &'a Node, path: &[Seg]) -> Option<&'a Node> {
    if root.path == path {
        return Some(root);
    }
    root.children.iter().find_map(|c| node_at(c, path))
}

/// Byte offset of the `n`-th char in `s` (==`s.len()` when `n` is the char count).
fn char_byte_idx(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map(|(i, _)| i).unwrap_or(s.len())
}

/// Minimally adjust a horizontal scroll offset so `cursor` stays within the
/// `width`-wide window `[scroll, scroll+width)`. The offset only moves when the
/// cursor would leave the window, so walking left after hitting the right edge
/// steps the cursor back through the window before the text scrolls.
fn clamp_scroll(scroll: usize, cursor: usize, len: usize, width: usize) -> usize {
    let w = width.max(1);
    let cur = cursor.min(len);
    let mut s = scroll;
    if cur < s {
        s = cur;
    } else if cur >= s + w {
        s = cur + 1 - w;
    }
    // Don't leave a blank gap past the end (e.g. after the buffer shrank). The
    // virtual length includes the trailing cursor slot.
    s.min((len + 1).saturating_sub(w))
}

/// First non-colliding key formed from `base` (`base`, `base_2`, `base_3`, …),
/// mirroring the `OnCollision::Rename` scheme in `toml_doc`.
fn unique_key(base: &str, existing: &[String]) -> String {
    if !existing.iter().any(|k| k == base) {
        return base.to_string();
    }
    let mut n = 2;
    loop {
        let cand = format!("{base}_{n}");
        if !existing.iter().any(|k| k == &cand) {
            return cand;
        }
        n += 1;
    }
}

/// Display type label for a freshly parsed fragment value, matching the labels
/// `rebuild_rows` assigns so an inline edit can detect a type change by string
/// comparison.
fn fragment_value_label(item: &toml_edit::Item) -> String {
    use toml_edit::{Item, Value};
    match item {
        Item::Value(Value::Array(_)) => "array".into(),
        Item::Value(Value::InlineTable(_)) => "inline".into(),
        Item::Value(v) => format!("{:?}", crate::model::project::scalar_type(v)).to_lowercase(),
        Item::Table(_) => "table".into(),
        Item::ArrayOfTables(_) => "array-of-tables".into(),
        Item::None => "none".into(),
    }
}

/// Insert `_` every `n` digits counting from the right (e.g. `1000000` → `1_000_000`).
fn group_right(digits: &str, n: usize) -> String {
    let len = digits.chars().count();
    let mut out = String::with_capacity(len + len / n);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(n) {
            out.push('_');
        }
        out.push(c);
    }
    out
}

/// Insert `_` every `n` digits counting from the left (for fractional digits,
/// e.g. `445991` → `445_991`).
fn group_left(digits: &str, n: usize) -> String {
    let mut out = String::with_capacity(digits.len() + digits.len() / n);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && i.is_multiple_of(n) {
            out.push('_');
        }
        out.push(c);
    }
    out
}

/// Re-apply underscore digit grouping to a freshly stepped integer repr: decimal
/// every 3, hex/oct/bin every 4 (after the base prefix).
fn regroup_int(repr: &str, fmt: Format) -> String {
    match fmt {
        Format::Hex | Format::Octal | Format::Binary => {
            let (prefix, digits) = repr.split_at(2); // "0x"/"0o"/"0b"
            format!("{prefix}{}", group_right(digits, 4))
        }
        _ => {
            let (sign, digits) = repr.strip_prefix('-').map_or(("", repr), |d| ("-", d));
            format!("{sign}{}", group_right(digits, 3))
        }
    }
}

/// Re-apply underscore grouping to a stepped decimal-float repr: integer part
/// every 3 from the right, fractional part every 3 from the left.
fn regroup_float(repr: &str) -> String {
    let (sign, body) = repr.strip_prefix('-').map_or(("", repr), |d| ("-", d));
    match body.split_once('.') {
        Some((int, frac)) => format!("{sign}{}.{}", group_right(int, 3), group_left(frac, 3)),
        None => format!("{sign}{}", group_right(body, 3)),
    }
}

/// Step a scalar's repr by `delta` (±1) preserving its written format. Bools
/// toggle (delta ignored); integers step at the unit place in their own base;
/// floats step at their least-significant displayed decimal. Returns `None` for
/// strings, datetimes, and reprs that don't fit the simple stepping model.
fn nudge_scalar(st: ScalarType, fmt: Format, repr: &str, delta: i64) -> Option<String> {
    let s = repr.trim();
    match st {
        ScalarType::Bool => match s {
            "true" => Some("false".into()),
            "false" => Some("true".into()),
            _ => None,
        },
        ScalarType::Integer => {
            let had_us = s.contains('_');
            let clean = s.replace('_', "");
            let out = match fmt {
                Format::Hex => {
                    let upper = clean[2..].chars().any(|c| c.is_ascii_uppercase());
                    let n = i64::from_str_radix(&clean[2..], 16).ok()? + delta;
                    if upper {
                        format!("0x{n:X}")
                    } else {
                        format!("0x{n:x}")
                    }
                }
                Format::Octal => {
                    let n = i64::from_str_radix(&clean[2..], 8).ok()? + delta;
                    format!("0o{n:o}")
                }
                Format::Binary => {
                    let n = i64::from_str_radix(&clean[2..], 2).ok()? + delta;
                    format!("0b{n:b}")
                }
                _ => {
                    let n = clean.parse::<i64>().ok()? + delta;
                    n.to_string()
                }
            };
            Some(if had_us { regroup_int(&out, fmt) } else { out })
        }
        ScalarType::Float => {
            let had_us = s.contains('_');
            let clean = s.replace('_', "");
            // Only handle plain decimal floats (no exponent / inf / nan).
            if clean
                .bytes()
                .any(|b| matches!(b, b'e' | b'E') || b.is_ascii_alphabetic())
            {
                return None;
            }
            let places = clean
                .split_once('.')
                .map(|(_, frac)| frac.len())
                .unwrap_or(0);
            let val = clean.parse::<f64>().ok()?;
            let step = 10f64.powi(-(places as i32));
            let next = val + delta as f64 * step;
            let out = format!("{next:.*}", places);
            Some(if had_us { regroup_float(&out) } else { out })
        }
        _ => None,
    }
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
    fn apply_edit_comment_updates_doc_and_rows() {
        use crate::model::document::ConfigDocument;
        let mut app = app_with("# old\nx = 1\n");
        let cpath = app.rows[1].path.clone(); // row 0 is root, row 1 the comment
        app.apply_edit_comment(cpath, "# new\n".into());
        assert!(app.status.is_none(), "unexpected status: {:?}", app.status);
        let s = app.doc.as_ref().unwrap().serialize();
        assert!(
            s.contains("# new") && !s.contains("# old"),
            "serialize: {s:?}"
        );
        // The rebuilt rows reflect the edited comment.
        assert_eq!(app.rows[1].value.as_deref(), Some("# new"));
    }

    #[test]
    fn apply_edit_comment_rejects_non_comment_and_keeps_doc() {
        let mut app = app_with("# keep\nx = 1\n");
        let before = app.doc.as_ref().unwrap().serialize();
        let cpath = app.rows[1].path.clone();
        app.apply_edit_comment(cpath, "not a comment\n".into());
        assert!(
            app.status.is_some(),
            "invalid comment must surface in status"
        );
        assert_eq!(app.doc.as_ref().unwrap().serialize(), before);
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

    // --- Task 19: save + dirty-aware quit ---

    #[test]
    fn save_writes_to_file_and_resets_dirty() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"port = 8080\n").unwrap();
        let path = f.path().to_path_buf();
        // Keep the NamedTempFile alive so the path isn't deleted
        let doc = crate::model::toml_doc::TomlDocument::load(&path).unwrap();
        let mut app = App::new(doc);
        // Mutate to make dirty
        app.apply_replace(vec![Seg::Key("port".into())], "port = 9090\n".into());
        assert!(
            app.doc.as_ref().unwrap().is_dirty(),
            "should be dirty after mutation"
        );
        // Save
        app.save();
        // File on disk should have new content
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(
            on_disk.contains("9090"),
            "saved file must contain new value: {on_disk:?}"
        );
        // After save, is_dirty() must be false
        assert!(
            !app.doc.as_ref().unwrap().is_dirty(),
            "must not be dirty after save"
        );
        assert!(
            app.status.as_deref() == Some("Saved"),
            "status must be 'Saved'"
        );
    }

    #[test]
    fn quit_when_dirty_enters_confirm_quit() {
        let mut app = app_with("a = 1\n");
        app.apply_replace(vec![Seg::Key("a".into())], "a = 2\n".into());
        assert!(app.doc.as_ref().unwrap().is_dirty());
        let should_quit = app.quit_requested();
        assert!(!should_quit, "should NOT quit immediately when dirty");
        assert!(
            matches!(app.mode, Mode::Prompt(PromptKind::ConfirmQuit)),
            "must enter ConfirmQuit prompt"
        );
    }

    #[test]
    fn quit_when_clean_signals_quit() {
        let mut app = app_with("a = 1\n");
        assert!(
            !app.doc.as_ref().unwrap().is_dirty(),
            "fresh doc must be clean"
        );
        let should_quit = app.quit_requested();
        assert!(should_quit, "must return true (quit) when clean");
        assert!(
            matches!(app.mode, Mode::Normal),
            "mode unchanged when clean"
        );
    }

    // --- inline editor / format refactor ---

    fn idx_of(app: &App, key: &str) -> usize {
        app.rows.iter().position(|r| r.key == key).unwrap()
    }

    #[test]
    fn edit_target_kind_classifies_inline_vs_external() {
        let mut app =
            app_with("port = 8080\n[server]\nhost = \"h\"\narr = [1, 2]\npt = { y = 3 }\n");
        app.expand_all();
        app.rebuild_rows();
        // scalar directly under Root → inline
        app.cursor = idx_of(&app, "port");
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        // scalar directly under a [table] → inline
        app.cursor = idx_of(&app, "host");
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        // table / array branches → external
        app.cursor = idx_of(&app, "server");
        assert_eq!(app.edit_target_kind(), EditKind::External);
        app.cursor = idx_of(&app, "arr");
        assert_eq!(app.edit_target_kind(), EditKind::External);
        // scalar element directly in a top-level array → inline
        app.cursor = idx_of(&app, "[0]");
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        // member of an inline table → external
        app.cursor = idx_of(&app, "y");
        assert_eq!(app.edit_target_kind(), EditKind::External);
    }

    #[test]
    fn edit_target_kind_routes_multiline_string_external() {
        let mut app = app_with("ml = \"\"\"\nline1\nline2\n\"\"\"\nsingle = \"x\"\n");
        app.expand_all();
        app.rebuild_rows();
        // multiline string scalar → external (inline editor is single-line)
        app.cursor = idx_of(&app, "ml");
        assert_eq!(app.edit_target_kind(), EditKind::External);
        // single-line string scalar → inline (control)
        app.cursor = idx_of(&app, "single");
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
    }

    #[test]
    fn edit_target_kind_multiline_array_element_is_inline() {
        // A string element of a *multiline array* carries newline indentation in
        // its repr but is itself an ordinary single-line string — must edit inline
        // (regression: a raw `\n` scan wrongly routed it to $EDITOR).
        let mut app = app_with("arr = [\n  \"first\",\n  \"second\",\n]\n");
        app.expand_all();
        app.rebuild_rows();
        app.cursor = idx_of(&app, "[0]");
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
    }

    #[test]
    fn edit_target_kind_nested_array_scalar_is_inline() {
        // A scalar in an array-of-arrays (`Key Index Index`) edits inline.
        let mut app = app_with("nested = [[1, 2], [3, 4]]\n");
        app.expand_all();
        app.rebuild_rows();
        // the inner `[0]` rows repeat; pick a scalar leaf (value "3")
        let pos = app
            .rows
            .iter()
            .position(|r| r.value.as_deref() == Some("3"))
            .unwrap();
        app.cursor = pos;
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
    }

    #[test]
    fn nudge_scalar_steps_each_type_preserving_format() {
        assert_eq!(
            nudge_scalar(ScalarType::Integer, Format::Decimal, "41", 1).as_deref(),
            Some("42")
        );
        assert_eq!(
            nudge_scalar(ScalarType::Integer, Format::Hex, "0xFF", 1).as_deref(),
            Some("0x100")
        );
        assert_eq!(
            nudge_scalar(ScalarType::Integer, Format::Hex, "0x0a", 1).as_deref(),
            Some("0xb"),
            "lowercase hex preserved"
        );
        assert_eq!(
            nudge_scalar(ScalarType::Float, Format::Plain, "1.50", 1).as_deref(),
            Some("1.51"),
            "float steps at its displayed precision"
        );
        assert_eq!(
            nudge_scalar(ScalarType::Float, Format::Plain, "1.50", -1).as_deref(),
            Some("1.49")
        );
        assert_eq!(
            nudge_scalar(ScalarType::Bool, Format::Plain, "true", 1).as_deref(),
            Some("false")
        );
        // strings / datetimes are not nudgeable
        assert_eq!(
            nudge_scalar(ScalarType::String, Format::BasicString, "\"hi\"", 1),
            None
        );
    }

    #[test]
    fn nudge_reapplies_underscore_grouping() {
        // decimal regroups every 3 from the right
        assert_eq!(
            nudge_scalar(ScalarType::Integer, Format::Decimal, "1_000_000", 1).as_deref(),
            Some("1_000_001")
        );
        // hex regroups every 4 (after the 0x prefix)
        assert_eq!(
            nudge_scalar(ScalarType::Integer, Format::Hex, "0xDEAD_BEEF", 1).as_deref(),
            Some("0xDEAD_BEF0")
        );
        // float: int part every 3 from right, frac part every 3 from left
        assert_eq!(
            nudge_scalar(ScalarType::Float, Format::Plain, "9_224_617.445_991", 1).as_deref(),
            Some("9_224_617.445_992")
        );
        // no underscore in, no underscore out
        assert_eq!(
            nudge_scalar(ScalarType::Integer, Format::Decimal, "999", 1).as_deref(),
            Some("1000")
        );
    }

    #[test]
    fn nudge_writes_back_through_replace() {
        let mut app = app_with("port = 8080\n");
        app.cursor = 1; // on port
        app.nudge(1);
        assert!(app
            .doc
            .as_ref()
            .unwrap()
            .serialize()
            .contains("port = 8081"));
    }

    #[test]
    fn inline_commit_same_type_applies_replace() {
        let mut app = app_with("port = 8080\n");
        app.cursor = 1;
        app.begin_inline_edit();
        for _ in 0..4 {
            app.edit_backspace();
        }
        for c in "9090".chars() {
            app.edit_input_char(c);
        }
        app.edit_commit();
        assert!(matches!(app.mode, Mode::Normal));
        assert!(app
            .doc
            .as_ref()
            .unwrap()
            .serialize()
            .contains("port = 9090"));
    }

    #[test]
    fn inline_tab_edits_name_and_renames_key_on_commit() {
        use crate::tui::state::EditField;
        let mut app = app_with("port = 8080\n");
        app.cursor = 1;
        app.begin_inline_edit();
        // Tab switches to the Name field (active buffer becomes the key "port").
        app.edit_toggle_field();
        assert!(matches!(&app.mode, Mode::Edit(e) if e.field == EditField::Name));
        for _ in 0..4 {
            app.edit_backspace(); // clear "port"
        }
        for c in "addr".chars() {
            app.edit_input_char(c);
        }
        app.edit_commit();
        assert!(matches!(app.mode, Mode::Normal));
        // key renamed, value preserved, no stray old key
        let s = app.doc.as_ref().unwrap().serialize();
        assert_eq!(s, "addr = 8080\n");
    }

    #[test]
    fn inline_tab_is_noop_for_array_element() {
        use crate::tui::state::EditField;
        let mut app = app_with("arr = [1, 2]\n");
        app.expand_all();
        app.rebuild_rows();
        app.cursor = idx_of(&app, "[0]");
        app.begin_inline_edit();
        app.edit_toggle_field(); // array element has no name → stays on Value
        assert!(matches!(&app.mode, Mode::Edit(e) if e.field == EditField::Value));
    }

    #[test]
    fn inline_commit_type_change_enters_prompt_then_confirms() {
        let mut app = app_with("port = 8080\n");
        app.cursor = 1;
        app.begin_inline_edit();
        for _ in 0..4 {
            app.edit_backspace();
        }
        for c in "\"hi\"".chars() {
            app.edit_input_char(c);
        }
        app.edit_commit();
        assert!(
            matches!(app.mode, Mode::Prompt(PromptKind::TypeChange { .. })),
            "changing integer→string must confirm"
        );
        assert!(app.pending_edit.is_some());
        app.handle_prompt_key('y');
        assert!(matches!(app.mode, Mode::Normal));
        assert!(app
            .doc
            .as_ref()
            .unwrap()
            .serialize()
            .contains("port = \"hi\""));
    }

    #[test]
    fn inline_commit_invalid_toml_keeps_editor_open() {
        let mut app = app_with("port = 8080\n");
        let before = app.doc.as_ref().unwrap().serialize();
        app.cursor = 1;
        app.begin_inline_edit();
        for _ in 0..4 {
            app.edit_backspace();
        }
        for c in "= nope".chars() {
            app.edit_input_char(c);
        }
        app.edit_commit();
        assert!(matches!(app.mode, Mode::Edit(_)), "stay in editor to fix");
        assert!(app.status.is_some());
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            before,
            "doc unchanged"
        );
    }

    #[test]
    fn clamp_scroll_separates_viewport_from_cursor() {
        // width 10, buffer length 20.
        // Walk to the right edge: scroll pins the cursor at the right of the window.
        assert_eq!(clamp_scroll(0, 20, 20, 10), 11);
        // Moving left from there stays within the window — text does NOT scroll
        // (this is the bug fix: cursor walks back through the viewport first).
        assert_eq!(clamp_scroll(11, 19, 20, 10), 11);
        assert_eq!(clamp_scroll(11, 12, 20, 10), 11);
        // Only once the cursor reaches the left edge does the text scroll left.
        assert_eq!(clamp_scroll(11, 11, 20, 10), 11);
        assert_eq!(clamp_scroll(11, 10, 20, 10), 10);
        // Cursor near the start keeps the window pinned at 0.
        assert_eq!(clamp_scroll(0, 3, 20, 10), 0);
    }

    #[test]
    fn inline_editor_home_end_move_cursor() {
        let mut app = app_with("port = 8080\n");
        app.cursor = 1;
        app.begin_inline_edit();
        // buffer is "8080", cursor starts at end (4)
        app.edit_cursor_home();
        if let Mode::Edit(ref e) = app.mode {
            assert_eq!(e.cursor, 0);
        } else {
            panic!("not in edit mode");
        }
        app.edit_cursor_end();
        if let Mode::Edit(ref e) = app.mode {
            assert_eq!(e.cursor, e.buffer.chars().count());
        } else {
            panic!("not in edit mode");
        }
    }

    #[test]
    fn add_node_inserts_empty_string_and_enters_edit() {
        let mut app = app_with("a = 1\n");
        app.cursor = 1; // on a
        app.add_node();
        assert!(
            matches!(app.mode, Mode::Edit(_)),
            "add should open the inline editor"
        );
        assert!(
            app.doc
                .as_ref()
                .unwrap()
                .serialize()
                .contains("new_field = \"\""),
            "placeholder inserted: {}",
            app.doc.as_ref().unwrap().serialize()
        );
    }

    #[test]
    fn toggle_detail_on_branch_shows_kind_and_child_count() {
        let mut app = app_with("[server]\nhost = \"h\"\nport = 8080\n");
        app.expand_all();
        app.rebuild_rows();
        app.cursor = app.rows.iter().position(|r| r.key == "server").unwrap();
        app.toggle_detail();
        assert!(matches!(app.mode, Mode::Detail));
        let d = app.detail_text.clone().unwrap();
        assert!(
            d.contains("Type:") && d.contains("table"),
            "shows kind: {d}"
        );
        assert!(
            d.contains("Format:") && d.contains("table"),
            "branch detail shows a format line: {d}"
        );
        assert!(
            d.contains("Children:") && d.contains('2'),
            "branch detail shows child count: {d}"
        );
        // toggling again closes it
        app.toggle_detail();
        assert!(matches!(app.mode, Mode::Normal));
        assert!(app.detail_text.is_none());
    }

    #[test]
    fn detail_distinguishes_inline_table_format() {
        // `{ }` inline table reads as Type table / Format inline; a standard
        // `[table]` reads as Type table / Format table.
        let mut app = app_with("pt = { x = 1 }\n[srv]\nport = 8080\n");
        app.expand_all();
        app.rebuild_rows();
        app.cursor = app.rows.iter().position(|r| r.key == "pt").unwrap();
        app.open_detail();
        let d = app.detail_text.clone().unwrap();
        assert!(d.contains("Format:") && d.contains("inline"), "inline: {d}");

        app.exit_detail();
        app.cursor = app.rows.iter().position(|r| r.key == "srv").unwrap();
        app.open_detail();
        let d = app.detail_text.clone().unwrap();
        assert!(
            d.contains("Format:") && d.contains("table"),
            "standard: {d}"
        );
    }

    #[test]
    fn detail_scroll_clamps_to_range() {
        let mut app = app_with("port = 8080\n");
        app.cursor = 1;
        app.open_detail();
        assert_eq!(app.detail_scroll, 0, "opens at top");
        app.detail_scroll_by(-1, 5);
        assert_eq!(app.detail_scroll, 0, "cannot scroll above the top");
        app.detail_scroll_by(3, 5);
        assert_eq!(app.detail_scroll, 3);
        app.detail_scroll_by(10, 5);
        assert_eq!(app.detail_scroll, 5, "clamped to max");
        app.detail_set_scroll(0);
        assert_eq!(app.detail_scroll, 0);
    }

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

    #[test]
    fn detail_view_shows_comment_type_and_full_text() {
        let mut app = app_with("# one\n# two\na = 1\n");
        app.cursor = 1; // on the merged comment node (row 0 is root)
        app.open_detail();
        let detail = app.detail_text.as_ref().expect("detail should be set");
        assert!(
            detail.contains("comment"),
            "detail should label the type as comment, got: {detail}"
        );
        assert!(
            detail.contains("# one") && detail.contains("# two"),
            "detail should show the full multi-line comment, got: {detail}"
        );
    }
}
