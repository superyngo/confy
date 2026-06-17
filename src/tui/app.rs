use crate::model::document::{ConfigDocument, Mutation, OnCollision, Target};
use crate::model::node::{Format, KeySign, NodeKind, NodeTree, Path, ScalarType, Seg};
use crate::tui::search::{fuzzy_match, haystack};
use crate::tui::selection::Selection;
use crate::tui::state::{Clipboard, EditState, History, Mode, PasteSlot, PromptKind};
use crate::tui::type_filter::TypeFilter;
use std::collections::HashSet;

/// Which filter layer was most recently (re)applied — drives the one-layer Esc
/// peel when both the `/` text filter and the `f` type filter are active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterLayer {
    Text,
    Type,
}

/// The action a TypeChange confirmation (`y`) applies. A plain value edit replaces
/// in place; a Name edit that changes the node's type (e.g. `foo` → `foo.x`, scalar
/// → `[T/D]` table) is deferred whole so `n` leaves the document untouched.
pub enum PendingCommit {
    /// Replace the node's value with this `key = value` fragment.
    Replace(String),
    /// Rename the key to `new_name` (may introduce dots), then set the value.
    Rename { new_name: String, value: String },
}

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
    pub doc: Option<crate::model::any_doc::AnyDocument>,
    /// The source file path (interactive mode), used to seed the `C` convert
    /// flow's default output path. `None` in headless tests.
    pub source_path: Option<std::path::PathBuf>,
    pub history: Option<History>,
    /// Status message shown in the bottom bar (info messages).
    pub status: Option<String>,
    /// Error message shown in the bottom bar with red styling, always visible
    /// even when clipboard is active or in filter mode.
    pub error: Option<String>,
    pub mode: Mode,
    pub clipboard: Option<Clipboard>,
    /// Active paste-mode insertion slot. `None` = not navigated, so it defaults to
    /// `After(cursor)` (the pre-line-paste behavior). Set on copy/cut and as `↑/↓`
    /// step through slots; reset to `None` on any row rebuild while pasting.
    pub paste_slot: Option<PasteSlot>,
    /// Filter state: current filter string. When non-empty, rows are filtered.
    pub filter: String,
    /// Caret position (char index) within `filter` while in Filter mode.
    pub filter_cursor: usize,
    /// Last committed filter query, remembered across filter sessions so `/`
    /// restores the previous search instead of starting blank.
    pub last_filter: String,
    /// Set of node paths that match the current filter (including ancestors kept
    /// for context). This is the **combined** result of the `/` text filter and
    /// the `f` type filter (AND intersection); `None` when neither is active.
    pub filtered_paths: Option<HashSet<Path>>,
    /// Type-filter (`f`) selections + popup cursor. Selections persist across
    /// popup opens (the type-filter analogue of `last_filter`).
    pub type_filter: TypeFilter,
    /// The filter layer applied most recently — Esc in `FilterResults` peels this
    /// one first so two active filters come off one at a time.
    pub last_filter_applied: Option<FilterLayer>,
    /// Read-only detail text for the current detail popup.
    pub detail_text: Option<String>,
    /// Saved inline-edit awaiting a TypeChange confirmation.
    pub pending_edit: Option<(EditState, PendingCommit)>,
    /// A trailing-comment change staged by the inline editor, consumed by the next
    /// `apply_replace` (so the value edit and its comment commit as one undo step).
    /// `Some(Some(c))` sets/changes the comment, `Some(None)` clears it, the outer
    /// `None` means no change.
    pub pending_trailing: Option<Option<String>>,
    /// Vertical scroll offset (in display rows) of the detail popup.
    pub detail_scroll: u16,
    /// Vertical scroll offset (in display rows) of the help overlay.
    pub help_scroll: u16,
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
    /// Word label for the node's type (scalar type, branch kind, or "comment") —
    /// used by the detail popup, colouring, and type-change detection.
    pub type_label: String,
    /// Fixed-pitch TYPE-column tag, e.g. `(B) [S:str ]` (always 12 chars).
    pub type_tag: String,
    /// Writing style of a scalar leaf (`Plain` for branches/comments).
    pub format: Format,
    pub trailing_comment: Option<String>,
}

pub enum PromptOutcome {
    Consumed,
    Quit,
}

impl App {
    /// Construct an App backed by a real document (interactive mode).
    pub fn new(doc: crate::model::any_doc::AnyDocument) -> Self {
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
            source_path: None,
            history: Some(history),
            status: None,
            error: None,
            mode: Mode::Normal,
            clipboard: None,
            paste_slot: None,
            filter: String::new(),
            filter_cursor: 0,
            last_filter: String::new(),
            filtered_paths: None,
            type_filter: TypeFilter::default(),
            last_filter_applied: None,
            detail_text: None,
            pending_edit: None,
            pending_trailing: None,
            detail_scroll: 0,
            help_scroll: 0,
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
            source_path: None,
            history: None,
            status: None,
            error: None,
            mode: Mode::Normal,
            clipboard: None,
            paste_slot: None,
            filter: String::new(),
            filter_cursor: 0,
            last_filter: String::new(),
            filtered_paths: None,
            type_filter: TypeFilter::default(),
            last_filter_applied: None,
            detail_text: None,
            pending_edit: None,
            pending_trailing: None,
            detail_scroll: 0,
            help_scroll: 0,
            table_offset: std::cell::Cell::new(0),
        }
    }
    pub fn rebuild_rows(&mut self) {
        let doc = self.doc_format();
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
                let type_label = node_type_label(&r.node.kind);
                let type_tag = type_tag(&r.node.kind, r.node.format, doc, r.node.read_only);
                RowSnapshot {
                    key: r.node.key.clone(),
                    path: r.node.path.clone(),
                    depth: r.depth,
                    is_branch: r.node.is_branch(),
                    value: r.node.value.clone(),
                    scalar_type,
                    type_label,
                    type_tag,
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
        // Row indices in `paste_slot` are invalidated by any structural change
        // (expand/collapse or mutation), so drop it; `effective_paste_slot` then
        // falls back to `After(cursor)` near the (re-clamped) cursor.
        if self.clipboard.is_some() {
            self.paste_slot = None;
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
        if self.clipboard.is_some() {
            self.move_paste_slot(1);
            return;
        }
        if self.cursor + 1 < self.rows.len() {
            self.cursor += 1;
        }
    }
    pub fn cursor_up(&mut self) {
        if self.clipboard.is_some() {
            self.move_paste_slot(-1);
            return;
        }
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
    /// `1` — reveal one more depth level of the branch under the cursor: each
    /// press opens the shallowest not-yet-expanded level within that subtree,
    /// until it is fully open. No-op on a leaf/comment or when already full.
    pub fn expand_level(&mut self) {
        let base = match self.rows.get(self.cursor) {
            Some(r) if r.is_branch => r.path.clone(),
            _ => return,
        };
        // Collect every branch in the subtree rooted at `base` (incl. base
        // itself), keyed by depth (= path length).
        let mut branches: Vec<Path> = Vec::new();
        fn walk(n: &crate::model::node::Node, base: &Path, out: &mut Vec<Path>) {
            if n.is_branch() && n.path.len() >= base.len() && n.path[..base.len()] == base[..] {
                out.push(n.path.clone());
            }
            for c in &n.children {
                walk(c, base, out);
            }
        }
        walk(&self.tree.root, &base, &mut branches);
        // Shallowest depth that still has an unexpanded branch.
        let frontier = branches
            .iter()
            .filter(|p| !self.expanded.contains(*p))
            .map(|p| p.len())
            .min();
        let Some(d) = frontier else { return };
        for p in branches.into_iter().filter(|p| p.len() <= d) {
            self.expanded.insert(p);
        }
        self.rebuild_rows();
        if let Some(i) = self.rows.iter().position(|r| r.path == base) {
            self.cursor = i;
        }
    }
    /// `2` — collapse one level and climb. An open branch under the cursor
    /// collapses in place; otherwise the cursor moves up to its parent branch
    /// and collapses that. Repeated presses ascend the tree.
    pub fn collapse_level(&mut self) {
        let path = match self.rows.get(self.cursor) {
            Some(r) => r.path.clone(),
            None => return,
        };
        let is_open_branch = self.rows[self.cursor].is_branch && self.expanded.contains(&path);
        let target = if is_open_branch {
            path
        } else if path.is_empty() {
            return; // already at root; nothing above
        } else {
            path[..path.len() - 1].to_vec()
        };
        self.expanded.remove(&target);
        self.rebuild_rows();
        if let Some(i) = self.rows.iter().position(|r| r.path == target) {
            self.cursor = i;
        }
    }
    pub fn page_up(&mut self, page_size: usize) {
        let step = page_size.max(1);
        if self.clipboard.is_some() {
            self.move_paste_slot(-(step as isize));
            return;
        }
        self.cursor = self.cursor.saturating_sub(step);
    }
    pub fn page_down(&mut self, page_size: usize) {
        let step = page_size.max(1);
        if self.clipboard.is_some() {
            self.move_paste_slot(step as isize);
            return;
        }
        let max = self.rows.len().saturating_sub(1);
        self.cursor = (self.cursor + step).min(max);
    }
    pub fn cursor_home(&mut self) {
        if self.clipboard.is_some() {
            self.move_paste_slot(isize::MIN / 2);
            return;
        }
        self.cursor = 0;
    }
    pub fn cursor_end(&mut self) {
        if self.clipboard.is_some() {
            self.move_paste_slot(isize::MAX / 2);
            return;
        }
        self.cursor = self.rows.len().saturating_sub(1);
    }

    // ---- Paste-mode insertion slots (line/branch targeting) ----

    /// The merged, top-to-bottom sequence of paste slots over the visible rows:
    /// each branch row contributes an `Into` (append last child) followed by an
    /// `After` (line below it); each leaf contributes just an `After`.
    pub fn paste_slots(&self) -> Vec<PasteSlot> {
        let mut slots = Vec::with_capacity(self.rows.len() * 2);
        for (i, row) in self.rows.iter().enumerate() {
            if row.is_branch {
                slots.push(PasteSlot::Into(i));
            }
            slots.push(PasteSlot::After(i));
        }
        slots
    }

    /// The active slot, defaulting to `After(cursor)` when not yet navigated — so
    /// a paste right after copy/cut lands exactly where the pre-line-paste cursor
    /// would have (`resolve_target` parity).
    pub fn effective_paste_slot(&self) -> PasteSlot {
        self.paste_slot.unwrap_or(PasteSlot::After(self.cursor))
    }

    /// Step the active slot by `delta` through `paste_slots`, clamped to range, and
    /// keep `cursor` on the slot's row so the viewport follows.
    fn move_paste_slot(&mut self, delta: isize) {
        let slots = self.paste_slots();
        if slots.is_empty() {
            return;
        }
        let cur = self.effective_paste_slot();
        let idx = slots.iter().position(|s| *s == cur).unwrap_or(0) as isize;
        let max = slots.len() as isize - 1;
        let next = (idx + delta).clamp(0, max) as usize;
        let slot = slots[next];
        self.paste_slot = Some(slot);
        self.cursor = match slot {
            PasteSlot::Into(i) | PasteSlot::After(i) => i,
        };
    }

    /// Resolve a paste slot to a concrete insertion `Target` (the row index is
    /// re-checked against the current rows; `None` if it is stale).
    fn slot_target(&self, slot: PasteSlot) -> Option<Target> {
        match slot {
            PasteSlot::Into(i) => {
                let row = self.rows.get(i)?;
                let index = self
                    .tree
                    .node_at(&row.path)
                    .map(|n| n.children.len())
                    .unwrap_or(0);
                Some(Target {
                    parent: row.path.clone(),
                    index,
                })
            }
            PasteSlot::After(i) => {
                let row = self.rows.get(i)?;
                let expanded = self.expanded.contains(&row.path);
                let sibling_index = self.true_sibling_index(&row.path);
                Some(crate::tui::insertion::resolve_target(
                    row,
                    expanded,
                    sibling_index,
                ))
            }
        }
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
        self.last_filter_applied = Some(FilterLayer::Text);
        self.mode = Mode::FilterResults;
        self.rebuild_rows();
    }

    /// Esc in the filtered-result selection mode: peel **one** filter layer (the
    /// most-recently-applied one) so two active filters come off one at a time.
    /// `last_filter` is kept so `/` can restore the text query.
    pub fn exit_filter_results(&mut self) {
        let peel_text = match self.last_filter_applied {
            // Peel the named layer if it is actually active; otherwise peel
            // whichever single layer is active.
            Some(FilterLayer::Text) if !self.filter.is_empty() => true,
            Some(FilterLayer::Type) if self.type_filter.is_active() => false,
            _ => !self.filter.is_empty(),
        };
        if peel_text {
            self.filter.clear();
            self.filter_cursor = 0;
            self.last_filter_applied = self.type_filter.is_active().then_some(FilterLayer::Type);
        } else {
            self.type_filter.clear();
            self.last_filter_applied = (!self.filter.is_empty()).then_some(FilterLayer::Text);
        }
        self.recompute_filter();
        self.mode = self.resting_mode();
        self.rebuild_rows();
    }

    // ---- Type filter (f) ----

    /// `f` — open the type-filter checkbox popup. Selections persist from the
    /// previous open; the live filtered view is recomputed immediately.
    pub fn enter_type_filter(&mut self) {
        self.mode = Mode::TypeFilter;
        self.recompute_filter();
        self.rebuild_rows();
    }

    /// Move the popup cursor; no document/filter change.
    pub fn type_filter_move(&mut self, dr: i32, dc: i32) {
        let fmt = self.doc_format();
        self.type_filter.move_cursor(dr, dc, fmt);
    }

    /// Space — toggle the focused checkbox and live-update the filtered view.
    pub fn type_filter_toggle(&mut self) {
        let fmt = self.doc_format();
        self.type_filter.toggle_current(fmt);
        if self.type_filter.is_active() {
            self.last_filter_applied = Some(FilterLayer::Type);
        }
        self.recompute_filter();
        self.rebuild_rows();
    }

    /// Enter — apply the type filter and close the popup into the resting mode
    /// (`FilterResults` if any filter is active, else `Normal`).
    pub fn commit_type_filter(&mut self) {
        if self.type_filter.is_active() {
            self.last_filter_applied = Some(FilterLayer::Type);
        }
        self.recompute_filter();
        self.mode = self.resting_mode();
        self.rebuild_rows();
    }

    /// Esc inside the popup: peel the type layer (clear its selections), then
    /// close into the resting mode (keeps the `/` text filter if active).
    pub fn exit_type_filter(&mut self) {
        self.type_filter.clear();
        self.last_filter_applied = (!self.filter.is_empty()).then_some(FilterLayer::Text);
        self.recompute_filter();
        self.mode = self.resting_mode();
        self.rebuild_rows();
    }

    // ---- Kind switch (K) ----

    /// `K` — open the kind-switch popup for the cursor node: a single-select
    /// list of the kinds/notations it can convert to. Non-convertible nodes
    /// (Root, comments, AoT entries) report via the status line.
    /// The backend's format (Toml when there is no document, e.g. in tests).
    pub fn doc_format(&self) -> crate::model::document::DocFormat {
        self.doc
            .as_ref()
            .map_or(crate::model::document::DocFormat::Toml, |d| d.format())
    }

    pub fn open_kind_switch(&mut self) {
        let Some(row) = self.rows.get(self.cursor) else {
            return;
        };
        let path = row.path.clone();
        let Some(doc) = &self.doc else {
            return;
        };
        let options = doc.kind_options(&path);
        if options.is_empty() {
            self.error = Some("this node's kind cannot be switched".into());
            return;
        }
        self.mode = Mode::KindSwitch(crate::tui::state::KindSwitchState {
            path,
            options,
            cursor: 0,
        });
    }

    /// Move the kind-switch popup cursor.
    pub fn kind_switch_move(&mut self, delta: i32) {
        if let Mode::KindSwitch(st) = &mut self.mode {
            let n = st.options.len() as i32;
            if n > 0 {
                st.cursor = (st.cursor as i32 + delta).rem_euclid(n) as usize;
            }
        }
    }

    /// Enter — apply the selected conversion and close the popup.
    pub fn kind_switch_commit(&mut self) {
        let Mode::KindSwitch(st) = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return;
        };
        self.mode = self.resting_mode();
        let Some((label, target)) = st.options.get(st.cursor).cloned() else {
            return;
        };
        let Some(doc) = self.doc.as_mut() else {
            return;
        };
        match doc.apply(crate::model::document::Mutation::ConvertKind {
            path: st.path,
            target,
        }) {
            Ok(()) => {
                self.on_mutation_success();
                self.status = Some(format!("converted to {label}"));
            }
            Err(e) => self.error = Some(format!("kind switch: {e}")),
        }
    }

    /// Esc — close the popup without converting.
    pub fn exit_kind_switch(&mut self) {
        self.mode = self.resting_mode();
        self.status = None;
    }

    // ── document conversion (`C`, Root node) ────────────────────────────────────

    /// `C` — open the document-conversion flow. Only valid on the Root/file node.
    pub fn open_convert(&mut self) {
        let Some(row) = self.rows.get(self.cursor) else {
            return;
        };
        if !row.path.is_empty() {
            self.error = Some("convert: move to the root/file node first (key C)".into());
            return;
        }
        let Some(doc) = &self.doc else {
            return;
        };
        let current = doc.format();
        let options: Vec<crate::model::document::DocFormat> = [
            crate::model::document::DocFormat::Toml,
            crate::model::document::DocFormat::Json,
            crate::model::document::DocFormat::Yaml,
        ]
        .into_iter()
        .filter(|f| *f != current)
        .collect();
        self.mode = Mode::Convert(crate::tui::state::ConvertState {
            step: crate::tui::state::ConvertStep::Format,
            options,
            cursor: 0,
            target: current,
            path: String::new(),
            path_cursor: 0,
            warnings: Vec::new(),
            text: String::new(),
        });
    }

    /// Move the target-format selection cursor (Format step).
    pub fn convert_move(&mut self, delta: i32) {
        if let Mode::Convert(st) = &mut self.mode {
            let n = st.options.len() as i32;
            if n > 0 {
                st.cursor = (st.cursor as i32 + delta).rem_euclid(n) as usize;
            }
        }
    }

    /// Enter on the Format step: lock the target and seed the output path.
    pub fn convert_pick_format(&mut self) {
        let default_path = self.source_path.clone();
        if let Mode::Convert(st) = &mut self.mode {
            let Some(target) = st.options.get(st.cursor).copied() else {
                return;
            };
            st.target = target;
            let ext = match target {
                crate::model::document::DocFormat::Toml => "toml",
                crate::model::document::DocFormat::Json => "json",
                crate::model::document::DocFormat::Yaml => "yaml",
            };
            st.path = default_path
                .map(|p| p.with_extension(ext).to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("out.{ext}"));
            st.path_cursor = st.path.chars().count();
            st.step = crate::tui::state::ConvertStep::Path;
        }
    }

    /// Caret-field editing for the output-path input (Path step).
    pub fn convert_path_char(&mut self, c: char) {
        if let Mode::Convert(st) = &mut self.mode {
            let at = char_byte_idx(&st.path, st.path_cursor);
            st.path.insert(at, c);
            st.path_cursor += 1;
        }
    }
    pub fn convert_path_backspace(&mut self) {
        if let Mode::Convert(st) = &mut self.mode {
            if st.path_cursor > 0 {
                let at = char_byte_idx(&st.path, st.path_cursor - 1);
                st.path.remove(at);
                st.path_cursor -= 1;
            }
        }
    }
    pub fn convert_path_delete(&mut self) {
        if let Mode::Convert(st) = &mut self.mode {
            if st.path_cursor < st.path.chars().count() {
                let at = char_byte_idx(&st.path, st.path_cursor);
                st.path.remove(at);
            }
        }
    }
    pub fn convert_path_left(&mut self) {
        if let Mode::Convert(st) = &mut self.mode {
            st.path_cursor = st.path_cursor.saturating_sub(1);
        }
    }
    pub fn convert_path_right(&mut self) {
        if let Mode::Convert(st) = &mut self.mode {
            st.path_cursor = (st.path_cursor + 1).min(st.path.chars().count());
        }
    }
    pub fn convert_path_home(&mut self) {
        if let Mode::Convert(st) = &mut self.mode {
            st.path_cursor = 0;
        }
    }
    pub fn convert_path_end(&mut self) {
        if let Mode::Convert(st) = &mut self.mode {
            st.path_cursor = st.path.chars().count();
        }
    }

    /// Enter on the Path step: render the conversion. Writes immediately when
    /// there are no warnings, else advances to the Confirm step.
    pub fn convert_run(&mut self) {
        let (target, path) = match &self.mode {
            Mode::Convert(st) => (st.target, st.path.clone()),
            _ => return,
        };
        let Some(doc) = &self.doc else {
            return;
        };
        match crate::model::convert::convert(doc, target) {
            Ok(result) => {
                if result.warnings.is_empty() {
                    self.convert_write(&path, &result.text);
                } else if let Mode::Convert(st) = &mut self.mode {
                    st.warnings = result.warnings;
                    st.text = result.text;
                    st.step = crate::tui::state::ConvertStep::Confirm;
                }
            }
            Err(abort) => {
                self.error = Some(format!("conversion aborted: {abort}"));
                self.mode = self.resting_mode();
            }
        }
    }

    /// `y` on the Confirm step: write the rendered output.
    pub fn convert_confirm(&mut self) {
        let (path, text) = match &self.mode {
            Mode::Convert(st) => (st.path.clone(), st.text.clone()),
            _ => return,
        };
        self.convert_write(&path, &text);
    }

    /// Write the converted output and close the flow. The open document is never
    /// modified (no reload, dirty state untouched).
    fn convert_write(&mut self, path: &str, text: &str) {
        match std::fs::write(path, text) {
            Ok(()) => self.status = Some(format!("converted → {path}")),
            Err(e) => self.error = Some(format!("convert write failed: {e}")),
        }
        self.mode = self.resting_mode();
    }

    /// Esc — abandon the conversion flow without writing.
    pub fn exit_convert(&mut self) {
        self.mode = self.resting_mode();
        self.status = None;
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
    /// Recompute the combined filtered-path set from the `/` text filter and the
    /// `f` type filter. A node passes iff it matches **both** (AND); matched nodes
    /// drag in their ancestors for context. `None` when neither filter is active.
    fn recompute_filter(&mut self) {
        if self.filter.is_empty() && !self.type_filter.is_active() {
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
            type_filter: &TypeFilter,
            doc: crate::model::document::DocFormat,
        ) {
            // Text match: the node's key/path (positional segments — array elements
            // and comments — have no key), plus, for a Comment node, its own text,
            // so a comment is searchable as a standalone node. A scalar's *value*
            // is never matched, which keeps a loose query like `array` from fuzzily
            // dragging in unrelated section comments. An empty needle matches all.
            let path_keys: Vec<&str> = n
                .path
                .iter()
                .filter_map(|s| match s {
                    Seg::Key(k) => Some(k.as_str()),
                    _ => None,
                })
                .collect();
            let comment_text = match &n.kind {
                crate::model::node::NodeKind::Comment(c) => Some(c.as_str()),
                _ => None,
            };
            let h = haystack(&path_keys, None, comment_text);
            let text_ok = fuzzy_match(&h, needle);
            // Type match: empty type filter imposes no constraint.
            let type_ok = type_filter.matches(n.key_sign, &n.kind, n.format, doc, n.read_only);
            if text_ok && type_ok {
                matching.insert(n.path.clone());
                for anc in ancestor_paths.iter() {
                    ancestors.insert(anc.clone());
                }
            }
            ancestor_paths.push(n.path.clone());
            for c in &n.children {
                walk(
                    c,
                    ancestor_paths,
                    matching,
                    ancestors,
                    needle,
                    type_filter,
                    doc,
                );
            }
            ancestor_paths.pop();
        }
        let doc = self.doc_format();
        walk(
            &self.tree.root,
            &mut Vec::new(),
            &mut matching,
            &mut ancestors,
            &self.filter,
            &self.type_filter,
            doc,
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
        // Full path, indices included: keys join with `.`, positional segments
        // (array elements, AoT entries, comments) render as `[i]` so an element's
        // path reads `a.b[2].c` rather than the truncated `a.b.c`.
        let dotted = if row.path.is_empty() {
            "(root)".to_string()
        } else {
            let mut s = String::new();
            for seg in &row.path {
                match seg {
                    Seg::Key(k) => {
                        if !s.is_empty() {
                            s.push('.');
                        }
                        s.push_str(k);
                    }
                    Seg::Index(i) => s.push_str(&format!("[{i}]")),
                }
            }
            s
        };
        let mut detail = if row.is_branch {
            // Branch nodes carry their writing style in the kind: a table can be
            // a standard `[table]` or an `{ inline }` table, an array a standard
            // `[...]` or an `[[array-of-tables]]`. Surface both axes — the coarse
            // Type and the concrete Format — plus the child count.
            let node = self.tree.node_at(&row.path);
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
            // Compact format label, matching the KIND column; single-style
            // scalars (bool/float/datetime) read as "plain".
            let fmt_str = crate::tui::ui::format_label(row.format).unwrap_or("plain");
            format!("Path:     {dotted}\nType:     {type_str}\nFormat:   {fmt_str}\nValue:    {val_str}")
        };
        // Key-sign facet: no longer shown in the KIND column, surfaced here as a
        // word. Positional nodes (array elements, AoT entries, comments) read
        // `none`.
        let sign_str = match self.tree.node_at(&row.path).map(|n| n.key_sign) {
            Some(KeySign::Bare) => "bare",
            Some(KeySign::Quoted) => "quoted",
            Some(KeySign::Dotted) => "dotted",
            _ => "none",
        };
        detail.push_str(&format!("\nSign:     {sign_str}"));
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
        self.help_scroll = 0;
        self.mode = Mode::Help;
    }

    /// Scroll the help overlay by `delta` rows, clamped to `[0, max]`.
    pub fn help_scroll_by(&mut self, delta: i32, max: u16) {
        let v = (self.help_scroll as i32 + delta).clamp(0, max as i32);
        self.help_scroll = v as u16;
    }

    /// Jump the help overlay to an absolute scroll offset (Home/End).
    pub fn help_set_scroll(&mut self, v: u16) {
        self.help_scroll = v;
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

    /// Return true when the cursor node is read-only (a JSONC `/* */` block
    /// comment). Such nodes may be copied but must not be edited, deleted, cut,
    /// or remarked.
    fn cursor_is_read_only(&self) -> bool {
        self.rows
            .get(self.cursor)
            .and_then(|r| self.tree.node_at(&r.path))
            .map(|n| n.read_only)
            .unwrap_or(false)
    }

    /// `e` — edit the cursor node's fragment in $EDITOR and apply Replace.
    /// On MutateError::Fragment: show error in status line, leave doc unchanged.
    pub fn edit_node(&mut self) {
        if self.cursor_is_read_only() {
            self.status = Some("read-only node (block comment)".into());
            return;
        }
        let cursor_row = match self.rows.get(self.cursor) {
            Some(r) => r.clone(),
            None => return,
        };
        // A comment node has no real item to serialize: open $EDITOR with its raw
        // `#`-prefixed text and write the edit back into the decor. This covers any
        // decor-addressable comment (no `Array` ancestor — including ones inside an
        // AoT entry, whose path carries an `Index`); a comment with an `Array`
        // ancestor is not addressable and falls through to container editing.
        if let Some(node) = self.tree.node_at(&cursor_row.path) {
            if let NodeKind::Comment(text) = &node.kind {
                if self.no_array_ancestor(&cursor_row.path) {
                    let initial = format!("{text}\n");
                    let edited = match crate::tui::editor::edit_text(&initial) {
                        Ok(t) => t,
                        Err(e) => {
                            self.error = Some(format!("editor error: {e}"));
                            return;
                        }
                    };
                    self.apply_edit_comment(cursor_row.path.clone(), edited);
                    return;
                }
            }
        }
        // Decide the capture/Replace path and whether it is a standard-array
        // element that must be wrapped on commit (see `external_edit_path`).
        let (path, wrap_element) = self.external_edit_path(&cursor_row.path);
        // Serialize just the node's own fragment. An adjacent standalone comment is
        // an independent node in the CST and is NOT part of the fragment — the
        // editor opens at the node's own header/value line.
        let fragment = match self.doc.as_ref() {
            Some(d) => d.serialize_fragment(&path),
            None => return,
        };
        let edited = match crate::tui::editor::edit_text(&fragment) {
            Ok(t) => t,
            Err(e) => {
                self.error = Some(format!("editor error: {e}"));
                return;
            }
        };
        // A standard-array element's fragment is a bare value repr; wrap it as the
        // backend's value-Replace element form so `Replace` splices just that
        // element instead of the whole array (TOML `__elem__ = …`, JSON bare).
        let edited = if wrap_element {
            match self.doc.as_ref() {
                Some(d) => d.scalar_fragment(None, edited.trim_end_matches('\n')),
                None => return,
            }
        } else {
            edited
        };
        self.apply_replace(path, edited);
    }

    /// The path to capture/Replace for an external (`$EDITOR`) edit of `path`, and
    /// whether that capture is a standard-array **element** whose bare fragment must
    /// be wrapped as the backend's value-Replace element form on commit.
    ///
    /// Every node edits *precisely* — just that node — in every backend. A standard-array
    /// **element** (`x[0]`, `x[0][1]`) has no key, and its bare repr isn't
    /// `Replace`-addressable on its own in TOML/JSON, so the caller wraps it
    /// (`scalar_fragment(None, …)` → TOML `__elem__ = …`, JSON a bare value); YAML's
    /// `- value` fragment is addressable directly, so no wrap. Any other node — including
    /// a key/index reached *through* an array index (`x[0].a`, `x[0].a.b`) — is
    /// `Replace`-addressable directly: the inline splice rebuilds the enclosing
    /// `{ … }` / `[ … ]` element in place, so the path is kept whole and never truncated
    /// to the array (matching YAML; AoT-entry indices and their keys were already kept).
    pub(crate) fn external_edit_path(&self, path: &Path) -> (Path, bool) {
        let is_array_element = matches!(path.last(), Some(Seg::Index(_)))
            && path
                .len()
                .checked_sub(1)
                .and_then(|plen| self.tree.node_at(&path[..plen]))
                .map(|n| matches!(n.kind, NodeKind::Array))
                .unwrap_or(false);
        if is_array_element {
            let addressable = self
                .doc
                .as_ref()
                .map(|d| d.array_elements_addressable())
                .unwrap_or(false);
            return (path.clone(), !addressable);
        }
        (path.clone(), false)
    }

    /// Apply edited text as a Replace at `path` (the post-editor half of `e`,
    /// split out so it is unit-testable without spawning $EDITOR). On error the
    /// error field is set and the document is left unchanged.
    pub(crate) fn apply_replace(&mut self, path: Path, edited: String) {
        // A staged trailing-comment change rides along: applied as a second
        // mutation after the value, then both are committed as one history step.
        let trailing = self.pending_trailing.take();
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        let fmt = doc.format().name();
        match doc.apply(crate::model::document::Mutation::Replace {
            path: path.clone(),
            fragment: edited,
        }) {
            Ok(()) => {
                if let Some(comment) = trailing {
                    if let Err(e) =
                        doc.apply(crate::model::document::Mutation::SetTrailingComment {
                            path,
                            comment,
                        })
                    {
                        self.error = Some(format!("comment update failed: {e}"));
                    }
                }
                self.on_mutation_success();
            }
            Err(crate::model::document::MutateError::Fragment(msg)) => {
                self.error = Some(format!("invalid {fmt}: {msg}"));
            }
            Err(e) => self.error = Some(format!("error: {e}")),
        }
    }

    /// Apply edited comment text as an `EditComment` at `path` (the post-editor
    /// half of editing a comment node). On error the error field is set and the
    /// document is left unchanged.
    pub(crate) fn apply_edit_comment(&mut self, path: Path, text: String) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        match doc.apply(crate::model::document::Mutation::EditComment { path, text }) {
            Ok(()) => self.on_mutation_success(),
            Err(crate::model::document::MutateError::Fragment(msg)) => {
                self.error = Some(format!("invalid comment: {msg}"));
            }
            Err(e) => self.error = Some(format!("error: {e}")),
        }
    }

    /// True when no `Array` node sits above `path`, i.e. every `Index` in it
    /// descends an array-of-tables entry (a table, addressable by Replace / Rename
    /// / EditComment) rather than a standard array element (which is not). Empty and
    /// length-1 paths trivially qualify.
    fn no_array_ancestor(&self, path: &[Seg]) -> bool {
        (1..path.len()).all(|i| {
            self.tree
                .node_at(&path[..i])
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
        let node = match self.tree.node_at(path) {
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
        // Single-line arrays / inline tables / objects edit inline as their one-line
        // repr (the projection put it in `value`; a multiline container leaves
        // `value == None`). A TOML inline table is `NodeKind::InlineTable`; a JSON
        // inline object is `NodeKind::Table` with `Format::Inline` (no scope tables in
        // JSON) — both carry a one-line repr and are inline-editable. A TOML scope /
        // dotted table is a `Table` with another Format and stays $EDITOR.
        let inline_object = matches!(node.kind, NodeKind::Table) && node.format == Format::Inline;
        let structured_inline =
            matches!(node.kind, NodeKind::Array | NodeKind::InlineTable) || inline_object;
        if !matches!(node.kind, NodeKind::Scalar(_)) && !structured_inline {
            return EditKind::External;
        }
        if structured_inline && node.value.is_none() {
            return EditKind::External; // multiline container
        }
        // Multiline strings carry real newlines the single-line inline editor
        // cannot represent — route them to $EDITOR. Keyed on Format (not on a raw
        // `\n` scan): an element of a *multiline array* carries indentation-newline
        // decor in its repr yet is itself an ordinary single-line string. YAML's
        // literal `|` and folded `>` block scalars are likewise multi-line.
        if matches!(
            node.format,
            Format::MultilineBasic
                | Format::MultilineLiteral
                | Format::LiteralBlock
                | Format::Folded
        ) {
            return EditKind::External;
        }
        // YAML addresses any scalar reachable through block-sequence / flow elements
        // (`resolve` descends `Index`→`Key`), so an `Array` ancestor does NOT force
        // $EDITOR the way it must for TOML/JSON (whose array elements aren't
        // `Replace`-addressable). Gate the array-ancestor checks below on the facet.
        let addressable = self
            .doc
            .as_ref()
            .map(|d| d.array_elements_addressable())
            .unwrap_or(false);
        let parent_path = &path[..path.len() - 1];
        let parent = self.tree.node_at(parent_path);
        match path.last() {
            // Scalar element of a plain array: inline-editable wherever the array
            // sits. `Replace` addresses the element directly (`Target::ArrayElement`)
            // even when the array is nested under a key inside an inline-table / AoT
            // element (e.g. `x[0].vals[1]`) — so the old `Key`-after-`Index`
            // restriction is dropped (it was over-conservative; verified across all
            // backends). A *multiline* string element was already routed to $EDITOR
            // above by its Format. The immediate parent being an `Array` is the whole
            // condition (an AoT group is `ArrayOfTables`, not `Array`, so its entries
            // still go External as before).
            Some(Seg::Index(_)) => {
                let parent_is_array = parent
                    .map(|p| matches!(p.kind, NodeKind::Array))
                    .unwrap_or(false);
                if parent_is_array {
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
                let parent_ok = path.len() == 1
                    || parent
                        .map(|p| {
                            matches!(
                                p.kind,
                                NodeKind::Table | NodeKind::Root | NodeKind::InlineTable
                            )
                        })
                        .unwrap_or(false);
                // A single-line member of an inline table / inline object IS
                // `Replace`-addressable even under an `Array` ancestor (`x = [{ a = 1 }]`
                // → `x[0].a`): the projection indexes the member as a `Target::Entry` /
                // `Target::Member`, and the replace splice rebuilds the `{ … }` in place.
                // So an `Array` ancestor no longer forces $EDITOR when the immediate
                // parent is an inline container. YAML stays addressable everywhere.
                let parent_inline_container = parent
                    .map(|p| {
                        matches!(p.kind, NodeKind::InlineTable)
                            || (matches!(p.kind, NodeKind::Table) && p.format == Format::Inline)
                    })
                    .unwrap_or(false);
                let addressable = parent_ok
                    && (addressable || self.no_array_ancestor(path) || parent_inline_container);
                if addressable {
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
        let is_comment = self
            .tree
            .node_at(&row.path)
            .map(|n| matches!(n.kind, NodeKind::Comment(_)))
            .unwrap_or(false);
        // A comment has no editable key and is not an array element (its path may end
        // in a `Seg::Index` under the CST backend); the `is_comment` flag drives it.
        let (key, is_element) = if is_comment {
            (String::new(), false)
        } else {
            match row.path.last() {
                Some(Seg::Key(k)) => (k.clone(), false),
                Some(Seg::Index(_)) => (String::new(), true), // array element: no key
                None => return,
            }
        };
        // `value` is `Value::to_string()`, which carries the decor whitespace
        // around the `=` (e.g. " 8080"); trim it so the edited literal doesn't
        // accumulate a leading space on write-back. A comment's value is its raw
        // `# …` text — keep it verbatim apart from trimming trailing whitespace.
        let mut buffer = row.value.clone().unwrap_or_default().trim().to_string();
        // A keyed scalar's trailing inline comment is appended to the Value buffer so
        // it's edited inline together; commit splits it back out. (Array elements and
        // comment nodes don't carry an editable trailing comment here.)
        // Seed any trailing comment into the Value buffer so it's edited inline. An
        // array element carries one too (a multiline-array `1,  # note`); only a
        // comment *node* has no separate trailing comment.
        let orig_trailing = if is_comment {
            None
        } else {
            row.trailing_comment.clone()
        };
        if let Some(tc) = &orig_trailing {
            buffer.push_str("  ");
            buffer.push_str(tc);
        }
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
            rename_only: false,
            buffer,
            cursor,
            scroll: 0,
            other_buffer: key,
            other_cursor: name_cursor,
            other_scroll: 0,
            orig_trailing,
            created_on_add: false,
        });
        self.status = None;
    }

    /// `F2` — open the inline editor on the Name field for any keyed node,
    /// including [T/D] synthetic tables, [T/S] scope tables, and [A/T] groups.
    /// Skips the value Replace step on commit (rename-only).
    pub fn begin_inline_rename(&mut self) {
        let row = match self.rows.get(self.cursor) {
            Some(r) => r,
            None => return,
        };
        // Only nodes addressed by a key can be renamed (not root, array elements).
        let key = match row.path.last() {
            Some(Seg::Key(k)) => k.clone(),
            _ => return,
        };
        // Comments have no rename-able key.
        let is_comment = self
            .tree
            .node_at(&row.path)
            .map(|n| matches!(n.kind, NodeKind::Comment(_)))
            .unwrap_or(false);
        if is_comment {
            return;
        }
        let name_cursor = key.chars().count();
        self.mode = Mode::Edit(crate::tui::state::EditState {
            path: row.path.clone(),
            key: key.clone(),
            field: crate::tui::state::EditField::Name,
            is_element: false,
            is_comment: false,
            rename_only: true,
            buffer: key.clone(),
            cursor: name_cursor,
            scroll: 0,
            other_buffer: String::new(),
            other_cursor: 0,
            other_scroll: 0,
            orig_trailing: None,
            created_on_add: false,
        });
        self.status = None;
        self.error = None;
    }

    /// `Tab` in the inline editor: swap focus between the Value and Name fields,
    /// stashing the active working set and loading the other. No-op for array
    /// elements (no name).
    pub fn edit_toggle_field(&mut self) {
        if let Mode::Edit(ref mut e) = self.mode {
            if e.is_element || e.is_comment || e.rename_only {
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
        // Esc on a freshly-added seed (opened by `a`) rolls the insert back so the
        // add leaves no trace — the document and undo/redo history return to their
        // pre-add state, and the cursor falls back to the prior row.
        let created_on_add = matches!(&self.mode, Mode::Edit(e) if e.created_on_add);
        self.mode = self.resting_mode();
        self.pending_edit = None;
        // A staged trailing-comment change must not survive a cancelled edit, or a
        // later `apply_replace` (e.g. a nudge) would consume it on another node.
        self.pending_trailing = None;
        self.status = None;
        if created_on_add {
            self.cancel_added_node();
        }
    }

    /// Roll back the most recent `a` insert (the seed whose inline edit was just
    /// cancelled): restore the prior document snapshot without leaving an undo/redo
    /// crumb, then re-project and clamp the cursor.
    fn cancel_added_node(&mut self) {
        let snapshot = match self.history.as_mut().and_then(|h| h.cancel_last()) {
            Some(s) => s,
            None => return,
        };
        if let Some(doc) = self.doc.as_mut() {
            if doc.replace_from_str(&snapshot).is_ok() {
                self.tree = doc.project();
            }
        }
        self.rebuild_rows();
        if self.cursor >= self.rows.len() {
            self.cursor = self.rows.len().saturating_sub(1);
        }
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
        let (name_str, raw_value) = match e.field {
            EditField::Value => (e.other_buffer.clone(), e.buffer.clone()),
            EditField::Name => (e.buffer.clone(), e.other_buffer.clone()),
        };
        // Array elements have no key; validate/label the bare value under a
        // placeholder key. The model's Replace ignores the key for an Index path.
        let is_element = matches!(e.path.last(), Some(Seg::Index(_)));
        // Split a scalar/element buffer back into value + trailing comment (the
        // backend lexer handles a `#`/`//` inside a string). A comment-less backend
        // leaves the buffer as the value verbatim. A change from the baseline is
        // staged for `apply_replace` to commit alongside the value.
        let split = self
            .doc
            .as_ref()
            .filter(|d| d.supports_comments())
            .map(|d| d.split_value_comment(&raw_value));
        let (value_str, new_trailing) = match split {
            Some((v, c)) => (v, c),
            None => (raw_value.clone(), None),
        };
        // A trailing comment can't share the single line of an inline array / flow
        // collection (`[1, 2]`, `{a: 1}`). Reject cleanly before any mutation so the
        // edit is atomic; the user can `K` to a multiline/block form first.
        if new_trailing.is_some() {
            let in_inline = (1..e.path.len()).any(|i| {
                self.tree
                    .node_at(&e.path[..i])
                    .map(|n| {
                        matches!(n.kind, NodeKind::InlineTable)
                            || (matches!(n.kind, NodeKind::Array) && n.format == Format::Inline)
                    })
                    .unwrap_or(false)
            });
            if in_inline {
                self.status = Some(
                    "inline collection can't hold a trailing comment — switch to multiline (K) first"
                        .into(),
                );
                self.mode = Mode::Edit(e);
                return;
            }
        }
        // Stage the comment for `apply_replace`. Normally only a real change is
        // staged; but a backend whose `Replace` drops the comment (YAML's whole-entry
        // swap) must re-assert an existing comment even when its text is unchanged.
        let preserves = self
            .doc
            .as_ref()
            .map(|d| d.replace_preserves_trailing_comment())
            .unwrap_or(true);
        let changed = new_trailing != e.orig_trailing;
        let reassert = !preserves && new_trailing.is_some();
        self.pending_trailing = (changed || reassert).then_some(new_trailing);
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
                // A rename that changes the node's *type* (e.g. a TOML dotted key
                // turns a scalar into a `[T/D]` table) is confirmed first and
                // deferred whole, so `n` leaves the document untouched. JSON has no
                // dotted keys, so a rename never changes type — skip the check.
                let old_label = self
                    .rows
                    .get(self.cursor)
                    .map(|r| r.type_label.clone())
                    .unwrap_or_default();
                let new_label = self
                    .doc
                    .as_ref()
                    .map(|d| d.rename_can_change_type())
                    .unwrap_or(false)
                    .then(|| project_first_label(&format!("{new_name} = {value_str}\n")))
                    .flatten();
                if let Some(new_label) = new_label {
                    if new_label != old_label {
                        self.status = Some(format!("type {old_label} → {new_label}? y/n"));
                        self.pending_edit = Some((
                            e,
                            PendingCommit::Rename {
                                new_name,
                                value: value_str,
                            },
                        ));
                        self.mode = Mode::Prompt(PromptKind::TypeChange {
                            from: old_label,
                            to: new_label,
                        });
                        return;
                    }
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
        // F2 rename-only: skip the value Replace step entirely.
        if e.rename_only {
            self.mode = self.resting_mode();
            return;
        }
        // 2. Value replace. The fragment and the type-change projection are built
        //    by the backend so the TUI never hard-codes `key = value` vs JSON's
        //    `"key": value` (a bad value keeps the editor open).
        let key_arg = (!is_element).then_some(frag_key.as_str());
        let (fragment, new_label) = match self.doc.as_ref() {
            Some(doc) => {
                let fragment = doc.scalar_fragment(key_arg, &value_str);
                match doc.value_kind(&value_str) {
                    Ok(kind) => (fragment, node_type_label(&kind)),
                    Err(msg) => {
                        self.status = Some(format!("invalid value: {msg}"));
                        self.mode = Mode::Edit(e); // stay in the editor to fix it
                        return;
                    }
                }
            }
            None => (format!("{frag_key} = {value_str}\n"), String::new()),
        };
        let old_label = self
            .rows
            .get(self.cursor)
            .map(|r| r.type_label.clone())
            .unwrap_or_default();
        if new_label != old_label {
            self.status = Some(format!("type {old_label} → {new_label}? y/n"));
            self.pending_edit = Some((e, PendingCommit::Replace(fragment)));
            self.mode = Mode::Prompt(PromptKind::TypeChange {
                from: old_label,
                to: new_label,
            });
            return;
        }
        self.apply_replace(e.path, fragment);
    }

    /// Apply a confirmed type-changing Name edit: rename the key (may introduce
    /// dots, turning a scalar into a `[T/D]` table), then set the value on the
    /// resulting leaf. On rename failure the document is left untouched.
    fn apply_deferred_rename(&mut self, mut e: EditState, new_name: String, value: String) {
        let res = match self.doc.as_mut() {
            Some(doc) => doc.apply(crate::model::document::Mutation::Rename {
                path: e.path.clone(),
                new_key: new_name.clone(),
            }),
            None => return,
        };
        if let Err(err) = res {
            self.error = Some(format!("rename failed: {err}"));
            return;
        }
        self.on_mutation_success();
        // The node moved to parent + the new key's segments; set the value there.
        let parent_len = e.path.len() - 1;
        let new_segs: Vec<Seg> = new_name
            .split('.')
            .map(|s| Seg::Key(s.to_string()))
            .collect();
        let leaf_key = match new_segs.last() {
            Some(Seg::Key(k)) => k.clone(),
            _ => new_name.clone(),
        };
        e.path.truncate(parent_len);
        e.path.extend(new_segs);
        self.apply_replace(e.path, format!("{leaf_key} = {value}\n"));
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
        let node = match self.tree.node_at(&path) {
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
        let format = node.format;
        // Capture the trailing comment now and drop the `node` (self.tree) borrow so
        // `pending_trailing` can be staged below.
        let trailing = node.trailing_comment.clone();
        if let Some(new_repr) = nudge_scalar(st, format, &repr, delta) {
            let key_arg = (frag_key != "__elem__").then_some(frag_key.as_str());
            let preserves = self
                .doc
                .as_ref()
                .map(|d| d.replace_preserves_trailing_comment())
                .unwrap_or(true);
            let fragment = match self.doc.as_ref() {
                Some(doc) => doc.scalar_fragment(key_arg, &new_repr),
                None => format!("{frag_key} = {new_repr}\n"),
            };
            // A backend whose value `Replace` drops the trailing comment (YAML's
            // whole-entry swap) must re-assert it, exactly as the inline editor does.
            if !preserves {
                if let Some(tc) = trailing {
                    self.pending_trailing = Some(Some(tc));
                }
            }
            self.apply_replace(path, fragment);
        }
    }

    /// `a` — add a new node. The **root or an expanded branch** appends an empty
    /// scalar as its **last child**; **any other node** (a collapsed branch or a
    /// leaf) gets a **next sibling of its own kind** in the same scope — a scalar
    /// beside a scalar, an (empty) array beside an array, a table beside a table,
    /// and so on. The seed for a scalar is an empty string opened in the inline
    /// editor (TOML has no null); a container seed is empty (`[]`/`{}`, or a
    /// `[key]`/`[[key]]` TOML header) and is named `placeholder` for `e` to rename.
    pub fn add_node(&mut self) {
        if self.doc.is_none() {
            return;
        }
        let cursor_row = match self.rows.get(self.cursor).cloned() {
            Some(r) => r,
            None => return,
        };
        let expanded = self.expanded.contains(&cursor_row.path);
        let is_append = cursor_row.path.is_empty() || (cursor_row.is_branch && expanded);
        // The cursor's own kind drives the same-kind sibling seed.
        let cursor_kind = self.tree.node_at(&cursor_row.path).map(|n| n.kind.clone());

        let mut target = if is_append {
            let n = self
                .tree
                .node_at(&cursor_row.path)
                .map(|p| p.children.len())
                .unwrap_or(0);
            Target {
                parent: cursor_row.path.clone(),
                index: n,
            }
        } else {
            let mut parent = cursor_row.path.clone();
            parent.pop();
            Target {
                parent,
                index: self.true_sibling_index(&cursor_row.path) + 1,
            }
        };

        let parent_node = self.tree.node_at(&target.parent);
        let parent_is_array = parent_node
            .map(|n| matches!(n.kind, NodeKind::Array))
            .unwrap_or(false);
        let existing: Vec<String> = parent_node
            .map(|p| p.children.iter().map(|c| c.key.clone()).collect())
            .unwrap_or_default();

        // The seed kind: a scalar for the append (child) path; otherwise the
        // cursor's own kind so the sibling matches it.
        let seed_kind = if is_append {
            NodeKind::Scalar(ScalarType::String)
        } else {
            cursor_kind.unwrap_or(NodeKind::Scalar(ScalarType::String))
        };

        // A sibling beside a comment is another standalone comment.
        if matches!(seed_kind, NodeKind::Comment(_)) {
            self.add_comment_sibling(target);
            return;
        }

        // Appending a scalar into a non-array branch clamps it to the leading
        // region (before any `[table]`/`[[aot]]`) so it stays legal (D5) — this
        // only applies to the append path; a same-kind sibling already respects
        // the partition. A `[T/D]` dotted table is not a capturing header.
        if is_append && !parent_is_array && matches!(seed_kind, NodeKind::Scalar(_)) {
            let split = parent_node
                .map(|p| {
                    p.children
                        .iter()
                        .position(|c| {
                            matches!(c.kind, NodeKind::Table | NodeKind::ArrayOfTables)
                                && c.format != Format::Dotted
                        })
                        .unwrap_or(p.children.len())
                })
                .unwrap_or(0);
            if target.index > split {
                target.index = split;
            }
        }

        // Ensure the destination branch is expanded so the new node is visible.
        if !target.parent.is_empty() {
            self.expanded.insert(target.parent.clone());
        }

        // Build the seed fragment from the backend (so notation isn't hard-coded),
        // except TOML's header-only `[table]`/`[[aot]]` which have no `key = value`
        // form. `key = None` yields a bare array element.
        let doc = self.doc.as_ref().unwrap();
        let bare = parent_is_array;
        let key = if bare {
            None
        } else {
            Some(unique_key(
                if matches!(seed_kind, NodeKind::Scalar(_)) {
                    "new_field"
                } else {
                    "placeholder"
                },
                &existing,
            ))
        };
        // A keyless array element uses the backend's bare-element form (uniform
        // across TOML/JSON/YAML); a keyed member uses `scalar_fragment`.
        let seed_value = |v: &str| -> String {
            if bare {
                doc.array_element_fragment(v)
            } else {
                doc.scalar_fragment(key.as_deref(), v)
            }
        };
        let (fragment, inline) = match &seed_kind {
            NodeKind::Scalar(_) | NodeKind::Root | NodeKind::Comment(_) => {
                (seed_value("\"\""), true)
            }
            // Containers: the backend forms the empty seed in its own notation
            // (TOML `[table]`/`[[aot]]`, JSON/YAML `{}`/`[]`), so no format sniff
            // and no hard-coded notation here.
            NodeKind::Array | NodeKind::InlineTable | NodeKind::ArrayOfTables | NodeKind::Table => {
                (
                    doc.empty_container_fragment(&seed_kind, key.as_deref()),
                    false,
                )
            }
        };

        self.apply_insert(target.clone(), fragment);
        if self.error.is_some() {
            return; // insert failed; error already set
        }
        // Locate the freshly inserted row and move the cursor to it.
        let mut new_path = target.parent.clone();
        match &key {
            Some(k) => new_path.push(Seg::Key(k.clone())),
            None => new_path.push(Seg::Index(target.index)),
        }
        if let Some(idx) = self.rows.iter().position(|r| r.path == new_path) {
            self.cursor = idx;
            if inline {
                self.begin_inline_edit();
                // Flag the session so Esc rolls the just-inserted seed back.
                if let Mode::Edit(e) = &mut self.mode {
                    e.created_on_add = true;
                }
            } else {
                self.status = Some("added placeholder node — rename with e".into());
            }
        }
    }

    /// Insert an empty standalone comment line at `target` (the same-kind sibling
    /// of a comment cursor) and move the cursor onto it.
    fn add_comment_sibling(&mut self, target: Target) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        if !doc.supports_comments() {
            self.status = Some("comments not supported here".into());
            return;
        }
        let text = format!("{} ", doc.comment_prefix());
        match doc.apply(crate::model::document::Mutation::InsertComment {
            target: target.clone(),
            text,
        }) {
            Ok(()) => self.on_mutation_success(),
            Err(e) => {
                self.error = Some(format!("add error: {e}"));
                return;
            }
        }
        let mut new_path = target.parent.clone();
        new_path.push(Seg::Index(target.index));
        if let Some(idx) = self.rows.iter().position(|r| r.path == new_path) {
            self.cursor = idx;
            self.status = Some("added comment — edit with e".into());
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
        let fmt = doc.format().name();
        match doc.apply(crate::model::document::Mutation::Insert {
            target,
            fragment: edited,
            on_collision: crate::model::document::OnCollision::Cancel,
        }) {
            Ok(()) => self.on_mutation_success(),
            Err(crate::model::document::MutateError::Collision(key)) => {
                self.error = Some(format!(
                    "key collision: {key} (rename/overwrite not yet prompted)"
                ));
            }
            Err(crate::model::document::MutateError::Fragment(msg)) => {
                self.error = Some(format!("invalid {fmt}: {msg}"));
            }
            Err(e) => self.error = Some(format!("error: {e}")),
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
        self.error = None;
    }

    // ---- §6 operations: d/x/c/v/m/r/z/y ----

    /// `d` — delete selected or cursor node(s).
    pub fn delete_selected(&mut self) {
        if self.cursor_is_read_only() {
            self.status = Some("read-only node (block comment)".into());
            return;
        }
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
                self.error = Some(format!("delete error: {e}"));
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
            fragments.push(doc.serialize_fragment_relative(p));
        }
        self.clipboard = Some(Clipboard {
            fragments,
            cut: false,
            sources: paths,
        });
        // Fresh capture: the green line starts at the current cursor (After(cursor)).
        self.paste_slot = None;
        self.status = Some(format!(
            "copied {} node(s)",
            self.clipboard.as_ref().unwrap().fragments.len()
        ));
    }

    /// `x` — cut: copy fragments + remember sources. Deletion deferred to paste (wenv-style).
    pub fn cut_selected(&mut self) {
        if self.cursor_is_read_only() {
            self.status = Some("read-only node (block comment)".into());
            return;
        }
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
            fragments.push(doc.serialize_fragment_relative(p));
        }
        self.clipboard = Some(Clipboard {
            fragments,
            cut: true,
            sources: paths,
        });
        // Fresh capture: the green line starts at the current cursor (After(cursor)).
        self.paste_slot = None;
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
        let target = match self.slot_target(self.effective_paste_slot()) {
            Some(t) => t,
            None => {
                self.clipboard = Some(cb);
                return;
            }
        };
        self.do_paste(cb, target, OnCollision::Cancel, false);
    }

    /// Core paste logic, split out so it can be re-issued after a collision prompt.
    /// Takes ownership of the `Clipboard` and restores it on any failure so the
    /// user can retry (collision → remaining fragments; other errors → same).
    /// CUT uses atomic `Mutation::Move` (delete-before-reinsert, no partial state,
    /// same-scope paste never collides). COPY uses the per-fragment insert loop.
    /// Core paste logic, split out so it can be re-issued after a collision prompt.
    /// Node entries: CUT uses atomic `Mutation::Move` (delete-before-reinsert,
    /// same-scope never collides); COPY uses a per-fragment insert loop. Comment
    /// entries (`Seg::Index`-addressed) are pasted via `Mutation::InsertComment`
    /// and never collide; for CUT the source comment is deleted first (so an
    /// identical-text comment elsewhere isn't removed by the delete sweep).
    /// The clipboard is restored on any failure so the user can retry.
    pub(crate) fn do_paste(
        &mut self,
        clipboard: Clipboard,
        target: Target,
        on_collision: OnCollision,
        allow_upgrade: bool,
    ) {
        let Clipboard {
            fragments,
            cut: is_cut,
            sources,
        } = clipboard;

        // Identify comment sources by the projected node's *kind* (a comment path
        // ends in `Seg::Index`, which an array element shares — the kind is the
        // only reliable discriminator).
        let is_comment = |p: &Path| {
            self.tree
                .node_at(p)
                .map(|n| matches!(n.kind, NodeKind::Comment(_)))
                .unwrap_or(false)
        };

        // Pair each fragment with its source, then split node vs comment entries.
        // `sources` may be shorter than `fragments` in synthetic / test scenarios;
        // treat any unmatched fragments as node entries with an empty placeholder path.
        let mut node_entries: Vec<(String, Path)> = Vec::new();
        let mut comment_entries: Vec<(String, Path)> = Vec::new();
        let mut frags_iter = fragments.into_iter();
        let mut srcs_iter = sources.into_iter();
        loop {
            match frags_iter.next() {
                None => break,
                Some(frag) => {
                    let src = srcs_iter.next().unwrap_or_default();
                    if is_comment(&src) {
                        comment_entries.push((frag, src));
                    } else {
                        node_entries.push((frag, src));
                    }
                }
            }
        }

        // Rebuild a Clipboard from remaining node + comment entries (kept parallel).
        let rebuild =
            |is_cut: bool, nodes: &[(String, Path)], comments: &[(String, Path)]| -> Clipboard {
                let mut fragments = Vec::new();
                let mut sources = Vec::new();
                for (f, s) in nodes.iter().chain(comments.iter()) {
                    fragments.push(f.clone());
                    sources.push(s.clone());
                }
                Clipboard {
                    fragments,
                    cut: is_cut,
                    sources,
                }
            };

        if self.doc.is_none() {
            self.clipboard = Some(rebuild(is_cut, &node_entries, &comment_entries));
            return;
        }

        // Validate the comment destination *before* any mutation: comments need a
        // table/root decor slot or an array, so a cut must never delete a comment
        // it then can't paste (which would lose it). Abort non-destructively,
        // keeping the whole clipboard, so cut behaves like copy on an illegal
        // target. A *single-line* array is legal but rewrites the array's layout
        // (`InsertComment` upgrades it to multiline), so it prompts y/n first
        // unless this paste was re-issued from that prompt (`allow_upgrade`).
        // TODO(jsonc-upgrade): also gate comment-paste/InsertComment — when
        // `!self.doc.as_ref().map(|d| d.supports_comments()).unwrap_or(true)` and
        // `!comment_entries.is_empty()`, show a `PromptKind::JsoncUpgrade` with a
        // `PendingComment` variant that re-issues `do_paste` after `enable_comments()`.
        if !comment_entries.is_empty() {
            enum Dest {
                Ok,
                Prompt,
                Illegal,
            }
            let dest = self
                .tree
                .node_at(&target.parent)
                .map(|n| match n.kind {
                    NodeKind::Root | NodeKind::Table => Dest::Ok,
                    // multiline array (single-line arrays have a one-line `value`)
                    NodeKind::Array if n.value.is_none() => Dest::Ok,
                    NodeKind::Array if allow_upgrade => Dest::Ok,
                    NodeKind::Array => Dest::Prompt,
                    _ => Dest::Illegal,
                })
                .unwrap_or(Dest::Illegal);
            match dest {
                Dest::Ok => {}
                Dest::Prompt => {
                    self.clipboard = Some(rebuild(is_cut, &node_entries, &comment_entries));
                    self.status =
                        Some("single-line array — reformat to multiline and insert? y/n".into());
                    self.mode = Mode::Prompt(PromptKind::ArrayUpgrade {
                        target,
                        on_collision,
                    });
                    return;
                }
                Dest::Illegal => {
                    self.clipboard = Some(rebuild(is_cut, &node_entries, &comment_entries));
                    self.error = Some(
                        "paste error: comments can only go into a table or the document".into(),
                    );
                    return;
                }
            }
        }

        // ---- NODE PHASE ----
        if is_cut {
            let node_sources: Vec<Path> = node_entries.iter().map(|(_, s)| s.clone()).collect();
            if !node_sources.is_empty() {
                let doc = self.doc.as_mut().unwrap();
                match doc.apply(Mutation::Move {
                    sources: node_sources,
                    target: target.clone(),
                    on_collision,
                }) {
                    Ok(()) => {}
                    Err(crate::model::document::MutateError::Collision(key)) => {
                        self.clipboard = Some(rebuild(is_cut, &node_entries, &comment_entries));
                        self.error = Some(format!("collision on key '{key}' — o/r/c"));
                        self.mode = Mode::Prompt(PromptKind::Collision { key });
                        return;
                    }
                    Err(e) => {
                        self.clipboard = Some(rebuild(is_cut, &node_entries, &comment_entries));
                        self.error = Some(format!("paste error: {e}"));
                        return;
                    }
                }
            }
        } else {
            // Destination `[A/T]` group or plain array: pack all copied keyed nodes
            // into ONE new `[[…]]` entry / `{ … }` element by joining their fragments
            // into a single Insert (mirrors the cut path's join in `move_nodes`).
            let dest_packs = self
                .tree
                .node_at(&target.parent)
                .map(|n| matches!(n.kind, NodeKind::ArrayOfTables | NodeKind::Array))
                .unwrap_or(false);
            let grouped: Vec<(String, usize)> = if dest_packs
                && node_entries.len() > 1
                && node_entries
                    .iter()
                    .all(|(f, _)| crate::model::cst_edit::joinable_entry(f))
            {
                let joined: String = node_entries.iter().map(|(f, _)| f.as_str()).collect();
                vec![(joined, 0)]
            } else {
                node_entries
                    .iter()
                    .enumerate()
                    .map(|(i, (f, _))| (f.clone(), i))
                    .collect()
            };
            let doc = self.doc.as_mut().unwrap();
            for (frag, i) in &grouped {
                let i = *i;
                match doc.apply(Mutation::Insert {
                    target: target.clone(),
                    fragment: frag.clone(),
                    on_collision,
                }) {
                    Ok(()) => {}
                    Err(crate::model::document::MutateError::Collision(key)) => {
                        self.clipboard =
                            Some(rebuild(is_cut, &node_entries[i..], &comment_entries));
                        self.error = Some(format!("collision on key '{key}' — o/r/c"));
                        self.mode = Mode::Prompt(PromptKind::Collision { key });
                        return;
                    }
                    Err(e) => {
                        self.clipboard =
                            Some(rebuild(is_cut, &node_entries[i..], &comment_entries));
                        self.error = Some(format!("paste error: {e}"));
                        return;
                    }
                }
            }
        }

        // ---- COMMENT PHASE (never collides) ----
        // Each `InsertComment` *prepends* its block at the target slot, so to keep
        // multiple pasted comments in their original order we apply them last-first.
        // `oi` is the original index (high→low); restore slices stay in original order.
        //
        // `target.index` is a *pre-deletion* ordinal in the parent's full child
        // sequence. On a CUT, sources sitting *before* it are deleted before the
        // insert, shifting the surviving slots up — so the insert index must drop by
        // the count of already-deleted same-parent sources below the target, or the
        // comment overshoots (lands one slot too far down). `self.tree` is still the
        // pre-paste projection here (refreshed only by `on_mutation_success` at the
        // end), so it gives the original ordinals. (Copy deletes nothing ⇒ no shift.)
        // Re-inserted comments stack *at* the slot, never before it, so they don't
        // count.
        let orig_ord = |p: &Path| -> Option<usize> {
            self.tree
                .node_at(&target.parent)
                .and_then(|par| par.children.iter().position(|c| &c.path == p))
        };
        // Nodes were already removed by the Move above (cut only); count those below.
        let node_shift = if is_cut {
            node_entries
                .iter()
                .filter(|(_, s)| orig_ord(s).is_some_and(|o| o < target.index))
                .count()
        } else {
            0
        };
        // Comment source ordinals (computed before any comment deletion).
        let comment_ords: Vec<Option<usize>> =
            comment_entries.iter().map(|(_, s)| orig_ord(s)).collect();
        let n_comments = comment_entries.len();
        for rev in 0..n_comments {
            let oi = n_comments - 1 - rev;
            let (frag, src) = &comment_entries[oi];
            // Comments processed so far (indices `oi..`) have had their sources
            // deleted; add those sitting below the target to the shift.
            let comment_shift = if is_cut {
                comment_ords[oi..]
                    .iter()
                    .filter(|o| o.is_some_and(|o| o < target.index))
                    .count()
            } else {
                0
            };
            let ctarget = Target {
                parent: target.parent.clone(),
                index: target.index.saturating_sub(node_shift + comment_shift),
            };
            if is_cut {
                // Delete the source first so an identical-text comment elsewhere
                // isn't removed by the text-matching delete sweep after insert.
                let doc = self.doc.as_mut().unwrap();
                if let Err(e) = doc.apply(Mutation::Delete { path: src.clone() }) {
                    // Earlier phases (node moves/inserts, later comments) are already
                    // committed — refresh the projection so the tree isn't stale. The
                    // not-yet-applied comments are the lower indices, including `oi`.
                    self.on_mutation_success();
                    self.clipboard = Some(rebuild(is_cut, &[], &comment_entries[..=oi]));
                    self.error = Some(format!("paste error: {e}"));
                    return;
                }
            }
            let doc = self.doc.as_mut().unwrap();
            if let Err(e) = doc.apply(Mutation::InsertComment {
                target: ctarget.clone(),
                text: frag.clone(),
            }) {
                // For cut, `oi`'s source was already deleted, so only lower indices
                // remain; for copy, `oi` itself can retry. Prior mutations in this
                // paste are committed — refresh the projection before reporting.
                let end = if is_cut { oi } else { oi + 1 };
                self.on_mutation_success();
                self.clipboard = Some(rebuild(is_cut, &[], &comment_entries[..end]));
                self.error = Some(format!("paste error: {e}"));
                return;
            }
        }

        self.on_mutation_success();
    }

    pub fn escape(&mut self) {
        self.error = None;
        match &self.mode {
            Mode::Prompt(_) => {
                self.mode = Mode::Normal;
                self.clipboard = None;
                self.pending_edit = None;
                self.status = None;
            }
            Mode::Filter => self.exit_filter(),
            Mode::FilterResults => self.exit_filter_results(),
            Mode::TypeFilter => self.exit_type_filter(),
            Mode::KindSwitch(_) => self.exit_kind_switch(),
            Mode::Convert(_) => self.exit_convert(),
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
        if self.cursor_is_read_only() {
            self.status = Some("read-only node (block comment)".into());
            return;
        }
        let path = match self.rows.get(self.cursor) {
            Some(r) => r.path.clone(),
            None => return,
        };
        // Determine direction: if the current node is already a Comment, remark
        // un-comments it (no comment authoring). If it's a live node, remark authors
        // a comment — so we need `supports_comments()`.
        let authoring = self
            .tree
            .node_at(&path)
            .map(|n| !matches!(n.kind, NodeKind::Comment(_)))
            .unwrap_or(false);
        let supports = self
            .doc
            .as_ref()
            .map(|d| d.supports_comments())
            .unwrap_or(true);
        if authoring && !supports {
            self.mode = Mode::Prompt(PromptKind::JsoncUpgrade {
                pending: crate::tui::state::PendingComment::Remark { path },
            });
            return;
        }
        self.do_remark(path);
    }

    /// Apply `Mutation::Remark` for the given path (post-gate).
    fn do_remark(&mut self, path: Path) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        match doc.apply(Mutation::Remark { path }) {
            Ok(()) => self.on_mutation_success(),
            Err(crate::model::document::MutateError::Fragment(_)) => {
                self.status = Some("not valid comment, kept as-is".into());
            }
            Err(e) => self.error = Some(format!("remark error: {e}")),
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
                self.error = Some(format!("save error: {e}"));
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
            Err(e) => self.error = Some(format!("undo restore error: {e}")),
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
            Err(e) => self.error = Some(format!("redo restore error: {e}")),
        }
    }

    /// Handle a key press while in a prompt mode. Returns true if consumed.
    pub fn handle_prompt_key(&mut self, c: char) -> PromptOutcome {
        match &self.mode {
            Mode::Prompt(PromptKind::TypeChange { .. }) => {
                match c {
                    'y' => {
                        if let Some((e, commit)) = self.pending_edit.take() {
                            self.mode = Mode::Normal;
                            match commit {
                                PendingCommit::Replace(fragment) => {
                                    self.apply_replace(e.path, fragment)
                                }
                                PendingCommit::Rename { new_name, value } => {
                                    self.apply_deferred_rename(e, new_name, value)
                                }
                            }
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
                let sibling_index = self.true_sibling_index(&cursor_row.path);
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
                    false,
                );
                PromptOutcome::Consumed
            }
            Mode::Prompt(PromptKind::ArrayUpgrade { .. }) => {
                if c != 'y' {
                    // Keep the clipboard so the user can pick another target.
                    self.mode = Mode::Normal;
                    self.status = Some("paste cancelled — clipboard kept".into());
                    return PromptOutcome::Consumed;
                }
                let (target, oc) = match &self.mode {
                    Mode::Prompt(PromptKind::ArrayUpgrade {
                        target,
                        on_collision,
                    }) => (target.clone(), *on_collision),
                    _ => unreachable!(),
                };
                self.mode = Mode::Normal;
                match self.clipboard.take() {
                    Some(cb) => self.do_paste(cb, target, oc, true),
                    None => self.status = None,
                }
                PromptOutcome::Consumed
            }
            Mode::Prompt(PromptKind::JsoncUpgrade { .. }) => {
                match c {
                    'y' | 'Y' => {
                        if let Mode::Prompt(PromptKind::JsoncUpgrade { pending }) =
                            std::mem::replace(&mut self.mode, Mode::Normal)
                        {
                            if let Some(d) = self.doc.as_mut() {
                                d.enable_comments();
                            }
                            match pending {
                                crate::tui::state::PendingComment::Remark { path } => {
                                    self.do_remark(path)
                                }
                            }
                        }
                    }
                    _ => {
                        self.mode = self.resting_mode();
                    }
                }
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
    /// full (unfiltered) NodeTree, so insertion positions are correct even in
    /// FilterResults mode (where `self.rows` hides non-matching siblings).
    fn true_sibling_index(&self, path: &Path) -> usize {
        if path.is_empty() {
            return 0;
        }
        let parent_path = &path[..path.len() - 1];
        self.tree
            .node_at(parent_path)
            .and_then(|parent| parent.children.iter().position(|c| &c.path == path))
            .unwrap_or(0)
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
/// mirroring the `OnCollision::Rename` scheme in `cst_edit`.
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

/// The type label of the first node a `key = value` (or dotted) fragment projects
/// to — `None` if it doesn't parse. Used to detect whether a Name edit changes the
/// node's type (e.g. `foo` → `foo.x` projects to a `table`).
fn project_first_label(fragment: &str) -> Option<String> {
    let parse = taplo::parser::parse(fragment);
    if !parse.errors.is_empty() {
        return None;
    }
    crate::model::cst_project::project(&parse.into_syntax(), "")
        .root
        .children
        .first()
        .map(|n| node_type_label(&n.kind))
}

/// Display type label for a node kind — used by `rebuild_rows` for the TYPE
/// column and on a freshly parsed inline-edit fragment, so an edit can detect
/// a type change by string comparison.
fn node_type_label(kind: &crate::model::node::NodeKind) -> String {
    use crate::model::node::NodeKind;
    match kind {
        NodeKind::Root => String::new(),
        NodeKind::Table => "table".into(),
        NodeKind::ArrayOfTables => "array-of-tables".into(),
        NodeKind::Array => "array".into(),
        NodeKind::InlineTable => "inline".into(),
        NodeKind::Scalar(st) => format!("{st:?}").to_lowercase(),
        NodeKind::Comment(_) => "comment".into(),
    }
}

/// Fixed-pitch TYPE-column tag: a 3-char key sign + a padded 8-char type slot,
/// always 12 columns so the column never shifts. Containers use 5-char tags
/// (`[A/I]` inline array, `[A/M]` multiline, `[A/T]` AoT, `[T/I]` inline table,
/// `[T/S]` table scope, `[T/D]` dotted-key table, `[G]` root, `[C]` comment)
/// padded to the slot; scalars
/// use `[X:xxxx]` (type letter + 4-char format). An AoT *entry* projects as a
/// Table, so it reads `(-) [T/S]`.
fn type_tag(
    kind: &NodeKind,
    format: Format,
    doc: crate::model::document::DocFormat,
    read_only: bool,
) -> String {
    use crate::model::document::DocFormat;
    // The key-sign facet (`(B)/(Q)/(D)/(-)`) is no longer surfaced in the KIND
    // column — it reads as the word `Sign:` in the detail popup instead. The
    // column shows only the fixed-pitch 8-column type/notation slot.
    // A YAML opaque (out-of-subset) node is read-only — tag it `[opaq ]`
    // regardless of its underlying kind. (JSONC `/* */` block comments are also
    // read-only but stay `[C]`, so gate on the YAML format.)
    if read_only && doc == DocFormat::Yaml {
        return format!("{:<8}", "[opaq ]");
    }
    let slot: &str = match kind {
        NodeKind::Root => "[G]",
        NodeKind::Comment(_) => "[C]",
        NodeKind::Array => match (doc, format) {
            (DocFormat::Yaml, Format::Block) => "[A/B]",
            (DocFormat::Yaml, _) => "[A/F]", // inline flow sequence
            (_, Format::Multiline) => "[A/M]",
            _ => "[A/I]",
        },
        NodeKind::ArrayOfTables => "[A/T]",
        NodeKind::InlineTable => match doc {
            DocFormat::Yaml => "[T/F]",
            _ => "[T/I]",
        },
        NodeKind::Table => match (doc, format) {
            (DocFormat::Yaml, Format::Block) => "[T/B]",
            (DocFormat::Yaml, _) => "[T/F]",
            // JSON has no scope table: an object is inline `[T/I]` or multiline `[T/M]`.
            (DocFormat::Json, Format::Multiline) => "[T/M]",
            (DocFormat::Json, _) => "[T/I]",
            (_, Format::Dotted) => "[T/D]",
            (_, Format::Multiline) => "[T/M]",
            _ => "[T/S]",
        },
        NodeKind::Scalar(st) => match (st, format) {
            (ScalarType::String, Format::MultilineBasic) => "[S:mstr]",
            (ScalarType::String, Format::Literal) => "[S:lit ]",
            (ScalarType::String, Format::MultilineLiteral) => "[S:mlit]",
            (ScalarType::String, Format::SingleQuoted) => "[S:sq  ]",
            (ScalarType::String, Format::DoubleQuoted) => "[S:dq  ]",
            (ScalarType::String, Format::LiteralBlock) => "[S:lit ]",
            (ScalarType::String, Format::Folded) => "[S:fold]",
            (ScalarType::String, _) => "[S:str ]",
            (ScalarType::Integer, Format::Hex) => "[I:hex ]",
            (ScalarType::Integer, Format::Octal) => "[I:oct ]",
            (ScalarType::Integer, Format::Binary) => "[I:bin ]",
            (ScalarType::Integer, _) => "[I:dec ]",
            (ScalarType::Float, Format::Inf) => "[F:inf ]",
            (ScalarType::Float, Format::Nan) => "[F:nan ]",
            (ScalarType::Float, Format::Exponent) => "[F:exp ]",
            (ScalarType::Float, _) => "[F:flt ]",
            (ScalarType::Bool, _) => "[B:bool]",
            (ScalarType::Null, _) => "[S:null]",
            (ScalarType::OffsetDatetime, _) => "[D:odt ]",
            (ScalarType::LocalDatetime, _) => "[D:ldt ]",
            (ScalarType::LocalDate, _) => "[D:ldat]",
            (ScalarType::LocalTime, _) => "[D:ltim]",
        },
    };
    format!("{slot:<8}")
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

    /// root > [port (bare decimal int), host (bare basic string)].
    fn typed_sample() -> App {
        let mut port = Node::leaf("port", NodeKind::Scalar(ScalarType::Integer));
        port.path = vec![Seg::Key("port".into())];
        port.key_sign = KeySign::Bare;
        let mut host = Node::leaf("host", NodeKind::Scalar(ScalarType::String));
        host.path = vec![Seg::Key("host".into())];
        host.key_sign = KeySign::Bare;
        let mut root = Node::branch("f.toml", NodeKind::Root);
        root.children = vec![port, host];
        App::from_tree(NodeTree { root })
    }

    #[test]
    fn combined_text_and_type_filter_intersect() {
        use crate::tui::type_filter::TypeToken;
        let port_path: Path = vec![Seg::Key("port".into())];
        let host_path: Path = vec![Seg::Key("host".into())];

        // Neither filter active -> no filtering.
        let mut app = typed_sample();
        app.recompute_filter();
        assert!(app.filtered_paths.is_none());

        // Type only: integers -> keep `port` (+ root ancestor), drop `host`.
        let mut app = typed_sample();
        app.type_filter.types.insert(TypeToken::IntDec);
        app.recompute_filter();
        let fp = app.filtered_paths.clone().unwrap();
        assert!(fp.contains(&port_path));
        assert!(!fp.contains(&host_path));
        assert!(fp.contains(&Vec::<Seg>::new()), "root ancestor kept");

        // Text only: "host" -> keep `host`, drop `port`.
        let mut app = typed_sample();
        app.filter = "host".into();
        app.recompute_filter();
        let fp = app.filtered_paths.clone().unwrap();
        assert!(fp.contains(&host_path));
        assert!(!fp.contains(&port_path));

        // AND: text "port" + type string -> intersection is empty (no leaf passes).
        let mut app = typed_sample();
        app.filter = "port".into();
        app.type_filter.types.insert(TypeToken::StrBasic);
        app.recompute_filter();
        let fp = app.filtered_paths.clone().unwrap();
        assert!(!fp.contains(&port_path));
        assert!(!fp.contains(&host_path));
    }

    #[test]
    fn type_tag_is_fixed_pitch() {
        use crate::model::document::DocFormat::{Json, Toml, Yaml};
        // The key-sign facet is no longer part of the tag; the column is the
        // 8-column type/notation slot only.
        let cases = [
            (NodeKind::Root, Format::Plain, Toml, false, "[G]     "),
            (
                NodeKind::Comment("# c".into()),
                Format::Plain,
                Toml,
                false,
                "[C]     ",
            ),
            (NodeKind::Array, Format::Inline, Toml, false, "[A/I]   "),
            (NodeKind::Array, Format::Multiline, Toml, false, "[A/M]   "),
            (
                NodeKind::ArrayOfTables,
                Format::Plain,
                Toml,
                false,
                "[A/T]   ",
            ),
            (
                NodeKind::InlineTable,
                Format::Inline,
                Toml,
                false,
                "[T/I]   ",
            ),
            (NodeKind::Table, Format::Scope, Toml, false, "[T/S]   "),
            (NodeKind::Table, Format::Dotted, Toml, false, "[T/D]   "),
            (
                NodeKind::Scalar(ScalarType::String),
                Format::MultilineLiteral,
                Toml,
                false,
                "[S:mlit]",
            ),
            (
                NodeKind::Scalar(ScalarType::Float),
                Format::Inf,
                Toml,
                false,
                "[F:inf ]",
            ),
            (
                NodeKind::Scalar(ScalarType::LocalDate),
                Format::Plain,
                Toml,
                false,
                "[D:ldat]",
            ),
            // YAML-specific tags.
            (NodeKind::Table, Format::Block, Yaml, false, "[T/B]   "),
            (NodeKind::Table, Format::Inline, Yaml, false, "[T/F]   "),
            (
                NodeKind::InlineTable,
                Format::Inline,
                Yaml,
                false,
                "[T/F]   ",
            ),
            (NodeKind::Array, Format::Block, Yaml, false, "[A/B]   "),
            (NodeKind::Array, Format::Inline, Yaml, false, "[A/F]   "),
            (
                NodeKind::Scalar(ScalarType::String),
                Format::SingleQuoted,
                Yaml,
                false,
                "[S:sq  ]",
            ),
            (
                NodeKind::Scalar(ScalarType::String),
                Format::DoubleQuoted,
                Yaml,
                false,
                "[S:dq  ]",
            ),
            (
                NodeKind::Scalar(ScalarType::String),
                Format::Folded,
                Yaml,
                false,
                "[S:fold]",
            ),
            // A read-only YAML opaque node tags `[opaq ]` whatever its kind.
            (
                NodeKind::Scalar(ScalarType::String),
                Format::Plain,
                Yaml,
                true,
                "[opaq ] ",
            ),
            // The opaque gate is YAML-only: a read-only JSONC block comment
            // still renders `[C]`, not `[opaq ]`.
            (
                NodeKind::Comment("/* x */".into()),
                Format::Plain,
                Json,
                true,
                "[C]     ",
            ),
            // JSON has no scope table: an inline object is `[T/I]`, multiline `[T/M]`.
            (NodeKind::Table, Format::Inline, Json, false, "[T/I]   "),
            (NodeKind::Table, Format::Multiline, Json, false, "[T/M]   "),
        ];
        for (kind, fmt, doc, read_only, expected) in cases {
            let tag = type_tag(&kind, fmt, doc, read_only);
            assert_eq!(tag, expected);
            assert_eq!(tag.chars().count(), 8, "tag must be 8 cols: {tag:?}");
        }
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
    fn expand_level_reveals_one_depth_per_press() {
        // Nested headers: a > { p, b > { q, c > { r } } }.
        let mut app = app_with("[a]\np = 1\n[a.b]\nq = 2\n[a.b.c]\nr = 3\n");
        // visible_keys()[0] is the root (temp-file name); compare the rest.
        let below = |app: &App| app.visible_keys()[1..].to_vec();
        app.collapse_all();
        app.rebuild_rows();
        assert_eq!(below(&app), vec!["a"]);
        app.cursor = app.visible_keys().iter().position(|k| k == "a").unwrap();
        app.expand_level();
        assert_eq!(below(&app), vec!["a", "p", "b"]);
        app.expand_level();
        assert_eq!(below(&app), vec!["a", "p", "b", "q", "c"]);
        app.expand_level();
        assert_eq!(below(&app), vec!["a", "p", "b", "q", "c", "r"]);
        // Fully open: another press is a no-op and the cursor stays on `a`.
        let before = below(&app);
        app.expand_level();
        assert_eq!(below(&app), before);
        assert_eq!(app.rows[app.cursor].key, "a");
    }

    #[test]
    fn collapse_level_in_place_on_open_branch_else_ascends() {
        let mut app = app_with("[a]\np = 1\n[a.b]\nq = 2\n");
        let below = |app: &App| app.visible_keys()[1..].to_vec();
        app.collapse_all();
        app.rebuild_rows();
        app.cursor = app.visible_keys().iter().position(|k| k == "a").unwrap();
        app.expand_level();
        assert_eq!(below(&app), vec!["a", "p", "b"]);
        // Cursor on the open branch `a` -> collapse in place, cursor stays.
        app.collapse_level();
        assert_eq!(below(&app), vec!["a"]);
        assert_eq!(app.rows[app.cursor].key, "a");
        // Reopen, drop cursor on leaf `p` -> collapse ascends to parent `a`.
        app.expand_level();
        app.cursor = app.visible_keys().iter().position(|k| k == "p").unwrap();
        app.collapse_level();
        assert_eq!(below(&app), vec!["a"]);
        assert_eq!(app.rows[app.cursor].key, "a");
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
    fn external_edit_fragment_is_clean_node_text() {
        // CST backend: a `[t]` opened in `$EDITOR` is just the table's own section
        // text — no leading blank, and no adjacent comment (comments are independent
        // nodes now, edited on their own row).
        let app = app_with("a = 1\n\n# c\n[t]\nx = 1\n");
        let doc = app.doc.as_ref().unwrap();
        let frag = doc.serialize_fragment(&[Seg::Key("t".into())]);
        assert!(
            !frag.starts_with('\n'),
            "fragment must not open with a blank line: {frag:?}"
        );
        assert!(
            frag.starts_with("[t]"),
            "should start at the header: {frag:?}"
        );
        assert!(
            !frag.contains("# c"),
            "comment must not be carried: {frag:?}"
        );
    }

    #[test]
    fn external_edit_fragment_does_not_carry_leading_comment() {
        // CST backend: a scalar's `$EDITOR` fragment is the entry line alone; its
        // adjacent comment is an independent node and is not pulled in.
        let app = app_with("a = 1\n\n# note\nport = 8080\n");
        let doc = app.doc.as_ref().unwrap();
        let frag = doc.serialize_fragment(&[Seg::Key("port".into())]);
        assert_eq!(frag, "port = 8080\n", "got: {frag:?}");
    }

    // --- e/n apply-path tests (post-editor logic, no $EDITOR spawned) ---

    fn app_with(src: &str) -> App {
        use crate::model::document::ConfigDocument;
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(src.as_bytes()).unwrap();
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::load(f.path()).unwrap(),
        );
        App::new(doc)
    }

    #[test]
    fn kind_switch_converts_scalar_via_popup() {
        let mut app = app_with("a = \"42\"\nb = 1\n");
        app.cursor = app.rows.iter().position(|r| r.key == "a").unwrap();
        app.open_kind_switch();
        let Mode::KindSwitch(st) = &app.mode else {
            panic!("popup should be open");
        };
        // A basic string offers the other three string notations.
        assert_eq!(st.options[0].0, "literal string  '…'");
        assert_eq!(st.options.len(), 3);
        app.kind_switch_commit();
        assert_eq!(app.doc.as_ref().unwrap().serialize(), "a = '42'\nb = 1\n");
        assert!(matches!(app.mode, Mode::Normal));
    }

    #[test]
    fn convert_flow_opens_only_on_root() {
        let mut app = app_with("a = 1\n");
        // On a non-root node it refuses.
        app.cursor = app.rows.iter().position(|r| r.key == "a").unwrap();
        app.open_convert();
        assert!(matches!(app.mode, Mode::Normal));
        assert!(app.error.as_deref().unwrap_or("").contains("root"));
        // On the root node it opens with the other two formats offered.
        app.error = None;
        app.cursor = app.rows.iter().position(|r| r.path.is_empty()).unwrap();
        app.open_convert();
        let Mode::Convert(st) = &app.mode else {
            panic!("convert flow should be open");
        };
        assert_eq!(st.options.len(), 2);
        assert!(!st
            .options
            .contains(&crate::model::document::DocFormat::Toml));
    }

    #[test]
    fn convert_flow_writes_target_file() {
        let mut app = app_with("a = 1\nb = \"x\"\n");
        let out = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        app.source_path = Some(out.path().to_path_buf());
        app.cursor = app.rows.iter().position(|r| r.path.is_empty()).unwrap();
        app.open_convert();
        // Pick JSON.
        if let Mode::Convert(st) = &mut app.mode {
            st.cursor = st
                .options
                .iter()
                .position(|f| *f == crate::model::document::DocFormat::Json)
                .unwrap();
        }
        app.convert_pick_format();
        // Path step seeded with the .json extension.
        if let Mode::Convert(st) = &app.mode {
            assert!(st.path.ends_with(".json"));
        } else {
            panic!("should be on the path step");
        }
        // Overwrite the path with the temp file and run (lossless → writes at once).
        if let Mode::Convert(st) = &mut app.mode {
            st.path = out.path().to_string_lossy().into_owned();
        }
        app.convert_run();
        assert!(matches!(app.mode, Mode::Normal));
        let written = std::fs::read_to_string(out.path()).unwrap();
        assert_eq!(written, "{\n  \"a\": 1,\n  \"b\": \"x\"\n}\n");
        // The open document is untouched.
        assert_eq!(app.doc.as_ref().unwrap().serialize(), "a = 1\nb = \"x\"\n");
    }

    #[test]
    fn convert_flow_lossy_requires_confirm() {
        let mut app = app_with("n = 0xFF\n");
        let out = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        app.cursor = app.rows.iter().position(|r| r.path.is_empty()).unwrap();
        app.open_convert();
        if let Mode::Convert(st) = &mut app.mode {
            st.cursor = st
                .options
                .iter()
                .position(|f| *f == crate::model::document::DocFormat::Json)
                .unwrap();
        }
        app.convert_pick_format();
        if let Mode::Convert(st) = &mut app.mode {
            st.path = out.path().to_string_lossy().into_owned();
        }
        app.convert_run();
        // A lossy conversion stops at the confirm step (warning shown, no write yet).
        let Mode::Convert(st) = &app.mode else {
            panic!("should pause at confirm");
        };
        assert!(matches!(st.step, crate::tui::state::ConvertStep::Confirm));
        assert!(st.warnings.iter().any(|w| w.contains("non-decimal")));
        app.convert_confirm();
        assert!(matches!(app.mode, Mode::Normal));
        assert_eq!(
            std::fs::read_to_string(out.path()).unwrap(),
            "{\n  \"n\": 255\n}\n"
        );
    }

    #[test]
    fn kind_switch_rejects_bool_scalar() {
        let mut app = app_with("a = true\n");
        app.cursor = app.rows.iter().position(|r| r.key == "a").unwrap();
        app.open_kind_switch();
        assert!(matches!(app.mode, Mode::Normal), "popup must not open");
        assert!(app.error.as_deref().unwrap_or("").contains("cannot"));
    }

    #[test]
    fn kind_switch_rejects_non_convertible_node() {
        let mut app = app_with("# c\na = 1\n");
        app.cursor = app
            .rows
            .iter()
            .position(|r| r.key.starts_with('#'))
            .unwrap();
        app.open_kind_switch();
        assert!(matches!(app.mode, Mode::Normal), "popup must not open");
        assert!(app.error.as_deref().unwrap_or("").contains("cannot"));
    }

    #[test]
    fn cut_paste_whole_dotted_table_into_scope() {
        // End-to-end TUI path: cut a whole `[T/D]` table and paste it into a scope —
        // routes through Mutation::Move's member fan-out.
        let mut app = app_with("a.x = 1\na.y = 2\n[dest]\nz = 0\n");
        for _ in 0..8 {
            app.expand_level();
        }
        app.rebuild_rows();
        let ai = app.rows.iter().position(|r| r.key == "a").unwrap();
        app.cursor = ai;
        app.cut_selected();
        let di = app.rows.iter().position(|r| r.key == "dest").unwrap();
        app.paste_slot = Some(crate::tui::app::PasteSlot::Into(di));
        app.cursor = di;
        app.paste();
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "[dest]\nz = 0\na.x = 1\na.y = 2\n",
            "status={:?}",
            app.status
        );
    }

    #[test]
    fn dotted_tables_load_collapsed() {
        // `a.b.c = 1` nests into `a → b → c`; like any branch, `[T/D]` tables start
        // collapsed, so only the top `a` shows until expanded.
        let app = app_with("a.b.c = 1\n");
        // [0] is the (temp-file) root key; the dotted table `a` follows, collapsed.
        assert_eq!(&app.visible_keys()[1..], &["a"]);
    }

    /// First visible row whose projected node is a Comment (identified by kind —
    /// a comment's `Seg::Index` path is indistinguishable from an array element's).
    fn comment_row(app: &App) -> usize {
        app.rows
            .iter()
            .position(|r| {
                app.tree
                    .node_at(&r.path)
                    .map(|n| matches!(n.kind, NodeKind::Comment(_)))
                    .unwrap_or(false)
            })
            .unwrap()
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
        assert!(app.error.is_some(), "invalid comment must surface in error");
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
        app.apply_replace(vec![Seg::Key("port".into())], "port = = nope".into());
        assert!(app.error.is_some(), "invalid TOML must surface in error");
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
        assert!(app.error.is_some(), "collision must surface in error");
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            before,
            "doc unchanged"
        );
    }

    #[test]
    fn apply_insert_invalid_toml_sets_status_and_leaves_doc() {
        // §10 rejection path for `n`: invalid fragment -> Fragment -> error, no change.
        let mut app = app_with("port = 8080\n");
        let before = app.doc.as_ref().unwrap().serialize();
        app.apply_insert(
            crate::model::document::Target {
                parent: vec![],
                index: 1,
            },
            "= = nope".into(),
        );
        assert!(app.error.is_some(), "invalid TOML must surface in error");
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

    #[test]
    fn pure_json_remark_prompts_then_upgrades() {
        use crate::model::document::ConfigDocument;
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::with_suffix(".json").unwrap();
        f.write_all(b"{\n  \"a\": 1\n}\n").unwrap();
        let doc = crate::model::any_doc::AnyDocument::load_as(
            f.path(),
            crate::model::document::DocFormat::Json,
        )
        .unwrap();
        let mut app = App::new(doc);

        // Expand the root so "a" appears as a row, then position the cursor on it.
        app.expand_level();
        app.rebuild_rows();
        let ai = app.rows.iter().position(|r| r.key == "a").unwrap();
        app.cursor = ai;

        // Remark on a live node in a pure .json must show the JSONC-upgrade prompt.
        app.remark();
        assert!(
            matches!(app.mode, Mode::Prompt(PromptKind::JsoncUpgrade { .. })),
            "expected JsoncUpgrade prompt, got {:?}",
            std::mem::discriminant(&app.mode)
        );
        // Document must be unchanged at this point.
        assert!(
            !app.doc.as_ref().unwrap().is_dirty(),
            "doc must be clean while prompt is pending"
        );

        // Confirm with 'y' — should enable comments and apply the remark.
        app.handle_prompt_key('y');
        let s = app.doc.as_ref().unwrap().serialize();
        assert!(
            s.contains("//"),
            "after upgrade the serialized output must contain a // comment: {s:?}"
        );
        assert!(
            app.doc.as_ref().unwrap().supports_comments(),
            "doc must now support comments"
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
            false,
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
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::load(&path).unwrap(),
        );
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
        // a [table] branch → external
        app.cursor = idx_of(&app, "server");
        assert_eq!(app.edit_target_kind(), EditKind::External);
        // a single-line array / inline table → inline (edited as its one-line repr)
        app.cursor = idx_of(&app, "arr");
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        app.cursor = idx_of(&app, "pt");
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        // scalar element directly in a top-level array → inline
        app.cursor = idx_of(&app, "[0]");
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        // scalar member of an inline table → inline (value Replace + key Rename
        // both address it via an all-`Key` path)
        app.cursor = idx_of(&app, "y");
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
    }

    #[test]
    fn single_line_array_and_inline_show_value_multiline_does_not() {
        let app = app_with("arr = [1, 2]\npt = { x = 1 }\nml = [\n  1,\n]\n");
        let arr = app.rows.iter().find(|r| r.key == "arr").unwrap();
        assert_eq!(arr.value.as_deref(), Some("[1, 2]"));
        let pt = app.rows.iter().find(|r| r.key == "pt").unwrap();
        assert_eq!(pt.value.as_deref(), Some("{ x = 1 }"));
        let ml = app.rows.iter().find(|r| r.key == "ml").unwrap();
        assert_eq!(ml.value, None, "multiline array carries no one-line value");
    }

    #[test]
    fn structured_array_element_edits_inline() {
        // #2: a single-line array / inline table that is an array *element* (not
        // top-level) is inline-editable, not pushed to $EDITOR.
        let mut app = app_with("aa = [[1, 2]]\nai = [{ a = 1 }]\n");
        app.expand_all();
        app.rebuild_rows();
        let p_arr = vec![Seg::Key("aa".into()), Seg::Index(0)];
        let p_inl = vec![Seg::Key("ai".into()), Seg::Index(0)];
        app.cursor = app.rows.iter().position(|r| r.path == p_arr).unwrap();
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        app.cursor = app.rows.iter().position(|r| r.path == p_inl).unwrap();
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
        let frag = doc.serialize_fragment(&[Seg::Key("product".into()), Seg::Index(1)]);
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
    fn edit_target_kind_array_of_inline_tables_scalar_is_inline() {
        // Group B: a scalar member of an inline table that is an array element
        // (`items[0].a`) IS `Replace`-addressable (the projection indexes the
        // member; the splice rebuilds the `{ … }` in place), so it edits inline.
        let mut app = app_with("items = [{ a = 1 }]\n");
        app.expand_all();
        app.rebuild_rows();
        let pos = app.rows.iter().position(|r| r.key == "a").unwrap();
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
    fn edit_cancel_clears_staged_trailing_comment() {
        // A staged trailing-comment change must not survive a cancelled edit, or a
        // later nudge/replace would stamp it onto an unrelated node.
        let mut app = app_with("port = 8080\ncount = 5\n");
        app.pending_trailing = Some(Some("# leak".into()));
        app.edit_cancel();
        assert!(app.pending_trailing.is_none());
        // A subsequent nudge writes only the value, no stray comment.
        app.cursor = 1;
        app.nudge(1);
        let out = app.doc.as_ref().unwrap().serialize();
        assert!(
            !out.contains("# leak"),
            "stale comment leaked into nudge: {out}"
        );
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
    fn inline_rename_to_dotted_confirms_and_converts_to_table() {
        // Editing a scalar's Name to `foo.x` asks "integer → table"; `y` applies the
        // rename, turning it into a `[T/D]` table (issue 4).
        use crate::tui::state::EditField;
        let mut app = app_with("foo = 1\n");
        app.cursor = 1;
        app.begin_inline_edit();
        app.edit_toggle_field();
        assert!(matches!(&app.mode, Mode::Edit(e) if e.field == EditField::Name));
        for c in ".x".chars() {
            app.edit_input_char(c); // "foo" -> "foo.x"
        }
        app.edit_commit();
        assert!(
            matches!(app.mode, Mode::Prompt(PromptKind::TypeChange { .. })),
            "dotted rename must confirm the type change"
        );
        app.handle_prompt_key('y');
        assert!(matches!(app.mode, Mode::Normal));
        assert_eq!(app.doc.as_ref().unwrap().serialize(), "foo.x = 1\n");
    }

    #[test]
    fn inline_rename_to_dotted_cancel_leaves_doc_untouched() {
        let mut app = app_with("foo = 1\n");
        app.cursor = 1;
        app.begin_inline_edit();
        app.edit_toggle_field();
        for c in ".x".chars() {
            app.edit_input_char(c);
        }
        app.edit_commit();
        app.handle_prompt_key('n'); // decline
        assert_eq!(app.doc.as_ref().unwrap().serialize(), "foo = 1\n");
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
    fn add_on_collapsed_table_adds_sibling_table() {
        // idea 3 / idea 5: `a` on a collapsed `[t]` adds a sibling `[placeholder]`
        // (a scalar there would be captured by `[t]`), no inline edit.
        let mut app = app_with("[t]\nx = 1\n");
        app.cursor = app.rows.iter().position(|r| r.key == "t").unwrap(); // collapsed
        app.add_node();
        assert!(
            !matches!(app.mode, Mode::Edit(_)),
            "structured add: no inline"
        );
        let s = app.doc.as_ref().unwrap().serialize();
        assert!(s.contains("[placeholder]"), "serialize: {s:?}");
        // It is a sibling of [t], not nested inside it.
        assert!(s.contains("[t]") && s.contains("[placeholder]"));
    }

    #[test]
    fn add_on_collapsed_dotted_table_adds_table_sibling() {
        // Same-kind model: `a` on a `[T/D]` table (a Table node) adds a sibling
        // table — an empty `[placeholder]` scope table, no inline edit.
        // `[T/D]` tables start collapsed, so `a` is a collapsed branch.
        let mut app = app_with("a.b = 1\n");
        app.cursor = app.rows.iter().position(|r| r.key == "a").unwrap();
        app.add_node();
        assert!(!matches!(app.mode, Mode::Edit(_)), "table add: no inline");
        let s = app.doc.as_ref().unwrap().serialize();
        assert!(s.contains("[placeholder]"), "serialize: {s:?}");
    }

    #[test]
    fn add_on_collapsed_array_adds_array_sibling() {
        // Same-kind model: `a` on a collapsed array adds an empty array sibling
        // right after it in the same scope — no stray scalar two rows up.
        let mut app = app_with("nums = [1, 2]\nname = \"x\"\n");
        app.cursor = app.rows.iter().position(|r| r.key == "nums").unwrap();
        app.add_node();
        assert!(!matches!(app.mode, Mode::Edit(_)), "array add: no inline");
        let s = app.doc.as_ref().unwrap().serialize();
        assert_eq!(s, "nums = [1, 2]\nplaceholder = []\nname = \"x\"\n");
    }

    #[test]
    fn add_on_toml_array_element_seeds_keyless_bare() {
        // Item 1: adding beside an array *element* seeds a keyless bare scalar
        // (`""`), not a `{ __elem__ = "" }` inline table — uniform with JSON/YAML.
        let mut app = app_with("nums = [1, 2]\n");
        app.expanded.insert(vec![Seg::Key("nums".into())]);
        app.rebuild_rows();
        app.cursor = app
            .rows
            .iter()
            .position(|r| r.path == vec![Seg::Key("nums".into()), Seg::Index(0)])
            .unwrap();
        app.add_node();
        assert!(matches!(app.mode, Mode::Edit(_)), "scalar element: inline");
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "nums = [1, \"\", 2]\n"
        );
    }

    #[test]
    fn esc_after_add_rolls_the_insert_back() {
        // Item 2a: `a` opens the inline editor on a seed; Esc removes the seed and
        // leaves the document (and undo history) exactly as before the add.
        let mut app = app_with("a = 1\nb = 2\n");
        app.cursor = app.rows.iter().position(|r| r.key == "a").unwrap();
        app.add_node();
        assert!(matches!(app.mode, Mode::Edit(_)));
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "a = 1\nnew_field = \"\"\nb = 2\n"
        );
        app.edit_cancel();
        assert!(!matches!(app.mode, Mode::Edit(_)));
        assert_eq!(app.doc.as_ref().unwrap().serialize(), "a = 1\nb = 2\n");
        // No undo/redo crumb: the add never happened.
        app.undo();
        assert_eq!(app.doc.as_ref().unwrap().serialize(), "a = 1\nb = 2\n");
    }

    #[test]
    fn esc_after_normal_edit_keeps_node() {
        // A cancelled edit of an *existing* node leaves it intact (created_on_add
        // is false), so the rollback path must not fire.
        let mut app = app_with("a = 1\nb = 2\n");
        app.cursor = app.rows.iter().position(|r| r.key == "a").unwrap();
        app.begin_inline_edit();
        app.edit_cancel();
        assert_eq!(app.doc.as_ref().unwrap().serialize(), "a = 1\nb = 2\n");
    }

    #[test]
    fn add_on_scalar_leaf_adds_scalar_sibling_after() {
        // `a` on a scalar leaf adds an empty-string scalar sibling immediately
        // after it (inline edit), in the same scope.
        let mut app = app_with("a = 1\nb = 2\n");
        app.cursor = app.rows.iter().position(|r| r.key == "a").unwrap();
        app.add_node();
        assert!(matches!(app.mode, Mode::Edit(_)), "scalar add opens inline");
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "a = 1\nnew_field = \"\"\nb = 2\n"
        );
    }

    #[test]
    fn add_on_expanded_table_appends_scalar_child() {
        // idea 3: `a` on an expanded `[t]` appends a scalar as its last child.
        let mut app = app_with("[t]\nx = 1\n");
        app.expanded.insert(vec![Seg::Key("t".into())]);
        app.rebuild_rows();
        app.cursor = app.rows.iter().position(|r| r.key == "t").unwrap();
        app.add_node();
        assert!(matches!(app.mode, Mode::Edit(_)), "scalar add opens inline");
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "[t]\nx = 1\nnew_field = \"\"\n"
        );
    }

    #[test]
    fn add_root_scalar_lands_before_first_table() {
        // D5 clamp: `a` on the root appends a scalar, clamped to before `[t]`.
        let mut app = app_with("a = 1\n[t]\nx = 1\n");
        app.cursor = 0; // root
        app.add_node();
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "a = 1\nnew_field = \"\"\n[t]\nx = 1\n"
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
    fn detail_path_includes_array_index() {
        let mut app = app_with("hosts = [\n  \"a\",\n  \"b\",\n]\n");
        // Expand the array branch so its elements are flattened into rows.
        app.expanded.insert(vec![Seg::Key("hosts".into())]);
        app.rebuild_rows();
        app.cursor = 3; // hosts[1] = "b" (root=0, hosts=1, [0]=2, [1]=3)
        app.open_detail();
        let detail = app.detail_text.as_ref().expect("detail should be set");
        assert!(
            detail.contains("hosts[1]"),
            "detail path should include the element index, got: {detail}"
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
    fn paste_slots_interleave_into_then_after() {
        let mut app = app_with("a = 1\n[t]\nx = 1\n");
        app.expanded.insert(vec![Seg::Key("t".into())]);
        app.rebuild_rows();
        // rows: 0 root(branch), 1 a(leaf), 2 [t](branch), 3 t.x(leaf)
        assert_eq!(
            app.paste_slots(),
            vec![
                PasteSlot::Into(0),
                PasteSlot::After(0),
                PasteSlot::After(1),
                PasteSlot::Into(2),
                PasteSlot::After(2),
                PasteSlot::After(3),
            ]
        );
    }

    #[test]
    fn default_paste_slot_is_after_cursor() {
        let mut app = app_with("a = 1\nb = 2\n");
        app.cursor = 1;
        assert_eq!(app.effective_paste_slot(), PasteSlot::After(1));
    }

    #[test]
    fn into_slot_targets_last_child_of_branch() {
        let mut app = app_with("[t]\nx = 1\ny = 2\n");
        app.expanded.insert(vec![Seg::Key("t".into())]);
        app.rebuild_rows();
        // rows: 0 root, 1 [t], 2 t.x, 3 t.y
        let target = app.slot_target(PasteSlot::Into(1)).unwrap();
        assert_eq!(target.parent, vec![Seg::Key("t".into())]);
        assert_eq!(target.index, 2, "append after both existing children");
    }

    #[test]
    fn paste_navigation_steps_slots_and_syncs_cursor() {
        let mut app = app_with("a = 1\nb = 2\n");
        // rows: 0 root, 1 a, 2 b → slots [Into(0),After(0),After(1),After(2)]
        app.cursor = 0;
        app.clipboard = Some(Clipboard {
            fragments: vec!["c = 3\n".into()],
            cut: false,
            sources: vec![vec![Seg::Key("a".into())]],
        });
        assert_eq!(app.effective_paste_slot(), PasteSlot::After(0));
        app.cursor_down();
        assert_eq!(app.effective_paste_slot(), PasteSlot::After(1));
        assert_eq!(app.cursor, 1, "cursor follows the slot's row");
        app.cursor_up();
        assert_eq!(app.effective_paste_slot(), PasteSlot::After(0));
    }

    #[test]
    fn paste_into_collapsed_branch_appends_as_child() {
        // [t] collapsed; paste with the Into(t) slot lands inside it (idea 2),
        // not as a top-level sibling.
        let mut app = app_with("[t]\nx = 1\n");
        // rows: 0 root, 1 [t] (collapsed by default)
        app.cursor = 1;
        app.clipboard = Some(Clipboard {
            fragments: vec!["y = 9\n".into()],
            cut: false,
            sources: vec![vec![Seg::Key("y".into())]],
        });
        app.paste_slot = Some(PasteSlot::Into(1));
        app.paste();
        assert!(app.status.is_none(), "unexpected status: {:?}", app.status);
        let s = app.doc.as_ref().unwrap().serialize();
        // y must live under [t], after x.
        let t = s.find("[t]").unwrap();
        let y = s.find("y = 9").unwrap();
        assert!(y > t, "y must be inside [t]: {s:?}");
    }

    #[test]
    fn paste_scalar_after_table_rejected_preserves_clipboard() {
        // D5/D4: pasting a scalar into the root slot *after* a table is illegal
        // (would be captured by the table). The paste must fail non-destructively.
        let mut app = app_with("a = 1\n[t]\nx = 1\n");
        // rows: 0 root, 1 a, 2 [t] (collapsed)
        app.clipboard = Some(Clipboard {
            fragments: vec!["z = 9\n".into()],
            cut: false,
            sources: vec![vec![Seg::Key("a".into())]],
        });
        // Aim the slot at "after [t]" (root append, past the header).
        app.paste_slot = Some(PasteSlot::After(2));
        app.paste();
        assert!(
            app.clipboard.is_some(),
            "clipboard must survive an illegal paste"
        );
        assert!(
            app.error.as_deref().unwrap_or("").contains("paste error"),
            "error: {:?}",
            app.error
        );
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "a = 1\n[t]\nx = 1\n",
            "document must be untouched"
        );
    }

    #[test]
    fn cut_comment_paste_into_multiline_array_moves_it() {
        // #6e: a cut comment can be moved into a multiline array (append slot).
        let mut app = app_with("# top\narr = [\n  1,\n  2,\n]\n");
        let crow = comment_row(&app);
        app.cursor = crow;
        app.cut_selected();
        let arow = app.rows.iter().position(|r| r.key == "arr").unwrap();
        app.paste_slot = Some(PasteSlot::Into(arow));
        app.paste();
        assert!(app.status.is_none(), "unexpected status: {:?}", app.status);
        let s = app.doc.as_ref().unwrap().serialize();
        assert_eq!(s, "arr = [\n  1,\n  2,\n  # top\n]\n", "got: {s:?}");
    }

    #[test]
    fn cut_comment_into_single_line_array_prompts_for_upgrade() {
        // A comment pasted into a single-line array no longer errors: it enters
        // the ArrayUpgrade y/n prompt, non-destructively (clipboard + doc intact).
        let mut app = app_with("# note\narr = [1]\n");
        let crow = comment_row(&app);
        app.cursor = crow;
        app.cut_selected();
        let arow = app.rows.iter().position(|r| r.key == "arr").unwrap();
        app.paste_slot = Some(PasteSlot::Into(arow));
        app.paste();
        assert!(
            matches!(app.mode, Mode::Prompt(PromptKind::ArrayUpgrade { .. })),
            "should prompt for the multiline upgrade"
        );
        assert!(app.clipboard.is_some(), "clipboard must be kept");
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "# note\narr = [1]\n",
            "nothing mutated before confirmation"
        );

        // 'n' cancels: clipboard kept, document untouched, back to Normal.
        app.handle_prompt_key('n');
        assert!(matches!(app.mode, Mode::Normal));
        assert!(app.clipboard.is_some(), "clipboard survives a 'n'");
        assert_eq!(app.doc.as_ref().unwrap().serialize(), "# note\narr = [1]\n");

        // Retry and confirm with 'y': the array upgrades to multiline, the
        // comment lands inside, and the cut source is deleted.
        app.paste_slot = Some(PasteSlot::Into(arow));
        app.paste();
        assert!(matches!(
            app.mode,
            Mode::Prompt(PromptKind::ArrayUpgrade { .. })
        ));
        app.handle_prompt_key('y');
        assert!(matches!(app.mode, Mode::Normal));
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "arr = [\n  1,\n  # note\n]\n",
            "upgrade + insert + cut-source delete"
        );
        assert!(app.clipboard.is_none(), "paste consumed the clipboard");
    }

    #[test]
    fn paste_bare_value_into_table_synthesizes_placeholder_key() {
        // C2 / key+: pasting a bare element value into a Table/Root synthesizes a
        // `placeholder` key instead of erroring.
        let mut app = app_with("a = 1\n");
        app.cursor = 0; // root
        app.clipboard = Some(Clipboard {
            fragments: vec!["42\n".into()],
            cut: false,
            sources: vec![vec![Seg::Key("a".into())]],
        });
        app.paste();
        assert!(app.status.is_none(), "unexpected status: {:?}", app.status);
        let s = app.doc.as_ref().unwrap().serialize();
        assert!(s.contains("placeholder = 42"), "serialize: {s:?}");
    }

    #[test]
    fn cut_paste_same_scope_moves_without_collision() {
        let mut app = app_with("a = 1\nb = 2\n");
        app.rebuild_rows();
        app.cursor = 1; // on `a`
        app.cut_selected();
        assert!(app.clipboard.is_some());
        app.cursor = 2; // on `b`
        app.paste();
        assert!(
            matches!(app.mode, Mode::Normal),
            "no collision prompt expected"
        );
        let out = app.doc.as_ref().unwrap().serialize();
        assert_eq!(out.matches("a =").count(), 1, "exactly one `a`: {out:?}");
        assert_eq!(out.matches("b =").count(), 1, "exactly one `b`: {out:?}");
        assert!(
            app.clipboard.is_none(),
            "clipboard consumed on successful move"
        );
    }

    #[test]
    fn copy_paste_comment_node() {
        let mut app = app_with("# note\na = 1\nb = 2\n");
        app.rebuild_rows();
        let cpos = comment_row(&app);
        app.cursor = cpos;
        app.copy_selected();
        assert!(app.clipboard.is_some());
        let bpos = app
            .rows
            .iter()
            .position(|r| matches!(r.path.last(), Some(Seg::Key(k)) if k == "b"))
            .unwrap();
        app.cursor = bpos;
        app.paste();
        let out = app.doc.as_ref().unwrap().serialize();
        assert_eq!(
            out.matches("# note").count(),
            2,
            "comment now appears twice:\n{out}"
        );
    }

    #[test]
    fn cut_paste_comment_node_moves_it() {
        let mut app = app_with("# note\na = 1\nb = 2\n");
        app.rebuild_rows();
        let cpos = comment_row(&app);
        app.cursor = cpos;
        app.cut_selected();
        let bpos = app
            .rows
            .iter()
            .position(|r| matches!(r.path.last(), Some(Seg::Key(k)) if k == "b"))
            .unwrap();
        app.cursor = bpos;
        app.paste();
        let out = app.doc.as_ref().unwrap().serialize();
        assert_eq!(
            out.matches("# note").count(),
            1,
            "comment moved, not duplicated:\n{out}"
        );
        assert!(
            out.find("# note").unwrap() > out.find("b = 2").unwrap(),
            "comment should be after b:\n{out}"
        );
    }

    #[test]
    fn copy_table_fragment_omits_leading_comment() {
        // A copied `[table]` carries its header decor (which holds the standalone
        // comment above it); the clipboard fragment must drop that comment so a
        // paste does not duplicate it — the comment stays at the source.
        let app = app_with("# hdr\n[srv]\nport = 8080\n");
        let doc = app.doc.as_ref().unwrap();
        let frag = doc.serialize_fragment(&[Seg::Key("srv".into())]);
        assert!(
            !frag.contains("# hdr"),
            "copied table fragment kept the comment: {frag:?}"
        );
        assert!(
            frag.contains("[srv]") && frag.contains("port = 8080"),
            "copied table body lost: {frag:?}"
        );
    }

    #[test]
    fn copy_comment_node_fragment_kept_whole() {
        // A Comment node's fragment *is* the comment text, so the strip must not
        // touch it (copying a comment still copies the comment).
        let app = app_with("# note\na = 1\n");
        let doc = app.doc.as_ref().unwrap();
        let tree = doc.project();
        let cpath = tree
            .root
            .children
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Comment(_)))
            .unwrap()
            .path
            .clone();
        let frag = doc.serialize_fragment(&cpath);
        assert_eq!(frag.trim(), "# note");
    }

    #[test]
    fn cut_string_pastes_below_comment_not_above_it() {
        // Regression (the live-app bug): cut `x`, put the cursor on the comment and
        // paste — paste lands *after* the cursor, so `x` goes between the comment and
        // `y`, NOT above the comment (the old toml_edit decor bug).
        let mut app = app_with("x = 1\n# note\ny = 2\n");
        app.rebuild_rows();
        app.cursor = app
            .rows
            .iter()
            .position(|r| matches!(r.path.last(), Some(Seg::Key(k)) if k == "x"))
            .unwrap();
        app.cut_selected();
        app.cursor = comment_row(&app);
        app.paste();
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "# note\nx = 1\ny = 2\n"
        );
    }

    #[test]
    fn cut_comment_pastes_elsewhere_without_vanishing() {
        // Regression: cutting a comment and pasting it elsewhere must move it, not
        // lose it.
        let mut app = app_with("# note\na = 1\nb = 2\nc = 3\n");
        app.rebuild_rows();
        app.cursor = comment_row(&app);
        app.cut_selected();
        app.cursor = app
            .rows
            .iter()
            .position(|r| matches!(r.path.last(), Some(Seg::Key(k)) if k == "c"))
            .unwrap();
        app.paste();
        let out = app.doc.as_ref().unwrap().serialize();
        assert_eq!(
            out.matches("# note").count(),
            1,
            "comment vanished/duped:\n{out}"
        );
        assert!(
            out.find("# note").unwrap() > out.find("b = 2").unwrap(),
            "comment should have moved down near c:\n{out}"
        );
    }

    #[test]
    fn cut_comment_moves_down_without_overshoot() {
        // Regression: a comment cut from *above* the cursor and pasted lands right
        // after the cursor — deleting the source (above) must not shift the insert
        // one slot too far down (the +1 overshoot bug).
        let mut app = app_with("# c\na = 1\nb = 2\n");
        app.rebuild_rows();
        app.cursor = comment_row(&app);
        app.cut_selected();
        app.cursor = app
            .rows
            .iter()
            .position(|r| matches!(r.path.last(), Some(Seg::Key(k)) if k == "a"))
            .unwrap();
        app.paste();
        assert_eq!(app.doc.as_ref().unwrap().serialize(), "a = 1\n# c\nb = 2\n");
    }

    #[test]
    fn cut_comment_moves_down_without_overshoot_jsonc() {
        let mut app = app_with_jsonc("{\n  // c\n  \"a\": 1,\n  \"b\": 2\n}\n");
        app.rebuild_rows();
        app.cursor = comment_row(&app);
        app.cut_selected();
        app.cursor = app
            .rows
            .iter()
            .position(|r| matches!(r.path.last(), Some(Seg::Key(k)) if k == "a"))
            .unwrap();
        app.paste();
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "{\n  \"a\": 1,\n  // c\n  \"b\": 2\n}\n"
        );
    }

    #[test]
    fn cut_comment_moves_down_without_overshoot_yaml() {
        let mut app = app_with_yaml("# c\na: 1\nb: 2\n");
        app.rebuild_rows();
        app.cursor = comment_row(&app);
        app.cut_selected();
        app.cursor = app
            .rows
            .iter()
            .position(|r| matches!(r.path.last(), Some(Seg::Key(k)) if k == "a"))
            .unwrap();
        app.paste();
        assert_eq!(app.doc.as_ref().unwrap().serialize(), "a: 1\n# c\nb: 2\n");
    }

    #[test]
    fn yaml_cut_multiline_comment_block_moves_whole_block() {
        // Cutting a merged 3-line `#` block from the top and pasting after
        // `multiline_literal` moves the WHOLE block (not just `# 1`) to the new
        // slot and leaves `decimal: 42` intact (no value corruption, no
        // `placeholder:` wrapping).
        let mut app = app_with_yaml(
            "# 1\n# 2\n# 3\nempty_string: \"\"\nmultiline_literal: \"multiline\"\ndecimal: 42\n",
        );
        app.rebuild_rows();
        app.cursor = comment_row(&app);
        app.cut_selected();
        app.cursor = app
            .rows
            .iter()
            .position(|r| matches!(r.path.last(), Some(Seg::Key(k)) if k == "multiline_literal"))
            .unwrap();
        app.paste();
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "empty_string: \"\"\nmultiline_literal: \"multiline\"\n# 1\n# 2\n# 3\ndecimal: 42\n"
        );
    }

    #[test]
    fn yaml_copy_multiline_comment_block_lands_at_target() {
        // Copying the 3-line block and pasting after `multiline_literal` keeps the
        // original at the top and lands the copy right after `multiline_literal`.
        let mut app = app_with_yaml(
            "# 1\n# 2\n# 3\nempty_string: \"\"\nmultiline_literal: \"multiline\"\ndecimal: 42\n",
        );
        app.rebuild_rows();
        app.cursor = comment_row(&app);
        app.copy_selected();
        app.cursor = app
            .rows
            .iter()
            .position(|r| matches!(r.path.last(), Some(Seg::Key(k)) if k == "multiline_literal"))
            .unwrap();
        app.paste();
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "# 1\n# 2\n# 3\nempty_string: \"\"\nmultiline_literal: \"multiline\"\n# 1\n# 2\n# 3\ndecimal: 42\n"
        );
    }

    #[test]
    fn paste_multiple_separate_comments_preserves_order() {
        // Two separate comment fragments pasted together must keep their order
        // (`# A` before `# B`), even though each InsertComment prepends at the slot.
        let mut app = app_with("# A\n\n# B\nx = 1\ny = 2\n");
        app.rebuild_rows();
        // The two top-level comment nodes, by their real (kind-detected) paths.
        let cpaths: Vec<Path> = app
            .rows
            .iter()
            .filter(|r| {
                app.tree
                    .node_at(&r.path)
                    .map(|n| matches!(n.kind, NodeKind::Comment(_)))
                    .unwrap_or(false)
            })
            .map(|r| r.path.clone())
            .collect();
        assert_eq!(cpaths.len(), 2);
        let doc = app.doc.as_ref().unwrap();
        let fragments: Vec<String> = cpaths.iter().map(|p| doc.serialize_fragment(p)).collect();
        app.clipboard = Some(Clipboard {
            fragments,
            cut: false,
            sources: cpaths,
        });
        // Paste onto `y` (so the copies land together, after the originals).
        app.cursor = app
            .rows
            .iter()
            .position(|r| matches!(r.path.last(), Some(Seg::Key(k)) if k == "y"))
            .unwrap();
        app.paste();
        let out = app.doc.as_ref().unwrap().serialize();
        // Each comment now appears twice; the pasted pair keeps A before B.
        assert_eq!(out.matches("# A").count(), 2, "got:\n{out}");
        assert_eq!(out.matches("# B").count(), 2, "got:\n{out}");
        let last_a = out.rfind("# A").unwrap();
        let last_b = out.rfind("# B").unwrap();
        assert!(
            last_a < last_b,
            "expected # A before # B in the paste, got:\n{out}"
        );
    }

    // --- Task 19: read-only guards for block-comment nodes ---

    fn app_with_jsonc(src: &str) -> App {
        use std::io::Write;
        let mut f = tempfile::Builder::new()
            .suffix(".jsonc")
            .tempfile()
            .unwrap();
        f.write_all(src.as_bytes()).unwrap();
        let doc = crate::model::any_doc::AnyDocument::load(f.path()).unwrap();
        App::new(doc)
    }

    fn app_with_yaml(src: &str) -> App {
        use std::io::Write;
        let mut f = tempfile::Builder::new().suffix(".yaml").tempfile().unwrap();
        f.write_all(src.as_bytes()).unwrap();
        let doc = crate::model::any_doc::AnyDocument::load(f.path()).unwrap();
        App::new(doc)
    }

    fn app_with_json(src: &str) -> App {
        use std::io::Write;
        let mut f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        f.write_all(src.as_bytes()).unwrap();
        let doc = crate::model::any_doc::AnyDocument::load(f.path()).unwrap();
        App::new(doc)
    }

    /// Move the cursor to the first row whose path ends in the given key segment,
    /// expanding everything first.
    fn cursor_to_key(app: &mut App, key: &str) {
        app.expand_all();
        app.rebuild_rows();
        app.cursor = app
            .rows
            .iter()
            .position(|r| r.key == key)
            .unwrap_or_else(|| panic!("row {key:?} not found"));
    }

    #[test]
    fn toml_inline_table_array_element_member_edits_inline() {
        // Group B item 2b.3: a member of a `[T/I]` element of an `[A/M]` array is
        // inline-editable and the edit applies in place.
        let mut app = app_with("arr = [\n  { a = 1, b = 2 },\n  { c = 3 },\n]\n");
        cursor_to_key(&mut app, "a");
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        inline_set_value(&mut app, "5");
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "arr = [\n  { a = 5, b = 2 },\n  { c = 3 },\n]\n"
        );
    }

    #[test]
    fn add_member_into_inline_table_array_element() {
        // Group B item 2b.2: `a` on a member of a `[T/I]` array element adds a
        // sibling member inside the same `{ … }` (previously "operation not
        // supported"). The seed opens the inline editor on the new member.
        let mut app = app_with("arr = [\n  { a = 1 },\n]\n");
        cursor_to_key(&mut app, "a");
        app.add_node();
        assert!(matches!(app.mode, Mode::Edit(_)), "member add opens inline");
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "arr = [\n  { a = 1, new_field = \"\" },\n]\n"
        );
    }

    #[test]
    fn toml_inline_table_array_element_edits_inline_keeping_comment() {
        // Group B items 2b.1 + 7: the `[T/I]` element itself edits inline as its
        // one-liner, and its trailing comment survives the edit.
        let mut app = app_with("arr = [\n  { a = 1 },  # note\n]\n");
        app.expanded.insert(vec![Seg::Key("arr".into())]);
        app.rebuild_rows();
        app.cursor = app
            .rows
            .iter()
            .position(|r| r.path == vec![Seg::Key("arr".into()), Seg::Index(0)])
            .unwrap();
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        // The editor seeds the buffer as `value  # comment`; editing the value while
        // keeping the comment must preserve it on commit.
        app.begin_inline_edit();
        let seeded = match &app.mode {
            Mode::Edit(e) => e.buffer.clone(),
            _ => panic!("inline editor open"),
        };
        assert_eq!(seeded, "{ a = 1 }  # note", "comment seeded into buffer");
        if let Mode::Edit(e) = &mut app.mode {
            e.buffer = "{ a = 2 }  # note".to_string();
            e.cursor = e.buffer.chars().count();
        }
        app.edit_commit();
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "arr = [\n  { a = 2 },  # note\n]\n"
        );
    }

    #[test]
    fn json_inline_object_array_element_edits_inline() {
        // Group B item 2c.1: a JSON inline object (`NodeKind::Table`+`Inline`) that
        // is an array element is inline-editable (was forced $EDITOR before).
        let mut app = app_with_json("{\n  \"arr\": [\n    { \"a\": 1, \"b\": 2 }\n  ]\n}\n");
        app.expand_all();
        app.rebuild_rows();
        app.cursor = app
            .rows
            .iter()
            .position(|r| r.path == vec![Seg::Key("arr".into()), Seg::Index(0)])
            .unwrap();
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        // And the one-liner edit applies through Replace.
        inline_set_value(&mut app, "{ \"a\": 1 }");
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "{\n  \"arr\": [\n    { \"a\": 1 }\n  ]\n}\n"
        );
    }

    #[test]
    fn add_member_into_json_inline_object_array_element() {
        // Group B parity: `a` on a member of a JSON inline-object array element
        // adds a sibling member inside the same object.
        let mut app = app_with_json("{\n  \"arr\": [\n    { \"a\": 1 }\n  ]\n}\n");
        cursor_to_key(&mut app, "a");
        app.add_node();
        assert!(matches!(app.mode, Mode::Edit(_)));
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "{\n  \"arr\": [\n    { \"a\": 1, \"new_field\": \"\" }\n  ]\n}\n"
        );
    }

    #[test]
    fn json_inline_object_array_element_member_edits_inline() {
        // Group B item 2c.2: a member of a JSON inline-object array element edits
        // inline and applies through Replace.
        let mut app = app_with_json("{\n  \"arr\": [\n    { \"a\": 1, \"b\": 2 }\n  ]\n}\n");
        cursor_to_key(&mut app, "a");
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        inline_set_value(&mut app, "5");
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "{\n  \"arr\": [\n    { \"a\": 5, \"b\": 2 }\n  ]\n}\n"
        );
    }

    /// Drive the inline editor to set the Value field to `new_value` and commit.
    fn inline_set_value(app: &mut App, new_value: &str) {
        app.begin_inline_edit();
        if let Mode::Edit(e) = &mut app.mode {
            e.buffer.clear();
            e.cursor = 0;
        }
        for c in new_value.chars() {
            app.edit_input_char(c);
        }
        app.edit_commit();
    }

    #[test]
    fn yaml_block_seq_scalar_child_edits_inline() {
        // ⑤ A keyed scalar inside a block-sequence element is inline-editable in
        // YAML (the block-array ancestor must not force $EDITOR).
        let mut app = app_with_yaml("plugins:\n  - name: a\n  - port: 8081\n");
        // Expand so the nested entries appear.
        app.expand_level();
        app.expand_level();
        let name_row = app
            .rows
            .iter()
            .position(|r| r.key == "name")
            .expect("name row visible");
        app.cursor = name_row;
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        let port_row = app.rows.iter().position(|r| r.key == "port").unwrap();
        app.cursor = port_row;
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        // And the edit actually applies through Replace.
        inline_set_value(&mut app, "9090");
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "plugins:\n  - name: a\n  - port: 9090\n"
        );
    }

    #[test]
    fn yaml_block_seq_map_element_external_fragment_is_the_element() {
        // ⑤ A block-map element of a sequence stays $EDITOR, but the captured
        // fragment is that element alone — not the whole `plugins` array.
        let app = app_with_yaml("plugins:\n  - name: a\n  - name: b\n");
        // plugins[1] is the second block-map element.
        let frag = app
            .doc
            .as_ref()
            .unwrap()
            .serialize_fragment(&[Seg::Key("plugins".into()), Seg::Index(1)]);
        // Just the element (with its source indent) — not the whole `plugins` seq.
        assert_eq!(frag, "  - name: b");
    }

    #[test]
    fn yaml_block_seq_map_element_replace_roundtrips() {
        // The indented element fragment `edit_node` captures applies cleanly back
        // to that element, leaving siblings intact.
        let mut app = app_with_yaml("plugins:\n  - name: a\n  - name: b\n");
        app.apply_replace(
            vec![Seg::Key("plugins".into()), Seg::Index(1)],
            "  - name: c\n".into(),
        );
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "plugins:\n  - name: a\n  - name: c\n"
        );
    }

    #[test]
    fn yaml_block_seq_first_multikey_element_replace_roundtrips() {
        // Replacing a non-last element that is a multi-key block map keeps the
        // sibling and the trailing newline.
        let mut app = app_with_yaml("plugins:\n  - name: a\n    port: 1\n  - name: b\n");
        app.apply_replace(
            vec![Seg::Key("plugins".into()), Seg::Index(0)],
            "  - name: z\n    port: 9\n".into(),
        );
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "plugins:\n  - name: z\n    port: 9\n  - name: b\n"
        );
    }

    #[test]
    fn toml_array_element_external_edit_replaces_only_that_element() {
        // BUG FIX: the `$EDITOR` path now captures + Replaces just the array element
        // (previously it truncated to the whole `arr`, matching YAML's per-element
        // precision). This mirrors edit_node's commit: the edited element repr is
        // wrapped via `scalar_fragment(None, …)` → TOML `__elem__ = …`.
        let mut app = app_with("arr = [\n  \"a\",\n  \"b\",\n]\n");
        let p = vec![Seg::Key("arr".into()), Seg::Index(0)];
        let (path, wrap) = app.external_edit_path(&p);
        assert_eq!(path, p, "edits the element, not the whole array");
        assert!(wrap, "TOML element fragment needs the __elem__ wrap");
        let wrapped = app.doc.as_ref().unwrap().scalar_fragment(None, "\"z\"");
        app.apply_replace(path, wrapped);
        assert!(
            app.status.is_none() && app.error.is_none(),
            "status {:?} error {:?}",
            app.status,
            app.error
        );
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "arr = [\n  \"z\",\n  \"b\",\n]\n",
            "only arr[0] changed"
        );
    }

    #[test]
    fn toml_array_element_nested_in_inline_table_edits_inline() {
        // A plain-array element is inline-editable wherever the array sits — here the
        // array is a member of an inline-table element of a multiline array
        // (`array_int[1].vals[0]`). `Replace` addresses it directly; the commit
        // changes only that element. (Works for any member key, incl. `__elem__`.)
        let mut app =
            app_with("array_int = [\n  3,\n  { vals = [123, 456], new_field = { a = 1 } },\n]\n");
        app.expand_all();
        app.rebuild_rows();
        let p = vec![
            Seg::Key("array_int".into()),
            Seg::Index(1),
            Seg::Key("vals".into()),
            Seg::Index(0),
        ];
        app.cursor = app
            .rows
            .iter()
            .position(|r| r.path == p)
            .expect("vals[0] visible");
        assert_eq!(
            app.edit_target_kind(),
            EditKind::Inline,
            "deep array element edits inline"
        );
        inline_set_value(&mut app, "999");
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "array_int = [\n  3,\n  { vals = [999, 456], new_field = { a = 1 } },\n]\n",
            "only vals[0] changed"
        );
    }

    #[test]
    fn json_array_element_nested_in_object_edits_inline() {
        // JSON parity: an array element nested under a key inside an array element
        // (`arr[1].vals[0]`) edits inline and Replaces only that element.
        let mut app =
            app_with_json("{\n  \"arr\": [\n    3,\n    { \"vals\": [123, 456] }\n  ]\n}\n");
        app.expand_all();
        app.rebuild_rows();
        let p = vec![
            Seg::Key("arr".into()),
            Seg::Index(1),
            Seg::Key("vals".into()),
            Seg::Index(0),
        ];
        app.cursor = app
            .rows
            .iter()
            .position(|r| r.path == p)
            .expect("vals[0] visible");
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        inline_set_value(&mut app, "999");
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "{\n  \"arr\": [\n    3,\n    { \"vals\": [999, 456] }\n  ]\n}\n",
            "only vals[0] changed"
        );
    }

    #[test]
    fn json_array_element_external_edit_replaces_only_that_element() {
        // BUG FIX parity: `E` on a JSON array element (e.g. an object) Replaces just
        // that element. The edited repr wraps as a bare value (`scalar_fragment(None)`).
        let mut app =
            app_with_json("{\n  \"arr\": [\n    { \"a\": 1 },\n    { \"b\": 2 }\n  ]\n}\n");
        let p = vec![Seg::Key("arr".into()), Seg::Index(0)];
        let (path, wrap) = app.external_edit_path(&p);
        assert_eq!(path, p);
        assert!(wrap, "JSON element fragment also wrapped (bare value)");
        let wrapped = app
            .doc
            .as_ref()
            .unwrap()
            .scalar_fragment(None, "{ \"a\": 9 }");
        app.apply_replace(path, wrapped);
        assert!(
            app.status.is_none() && app.error.is_none(),
            "status {:?} error {:?}",
            app.status,
            app.error
        );
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "{\n  \"arr\": [\n    { \"a\": 9 },\n    { \"b\": 2 }\n  ]\n}\n",
            "only arr[0] changed"
        );
    }

    #[test]
    fn yaml_block_seq_element_external_path_needs_no_wrap() {
        // YAML's `- value` element fragment is Replace-addressable directly, so the
        // external path is the element with NO wrap (the per-element standard the
        // TOML/JSON fix aligns to).
        let app = app_with_yaml("plugins:\n  - name: a\n  - name: b\n");
        let p = vec![Seg::Key("plugins".into()), Seg::Index(1)];
        let (path, wrap) = app.external_edit_path(&p);
        assert_eq!(path, p);
        assert!(!wrap, "YAML element needs no wrap");
    }

    #[test]
    fn aot_entry_external_path_is_the_entry_not_wrapped() {
        // Guard: an AoT entry (`product[1]`, parent is ArrayOfTables not Array) is not
        // a standard-array element — its whole `[[product]]` block is the fragment.
        let app = app_with("[[product]]\nname = \"Hammer\"\n[[product]]\nname = \"Nail\"\n");
        let p = vec![Seg::Key("product".into()), Seg::Index(1)];
        let (path, wrap) = app.external_edit_path(&p);
        assert_eq!(path, p);
        assert!(!wrap, "AoT entry is not a standard-array element");
    }

    #[test]
    fn toml_key_through_array_index_external_path_is_precise() {
        // GAP FIX: a key reached *through* a standard-array index (`arr[0].a`) is
        // `Replace`-addressable directly — the splice rebuilds the `{ … }` element in
        // place — so the external path keeps the WHOLE path (no wrap) instead of
        // truncating to the whole `arr` (matching YAML's per-node precision).
        let mut app = app_with("arr = [\n  { a = \"x\", b = 2 },\n  { a = \"y\" },\n]\n");
        let p = vec![Seg::Key("arr".into()), Seg::Index(0), Seg::Key("a".into())];
        let (path, wrap) = app.external_edit_path(&p);
        assert_eq!(path, p, "the member is addressed, not the whole array");
        assert!(!wrap, "a keyed member fragment needs no element wrap");
        app.apply_replace(path, "a = \"z\"\n".into());
        assert!(
            app.status.is_none() && app.error.is_none(),
            "status {:?} error {:?}",
            app.status,
            app.error
        );
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "arr = [\n  { a = \"z\", b = 2 },\n  { a = \"y\" },\n]\n",
            "only arr[0].a changed; sibling element + member b intact"
        );
    }

    #[test]
    fn json_key_through_array_index_external_path_is_precise() {
        // JSON parity for the gap fix: `arr[0].a` (a member of an object element)
        // keeps the whole path and Replaces only that member.
        let mut app = app_with_json(
            "{\n  \"arr\": [\n    { \"a\": 1, \"b\": 2 },\n    { \"a\": 3 }\n  ]\n}\n",
        );
        let p = vec![Seg::Key("arr".into()), Seg::Index(0), Seg::Key("a".into())];
        let (path, wrap) = app.external_edit_path(&p);
        assert_eq!(path, p);
        assert!(!wrap);
        app.apply_replace(path, "\"a\": 99\n".into());
        assert!(
            app.status.is_none() && app.error.is_none(),
            "status {:?} error {:?}",
            app.status,
            app.error
        );
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "{\n  \"arr\": [\n    { \"a\": 99, \"b\": 2 },\n    { \"a\": 3 }\n  ]\n}\n",
            "only arr[0].a changed"
        );
    }

    #[test]
    fn json_multiline_array_element_takes_trailing_comment() {
        // ③ A multiline-array element can gain a trailing `//` comment.
        let mut app = app_with_jsonc("{\n  \"arr\": [\n    1,\n    2\n  ]\n}\n");
        app.expand_level();
        app.expand_level();
        let row = app
            .rows
            .iter()
            .position(|r| r.path == vec![Seg::Key("arr".into()), Seg::Index(0)])
            .expect("arr[0] visible");
        app.cursor = row;
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        inline_set_value(&mut app, "1  // first");
        assert!(matches!(app.mode, Mode::Normal), "should commit");
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "{\n  \"arr\": [\n    1,  // first\n    2\n  ]\n}\n"
        );
    }

    #[test]
    fn json_inline_array_element_rejects_trailing_comment() {
        // ② An inline-array element can't take a trailing comment — reject cleanly,
        // leaving the document untouched and the editor open.
        let mut app = app_with_jsonc("{\n  \"arr\": [1, 2]\n}\n");
        app.expand_level();
        app.expand_level();
        let row = app
            .rows
            .iter()
            .position(|r| r.path == vec![Seg::Key("arr".into()), Seg::Index(0)])
            .expect("arr[0] visible");
        app.cursor = row;
        let before = app.doc.as_ref().unwrap().serialize();
        inline_set_value(&mut app, "1  // nope");
        assert!(matches!(app.mode, Mode::Edit(_)), "stays in editor");
        assert!(app
            .status
            .as_deref()
            .unwrap_or("")
            .contains("inline collection"));
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            before,
            "doc untouched"
        );
    }

    #[test]
    fn yaml_nudge_preserves_inline_comment() {
        // The ←/→ value nudge must keep a YAML trailing comment (YAML Replace drops
        // it, so the nudge re-asserts it like the editor does).
        let mut app = app_with_yaml("port: 8081  # bind\n");
        app.cursor = app.rows.iter().position(|r| r.key == "port").unwrap();
        app.nudge(1);
        assert_eq!(
            app.doc.as_ref().unwrap().serialize(),
            "port: 8082  # bind\n"
        );
    }

    #[test]
    fn yaml_value_edit_preserves_inline_comment() {
        // ④ YAML Replace swaps the whole entry (dropping the comment); the editor
        // must re-assert the unchanged comment so it survives a value-only edit.
        let mut app = app_with_yaml("host: x  # bind\n");
        app.cursor = app.rows.iter().position(|r| r.key == "host").unwrap();
        // Edit only the value; keep the comment text the same.
        inline_set_value(&mut app, "y  # bind");
        assert!(matches!(app.mode, Mode::Normal), "should commit cleanly");
        assert_eq!(app.doc.as_ref().unwrap().serialize(), "host: y  # bind\n");
    }

    #[test]
    fn block_comment_rejects_delete() {
        let mut app = app_with_jsonc("{\n  /* ro */\n  \"a\": 1\n}\n");
        // Expand root so children appear, then position cursor on the block comment.
        app.expand_level();
        app.rebuild_rows();
        let ci = app
            .rows
            .iter()
            .position(|r| r.key.contains("/* ro */"))
            .expect("block comment row not found");
        app.cursor = ci;
        // Verify the node is read_only before mutating.
        assert!(app.cursor_is_read_only(), "block comment must be read_only");
        app.delete_selected();
        assert!(
            app.status.as_deref().unwrap_or("").contains("read-only"),
            "expected read-only status, got: {:?}",
            app.status
        );
        use crate::model::document::ConfigDocument;
        assert!(
            app.doc.as_ref().unwrap().serialize().contains("/* ro */"),
            "document must not be mutated"
        );
    }

    #[test]
    fn block_comment_rejects_edit() {
        let mut app = app_with_jsonc("{\n  /* ro */\n  \"a\": 1\n}\n");
        app.expand_level();
        app.rebuild_rows();
        let ci = app
            .rows
            .iter()
            .position(|r| r.key.contains("/* ro */"))
            .expect("block comment row not found");
        app.cursor = ci;
        app.edit_node();
        assert!(
            app.status.as_deref().unwrap_or("").contains("read-only"),
            "expected read-only status, got: {:?}",
            app.status
        );
    }

    #[test]
    fn block_comment_rejects_cut() {
        let mut app = app_with_jsonc("{\n  /* ro */\n  \"a\": 1\n}\n");
        app.expand_level();
        app.rebuild_rows();
        let ci = app
            .rows
            .iter()
            .position(|r| r.key.contains("/* ro */"))
            .expect("block comment row not found");
        app.cursor = ci;
        app.cut_selected();
        assert!(
            app.status.as_deref().unwrap_or("").contains("read-only"),
            "expected read-only status, got: {:?}",
            app.status
        );
        assert!(
            app.clipboard.is_none(),
            "clipboard must not be set after rejected cut"
        );
    }

    #[test]
    fn block_comment_rejects_remark() {
        let mut app = app_with_jsonc("{\n  /* ro */\n  \"a\": 1\n}\n");
        app.expand_level();
        app.rebuild_rows();
        let ci = app
            .rows
            .iter()
            .position(|r| r.key.contains("/* ro */"))
            .expect("block comment row not found");
        app.cursor = ci;
        app.remark();
        assert!(
            app.status.as_deref().unwrap_or("").contains("read-only"),
            "expected read-only status, got: {:?}",
            app.status
        );
        use crate::model::document::ConfigDocument;
        assert!(
            app.doc.as_ref().unwrap().serialize().contains("/* ro */"),
            "document must not be mutated"
        );
    }

    #[test]
    fn block_comment_allows_copy() {
        let mut app = app_with_jsonc("{\n  /* ro */\n  \"a\": 1\n}\n");
        app.expand_level();
        app.rebuild_rows();
        let ci = app
            .rows
            .iter()
            .position(|r| r.key.contains("/* ro */"))
            .expect("block comment row not found");
        app.cursor = ci;
        assert!(app.cursor_is_read_only(), "block comment must be read_only");
        app.copy_selected();
        assert!(
            app.clipboard.is_some(),
            "copy of a read-only block comment must succeed"
        );
    }

    /// Regression: inline-editing a JSON value built a TOML `key = value`
    /// fragment the JSON backend rejected ("invalid TOML: unexpected token").
    /// The edit must now commit cleanly to a `"key": value` member.
    #[test]
    fn json_inline_value_edit_commits() {
        use crate::model::document::ConfigDocument;
        let mut app = app_with_jsonc("{\n  \"tags\": \"a\"\n}\n");
        app.expand_level();
        app.rebuild_rows();
        let ci = app
            .rows
            .iter()
            .position(|r| r.key == "tags")
            .expect("tags row not found");
        app.cursor = ci;
        app.begin_inline_edit();
        // Clear the value buffer and type a new JSON string literal `"b"`.
        if let Mode::Edit(ref mut e) = app.mode {
            e.buffer.clear();
            e.cursor = 0;
        }
        for c in "\"b\"".chars() {
            app.edit_input_char(c);
        }
        app.edit_commit();
        assert!(app.error.is_none(), "unexpected error: {:?}", app.error);
        assert!(
            app.status.as_deref() != Some("invalid value"),
            "edit should not have failed validation"
        );
        assert!(
            app.doc
                .as_ref()
                .unwrap()
                .serialize()
                .contains("\"tags\": \"b\""),
            "value must be updated: {}",
            app.doc.as_ref().unwrap().serialize()
        );
    }

    /// Regression: `←/→` nudge on a JSON integer built a TOML fragment too.
    #[test]
    fn json_nudge_integer_commits() {
        use crate::model::document::ConfigDocument;
        let mut app = app_with_jsonc("{\n  \"port\": 8080\n}\n");
        app.expand_level();
        app.rebuild_rows();
        let ci = app
            .rows
            .iter()
            .position(|r| r.key == "port")
            .expect("port row not found");
        app.cursor = ci;
        app.nudge(1);
        assert!(app.error.is_none(), "unexpected error: {:?}", app.error);
        assert!(
            app.doc
                .as_ref()
                .unwrap()
                .serialize()
                .contains("\"port\": 8081"),
            "nudged value must be 8081: {}",
            app.doc.as_ref().unwrap().serialize()
        );
    }
}
