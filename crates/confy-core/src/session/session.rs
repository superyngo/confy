use crate::model::any_doc::AnyDocument;
use crate::model::document::{
    ConfigDocument, DocFormat, MutateError, Mutation, OnCollision, Target,
};
use crate::model::node::{Format, KeySign, NodeKind, NodeTree, Path, ScalarType, Seg};
use crate::session::search::{fuzzy_match, haystack};
use crate::session::selection::Selection;
use crate::session::state::{
    Clipboard, EditField, EditKind, EditState, FilterLayer, History, KindSwitchState, Mode,
    PasteSlot, PendingComment, PendingCommit, PromptKind,
};
use crate::session::type_filter::TypeFilter;
use crate::session::view::{Update, ViewRow};
use std::collections::HashSet;

pub struct Session {
    pub doc: Option<AnyDocument>,
    pub tree: NodeTree,
    /// Cursor identity is the **path** of the selected node (§3 reshape).
    pub cursor: Path,
    pub expanded: HashSet<Path>,
    pub selection: Selection,
    pub last_action_was_shift_select: bool,
    pub history: Option<History>,
    pub status: Option<String>,
    pub error: Option<String>,
    pub mode: Mode,
    pub clipboard: Option<Clipboard>,
    pub paste_slot: Option<PasteSlot>,
    pub filter: String,
    pub filter_cursor: usize,
    pub last_filter: String,
    pub filtered_paths: Option<HashSet<Path>>,
    pub type_filter: TypeFilter,
    pub last_filter_applied: Option<FilterLayer>,
    pub detail_text: Option<String>,
    pub pending_edit: Option<(EditState, PendingCommit)>,
    pub pending_trailing: Option<Option<String>>,
}

impl Session {
    /// Construct a Session backed by a real document.
    pub fn new(doc: AnyDocument) -> Self {
        let tree = doc.project();
        let initial_snapshot = doc.serialize();
        let history = History::new(initial_snapshot);
        let mut s = Session {
            tree,
            doc: Some(doc),
            cursor: Vec::new(),
            expanded: HashSet::new(),
            selection: Selection::new(),
            last_action_was_shift_select: false,
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
        };
        s.expanded.insert(Vec::new());
        s
    }

    /// Construct a headless Session from a pre-built NodeTree (used in unit tests).
    pub fn from_tree(tree: NodeTree) -> Self {
        let expanded = HashSet::from([Vec::new()]);
        Session {
            tree,
            doc: None,
            cursor: Vec::new(),
            expanded,
            selection: Selection::new(),
            last_action_was_shift_select: false,
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
        }
    }

    // ---- Visible rows (pure) ----

    /// Pure: flatten the tree through the expand set and filter, baking in
    /// selection + cursor flags. No side effects.
    pub fn visible_rows(&self) -> Vec<ViewRow> {
        let expanded = &self.expanded;
        let rows: Vec<ViewRow> = self
            .tree
            .flatten(&|p| expanded.contains(p))
            .into_iter()
            .map(|r| {
                let scalar_type = match &r.node.kind {
                    NodeKind::Scalar(st) => Some(*st),
                    _ => None,
                };
                ViewRow {
                    path: r.node.path.clone(),
                    depth: r.depth,
                    is_branch: r.node.is_branch(),
                    key: r.node.key.clone(),
                    value: r.node.value.clone(),
                    scalar_type,
                    format: r.node.format,
                    trailing_comment: r.node.trailing_comment.clone(),
                    read_only: r.node.read_only,
                    selected: self.selection.contains(&r.node.path),
                    is_cursor: r.node.path == self.cursor,
                }
            })
            .collect();
        if let Some(ref fp) = self.filtered_paths {
            rows.into_iter().filter(|r| fp.contains(&r.path)).collect()
        } else {
            rows
        }
    }

    /// Stateful rebuild: compute visible rows, snap cursor, clear stale paste/selection.
    /// Returns the new rows for the host to map to RowSnapshot.
    pub fn compute_rows(&mut self) -> Vec<ViewRow> {
        let prev_index = {
            let rows = self.visible_rows();
            rows.iter().position(|r| r.path == self.cursor)
        };
        let rows = self.visible_rows();
        // Snap cursor if path is no longer visible.
        if !rows.iter().any(|r| r.path == self.cursor) {
            let i = prev_index.unwrap_or(0).min(rows.len().saturating_sub(1));
            self.cursor = rows.get(i).map(|r| r.path.clone()).unwrap_or_default();
        }
        if self.clipboard.is_some() {
            self.paste_slot = None;
        }
        rows
    }

    // ---- Visible row helpers ----

    /// Ordered paths of the currently visible rows.
    pub fn visible_paths(&self) -> Vec<Path> {
        self.visible_rows().iter().map(|r| r.path.clone()).collect()
    }

    /// Path the cursor is on, if visible.
    pub fn cursor_row_path(&self) -> Option<Path> {
        let rows = self.visible_rows();
        rows.iter()
            .find(|r| r.path == self.cursor)
            .map(|r| r.path.clone())
    }

    /// Cursor's visible-row index.
    pub fn cursor_row_index(&self) -> Option<usize> {
        self.visible_rows()
            .iter()
            .position(|r| r.path == self.cursor)
    }

    // ---- Navigation ----

    pub fn cursor_down(&mut self) {
        if self.clipboard.is_some() {
            self.move_paste_slot(1);
            return;
        }
        let rows = self.visible_rows();
        let idx = rows.iter().position(|r| r.path == self.cursor).unwrap_or(0);
        if idx + 1 < rows.len() {
            self.cursor = rows[idx + 1].path.clone();
        }
    }

    pub fn cursor_up(&mut self) {
        if self.clipboard.is_some() {
            self.move_paste_slot(-1);
            return;
        }
        let rows = self.visible_rows();
        let idx = rows.iter().position(|r| r.path == self.cursor).unwrap_or(0);
        if idx > 0 {
            self.cursor = rows[idx - 1].path.clone();
        }
    }

    pub fn toggle_expand(&mut self) {
        let rows = self.visible_rows();
        let Some((is_branch, path)) = rows
            .iter()
            .find(|r| r.path == self.cursor)
            .map(|r| (r.is_branch, r.path.clone()))
        else {
            return;
        };
        if is_branch && !self.expanded.remove(&path) {
            self.expanded.insert(path);
        }
    }

    pub fn collapse_all(&mut self) {
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

    pub fn expand_level(&mut self) {
        let rows = self.visible_rows();
        let base = match rows.iter().find(|r| r.path == self.cursor) {
            Some(r) if r.is_branch => r.path.clone(),
            _ => return,
        };
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
        let frontier = branches
            .iter()
            .filter(|p| !self.expanded.contains(*p))
            .map(|p| p.len())
            .min();
        let Some(d) = frontier else { return };
        for p in branches.into_iter().filter(|p| p.len() <= d) {
            self.expanded.insert(p);
        }
        // base is still visible; cursor stays on it.
        self.cursor = base;
    }

    pub fn collapse_level(&mut self) {
        let rows = self.visible_rows();
        let (path, is_branch) = match rows.iter().find(|r| r.path == self.cursor) {
            Some(r) => (r.path.clone(), r.is_branch),
            None => return,
        };
        let is_open_branch = is_branch && self.expanded.contains(&path);
        let target = if is_open_branch {
            path
        } else if path.is_empty() {
            return;
        } else {
            path[..path.len() - 1].to_vec()
        };
        self.expanded.remove(&target);
        self.cursor = target;
    }

    pub fn page_up(&mut self, page_size: usize) {
        let step = page_size.max(1);
        if self.clipboard.is_some() {
            self.move_paste_slot(-(step as isize));
            return;
        }
        let rows = self.visible_rows();
        let idx = rows
            .iter()
            .position(|r| r.path == self.cursor)
            .unwrap_or(0)
            .saturating_sub(step);
        if let Some(r) = rows.get(idx) {
            self.cursor = r.path.clone();
        }
    }

    pub fn page_down(&mut self, page_size: usize) {
        let step = page_size.max(1);
        if self.clipboard.is_some() {
            self.move_paste_slot(step as isize);
            return;
        }
        let rows = self.visible_rows();
        let max = rows.len().saturating_sub(1);
        let idx = (rows.iter().position(|r| r.path == self.cursor).unwrap_or(0) + step).min(max);
        if let Some(r) = rows.get(idx) {
            self.cursor = r.path.clone();
        }
    }

    pub fn cursor_home(&mut self) {
        if self.clipboard.is_some() {
            self.move_paste_slot(isize::MIN / 2);
            return;
        }
        let rows = self.visible_rows();
        if let Some(r) = rows.first() {
            self.cursor = r.path.clone();
        }
    }

    pub fn cursor_end(&mut self) {
        if self.clipboard.is_some() {
            self.move_paste_slot(isize::MAX / 2);
            return;
        }
        let rows = self.visible_rows();
        if let Some(r) = rows.last() {
            self.cursor = r.path.clone();
        }
    }

    // ---- Paste-mode insertion slots ----

    pub fn paste_slots(&self) -> Vec<PasteSlot> {
        let rows = self.visible_rows();
        let mut slots = Vec::with_capacity(rows.len() * 2);
        for row in rows.iter() {
            if row.is_branch {
                slots.push(PasteSlot::Into(row.path.clone()));
            }
            slots.push(PasteSlot::After(row.path.clone()));
        }
        slots
    }

    pub fn effective_paste_slot(&self) -> PasteSlot {
        self.paste_slot
            .clone()
            .unwrap_or_else(|| PasteSlot::After(self.cursor.clone()))
    }

    fn move_paste_slot(&mut self, delta: isize) {
        let slots = self.paste_slots();
        if slots.is_empty() {
            return;
        }
        let cur = self.effective_paste_slot();
        let idx = slots.iter().position(|s| *s == cur).unwrap_or(0) as isize;
        let max = slots.len() as isize - 1;
        let next = (idx + delta).clamp(0, max) as usize;
        let slot = slots[next].clone();
        self.cursor = match &slot {
            PasteSlot::Into(p) | PasteSlot::After(p) => p.clone(),
        };
        self.paste_slot = Some(slot);
    }

    pub fn slot_target(&self, slot: PasteSlot) -> Option<Target> {
        let rows = self.visible_rows();
        match slot {
            PasteSlot::Into(p) => {
                let row = rows.iter().find(|r| r.path == p)?;
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
            PasteSlot::After(p) => {
                let row = rows.iter().find(|r| r.path == p)?;
                let expanded = self.expanded.contains(&row.path);
                let sibling_index = self.true_sibling_index(&row.path);
                Some(crate::session::insertion::resolve_target(
                    &row.path,
                    row.is_branch,
                    expanded,
                    sibling_index,
                ))
            }
        }
    }

    pub fn is_expanded(&self, path: &Path) -> bool {
        self.expanded.contains(path)
    }

    fn resting_mode(&self) -> Mode {
        if self.filtered_paths.is_some() {
            Mode::FilterResults
        } else {
            Mode::Normal
        }
    }

    // ---- Doc format ----

    pub fn doc_format(&self) -> DocFormat {
        self.doc.as_ref().map_or(DocFormat::Toml, |d| d.format())
    }

    // ---- Filter (/) ----

    pub fn enter_filter(&mut self) {
        self.filter = self.last_filter.clone();
        self.filter_cursor = self.filter.chars().count();
        self.mode = Mode::Filter;
        self.recompute_filter();
    }

    pub fn commit_filter(&mut self) {
        if self.filter.is_empty() {
            self.exit_filter();
            return;
        }
        self.last_filter = self.filter.clone();
        self.last_filter_applied = Some(FilterLayer::Text);
        self.mode = Mode::FilterResults;
    }

    pub fn exit_filter_results(&mut self) {
        let peel_text = match self.last_filter_applied {
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
    }

    pub fn exit_filter(&mut self) {
        self.filter.clear();
        self.filter_cursor = 0;
        self.filtered_paths = None;
        self.mode = Mode::Normal;
    }

    pub fn filter_char(&mut self, c: char) {
        let at = char_byte_idx(&self.filter, self.filter_cursor);
        self.filter.insert(at, c);
        self.filter_cursor += 1;
        self.recompute_filter();
    }

    pub fn filter_backspace(&mut self) {
        if self.filter_cursor > 0 {
            let prev = char_byte_idx(&self.filter, self.filter_cursor - 1);
            self.filter.remove(prev);
            self.filter_cursor -= 1;
            self.recompute_filter();
        }
    }

    pub fn filter_delete(&mut self) {
        if self.filter_cursor < self.filter.chars().count() {
            let at = char_byte_idx(&self.filter, self.filter_cursor);
            self.filter.remove(at);
            self.recompute_filter();
        }
    }

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

    pub fn recompute_filter(&mut self) {
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
            doc: DocFormat,
        ) {
            let path_keys: Vec<&str> = n
                .path
                .iter()
                .filter_map(|s| match s {
                    Seg::Key(k) => Some(k.as_str()),
                    _ => None,
                })
                .collect();
            let comment_text = match &n.kind {
                NodeKind::Comment(c) => Some(c.as_str()),
                _ => None,
            };
            let h = haystack(&path_keys, None, comment_text);
            let text_ok = fuzzy_match(&h, needle);
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

    // ---- Type filter (f) ----

    pub fn enter_type_filter(&mut self) {
        self.mode = Mode::TypeFilter;
        self.recompute_filter();
    }

    pub fn type_filter_move(&mut self, dr: i32, dc: i32) {
        let fmt = self.doc_format();
        self.type_filter.move_cursor(dr, dc, fmt);
    }

    pub fn type_filter_toggle(&mut self) {
        let fmt = self.doc_format();
        self.type_filter.toggle_current(fmt);
        if self.type_filter.is_active() {
            self.last_filter_applied = Some(FilterLayer::Type);
        }
        self.recompute_filter();
    }

    pub fn commit_type_filter(&mut self) {
        if self.type_filter.is_active() {
            self.last_filter_applied = Some(FilterLayer::Type);
        }
        self.recompute_filter();
        self.mode = self.resting_mode();
    }

    pub fn exit_type_filter(&mut self) {
        self.type_filter.clear();
        self.last_filter_applied = (!self.filter.is_empty()).then_some(FilterLayer::Text);
        self.recompute_filter();
        self.mode = self.resting_mode();
    }

    // ---- Kind switch (K) ----

    pub fn open_kind_switch(&mut self) {
        let rows = self.visible_rows();
        let Some(row) = rows.iter().find(|r| r.path == self.cursor) else {
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
        self.mode = Mode::KindSwitch(KindSwitchState {
            path,
            options,
            cursor: 0,
        });
    }

    pub fn kind_switch_move(&mut self, delta: i32) {
        if let Mode::KindSwitch(st) = &mut self.mode {
            let n = st.options.len() as i32;
            if n > 0 {
                st.cursor = (st.cursor as i32 + delta).rem_euclid(n) as usize;
            }
        }
    }

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
        match doc.apply(Mutation::ConvertKind {
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

    pub fn exit_kind_switch(&mut self) {
        self.mode = self.resting_mode();
        self.status = None;
    }

    // ---- Document conversion (C) — pure orchestration; host does the fs write ----

    pub fn open_convert(&mut self) {
        let rows = self.visible_rows();
        let Some(row) = rows.iter().find(|r| r.path == self.cursor) else {
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
        let options: Vec<DocFormat> = [DocFormat::Toml, DocFormat::Json, DocFormat::Yaml]
            .into_iter()
            .filter(|f| *f != current)
            .collect();
        self.mode = Mode::Convert(crate::session::state::ConvertState {
            step: crate::session::state::ConvertStep::Format,
            options,
            cursor: 0,
            target: current,
            path: String::new(),
            path_cursor: 0,
            warnings: Vec::new(),
            text: String::new(),
        });
    }

    pub fn convert_move(&mut self, delta: i32) {
        if let Mode::Convert(st) = &mut self.mode {
            let n = st.options.len() as i32;
            if n > 0 {
                st.cursor = (st.cursor as i32 + delta).rem_euclid(n) as usize;
            }
        }
    }

    /// Lock the target format and seed the output path. The seed path string is
    /// passed in by the host (which owns `source_path`).
    pub fn convert_pick_format(&mut self, default_stem: Option<String>) {
        if let Mode::Convert(st) = &mut self.mode {
            let Some(target) = st.options.get(st.cursor).copied() else {
                return;
            };
            st.target = target;
            let ext = match target {
                DocFormat::Toml => "toml",
                DocFormat::Json => "json",
                DocFormat::Yaml => "yaml",
            };
            st.path = default_stem
                .map(|stem| format!("{stem}.{ext}"))
                .unwrap_or_else(|| format!("out.{ext}"));
            st.path_cursor = st.path.chars().count();
            st.step = crate::session::state::ConvertStep::Path;
        }
    }

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

    /// Run the conversion. Returns `Update` with `convert_write` set when a write
    /// is needed — the host performs the actual `fs::write`.
    pub fn convert_run(&mut self) -> Update {
        let (target, path) = match &self.mode {
            Mode::Convert(st) => (st.target, st.path.clone()),
            _ => return Update::default(),
        };
        let Some(doc) = &self.doc else {
            return Update::default();
        };
        match crate::model::convert::convert(doc, target) {
            Ok(result) => {
                if result.warnings.is_empty() {
                    self.mode = self.resting_mode();
                    Update {
                        convert_write: Some((path, result.text)),
                        ..Default::default()
                    }
                } else {
                    if let Mode::Convert(st) = &mut self.mode {
                        st.warnings = result.warnings;
                        st.text = result.text;
                        st.step = crate::session::state::ConvertStep::Confirm;
                    }
                    Update::default()
                }
            }
            Err(abort) => {
                self.error = Some(format!("conversion aborted: {abort}"));
                self.mode = self.resting_mode();
                Update::default()
            }
        }
    }

    /// `y` on the Confirm step: signal the host to write the rendered output.
    pub fn convert_confirm(&mut self) -> Update {
        let (path, text) = match &self.mode {
            Mode::Convert(st) => (st.path.clone(), st.text.clone()),
            _ => return Update::default(),
        };
        self.mode = self.resting_mode();
        Update {
            convert_write: Some((path, text)),
            ..Default::default()
        }
    }

    pub fn exit_convert(&mut self) {
        self.mode = self.resting_mode();
        self.status = None;
    }

    // ---- Detail popup ----

    pub fn toggle_detail(&mut self) {
        if matches!(self.mode, Mode::Detail) {
            self.exit_detail();
        } else {
            self.open_detail();
        }
    }

    pub fn open_detail(&mut self) {
        let rows = self.visible_rows();
        let row = match rows.iter().find(|r| r.path == self.cursor) {
            Some(r) => r.clone(),
            None => return,
        };
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
            let node = self.tree.node_at(&row.path);
            let (type_str, fmt_str) = node
                .map(|n| branch_type_format(&n.kind))
                .unwrap_or(("unknown", "-"));
            let children = node.map(|n| n.children.len()).unwrap_or(0);
            format!(
                "Path:     {dotted}\nType:     {type_str}\nFormat:   {fmt_str}\nChildren: {children}"
            )
        } else {
            let type_str = row.scalar_type.map(|st| format!("{st:?}").to_lowercase());
            let type_str = type_str.as_deref().unwrap_or(node_type_label_str(
                &self
                    .tree
                    .node_at(&row.path)
                    .map(|n| n.kind.clone())
                    .unwrap_or(NodeKind::Root),
            ));
            let val_str = row.value.as_deref().unwrap_or("");
            let fmt_str = format_label(row.format).unwrap_or("plain");
            format!("Path:     {dotted}\nType:     {type_str}\nFormat:   {fmt_str}\nValue:    {val_str}")
        };
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
        self.mode = Mode::Detail;
    }

    pub fn exit_detail(&mut self) {
        self.detail_text = None;
        self.mode = self.resting_mode();
    }

    // ---- Help ----

    pub fn enter_help(&mut self) {
        self.mode = Mode::Help;
    }

    pub fn exit_help(&mut self) {
        self.mode = Mode::Normal;
    }

    // ---- Selection ----

    pub fn toggle_select(&mut self) {
        if self.clipboard.is_some() {
            return;
        }
        self.selection.toggle(self.cursor.clone());
    }

    pub fn extend_select_up(&mut self) {
        if self.clipboard.is_some() {
            return;
        }
        let rows = self.visible_rows();
        if !self.last_action_was_shift_select {
            self.selection.begin_round(self.cursor.clone());
        }
        let idx = rows.iter().position(|r| r.path == self.cursor).unwrap_or(0);
        if idx > 0 {
            self.cursor = rows[idx - 1].path.clone();
            let visible = rows.iter().map(|r| r.path.clone()).collect::<Vec<_>>();
            let to = self.cursor.clone();
            self.selection.extend_round_to(&visible, &to);
        }
        self.last_action_was_shift_select = true;
    }

    pub fn extend_select_down(&mut self) {
        if self.clipboard.is_some() {
            return;
        }
        let rows = self.visible_rows();
        if !self.last_action_was_shift_select {
            self.selection.begin_round(self.cursor.clone());
        }
        let idx = rows.iter().position(|r| r.path == self.cursor).unwrap_or(0);
        if idx + 1 < rows.len() {
            self.cursor = rows[idx + 1].path.clone();
            let visible = rows.iter().map(|r| r.path.clone()).collect::<Vec<_>>();
            let to = self.cursor.clone();
            self.selection.extend_round_to(&visible, &to);
        }
        self.last_action_was_shift_select = true;
    }

    pub fn selected_paths(&self) -> Vec<Path> {
        let rows = self.visible_rows();
        if self.selection.is_empty() {
            return rows
                .iter()
                .find(|r| r.path == self.cursor)
                .map(|r| vec![r.path.clone()])
                .unwrap_or_default();
        }
        let paths: Vec<Path> = self.selection.iter().collect();
        crate::session::selection::normalize(paths)
    }

    fn cursor_is_read_only(&self) -> bool {
        let rows = self.visible_rows();
        rows.iter()
            .find(|r| r.path == self.cursor)
            .and_then(|r| self.tree.node_at(&r.path))
            .map(|n| n.read_only)
            .unwrap_or(false)
    }

    // ---- Edit routing ----

    pub fn edit_target_kind(&self) -> EditKind {
        let rows = self.visible_rows();
        let path = match rows.iter().find(|r| r.path == self.cursor) {
            Some(r) => r.path.clone(),
            None => return EditKind::External,
        };
        if path.is_empty() {
            return EditKind::External;
        }
        let node = match self.tree.node_at(&path) {
            Some(n) => n,
            None => return EditKind::External,
        };
        if let NodeKind::Comment(text) = &node.kind {
            let single_line = !text.contains('\n');
            return if single_line && self.no_array_ancestor(&path) {
                EditKind::Inline
            } else {
                EditKind::External
            };
        }
        let inline_object = matches!(node.kind, NodeKind::Table) && node.format == Format::Inline;
        let structured_inline =
            matches!(node.kind, NodeKind::Array | NodeKind::InlineTable) || inline_object;
        if !matches!(node.kind, NodeKind::Scalar(_)) && !structured_inline {
            return EditKind::External;
        }
        if structured_inline && node.value.is_none() {
            return EditKind::External;
        }
        if matches!(
            node.format,
            Format::MultilineBasic
                | Format::MultilineLiteral
                | Format::LiteralBlock
                | Format::Folded
        ) {
            return EditKind::External;
        }
        let addressable = self
            .doc
            .as_ref()
            .map(|d| d.array_elements_addressable())
            .unwrap_or(false);
        let parent_path = &path[..path.len() - 1];
        let parent = self.tree.node_at(parent_path);
        match path.last() {
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
                let parent_inline_container = parent
                    .map(|p| {
                        matches!(p.kind, NodeKind::InlineTable)
                            || (matches!(p.kind, NodeKind::Table) && p.format == Format::Inline)
                    })
                    .unwrap_or(false);
                let addressable = parent_ok
                    && (addressable || self.no_array_ancestor(&path) || parent_inline_container);
                if addressable {
                    EditKind::Inline
                } else {
                    EditKind::External
                }
            }
            None => EditKind::External,
        }
    }

    pub fn external_edit_path(&self, path: &Path) -> (Path, bool) {
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

    pub fn no_array_ancestor(&self, path: &[Seg]) -> bool {
        (1..path.len()).all(|i| {
            self.tree
                .node_at(&path[..i])
                .map(|n| !matches!(n.kind, NodeKind::Array))
                .unwrap_or(false)
        })
    }

    // ---- Inline editor ----

    pub fn begin_inline_edit(&mut self) {
        let rows = self.visible_rows();
        let row = match rows.iter().find(|r| r.path == self.cursor) {
            Some(r) => r.clone(),
            None => return,
        };
        let is_comment = self
            .tree
            .node_at(&row.path)
            .map(|n| matches!(n.kind, NodeKind::Comment(_)))
            .unwrap_or(false);
        let (key, is_element) = if is_comment {
            (String::new(), false)
        } else {
            match row.path.last() {
                Some(Seg::Key(k)) => (k.clone(), false),
                Some(Seg::Index(_)) => (String::new(), true),
                None => return,
            }
        };
        let orig_trailing = if is_comment {
            None
        } else {
            row.trailing_comment.clone()
        };
        let mut buffer = row.value.clone().unwrap_or_default().trim().to_string();
        if let Some(tc) = &orig_trailing {
            buffer.push_str("  ");
            buffer.push_str(tc);
        }
        let cursor = buffer.chars().count();
        let name_cursor = key.chars().count();
        self.mode = Mode::Edit(EditState {
            path: row.path.clone(),
            key: key.clone(),
            field: EditField::Value,
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

    pub fn begin_inline_rename(&mut self) {
        let rows = self.visible_rows();
        let row = match rows.iter().find(|r| r.path == self.cursor) {
            Some(r) => r.clone(),
            None => return,
        };
        let key = match row.path.last() {
            Some(Seg::Key(k)) => k.clone(),
            _ => return,
        };
        let is_comment = self
            .tree
            .node_at(&row.path)
            .map(|n| matches!(n.kind, NodeKind::Comment(_)))
            .unwrap_or(false);
        if is_comment {
            return;
        }
        let name_cursor = key.chars().count();
        self.mode = Mode::Edit(EditState {
            path: row.path.clone(),
            key: key.clone(),
            field: EditField::Name,
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

    pub fn edit_toggle_field(&mut self) {
        if let Mode::Edit(ref mut e) = self.mode {
            if e.is_element || e.is_comment || e.rename_only {
                return;
            }
            std::mem::swap(&mut e.buffer, &mut e.other_buffer);
            std::mem::swap(&mut e.cursor, &mut e.other_cursor);
            std::mem::swap(&mut e.scroll, &mut e.other_scroll);
            e.field = match e.field {
                EditField::Value => EditField::Name,
                EditField::Name => EditField::Value,
            };
            self.status = None;
        }
    }

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
        let created_on_add = matches!(&self.mode, Mode::Edit(e) if e.created_on_add);
        self.mode = self.resting_mode();
        self.pending_edit = None;
        self.pending_trailing = None;
        self.status = None;
        if created_on_add {
            self.cancel_added_node();
        }
    }

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
    }

    pub fn edit_commit(&mut self) {
        let rest = self.resting_mode();
        let mut e = match std::mem::replace(&mut self.mode, rest) {
            Mode::Edit(e) => e,
            other => {
                self.mode = other;
                return;
            }
        };
        // Comment node: commit via EditComment.
        if e.is_comment {
            let text = e.buffer.clone();
            let ok = match self.doc.as_mut() {
                Some(doc) => doc.apply(Mutation::EditComment {
                    path: e.path.clone(),
                    text,
                }),
                None => Ok(()),
            };
            match ok {
                Ok(()) => self.on_mutation_success(),
                Err(MutateError::Fragment(msg)) => {
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
        let (name_str, raw_value) = match e.field {
            EditField::Value => (e.other_buffer.clone(), e.buffer.clone()),
            EditField::Name => (e.buffer.clone(), e.other_buffer.clone()),
        };
        let is_element = matches!(e.path.last(), Some(Seg::Index(_)));
        let split = self
            .doc
            .as_ref()
            .filter(|d| d.supports_comments())
            .map(|d| d.split_value_comment(&raw_value));
        let (value_str, new_trailing) = match split {
            Some((v, c)) => (v, c),
            None => (raw_value.clone(), None),
        };
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
        // 1. Key rename (Name field changed).
        if !is_element {
            let new_name = name_str.trim().to_string();
            if new_name != e.key {
                if new_name.is_empty() {
                    self.status = Some("key cannot be empty".into());
                    self.mode = Mode::Edit(e);
                    return;
                }
                let old_label = node_type_label_str(
                    &self
                        .tree
                        .node_at(&e.path)
                        .map(|n| n.kind.clone())
                        .unwrap_or(NodeKind::Root),
                )
                .to_string();
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
                    Some(doc) => doc.apply(Mutation::Rename {
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
        // F2 rename-only: skip value Replace.
        if e.rename_only {
            self.mode = self.resting_mode();
            return;
        }
        // 2. Value replace.
        let key_arg = (!is_element).then_some(frag_key.as_str());
        let (fragment, new_label) = match self.doc.as_ref() {
            Some(doc) => {
                let fragment = doc.scalar_fragment(key_arg, &value_str);
                match doc.value_kind(&value_str) {
                    Ok(kind) => (fragment, node_type_label_str(&kind).to_string()),
                    Err(msg) => {
                        self.status = Some(format!("invalid value: {msg}"));
                        self.mode = Mode::Edit(e);
                        return;
                    }
                }
            }
            None => (format!("{frag_key} = {value_str}\n"), String::new()),
        };
        let old_label = node_type_label_str(
            &self
                .tree
                .node_at(&e.path)
                .map(|n| n.kind.clone())
                .unwrap_or(NodeKind::Root),
        )
        .to_string();
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

    fn apply_deferred_rename(&mut self, mut e: EditState, new_name: String, value: String) {
        let res = match self.doc.as_mut() {
            Some(doc) => doc.apply(Mutation::Rename {
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

    pub fn apply_replace(&mut self, path: Path, edited: String) {
        let trailing = self.pending_trailing.take();
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        let fmt = doc.format().name();
        match doc.apply(Mutation::Replace {
            path: path.clone(),
            fragment: edited,
        }) {
            Ok(()) => {
                if let Some(comment) = trailing {
                    if let Err(e) = doc.apply(Mutation::SetTrailingComment { path, comment }) {
                        self.error = Some(format!("comment update failed: {e}"));
                    }
                }
                self.on_mutation_success();
            }
            Err(MutateError::Fragment(msg)) => {
                self.error = Some(format!("invalid {fmt}: {msg}"));
            }
            Err(e) => self.error = Some(format!("error: {e}")),
        }
    }

    pub fn apply_edit_comment(&mut self, path: Path, text: String) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        match doc.apply(Mutation::EditComment { path, text }) {
            Ok(()) => self.on_mutation_success(),
            Err(MutateError::Fragment(msg)) => {
                self.error = Some(format!("invalid comment: {msg}"));
            }
            Err(e) => self.error = Some(format!("error: {e}")),
        }
    }

    // ---- Nudge (←/→ in Normal) ----

    pub fn nudge(&mut self, delta: i64) {
        let rows = self.visible_rows();
        let path = match rows.iter().find(|r| r.path == self.cursor) {
            Some(r) => r.path.clone(),
            None => return,
        };
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
            if !preserves {
                if let Some(tc) = trailing {
                    self.pending_trailing = Some(Some(tc));
                }
            }
            self.apply_replace(path, fragment);
        }
    }

    // ---- Add node ----

    pub fn add_node(&mut self) {
        if self.doc.is_none() {
            return;
        }
        let rows = self.visible_rows();
        let cursor_row = match rows.iter().find(|r| r.path == self.cursor).cloned() {
            Some(r) => r,
            None => return,
        };
        let expanded = self.expanded.contains(&cursor_row.path);
        let is_append = cursor_row.path.is_empty() || (cursor_row.is_branch && expanded);
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
        let seed_kind = if is_append {
            NodeKind::Scalar(ScalarType::String)
        } else {
            cursor_kind.unwrap_or(NodeKind::Scalar(ScalarType::String))
        };
        if matches!(seed_kind, NodeKind::Comment(_)) {
            self.add_comment_sibling(target);
            return;
        }
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
        if !target.parent.is_empty() {
            self.expanded.insert(target.parent.clone());
        }
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
            NodeKind::Array | NodeKind::InlineTable | NodeKind::ArrayOfTables | NodeKind::Table => {
                (
                    doc.empty_container_fragment(&seed_kind, key.as_deref()),
                    false,
                )
            }
        };
        self.apply_insert(target.clone(), fragment);
        if self.error.is_some() {
            return;
        }
        let mut new_path = target.parent.clone();
        match &key {
            Some(k) => new_path.push(Seg::Key(k.clone())),
            None => new_path.push(Seg::Index(target.index)),
        }
        let rows = self.visible_rows();
        if rows.iter().any(|r| r.path == new_path) {
            self.cursor = new_path;
            if inline {
                self.begin_inline_edit();
                if let Mode::Edit(e) = &mut self.mode {
                    e.created_on_add = true;
                }
            } else {
                self.status = Some("added placeholder node — rename with e".into());
            }
        }
    }

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
        match doc.apply(Mutation::InsertComment {
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
        let rows = self.visible_rows();
        if rows.iter().any(|r| r.path == new_path) {
            self.cursor = new_path;
            self.status = Some("added comment — edit with e".into());
        }
    }

    pub fn apply_insert(&mut self, target: Target, edited: String) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        let fmt = doc.format().name();
        match doc.apply(Mutation::Insert {
            target,
            fragment: edited,
            on_collision: OnCollision::Cancel,
        }) {
            Ok(()) => self.on_mutation_success(),
            Err(MutateError::Collision(key)) => {
                self.error = Some(format!(
                    "key collision: {key} (rename/overwrite not yet prompted)"
                ));
            }
            Err(MutateError::Fragment(msg)) => {
                self.error = Some(format!("invalid {fmt}: {msg}"));
            }
            Err(e) => self.error = Some(format!("error: {e}")),
        }
    }

    fn on_mutation_success(&mut self) {
        if let Some(doc) = self.doc.as_ref() {
            let snapshot = doc.serialize();
            let tree = doc.project();
            if let Some(h) = self.history.as_mut() {
                h.push(snapshot);
            }
            self.tree = tree;
        }
        self.status = None;
        self.error = None;
    }

    // ---- d/x/c/v/r/z/y operations ----

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

    pub fn copy_selected(&mut self) {
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
        self.paste_slot = None;
        self.status = Some(format!(
            "copied {} node(s)",
            self.clipboard.as_ref().unwrap().fragments.len()
        ));
    }

    pub fn cut_selected(&mut self) {
        if self.cursor_is_read_only() {
            self.status = Some("read-only node (block comment)".into());
            return;
        }
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
        self.paste_slot = None;
        self.status = Some(format!(
            "cut {} node(s)",
            self.clipboard.as_ref().unwrap().fragments.len()
        ));
    }

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

    pub fn do_paste(
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
        let is_comment = |p: &Path| {
            self.tree
                .node_at(p)
                .map(|n| matches!(n.kind, NodeKind::Comment(_)))
                .unwrap_or(false)
        };
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
                    Err(MutateError::Collision(key)) => {
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
                    Err(MutateError::Collision(key)) => {
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
        // ---- COMMENT PHASE ----
        let orig_ord = |p: &Path| -> Option<usize> {
            self.tree
                .node_at(&target.parent)
                .and_then(|par| par.children.iter().position(|c| &c.path == p))
        };
        let node_shift = if is_cut {
            node_entries
                .iter()
                .filter(|(_, s)| orig_ord(s).is_some_and(|o| o < target.index))
                .count()
        } else {
            0
        };
        let comment_ords: Vec<Option<usize>> =
            comment_entries.iter().map(|(_, s)| orig_ord(s)).collect();
        let n_comments = comment_entries.len();
        for rev in 0..n_comments {
            let oi = n_comments - 1 - rev;
            let (frag, src) = &comment_entries[oi];
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
                let doc = self.doc.as_mut().unwrap();
                if let Err(e) = doc.apply(Mutation::Delete { path: src.clone() }) {
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
                let end = if is_cut { oi } else { oi + 1 };
                self.on_mutation_success();
                self.clipboard = Some(rebuild(is_cut, &[], &comment_entries[..end]));
                self.error = Some(format!("paste error: {e}"));
                return;
            }
        }
        self.on_mutation_success();
    }

    pub fn remark(&mut self) {
        if self.cursor_is_read_only() {
            self.status = Some("read-only node (block comment)".into());
            return;
        }
        let rows = self.visible_rows();
        let path = match rows.iter().find(|r| r.path == self.cursor) {
            Some(r) => r.path.clone(),
            None => return,
        };
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
                pending: PendingComment::Remark { path },
            });
            return;
        }
        self.do_remark(path);
    }

    fn do_remark(&mut self, path: Path) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        match doc.apply(Mutation::Remark { path }) {
            Ok(()) => self.on_mutation_success(),
            Err(MutateError::Fragment(_)) => {
                self.status = Some("not valid comment, kept as-is".into());
            }
            Err(e) => self.error = Some(format!("remark error: {e}")),
        }
    }

    // ---- Undo / Redo ----

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
                self.status = None;
            }
            Err(e) => self.error = Some(format!("undo restore error: {e}")),
        }
    }

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
                self.status = None;
            }
            Err(e) => self.error = Some(format!("redo restore error: {e}")),
        }
    }

    // ---- Escape ----

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
            Mode::Normal => {
                if self.clipboard.is_some() {
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

    // ---- Prompt key handler ----

    pub fn handle_prompt_key(&mut self, c: char) -> bool {
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
                    _ => {
                        if let Some((e, _)) = self.pending_edit.take() {
                            self.mode = Mode::Edit(e);
                        } else {
                            self.mode = Mode::Normal;
                        }
                    }
                }
                false // not quit
            }
            Mode::Prompt(PromptKind::Collision { key: _ }) => {
                let oc = match c {
                    'o' => OnCollision::Overwrite,
                    'r' => OnCollision::Rename,
                    _ => OnCollision::Cancel,
                };
                if !matches!(c, 'o' | 'r') {
                    self.mode = Mode::Normal;
                    self.clipboard = None;
                    self.status = None;
                    return false;
                }
                let cb = self.clipboard.take();
                let (fragments, is_cut, sources) = match cb {
                    Some(cb) => (cb.fragments, cb.cut, cb.sources),
                    None => {
                        self.mode = Mode::Normal;
                        return false;
                    }
                };
                let rows = self.visible_rows();
                let cursor_row = match rows.iter().find(|r| r.path == self.cursor).cloned() {
                    Some(r) => r,
                    None => {
                        self.mode = Mode::Normal;
                        return false;
                    }
                };
                let expanded = self.expanded.contains(&cursor_row.path);
                let sibling_index = self.true_sibling_index(&cursor_row.path);
                let target = crate::session::insertion::resolve_target(
                    &cursor_row.path,
                    cursor_row.is_branch,
                    expanded,
                    sibling_index,
                );
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
                false
            }
            Mode::Prompt(PromptKind::ArrayUpgrade { .. }) => {
                if c != 'y' {
                    self.mode = Mode::Normal;
                    self.status = Some("paste cancelled — clipboard kept".into());
                    return false;
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
                false
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
                                PendingComment::Remark { path } => self.do_remark(path),
                            }
                        }
                    }
                    _ => {
                        self.mode = self.resting_mode();
                    }
                }
                false
            }
            Mode::Prompt(PromptKind::ConfirmQuit) => match c {
                'y' => {
                    self.mode = Mode::Normal;
                    self.clipboard = None;
                    self.status = None;
                    true // quit
                }
                _ => {
                    self.mode = Mode::Normal;
                    self.clipboard = None;
                    self.status = None;
                    false
                }
            },
            _ => false,
        }
    }

    pub fn confirm_quit(&self) -> bool {
        matches!(&self.mode, Mode::Prompt(PromptKind::ConfirmQuit))
    }

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

    // ---- Utilities ----

    pub fn serialize(&self) -> Option<String> {
        self.doc.as_ref().map(|d| d.serialize())
    }

    pub fn is_dirty(&self) -> bool {
        self.doc.as_ref().map(|d| d.is_dirty()).unwrap_or(false)
    }

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

    /// Test helper: place cursor on visible row at index `i`.
    #[cfg(test)]
    pub fn select_row(&mut self, i: usize) {
        let rows = self.visible_rows();
        self.cursor = rows[i].path.clone();
    }

    /// Test helper: path of visible row at index `i`.
    #[cfg(test)]
    pub fn row_path(&self, i: usize) -> Path {
        self.visible_rows()[i].path.clone()
    }

    /// Test helper: keys of all visible rows.
    #[cfg(test)]
    pub fn visible_keys(&self) -> Vec<String> {
        self.visible_rows().iter().map(|r| r.key.clone()).collect()
    }
}

// ---- Free functions (CORE) ----

pub fn node_type_label_str(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::Root => "",
        NodeKind::Table => "table",
        NodeKind::ArrayOfTables => "array-of-tables",
        NodeKind::Array => "array",
        NodeKind::InlineTable => "inline",
        NodeKind::Scalar(ScalarType::String) => "string",
        NodeKind::Scalar(ScalarType::Integer) => "integer",
        NodeKind::Scalar(ScalarType::Float) => "float",
        NodeKind::Scalar(ScalarType::Bool) => "bool",
        NodeKind::Scalar(ScalarType::Null) => "null",
        NodeKind::Scalar(ScalarType::OffsetDatetime) => "offsetdatetime",
        NodeKind::Scalar(ScalarType::LocalDatetime) => "localdatetime",
        NodeKind::Scalar(ScalarType::LocalDate) => "localdate",
        NodeKind::Scalar(ScalarType::LocalTime) => "localtime",
        NodeKind::Comment(_) => "comment",
    }
}

/// The full type label for a node kind (matches node_type_label in app.rs).
pub fn node_type_label(kind: &NodeKind) -> String {
    match kind {
        NodeKind::Scalar(st) => format!("{st:?}").to_lowercase(),
        other => node_type_label_str(other).to_string(),
    }
}

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

pub fn format_label(fmt: Format) -> Option<&'static str> {
    match fmt {
        Format::Literal => Some("literal"),
        Format::MultilineBasic => Some("multiline-basic"),
        Format::MultilineLiteral => Some("multiline-literal"),
        Format::Hex => Some("hex"),
        Format::Octal => Some("octal"),
        Format::Binary => Some("binary"),
        Format::Inline => Some("inline"),
        Format::Dotted => Some("dotted"),
        Format::Scope => Some("scope"),
        Format::Multiline => Some("multiline"),
        Format::SingleQuoted => Some("single-quoted"),
        Format::DoubleQuoted => Some("double-quoted"),
        Format::LiteralBlock => Some("literal-block"),
        Format::Folded => Some("folded"),
        Format::Block => Some("block"),
        Format::Inf => Some("inf"),
        Format::Nan => Some("nan"),
        Format::Exponent => Some("exponent"),
        Format::BasicString => Some("basic-string"),
        Format::Decimal => Some("decimal"),
        Format::Plain => None,
    }
}

fn char_byte_idx(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map(|(i, _)| i).unwrap_or(s.len())
}

fn clamp_scroll(scroll: usize, cursor: usize, len: usize, width: usize) -> usize {
    let w = width.max(1);
    let cur = cursor.min(len);
    let mut s = scroll;
    if cur < s {
        s = cur;
    } else if cur >= s + w {
        s = cur + 1 - w;
    }
    s.min((len + 1).saturating_sub(w))
}

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

fn regroup_int(repr: &str, fmt: Format) -> String {
    match fmt {
        Format::Hex | Format::Octal | Format::Binary => {
            let (prefix, digits) = repr.split_at(2);
            format!("{prefix}{}", group_right(digits, 4))
        }
        _ => {
            let (sign, digits) = repr.strip_prefix('-').map_or(("", repr), |d| ("-", d));
            format!("{sign}{}", group_right(digits, 3))
        }
    }
}

fn regroup_float(repr: &str) -> String {
    let (sign, body) = repr.strip_prefix('-').map_or(("", repr), |d| ("-", d));
    match body.split_once('.') {
        Some((int, frac)) => {
            format!("{sign}{}.{}", group_right(int, 3), group_left(frac, 3))
        }
        None => format!("{sign}{}", group_right(body, 3)),
    }
}
