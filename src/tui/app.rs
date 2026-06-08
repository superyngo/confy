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
    /// True while the previous key was a shift+arrow (so the next shift+arrow
    /// continues the same multi-select round). Any non-shift action resets it,
    /// which makes the next shift+arrow start a fresh round.
    pub last_action_was_shift_select: bool,
    /// Present when the app was constructed with a real document (interactive mode).
    pub doc: Option<crate::model::toml_doc::TomlDocument>,
    pub history: Option<History>,
    /// Status message shown in the bottom bar (errors, info).
    pub status: Option<String>,
    pub mode: Mode,
    pub clipboard: Option<Clipboard>,
    /// Filter state: current filter string. When non-empty, rows are filtered.
    pub filter: String,
    /// Caret position (char index) within `filter` while in Filter mode.
    pub filter_cursor: usize,
    /// Last committed filter query, remembered across filter sessions so `/`
    /// restores the previous search instead of starting blank.
    pub last_filter: String,
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
            last_action_was_shift_select: false,
            doc: Some(doc),
            history: Some(history),
            status: None,
            mode: Mode::Normal,
            clipboard: None,
            filter: String::new(),
            filter_cursor: 0,
            last_filter: String::new(),
            filtered_paths: None,
            detail_text: None,
            pending_edit: None,
            detail_scroll: 0,
            table_offset: std::cell::Cell::new(0),
        };
        // Seed the root (empty path) as expanded so the file node starts open.
        app.expanded.insert(Vec::new());
        app.rebuild_rows();
        app
    }

    /// Construct a headless App from a pre-built NodeTree (used in unit tests).
    pub fn from_tree(tree: NodeTree) -> Self {
        // Seed the root (empty path) as expanded so the file node starts open.
        let expanded = HashSet::from([Vec::new()]);
        App {
            tree,
            expanded,
            cursor: 0,
            rows: Vec::new(),
            selection: Selection::new(),
            last_action_was_shift_select: false,
            doc: None,
            history: None,
            status: None,
            mode: Mode::Normal,
            clipboard: None,
            filter: String::new(),
            filter_cursor: 0,
            last_filter: String::new(),
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
        // Collapse every nested branch but keep the file/root node open — `0`
        // shouldn't hide the whole document behind the filename. (An explicit
        // toggle on the root row still collapses it.)
        self.expanded.clear();
        self.expanded.insert(Vec::new());
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

    /// The mode to rest in after a transient overlay/editor (detail popup, inline
    /// editor) closes: stay in the filtered-result selection when a filter is
    /// active, so the highlight, `[filter: …]` status, and Esc-clears-filter
    /// behavior persist; otherwise plain Normal.
    fn resting_mode(&self) -> Mode {
        if self.filtered_paths.is_some() {
            Mode::FilterResults
        } else {
            Mode::Normal
        }
    }

    // ---- Filter (/) ----

    /// `/` — enter the filter input, restoring the last committed query (if any)
    /// with the caret at the end and the live filtered view already applied.
    pub fn enter_filter(&mut self) {
        self.filter = self.last_filter.clone();
        self.filter_cursor = self.filter.chars().count();
        self.mode = Mode::Filter;
        self.recompute_filter();
        self.rebuild_rows();
    }

    /// Enter in the filter input: lock in the filtered set and switch to the
    /// filtered-result selection mode. An empty query just unfilters.
    pub fn commit_filter(&mut self) {
        if self.filter.is_empty() {
            self.exit_filter();
            return;
        }
        self.last_filter = self.filter.clone();
        self.mode = Mode::FilterResults;
        self.rebuild_rows();
    }

    /// Esc in the filtered-result selection mode: drop the active filter back to
    /// the full list (Normal), but keep `last_filter` so `/` can restore it.
    pub fn exit_filter_results(&mut self) {
        self.filter.clear();
        self.filter_cursor = 0;
        self.filtered_paths = None;
        self.mode = Mode::Normal;
        self.rebuild_rows();
    }

    /// Insert a character at the filter caret.
    pub fn filter_char(&mut self, c: char) {
        let at = char_byte_idx(&self.filter, self.filter_cursor);
        self.filter.insert(at, c);
        self.filter_cursor += 1;
        self.recompute_filter();
        self.rebuild_rows();
    }

    /// Backspace in filter mode — delete the char *before* the caret.
    pub fn filter_backspace(&mut self) {
        if self.filter_cursor > 0 {
            let prev = char_byte_idx(&self.filter, self.filter_cursor - 1);
            self.filter.remove(prev);
            self.filter_cursor -= 1;
            self.recompute_filter();
            self.rebuild_rows();
        }
    }

    /// `Del` in filter mode — delete the char *at* the caret (caret stays).
    pub fn filter_delete(&mut self) {
        if self.filter_cursor < self.filter.chars().count() {
            let at = char_byte_idx(&self.filter, self.filter_cursor);
            self.filter.remove(at);
            self.recompute_filter();
            self.rebuild_rows();
        }
    }

    /// Move the filter caret one char left / right / to either end.
    pub fn filter_cursor_left(&mut self) {
        self.filter_cursor = self.filter_cursor.saturating_sub(1);
    }
    pub fn filter_cursor_right(&mut self) {
        let len = self.filter.chars().count();
        if self.filter_cursor < len {
            self.filter_cursor += 1;
        }
    }
    pub fn filter_cursor_home(&mut self) {
        self.filter_cursor = 0;
    }
    pub fn filter_cursor_end(&mut self) {
        self.filter_cursor = self.filter.chars().count();
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
            // Match on the node's key/path (skipping the synthetic `#comment:N`
            // keys), plus — for a Comment node — its own text, so a comment is
            // searchable as a standalone node. A scalar's *value* is still never
            // matched, and matching the comment's single text (not the old
            // value+comment duplicate in the haystack) keeps a loose query like
            // `array` from fuzzily dragging in unrelated section comments.
            let path_keys: Vec<&str> = n
                .path
                .iter()
                .filter_map(|s| match s {
                    Seg::Key(k) if !k.starts_with("#comment:") => Some(k.as_str()),
                    _ => None,
                })
                .collect();
            let comment_text = match &n.kind {
                crate::model::node::NodeKind::Comment(c) => Some(c.as_str()),
                _ => None,
            };
            let h = haystack(&path_keys, None, comment_text);
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
        self.filter_cursor = 0;
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
        self.mode = self.resting_mode();
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
        if self.clipboard.is_some() {
            return; // clipboard mode: selection locked
        }
        self.selection.toggle(self.cursor);
    }

    /// Extend range selection upward (Shift+Up). A fresh shift run (the previous
    /// key wasn't a shift+arrow) starts a new round anchored at the cursor.
    pub fn extend_select_up(&mut self) {
        if self.clipboard.is_some() {
            return; // clipboard mode: use plain cursor movement instead
        }
        if !self.last_action_was_shift_select {
            self.selection.begin_round(self.cursor);
        }
        if self.cursor > 0 {
            self.cursor -= 1;
            self.selection.extend_round_to(self.cursor);
        }
        self.last_action_was_shift_select = true;
    }

    /// Extend range selection downward (Shift+Down).
    pub fn extend_select_down(&mut self) {
        if self.clipboard.is_some() {
            return; // clipboard mode: use plain cursor movement instead
        }
        if !self.last_action_was_shift_select {
            self.selection.begin_round(self.cursor);
        }
        if self.cursor + 1 < self.rows.len() {
            self.cursor += 1;
            self.selection.extend_round_to(self.cursor);
        }
        self.last_action_was_shift_select = true;
    }

    /// Return normalized selected paths (§6.2). Falls back to cursor path if nothing selected.
    pub fn selected_paths(&self) -> Vec<Path> {
        if self.selection.is_empty() {
            return self
                .rows
                .get(self.cursor)
                .map(|r| vec![r.path.clone()])
                .unwrap_or_default();
        }
        let paths: Vec<Path> = self
            .selection
            .iter()
            .filter_map(|i| self.rows.get(i).map(|r| r.path.clone()))
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
        // `#`-prefixed text and write the edit back into the decor. This covers any
        // decor-addressable comment (no `Array` ancestor — including ones inside an
        // AoT entry, whose path carries an `Index`); a comment with an `Array`
        // ancestor is not addressable and falls through to container editing.
        if let Some(node) = node_at(&self.tree.root, &cursor_row.path) {
            if let NodeKind::Comment(text) = &node.kind {
                if self.no_array_ancestor(&cursor_row.path) {
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
        // `Replace` addresses an all-`Key` path, an array-of-tables entry, and any
        // key nested inside one (via the `Key→Index` AoT descent), but NOT a element
        // of a standard array. So truncate the path only at the first `Index` whose
        // container is a real `Array` (editing the whole array there); AoT-entry
        // indices and the keys below them are kept and addressed directly.
        let truncate_at = cursor_row.path.iter().enumerate().find_map(|(i, s)| {
            if matches!(s, Seg::Index(_)) {
                let container_is_array = node_at(&self.tree.root, &cursor_row.path[..i])
                    .map(|n| matches!(n.kind, NodeKind::Array))
                    .unwrap_or(false);
                if container_is_array {
                    return Some(i);
                }
            }
            None
        });
        let path = match truncate_at {
            Some(i) => cursor_row.path[..i].to_vec(),
            None => cursor_row.path.clone(),
        };
        // Serialize just the node's own fragment, carrying its adjacent leading
        // comment(s) into the editor so they can be edited alongside the node. This
        // applies to every keyed node opened in `$EDITOR` — structured (table/inline
        // table/array/AoT) and scalar (multiline strings, `E`-forced leaves) alike;
        // the AoT-entry case carries its own decor in `serialize_node_fragment_opts`.
        // Array *elements* have no key and carry no comment.
        let keyed = matches!(path.last(), Some(Seg::Key(_)));
        let fragment = serialize_node_fragment_opts(doc, &path, keyed);
        let edited = match crate::tui::editor::edit_text(&fragment) {
            Ok(t) => t,
            Err(e) => {
                self.status = Some(format!("editor error: {e}"));
                return;
            }
        };
        // `$EDITOR` fragments are authoritative over key decor (the comment), so the
        // write-back syncs it; inline edits pass `false` and never touch the comment.
        self.apply_replace(path, edited, true);
    }

    /// Apply edited text as a Replace at `path` (the post-editor half of `e`,
    /// split out so it is unit-testable without spawning $EDITOR). On error the
    /// status line is set and the document is left unchanged.
    pub(crate) fn apply_replace(&mut self, path: Path, edited: String, sync_decor: bool) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        match doc.apply(crate::model::document::Mutation::Replace {
            path,
            toml: edited,
            sync_decor,
        }) {
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

    /// True when no `Array` node sits above `path`, i.e. every `Index` in it
    /// descends an array-of-tables entry (a table, addressable by Replace / Rename
    /// / EditComment) rather than a standard array element (which is not). Empty and
    /// length-1 paths trivially qualify.
    fn no_array_ancestor(&self, path: &[Seg]) -> bool {
        (1..path.len()).all(|i| {
            node_at(&self.tree.root, &path[..i])
                .map(|n| !matches!(n.kind, NodeKind::Array))
                .unwrap_or(false)
        })
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
        // A single-line comment edits inline (raw `#` text → `EditComment`), as long
        // as it is decor-addressable (no `Array` ancestor — an AoT-entry ancestor is
        // fine). A merged multi-line comment, or one with an `Array` ancestor, stays
        // in $EDITOR.
        if let NodeKind::Comment(text) = &node.kind {
            let single_line = !text.contains('\n');
            return if single_line && self.no_array_ancestor(path) {
                EditKind::Inline
            } else {
                EditKind::External
            };
        }
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
            // Scalar under a key: inline when the parent is a Table, the Root, or an
            // inline table AND no `Array` sits anywhere above it. An `Array` ancestor
            // means the scalar lives in an array element (e.g. `x = [{ a = 1 }]`)
            // that `Replace` cannot address; an array-of-tables ancestor is fine —
            // its entries ARE tables, reachable via the `Key→Index` AoT descent in
            // `parent_table_mut`/`concrete_table_mut`.
            Some(Seg::Key(_)) => {
                let no_array_ancestor = self.no_array_ancestor(path);
                let parent_ok = path.len() == 1
                    || parent
                        .map(|p| {
                            matches!(
                                p.kind,
                                NodeKind::Table | NodeKind::Root | NodeKind::InlineTable
                            )
                        })
                        .unwrap_or(false);
                if no_array_ancestor && parent_ok {
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
        // A single-line comment node edits its raw `#`-prefixed text as the sole
        // field (no key, no type check); `edit_commit` routes it to `EditComment`.
        let is_comment = node_at(&self.tree.root, &row.path)
            .map(|n| matches!(n.kind, NodeKind::Comment(_)))
            .unwrap_or(false);
        let (key, is_element) = match row.path.last() {
            Some(Seg::Key(k)) if !is_comment => (k.clone(), false),
            // Array element / comment: no editable key.
            Some(Seg::Index(_)) => (String::new(), true),
            Some(Seg::Key(_)) => (String::new(), true), // comment (is_comment is set)
            None => return,
        };
        // `value` is `Value::to_string()`, which carries the decor whitespace
        // around the `=` (e.g. " 8080"); trim it so the edited literal doesn't
        // accumulate a leading space on write-back. A comment's value is its raw
        // `# …` text — keep it verbatim apart from trimming trailing whitespace.
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
            is_comment,
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
            if e.is_element || e.is_comment {
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

    /// `Del` — remove the char *at* the cursor (forward delete); the cursor stays.
    pub fn edit_delete(&mut self) {
        if let Mode::Edit(ref mut e) = self.mode {
            let len = e.buffer.chars().count();
            if e.cursor < len {
                let at = char_byte_idx(&e.buffer, e.cursor);
                e.buffer.remove(at);
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
        self.mode = self.resting_mode();
        self.pending_edit = None;
        self.status = None;
    }

    /// Commit the inline edit. First apply a key rename if the Name field changed
    /// (its own undo step, position/decor-preserving), then reconstruct
    /// `key = <value>`, validate it parses, and either apply `Replace` directly or
    /// — if the scalar's displayed type would change — stash it and enter a
    /// TypeChange confirm prompt. On any failure: set status, stay in the editor.
    pub fn edit_commit(&mut self) {
        // Default to the resting mode (FilterResults when filtered, else Normal) so
        // a successful commit stays in the filtered-result selection; error paths
        // below override back to Edit.
        let rest = self.resting_mode();
        let mut e = match std::mem::replace(&mut self.mode, rest) {
            Mode::Edit(e) => e,
            other => {
                self.mode = other;
                return;
            }
        };
        use crate::tui::state::EditField;
        // A comment node commits its raw `#` text straight through `EditComment`
        // (no key, no `key = value` re-parse, no type check). On a validation
        // failure (`EditComment` rejected non-`#` text) stay in the editor.
        if e.is_comment {
            let text = e.buffer.clone();
            let ok = match self.doc.as_mut() {
                Some(doc) => doc.apply(crate::model::document::Mutation::EditComment {
                    path: e.path.clone(),
                    text: text.clone(),
                }),
                None => Ok(()),
            };
            match ok {
                Ok(()) => self.on_mutation_success(),
                Err(crate::model::document::MutateError::Fragment(msg)) => {
                    self.status = Some(format!("invalid comment: {msg}"));
                    self.mode = Mode::Edit(e);
                }
                Err(err) => {
                    self.status = Some(format!("error: {err}"));
                    self.mode = Mode::Edit(e);
                }
            }
            return;
        }
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
        self.apply_replace(e.path, fragment, false);
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
            self.apply_replace(path, format!("{frag_key} = {new_repr}\n"), false);
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
                let sibling_index = self.true_sibling_index(&r.path);
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
        // If clipboard is already loaded, toggle its mode to copy rather than
        // re-capturing the selection.
        if let Some(cb) = &mut self.clipboard {
            if cb.cut {
                cb.cut = false;
                let n = cb.fragments.len();
                self.status = Some(format!("copied {n} node(s) [changed from cut]"));
            }
            return;
        }
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
            sources: paths,
        });
        self.status = Some(format!(
            "copied {} node(s)",
            self.clipboard.as_ref().unwrap().fragments.len()
        ));
    }

    /// `x` — cut: copy fragments + remember sources. Deletion deferred to paste (wenv-style).
    pub fn cut_selected(&mut self) {
        // If clipboard is already loaded, toggle its mode to cut rather than
        // re-capturing the selection.
        if let Some(cb) = &mut self.clipboard {
            if !cb.cut {
                cb.cut = true;
                let n = cb.fragments.len();
                self.status = Some(format!("cut {n} node(s) [changed from copy]"));
            }
            return;
        }
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
        let cb = match self.clipboard.take() {
            Some(cb) => cb,
            None => {
                self.status = Some("clipboard empty".into());
                return;
            }
        };
        let cursor_row = match self.rows.get(self.cursor) {
            Some(r) => r.clone(),
            None => {
                self.clipboard = Some(cb);
                return;
            }
        };
        let expanded = self.expanded.contains(&cursor_row.path);
        let sibling_index = self.true_sibling_index(&cursor_row.path);
        let target = crate::tui::insertion::resolve_target(&cursor_row, expanded, sibling_index);
        self.do_paste(cb, target, OnCollision::Cancel);
    }

    /// Core paste logic, split out so it can be re-issued after a collision prompt.
    /// Takes ownership of the `Clipboard` and restores it on any failure so the
    /// user can retry (collision → remaining fragments; other errors → same).
    pub(crate) fn do_paste(
        &mut self,
        clipboard: Clipboard,
        target: Target,
        on_collision: OnCollision,
    ) {
        let Clipboard {
            fragments,
            cut: is_cut,
            sources,
        } = clipboard;
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => {
                // Restore clipboard so the user can try again.
                self.clipboard = Some(Clipboard {
                    fragments,
                    cut: is_cut,
                    sources,
                });
                return;
            }
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
                    // Non-collision error: restore the remaining clipboard so the
                    // user can navigate to a valid target and try again.
                    self.clipboard = Some(Clipboard {
                        fragments: fragments[i..].to_vec(),
                        cut: is_cut,
                        sources,
                    });
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

    pub fn escape(&mut self) {
        match &self.mode {
            Mode::Prompt(_) => {
                self.mode = Mode::Normal;
                self.clipboard = None;
                self.pending_edit = None;
                self.status = None;
            }
            Mode::Filter => self.exit_filter(),
            Mode::FilterResults => self.exit_filter_results(),
            Mode::Detail => self.exit_detail(),
            Mode::Help => self.exit_help(),
            Mode::Edit(_) => self.edit_cancel(),
            // Esc in normal mode clears any active multi-selection and clipboard.
            Mode::Normal => {
                if self.clipboard.is_some() {
                    // Peel back clipboard mode first. If a selection was live when the
                    // user pressed c/x, keep it — a second Esc will clear it below.
                    self.clipboard = None;
                    self.status = if !self.selection.is_empty() {
                        Some("clipboard cleared".into())
                    } else {
                        None
                    };
                } else if !self.selection.is_empty() {
                    self.selection.clear();
                    self.last_action_was_shift_select = false;
                    self.status = Some("selection cleared".into());
                }
            }
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
                            self.apply_replace(e.path, fragment, false);
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
                self.do_paste(
                    Clipboard {
                        fragments,
                        cut: is_cut,
                        sources,
                    },
                    target,
                    oc,
                );
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

    /// Return the 0-based index of `path` among its actual parent's children in the
    /// full (unfiltered) NodeTree. Unlike `sibling_index_of`, this is never fooled
    /// by FilterResults mode hiding siblings from `self.rows`.
    fn true_sibling_index(&self, path: &Path) -> usize {
        if path.is_empty() {
            return 0;
        }
        let parent_path = &path[..path.len() - 1];
        node_at(&self.tree.root, parent_path)
            .and_then(|parent| parent.children.iter().position(|c| &c.path == path))
            .unwrap_or(0)
    }

    /// Return whether the cursor sitting on `row` would be a valid paste target for
    /// the current clipboard contents. Used by the renderer to colour each row.
    ///
    /// A row is valid when:
    /// - There is a clipboard, AND
    /// - The resolved target parent's NodeKind is compatible with the clipboard
    ///   fragments (array-element fragments need an Array parent; everything else
    ///   needs Table / Root / InlineTable), AND
    /// - The row is not inside a source that was cut (circular-cut guard).
    pub fn is_valid_paste_target(&self, row: &RowSnapshot) -> bool {
        let cb = match &self.clipboard {
            Some(cb) => cb,
            None => return false,
        };

        // Circular-cut guard: disallow pasting inside any source that was cut.
        if cb.cut {
            for src in &cb.sources {
                if row.path.starts_with(src.as_slice()) {
                    return false;
                }
            }
        }

        // Resolve the Target we would insert at if the cursor were here.
        let expanded = self.expanded.contains(&row.path);
        let sibling_index = self.true_sibling_index(&row.path);
        let target = crate::tui::insertion::resolve_target(row, expanded, sibling_index);

        // Determine the NodeKind of the insertion parent.
        let parent_kind = node_at(&self.tree.root, &target.parent)
            .map(|n| &n.kind)
            .cloned();

        // Classify clipboard sources: all must be array elements for an array paste,
        // all must be table entries otherwise.
        let all_array_elements = cb.sources.iter().all(|src| {
            if src.is_empty() {
                return false;
            }
            matches!(src.last(), Some(Seg::Index(_)))
                && node_at(&self.tree.root, &src[..src.len() - 1])
                    .map(|n| matches!(n.kind, NodeKind::Array))
                    .unwrap_or(false)
        });

        match parent_kind {
            Some(NodeKind::Array) => all_array_elements,
            Some(NodeKind::Root | NodeKind::Table | NodeKind::InlineTable) => !all_array_elements,
            // AoT container, leaves (scalar/comment), or missing node — cannot insert here.
            _ => false,
        }
    }
}

/// Serialize a single node at `path` as a TOML fragment string.
fn serialize_node_fragment(
    doc: &crate::model::toml_doc::TomlDocument,
    path: &[crate::model::node::Seg],
) -> String {
    serialize_node_fragment_opts(doc, path, false)
}

/// As [`serialize_node_fragment`], but when `carry_key_comment` is set, copy the
/// source key's `leaf_decor` prefix onto the emitted key. For an array/inline
/// table or a scalar the leading standalone comment lives in that decor (not the
/// value item), so this is how it is carried into `$EDITOR`; tables carry theirs
/// in the item decor already, so the copy is an empty no-op for them.
fn serialize_node_fragment_opts(
    doc: &crate::model::toml_doc::TomlDocument,
    path: &[crate::model::node::Seg],
    carry_key_comment: bool,
) -> String {
    use crate::model::node::Seg;
    use crate::model::toml_doc::split_leading_blank_lines;
    use toml_edit::{ArrayOfTables, DocumentMut, Item, Value};
    if path.is_empty() {
        return doc.serialize();
    }
    let (parent_segs, last) = path.split_at(path.len().saturating_sub(1));
    // Array-of-tables entry (`product[0]`): emit a single-entry `[[key]]` block,
    // carrying the entry's own decor (its leading comment/blank lines) so an
    // `$EDITOR` round-trip preserves them.
    if let Some(Seg::Index(idx)) = last.first() {
        let (head, key_seg) = parent_segs.split_at(parent_segs.len().saturating_sub(1));
        let aot_key = match key_seg.first() {
            Some(Seg::Key(k)) => k.as_str(),
            _ => return String::new(),
        };
        let tbl = match walk_tablelike(doc.doc.as_table(), head) {
            Some(t) => t,
            None => return String::new(),
        };
        let entry = match tbl.get(aot_key) {
            Some(Item::ArrayOfTables(a)) => match a.get(*idx) {
                Some(t) => t.clone(),
                None => return String::new(),
            },
            _ => return String::new(),
        };
        let mut tmp = DocumentMut::new();
        let mut aot = ArrayOfTables::new();
        aot.push(entry);
        tmp.as_table_mut().insert(aot_key, Item::ArrayOfTables(aot));
        // Open at the entry's first content line, not its leading blank separator
        // (re-attached on write-back by `replace_aot_entry`).
        let s = tmp.to_string();
        return split_leading_blank_lines(&s).1.to_string();
    }
    let key = match last.first() {
        Some(Seg::Key(k)) => k.as_str(),
        _ => return String::new(),
    };
    // Walk to the parent table (AoT-aware: a `Key→Index` pair descends an AoT entry).
    let tbl = match walk_tablelike(doc.doc.as_table(), parent_segs) {
        Some(t) => t,
        None => return String::new(),
    };
    let item = match tbl.get(key) {
        Some(i) => i.clone(),
        None => return String::new(),
    };
    let key_comment = if carry_key_comment {
        tbl.key(key)
            .and_then(|k| k.leaf_decor().prefix().and_then(|r| r.as_str()))
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    } else {
        None
    };
    // Open the editor at the node's first content line (comment or header/value),
    // not its leading blank separator. The blanks are re-attached on write-back
    // (`replace`, gated on `sync_decor`) so file spacing round-trips. Structured
    // nodes (`[table]`, array, inline table) always trim; a scalar trims only on
    // the comment-carrying `$EDITOR` path (`carry_key_comment`), since the
    // clipboard copy that reuses this with `carry_key_comment == false` keeps the
    // raw separator.
    let trim = carry_key_comment
        || matches!(
            item,
            Item::Table(_) | Item::Value(Value::Array(_)) | Item::Value(Value::InlineTable(_))
        );
    let mut tmp = DocumentMut::new();
    tmp.as_table_mut().insert(key, item);
    if let Some(prefix) = key_comment {
        if let Some(mut km) = tmp.as_table_mut().key_mut(key) {
            km.leaf_decor_mut().set_prefix(prefix);
        }
    }
    let s = tmp.to_string();
    if trim {
        split_leading_blank_lines(&s).1.to_string()
    } else {
        s
    }
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

/// Walk a `&dyn TableLike` along `segs`, descending standard tables/inline tables
/// by `Key` and array-of-tables entries by a `Key→Index` pair (the AoT itself is
/// not table-like). Returns the table the path names, or `None` if it does not
/// resolve to one. The immutable mirror of `TomlDocument::parent_table_mut`.
fn walk_tablelike<'a>(
    root: &'a dyn toml_edit::TableLike,
    segs: &[Seg],
) -> Option<&'a dyn toml_edit::TableLike> {
    use toml_edit::Item;
    let mut tbl = root;
    let mut i = 0;
    while i < segs.len() {
        match &segs[i] {
            Seg::Key(k) => {
                let item = tbl.get(k)?;
                if let Item::ArrayOfTables(aot) = item {
                    let idx = match segs.get(i + 1) {
                        Some(Seg::Index(n)) => *n,
                        _ => return None,
                    };
                    tbl = aot.get(idx)?;
                    i += 2;
                    continue;
                }
                tbl = item.as_table_like()?;
                i += 1;
            }
            Seg::Index(_) => return None,
        }
    }
    Some(tbl)
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
    fn root_node_can_collapse_and_expand() {
        let mut app = sample();
        app.rebuild_rows();
        assert_eq!(app.visible_keys(), vec!["f.toml", "a", "b"]);
        // cursor is on the root row; toggling collapses the whole file node.
        app.toggle_expand();
        app.rebuild_rows();
        assert_eq!(app.visible_keys(), vec!["f.toml"]);
        // toggling again re-opens it.
        app.toggle_expand();
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
        assert!(!app.selection.is_empty());
        app.toggle_expand();
        app.rebuild_rows(); // structure changed
        assert!(app.selection.is_empty(), "selection must clear on rebuild");
    }

    #[test]
    fn selection_ops_are_blocked_while_clipboard_active() {
        let mut app = sample();
        // Move cursor to a leaf so we have something selectable.
        app.cursor = 1;
        // Load a clipboard (simulates copy).
        app.clipboard = Some(Clipboard {
            fragments: vec!["x = 1\n".into()],
            cut: false,
            sources: vec![vec![Seg::Key("a".into()), Seg::Key("x".into())]],
        });
        // toggle_select must be a no-op while clipboard is active.
        app.toggle_select();
        assert!(
            app.selection.is_empty(),
            "s should not select when clipboard active"
        );
        // extend_select_down must not alter selection either.
        app.extend_select_down();
        assert!(
            app.selection.is_empty(),
            "Shift+Down should not select when clipboard active"
        );
        // extend_select_up must not alter selection either.
        app.extend_select_up();
        assert!(
            app.selection.is_empty(),
            "Shift+Up should not select when clipboard active"
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

    #[test]
    fn shift_rounds_union_across_a_plain_move_and_esc_clears() {
        use std::collections::HashSet;
        let mut app = app_with("a = 1\nb = 2\nc = 3\nd = 4\ne = 5\n");
        app.rebuild_rows();
        // rows: f.toml(0) a(1) b(2) c(3) d(4) e(5)
        app.cursor = 1;
        app.extend_select_down(); // round 1 -> {1,2}
                                  // a non-shift key (handled in the event loop) resets the flag:
        app.last_action_was_shift_select = false;
        app.cursor = 4;
        app.extend_select_down(); // round 2 from a fresh anchor -> {4,5}
        let sel: HashSet<usize> = app.selection.iter().collect();
        assert_eq!(
            sel,
            HashSet::from([1, 2, 4, 5]),
            "second round must union, not extend from round 1's anchor"
        );
        app.escape(); // Esc in normal mode clears the selection
        assert!(app.selection.is_empty());
    }

    #[test]
    fn external_edit_fragment_trims_leading_blank() {
        // `[t]` sits below a blank separator + comment. The $EDITOR fragment must
        // open at the comment, not an empty line (the blank round-trips on save).
        let app = app_with("a = 1\n\n# c\n[t]\nx = 1\n");
        let doc = app.doc.as_ref().unwrap();
        let frag = serialize_node_fragment_opts(doc, &[Seg::Key("t".into())], true);
        assert!(
            !frag.starts_with('\n'),
            "fragment must not open with a blank line: {frag:?}"
        );
        assert!(
            frag.starts_with("# c"),
            "should start at the comment: {frag:?}"
        );
    }

    #[test]
    fn external_edit_fragment_carries_scalar_leading_comment() {
        // A scalar opened in `$EDITOR` now carries its adjacent leading comment, with
        // the blank separator trimmed from the view (re-attached on save).
        let app = app_with("a = 1\n\n# note\nport = 8080\n");
        let doc = app.doc.as_ref().unwrap();
        let frag = serialize_node_fragment_opts(doc, &[Seg::Key("port".into())], true);
        assert_eq!(frag, "# note\nport = 8080\n", "got: {frag:?}");
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
    fn single_line_comment_edits_inline() {
        let mut app = app_with("# old\nx = 1\n");
        app.expand_all();
        app.rebuild_rows();
        app.cursor = 1; // the comment node
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        app.begin_inline_edit();
        let e = match &app.mode {
            Mode::Edit(e) => e,
            _ => panic!("expected inline edit mode"),
        };
        assert!(e.is_comment, "comment edit must set is_comment");
        assert_eq!(e.buffer, "# old", "buffer seeded with raw comment text");
        // Tab is a no-op for a comment (no name field).
        app.edit_toggle_field();
        assert!(
            matches!(&app.mode, Mode::Edit(e) if e.field == crate::tui::state::EditField::Value)
        );
        // Commit an edited comment → EditComment round-trips into the doc.
        if let Mode::Edit(ref mut e) = app.mode {
            e.buffer = "# new".into();
        }
        app.edit_commit();
        assert!(matches!(app.mode, Mode::Normal));
        let s = app.doc.as_ref().unwrap().serialize();
        assert!(
            s.contains("# new") && !s.contains("# old"),
            "serialize: {s:?}"
        );
    }

    #[test]
    fn comment_between_aot_entries_edits_inline() {
        // The between-entries comment is an all-`Key` path (no Index), so it edits
        // inline and commits via EditComment into the AoT entry's decor prefix.
        let mut app =
            app_with("[[product]]\nname = \"Hammer\"\n# test\n[[product]]\nname = \"Nail\"\n");
        app.expand_all();
        app.rebuild_rows();
        let pos = app.rows.iter().position(|r| r.key == "# test").unwrap();
        app.cursor = pos;
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        app.begin_inline_edit();
        if let Mode::Edit(ref mut e) = app.mode {
            assert!(e.is_comment);
            e.buffer = "# changed".into();
        } else {
            panic!("expected inline edit mode");
        }
        app.edit_commit();
        assert!(matches!(app.mode, Mode::Normal));
        let s = app.doc.as_ref().unwrap().serialize();
        assert!(
            s.contains("# changed") && !s.contains("# test"),
            "serialize: {s:?}"
        );
    }

    #[test]
    fn comment_inside_aot_entry_edits_inline() {
        // `#123` before a key inside an AoT entry has an `Index` in its path but no
        // `Array` ancestor, so it edits inline (was: opened a blank $EDITOR).
        let mut app = app_with("[[product]]\n#123\nname = \"Hammer\"\n");
        app.expand_all();
        app.rebuild_rows();
        let pos = app.rows.iter().position(|r| r.key == "#123").unwrap();
        app.cursor = pos;
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        app.begin_inline_edit();
        if let Mode::Edit(ref mut e) = app.mode {
            assert!(e.is_comment);
            e.buffer = "#321".into();
        } else {
            panic!("expected inline edit mode");
        }
        app.edit_commit();
        let s = app.doc.as_ref().unwrap().serialize();
        assert_eq!(s, "[[product]]\n#321\nname = \"Hammer\"\n");
    }

    #[test]
    fn multiline_comment_routes_external() {
        let mut app = app_with("# a\n# b\nx = 1\n");
        app.expand_all();
        app.rebuild_rows();
        app.cursor = 1; // merged multi-line comment node
        assert_eq!(app.edit_target_kind(), EditKind::External);
    }

    #[test]
    fn inline_comment_commit_rejects_non_comment_and_stays_in_editor() {
        let mut app = app_with("# keep\nx = 1\n");
        let before = app.doc.as_ref().unwrap().serialize();
        app.expand_all();
        app.rebuild_rows();
        app.cursor = 1;
        app.begin_inline_edit();
        if let Mode::Edit(ref mut e) = app.mode {
            e.buffer = "not a comment".into();
        }
        app.edit_commit();
        assert!(matches!(app.mode, Mode::Edit(_)), "stay in editor on error");
        assert!(app.status.is_some(), "error surfaced in status");
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            before,
            "doc unchanged"
        );
    }

    #[test]
    fn apply_replace_invalid_toml_sets_status_and_leaves_doc() {
        let mut app = app_with("port = 8080\n");
        let before = app.doc.as_ref().unwrap().serialize();
        app.apply_replace(vec![Seg::Key("port".into())], "port = = nope".into(), false);
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
        app.apply_replace(vec![Seg::Key("port".into())], "port = 9090\n".into(), false);
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
            Clipboard {
                fragments: vec!["a = 1\n".into(), "b = 2\n".into()],
                cut: false,
                sources: vec![],
            },
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

    // --- Blocker 1: filter must match by scalar VALUE ---

    #[test]
    fn filter_matches_key_not_value() {
        let mut app = app_with("port = 8080\nhost = \"localhost\"\n");
        app.expand_all();
        app.rebuild_rows();
        // A scalar's value (`8080`) is never searched.
        app.enter_filter();
        for c in "8080".chars() {
            app.filter_char(c);
        }
        let keys = app.visible_keys();
        assert!(
            !keys.iter().any(|k| k == "port"),
            "value 8080 must not match the key `port`, got: {keys:?}"
        );
        // The key itself still matches; non-matching siblings are hidden.
        app.exit_filter();
        app.enter_filter();
        for c in "port".chars() {
            app.filter_char(c);
        }
        let keys = app.visible_keys();
        assert!(
            keys.iter().any(|k| k == "port"),
            "key match works: {keys:?}"
        );
        assert!(
            !keys.iter().any(|k| k == "host"),
            "host filtered out: {keys:?}"
        );
    }

    #[test]
    fn filter_matches_comment_by_its_text() {
        // A comment node is searchable by its own text (standalone node).
        let mut app = app_with("# database tuning\nport = 8080\n");
        app.rebuild_rows();
        app.enter_filter();
        for c in "database".chars() {
            app.filter_char(c);
        }
        assert!(
            app.visible_keys().iter().any(|k| k.contains("database")),
            "comment matched by its text, got: {:?}",
            app.visible_keys()
        );
    }

    #[test]
    fn filter_commit_then_esc_remembers_keyword() {
        let mut app = app_with("port = 8080\nhost = \"localhost\"\n");
        app.rebuild_rows();
        // type a query and lock it in
        app.enter_filter();
        for c in "port".chars() {
            app.filter_char(c);
        }
        app.commit_filter();
        assert!(matches!(app.mode, Mode::FilterResults));
        assert!(
            app.filtered_paths.is_some(),
            "filter stays applied after commit"
        );
        let keys = app.visible_keys();
        assert!(keys.iter().any(|k| k == "port"));
        assert!(!keys.iter().any(|k| k == "host"), "host filtered out");
        // Esc unfilters back to the full list but remembers the keyword.
        app.escape();
        assert!(matches!(app.mode, Mode::Normal));
        assert!(app.filtered_paths.is_none());
        assert_eq!(app.last_filter, "port");
        let keys = app.visible_keys();
        assert!(keys.iter().any(|k| k == "host"), "full list restored");
        // Re-entering the filter restores the remembered query + live results.
        app.enter_filter();
        assert_eq!(app.filter, "port");
        assert_eq!(app.filter_cursor, 4);
        assert!(app.filtered_paths.is_some());
    }

    #[test]
    fn detail_and_edit_return_to_filter_results_when_filtered() {
        let mut app = app_with("port = 8080\nhost = \"localhost\"\n");
        app.rebuild_rows();
        app.enter_filter();
        for c in "port".chars() {
            app.filter_char(c);
        }
        app.commit_filter();
        assert!(matches!(app.mode, Mode::FilterResults));
        // Detail popup: open then close returns to the filtered selection.
        app.open_detail();
        assert!(matches!(app.mode, Mode::Detail));
        app.exit_detail();
        assert!(matches!(app.mode, Mode::FilterResults));
        assert!(app.filtered_paths.is_some());
        assert_eq!(
            app.filter, "port",
            "filter (and its highlight) survives detail"
        );
        // Inline edit: cancel returns to the filtered selection too.
        app.cursor = app.rows.iter().position(|r| r.key == "port").unwrap();
        app.begin_inline_edit();
        assert!(matches!(app.mode, Mode::Edit(_)));
        app.edit_cancel();
        assert!(matches!(app.mode, Mode::FilterResults));
        assert_eq!(app.filter, "port");
    }

    #[test]
    fn edit_delete_removes_char_at_cursor() {
        let mut app = app_with("port = 8080\n");
        app.rebuild_rows();
        app.cursor = app.rows.iter().position(|r| r.key == "port").unwrap();
        app.begin_inline_edit();
        app.edit_cursor_home(); // caret before "8080"
        app.edit_delete(); // remove the '8'
        if let Mode::Edit(ref e) = app.mode {
            assert_eq!(e.buffer, "080");
            assert_eq!(e.cursor, 0, "caret stays after forward delete");
        } else {
            panic!("expected edit mode");
        }
    }

    #[test]
    fn filter_edits_at_caret() {
        let mut app = app_with("port = 8080\n");
        app.rebuild_rows();
        app.enter_filter();
        for c in "prt".chars() {
            app.filter_char(c);
        }
        // Insert 'o' between 'p' and 'r': caret left twice → at index 1.
        app.filter_cursor_left();
        app.filter_cursor_left();
        app.filter_char('o');
        assert_eq!(app.filter, "port");
        assert_eq!(app.filter_cursor, 2);
        // Home then Del removes the leading 'p'.
        app.filter_cursor_home();
        app.filter_delete();
        assert_eq!(app.filter, "ort");
        assert_eq!(app.filter_cursor, 0);
        // Backspace at the start is a no-op.
        app.filter_backspace();
        assert_eq!(app.filter, "ort");
        // End then Backspace removes the trailing 't'.
        app.filter_cursor_end();
        app.filter_backspace();
        assert_eq!(app.filter, "or");
        assert_eq!(app.filter_cursor, 2);
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
        app.apply_replace(vec![Seg::Key("port".into())], "port = 9090\n".into(), false);
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
        app.apply_replace(vec![Seg::Key("a".into())], "a = 2\n".into(), false);
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
        // scalar member of an inline table → inline (value Replace + key Rename
        // both address it via an all-`Key` path)
        app.cursor = idx_of(&app, "y");
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
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
    fn serialize_aot_entry_emits_single_block() {
        // `E` on an AoT entry serializes just that `[[product]]` block (not the
        // whole array-of-tables) for external editing.
        let app = app_with("[[product]]\nname = \"Hammer\"\n[[product]]\nname = \"Nail\"\n");
        let doc = app.doc.as_ref().unwrap();
        let frag = serialize_node_fragment(doc, &[Seg::Key("product".into()), Seg::Index(1)]);
        assert_eq!(frag, "[[product]]\nname = \"Nail\"\n");
    }

    #[test]
    fn apply_replace_on_aot_entry_updates_one_entry() {
        // The post-editor half of `E` on an AoT entry: Replace at the `[…,Index]`
        // path rewrites only that entry.
        let mut app = app_with("[[product]]\nname = \"Hammer\"\n[[product]]\nname = \"Nail\"\n");
        app.apply_replace(
            vec![Seg::Key("product".into()), Seg::Index(0)],
            "[[product]]\nname = \"Mallet\"\n".into(),
            true,
        );
        assert!(app.status.is_none(), "unexpected status: {:?}", app.status);
        let s = app.doc.as_ref().unwrap().serialize();
        assert_eq!(
            s,
            "[[product]]\nname = \"Mallet\"\n[[product]]\nname = \"Nail\"\n"
        );
    }

    #[test]
    fn edit_target_kind_aot_entry_scalar_is_inline() {
        // A scalar member of an array-of-tables entry (`product[0].sku`) edits
        // inline — its only `Index` ancestor is the AoT (not an `Array`).
        let mut app = app_with("[[product]]\nname = \"Hammer\"\nsku = 738\n");
        app.expand_all();
        app.rebuild_rows();
        let pos = app.rows.iter().position(|r| r.key == "sku").unwrap();
        app.cursor = pos;
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
    }

    #[test]
    fn edit_target_kind_array_of_inline_tables_scalar_is_external() {
        // A scalar inside an inline table that is itself an array element has an
        // `Array` ancestor, which `Replace` cannot address — stay External.
        let mut app = app_with("items = [{ a = 1 }]\n");
        app.expand_all();
        app.rebuild_rows();
        let pos = app.rows.iter().position(|r| r.key == "a").unwrap();
        app.cursor = pos;
        assert_eq!(app.edit_target_kind(), EditKind::External);
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

    #[test]
    fn esc_from_clipboard_with_selection_clears_clipboard_first() {
        let mut app = sample();
        app.cursor = 1;
        // Simulate: user selected row 1 then pressed 'c'
        app.selection.toggle(1);
        app.clipboard = Some(Clipboard {
            fragments: vec!["x = 1\n".into()],
            cut: false,
            sources: vec![vec![Seg::Key("a".into()), Seg::Key("x".into())]],
        });
        // First Esc: should clear clipboard, leave selection intact.
        app.escape();
        assert!(app.clipboard.is_none(), "first Esc must clear clipboard");
        assert!(
            !app.selection.is_empty(),
            "first Esc must leave selection intact"
        );
        // Second Esc: should clear selection.
        app.escape();
        assert!(app.selection.is_empty(), "second Esc must clear selection");
    }

    #[test]
    fn esc_from_clipboard_without_selection_clears_in_one_step() {
        let mut app = sample();
        // No selection — cursor-only clipboard.
        app.clipboard = Some(Clipboard {
            fragments: vec!["x = 1\n".into()],
            cut: false,
            sources: vec![vec![Seg::Key("a".into()), Seg::Key("x".into())]],
        });
        app.escape();
        assert!(app.clipboard.is_none(), "single Esc must clear clipboard");
        assert!(app.selection.is_empty(), "selection must stay empty");
    }

    #[test]
    fn paste_error_preserves_clipboard() {
        // Trying to paste a bare value ("42\n") into a Table/Root parent is a
        // Fragment error from insert_fragment. The clipboard must survive so the
        // user can retry at a valid location.
        let mut app = app_with("a = 1\n");
        app.rebuild_rows();
        app.cursor = 0; // root
        app.clipboard = Some(Clipboard {
            fragments: vec!["42\n".into()],
            cut: false,
            sources: vec![vec![Seg::Key("a".into())]],
        });
        app.paste();
        assert!(
            app.clipboard.is_some(),
            "clipboard must be preserved after a paste error"
        );
        assert!(
            app.status
                .as_deref()
                .map(|s| s.contains("paste error"))
                .unwrap_or(false),
            "status must show the error"
        );
    }
}
