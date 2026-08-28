use super::status_fmt::{
    branch_type_format, char_byte_idx, default_ext, format_label, key_sign_label, node_type_label,
    node_type_label_str,
};
use crate::model::any_doc::AnyDocument;
use crate::model::document::{ConfigDocument, DocFormat, Mutation, OnCollision, Target};
use crate::model::node::{Format, Node, NodeKind, NodeTree, Path, Seg, VisibleRow};
use crate::session::i18n::{tr_args, Lang};
use crate::session::notice::Notice;
use crate::session::search::{fuzzy_match, haystack};
use crate::session::selection::Selection;
use crate::session::state::{
    Clipboard, EditKind, EditState, FilterLayer, HelpTab, History, KindSwitchState, Mode,
    PasteSlot, PendingCommit, PendingExternalEdit, PromptKind,
};
use crate::session::type_filter::TypeFilter;
use crate::session::view::{ChildView, OutlineNode, ViewRow};
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
    pub notice: Option<Notice>,
    pub diag: crate::session::diag::DiagRing,
    pub schema: Option<crate::schema::SchemaState>,
    pub pending_schema_fetch: Option<crate::schema::SchemaSource>,
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
    /// In-flight async external edit (WASM §8.2); `None` except between the
    /// `BeginEdit` that routes external and the resolving `ApplyReplace`/`ApplyEditComment`.
    pub pending_external_edit: Option<PendingExternalEdit>,
    /// Set when a one-shot `commit_edit` (Web `CommitEdit`) deferred to a
    /// confirmation prompt: `Some(from_detail)`. The prompt resolution must not
    /// fall back into `Mode::Edit` (the one-shot host has no live editor) and —
    /// when `true` — returns to `Mode::Detail` so the host's panel stays open.
    pub prompt_from_commit_edit: Option<bool>,
    /// Active UI language (§i18n Phase 1). Drives `tr`/`tr_args` lookups for
    /// notice text; default `En`.
    pub lang: Lang,
    /// Host-supplied: true iff the open document's real file extension is
    /// plain `.json` (not `.jsonc`) — confy-core itself is extension-blind
    /// (`DocFormat::Json` covers both), so only the host knows this. Drives
    /// the per-row `comment_advisory` decoration on `ViewRow`: a comment in
    /// such a file is non-standard JSON silently upgraded to JSONC, worth an
    /// advisory even though the edit itself is never blocked. Set once after
    /// `Session::new`/`from_tree`; never toggled by mutations. Default `false`.
    pub strict_json: bool,
}

/// Paste-mode slot navigation step: a relative move or a jump to either edge.
enum SlotMove {
    Delta(isize),
    Home,
    End,
}

/// Parse a fetched schema document's text as JSON — tolerant of `//`/`/* */`
/// comments anywhere in the text (a schema authored JSONC-style), by going
/// through the project's own lossless JSON/JSONC parser rather than
/// `serde_json::from_str` directly. `Err` carries a display-ready message,
/// matching `serde_json::Error`'s `Display` shape closely enough for
/// `apply_schema_text`'s existing "schema is not valid JSON: {e}" wrapping.
fn parse_schema_json(text: &str) -> Result<serde_json::Value, String> {
    let doc = AnyDocument::from_str_as(text, DocFormat::Json).map_err(|e| e.to_string())?;
    let (value, _warnings) = doc.to_value().map_err(|e| e.to_string())?;
    Ok(crate::schema::value_bridge::value_to_json(&value))
}

impl Session {
    /// Construct a Session backed by a real document.
    pub fn new(doc: AnyDocument) -> Self {
        let tree = doc.project();
        let history = History::new(doc.serialize());
        let mut s = Session::from_tree(tree);
        s.doc = Some(doc);
        s.history = Some(history);
        s.pending_schema_fetch = s.detect_and_request_schema();
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
            notice: None,
            diag: Default::default(),
            schema: None,
            pending_schema_fetch: None,
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
            pending_external_edit: None,
            prompt_from_commit_edit: None,
            lang: Lang::default(),
            strict_json: false,
        }
    }

    /// Sole write path for `notice` — every core/host notice assignment
    /// goes through here so the diag ring sees "what did the user see, in
    /// order" for every notice, not just host ones (design spec §7, §12 Q3).
    pub fn set_notice(&mut self, notice: Notice) {
        self.diag.push(
            crate::session::diag::DiagLevel::Info,
            "notice",
            format!(
                "severity={:?} source={:?} text={:?}",
                notice.severity, notice.source, notice.text
            ),
        );
        self.notice = Some(notice);
    }

    /// Switch the active UI language. Subsequent notice text uses the new
    /// language's catalog; any showing notice is cleared so stale
    /// old-language text never lingers (§12 Q12).
    pub fn set_lang(&mut self, lang: Lang) {
        self.lang = lang;
        self.notice = None;
    }

    /// Pure: flatten the tree through the expand set and filter — borrowed
    /// rows, zero clones. Cursor/selection/lookup helpers use this;
    /// `visible_rows` builds the owned `ViewRow` transport on top of it.
    fn visible_nodes(&self) -> Vec<VisibleRow<'_>> {
        let expanded = &self.expanded;
        let rows = self.tree.flatten(&|p| expanded.contains(p));
        match &self.filtered_paths {
            Some(fp) => rows
                .into_iter()
                .filter(|r| fp.contains(&r.node.path))
                .collect(),
            None => rows,
        }
    }

    /// Pure: flatten the tree through the expand set and filter, baking in
    /// selection + cursor flags. No side effects.
    pub fn visible_rows(&self) -> Vec<ViewRow> {
        self.visible_nodes()
            .into_iter()
            .map(|r| self.to_view_row(r.node, r.depth))
            .collect()
    }

    /// Build one `ViewRow` transport struct from a tree `Node` + its depth.
    /// Single source of truth for `visible_rows()`'s per-row projection and
    /// `view_row_at()`'s direct single-path lookup — the two must never drift.
    fn to_view_row(&self, node: &Node, depth: usize) -> ViewRow {
        let scalar_type = match &node.kind {
            NodeKind::Scalar(st) => Some(*st),
            _ => None,
        };
        ViewRow {
            path: node.path.clone(),
            depth,
            is_branch: node.is_branch(),
            key: node.key.clone(),
            value: node.value.clone(),
            scalar_type,
            format: node.format,
            type_label: node_type_label_str(&node.kind).to_string(),
            child_count: node.children.len(),
            trailing_comment: node.trailing_comment.clone(),
            key_sign: key_sign_label(node.key_sign).to_string(),
            read_only: node.read_only,
            selected: self.selection.contains(&node.path),
            is_cursor: node.path == self.cursor,
            violations: self.schema.as_ref().and_then(|s| {
                let msgs: Vec<String> = s
                    .violations
                    .iter()
                    .filter(|v| v.path == node.path)
                    .map(|v| v.message.clone())
                    .collect();
                (!msgs.is_empty()).then_some(msgs)
            }),
            has_descendant_violation: node.is_branch()
                && self
                    .schema
                    .as_ref()
                    .is_some_and(|s| s.warning_ancestors.contains(&node.path)),
            comment_advisory: self.comment_advisory_for(node),
        }
    }

    /// `Some(message)` when `node` is a standalone comment or carries a
    /// trailing comment, and `strict_json` flags the open document as a
    /// plain `.json` (not `.jsonc`) file. See `ViewRow::comment_advisory`.
    fn comment_advisory_for(&self, node: &Node) -> Option<String> {
        if !self.strict_json {
            return None;
        }
        let is_comment = matches!(node.kind, NodeKind::Comment(_)) || node.trailing_comment.is_some();
        is_comment.then(|| tr_args(self.lang, "core.comment.advisory", &[]))
    }

    /// Whether `path` is currently visible: every ancestor prefix must be
    /// expanded (mirrors `NodeTree::flatten`'s descent gate) and, when a
    /// filter is active, `path` itself must be a filter match. O(depth), not
    /// O(document size) — replicated here instead of delegating to
    /// `visible_nodes()` so `view_row_at` never pays for a full-tree flatten
    /// just to check one path.
    fn is_path_visible(&self, path: &Path) -> bool {
        if let Some(fp) = &self.filtered_paths {
            if !fp.contains(path) {
                return false;
            }
        }
        (0..path.len()).all(|i| self.expanded.contains(&path[..i]))
    }

    /// O(depth) lookup of the `ViewRow` for one path, without materializing
    /// the full `visible_rows()` list — for the many call sites that only
    /// ever need one row (usually the cursor's), not the whole visible
    /// ordering. Returns `None` exactly when `path` would be absent from
    /// `visible_rows()` (not found in the tree, or hidden by a collapsed
    /// ancestor / active filter) — same semantics, cheaper path.
    pub fn view_row_at(&self, path: &Path) -> Option<ViewRow> {
        if !self.is_path_visible(path) {
            return None;
        }
        let node = self.tree.node_at(path)?;
        Some(self.to_view_row(node, path.len()))
    }

    /// `view_row_at(&self.cursor)` — the single most common single-row lookup.
    pub fn cursor_row(&self) -> Option<ViewRow> {
        self.view_row_at(&self.cursor)
    }

    /// Stateful rebuild: compute visible rows, snap cursor, clear stale paste slot.
    /// Returns the new rows for the host to map to RowSnapshot.
    pub fn compute_rows(&mut self) -> Vec<ViewRow> {
        let rows = self.visible_rows();
        // Snap cursor if path is no longer visible.
        if !rows.iter().any(|r| r.path == self.cursor) {
            self.cursor = rows.first().map(|r| r.path.clone()).unwrap_or_default();
        }
        // Drop a paste slot whose row is no longer visible (stale after a
        // structural change); a still-valid slot survives paste-mode navigation.
        if let Some(PasteSlot::Into(p) | PasteSlot::After(p)) = &self.paste_slot {
            if !rows.iter().any(|r| &r.path == p) {
                self.paste_slot = None;
            }
        }
        rows
    }

    /// Ordered paths of the currently visible rows.
    pub fn visible_paths(&self) -> Vec<Path> {
        self.visible_nodes()
            .iter()
            .map(|r| r.node.path.clone())
            .collect()
    }

    /// Path the cursor is on, if visible.
    pub fn cursor_row_path(&self) -> Option<Path> {
        self.is_path_visible(&self.cursor)
            .then(|| self.cursor.clone())
    }

    /// Cursor's visible-row index.
    pub fn cursor_row_index(&self) -> Option<usize> {
        self.visible_nodes()
            .iter()
            .position(|r| r.node.path == self.cursor)
    }

    /// Place the cursor on a visible row by path (pointer analogue of
    /// `select_row`). No-op if the path is not currently visible.
    pub fn set_cursor(&mut self, path: Path) {
        let visible = self.visible_nodes().iter().any(|r| r.node.path == path);
        if visible {
            self.cursor = path;
        }
    }

    /// **Reveal** (CONTEXT.md §Operations): expand every ancestor prefix of
    /// `path`, then place the cursor on it. Unknown paths are ignored; if an
    /// active filter still hides the row, the expansion sticks, the cursor
    /// stays put, and the status line says so.
    pub fn reveal_path(&mut self, path: Path) {
        if self.tree.node_at(&path).is_none() {
            return;
        }
        for i in 0..path.len() {
            self.expanded.insert(path[..i].to_vec());
        }
        let visible = self.visible_nodes().iter().any(|r| r.node.path == path);
        if visible {
            self.cursor = path.clone();
            // Reveal also selects the target (single-node selection) — except
            // the root, which has no selectable row, and paste mode, where the
            // clipboard freezes the selection.
            if self.clipboard.is_none() && !path.is_empty() {
                self.selection.set_all(vec![path]);
            }
        } else {
            self.set_notice(Notice::core(self.lang, "core.reveal.hidden-by-filter", &[]));
        }
    }

    /// Immediate children of the node at `path`, independent of expansion
    /// state — the Web UI breadcrumb mini-tree's lazy query (read-only,
    /// mirrors the `kind_options` pattern). Unknown paths return an empty list.
    pub fn children_of(&self, path: &Path) -> Vec<ChildView> {
        let Some(node) = self.tree.node_at(path) else {
            return Vec::new();
        };
        node.children
            .iter()
            .map(|c| ChildView {
                key: c.key.clone(),
                path: c.path.clone(),
                type_label: node_type_label(&c.kind),
                is_branch: c.is_branch(),
            })
            .collect()
    }

    /// Read-only outline tree for editor Outline/breadcrumb integrations —
    /// the whole document, independent of `Session`'s own cursor/expansion
    /// state. Root itself is not included (its children are returned
    /// directly); `Comment` nodes are omitted.
    pub fn outline(&self) -> Vec<OutlineNode> {
        fn convert(n: &Node) -> Option<OutlineNode> {
            if matches!(n.kind, NodeKind::Comment(_)) {
                return None;
            }
            Some(OutlineNode {
                key: n.key.clone(),
                path: n.path.clone(),
                type_label: node_type_label(&n.kind),
                value: if n.is_leaf() { n.value.clone() } else { None },
                text_range: (n.text_range.start as u32, n.text_range.end as u32),
                key_text_range: n
                    .key_text_range
                    .as_ref()
                    .map(|r| (r.start as u32, r.end as u32)),
                children: n.children.iter().filter_map(convert).collect(),
            })
        }
        self.tree.root.children.iter().filter_map(convert).collect()
    }

    pub fn cursor_down(&mut self) {
        if self.clipboard.is_some() {
            self.move_paste_slot(SlotMove::Delta(1));
            return;
        }
        let rows = self.visible_nodes();
        let idx = rows
            .iter()
            .position(|r| r.node.path == self.cursor)
            .unwrap_or(0);
        let next = rows.get(idx + 1).map(|r| r.node.path.clone());
        if let Some(p) = next {
            self.cursor = p;
        }
    }

    pub fn cursor_up(&mut self) {
        if self.clipboard.is_some() {
            self.move_paste_slot(SlotMove::Delta(-1));
            return;
        }
        let rows = self.visible_nodes();
        let idx = rows
            .iter()
            .position(|r| r.node.path == self.cursor)
            .unwrap_or(0);
        let prev = idx
            .checked_sub(1)
            .and_then(|i| rows.get(i))
            .map(|r| r.node.path.clone());
        if let Some(p) = prev {
            self.cursor = p;
        }
    }

    pub fn toggle_expand(&mut self) {
        let rows = self.visible_nodes();
        let Some((is_branch, path)) = rows
            .iter()
            .find(|r| r.node.path == self.cursor)
            .map(|r| (r.node.is_branch(), r.node.path.clone()))
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
        let rows = self.visible_nodes();
        let base = match rows.iter().find(|r| r.node.path == self.cursor) {
            Some(r) if r.node.is_branch() => r.node.path.clone(),
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
        let rows = self.visible_nodes();
        let (path, is_branch) = match rows.iter().find(|r| r.node.path == self.cursor) {
            Some(r) => (r.node.path.clone(), r.node.is_branch()),
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
        if target.is_empty() {
            // Never collapse the root itself: like `CollapseAll`, the root
            // always stays expanded so the first-layer nodes remain visible.
            return;
        }
        self.expanded.remove(&target);
        self.cursor = target;
    }

    pub fn page_up(&mut self, page_size: usize) {
        let step = page_size.max(1);
        if self.clipboard.is_some() {
            self.move_paste_slot(SlotMove::Delta(-(step as isize)));
            return;
        }
        let rows = self.visible_nodes();
        let idx = rows
            .iter()
            .position(|r| r.node.path == self.cursor)
            .unwrap_or(0)
            .saturating_sub(step);
        let target = rows.get(idx).map(|r| r.node.path.clone());
        if let Some(p) = target {
            self.cursor = p;
        }
    }

    pub fn page_down(&mut self, page_size: usize) {
        let step = page_size.max(1);
        if self.clipboard.is_some() {
            self.move_paste_slot(SlotMove::Delta(step as isize));
            return;
        }
        let rows = self.visible_nodes();
        let max = rows.len().saturating_sub(1);
        let idx = (rows
            .iter()
            .position(|r| r.node.path == self.cursor)
            .unwrap_or(0)
            + step)
            .min(max);
        let target = rows.get(idx).map(|r| r.node.path.clone());
        if let Some(p) = target {
            self.cursor = p;
        }
    }

    pub fn cursor_home(&mut self) {
        if self.clipboard.is_some() {
            self.move_paste_slot(SlotMove::Home);
            return;
        }
        let first = self.visible_nodes().first().map(|r| r.node.path.clone());
        if let Some(p) = first {
            self.cursor = p;
        }
    }

    pub fn cursor_end(&mut self) {
        if self.clipboard.is_some() {
            self.move_paste_slot(SlotMove::End);
            return;
        }
        let last = self.visible_nodes().last().map(|r| r.node.path.clone());
        if let Some(p) = last {
            self.cursor = p;
        }
    }

    pub fn paste_slots(&self) -> Vec<PasteSlot> {
        let rows = self.visible_nodes();
        let mut slots = Vec::with_capacity(rows.len() * 2);
        for row in rows.iter() {
            if row.node.is_branch() {
                slots.push(PasteSlot::Into(row.node.path.clone()));
            }
            slots.push(PasteSlot::After(row.node.path.clone()));
        }
        slots
    }

    pub fn effective_paste_slot(&self) -> PasteSlot {
        self.paste_slot
            .clone()
            .unwrap_or_else(|| PasteSlot::After(self.cursor.clone()))
    }

    fn move_paste_slot(&mut self, mv: SlotMove) {
        let slots = self.paste_slots();
        if slots.is_empty() {
            return;
        }
        let max = slots.len() - 1;
        let next = match mv {
            SlotMove::Home => 0,
            SlotMove::End => max,
            SlotMove::Delta(delta) => {
                let cur = self.effective_paste_slot();
                let idx = slots.iter().position(|s| *s == cur).unwrap_or(0) as isize;
                (idx + delta).clamp(0, max as isize) as usize
            }
        };
        let slot = slots[next].clone();
        self.cursor = match &slot {
            PasteSlot::Into(p) | PasteSlot::After(p) => p.clone(),
        };
        self.paste_slot = Some(slot);
    }

    pub fn slot_target(&self, slot: PasteSlot) -> Option<Target> {
        let rows = self.visible_nodes();
        match slot {
            PasteSlot::Into(p) => {
                let row = rows.iter().find(|r| r.node.path == p)?;
                Some(Target {
                    parent: row.node.path.clone(),
                    index: row.node.children.len(),
                })
            }
            PasteSlot::After(p) => {
                let row = rows.iter().find(|r| r.node.path == p)?;
                let expanded = self.expanded.contains(&row.node.path);
                let sibling_index = self.true_sibling_index(&row.node.path);
                Some(crate::session::insertion::resolve_target(
                    &row.node.path,
                    row.node.is_branch(),
                    expanded,
                    sibling_index,
                ))
            }
        }
    }

    /// Pointer analogue of arrow-key `PasteSlot` stepping: turn "this row,
    /// this relative vertical position" (`0.0` = row top, `1.0` = row bottom)
    /// into a `PasteSlot`, so every pointer host (Web mouse click, touch tap,
    /// drag-drop into-eligibility) shares one target-classification algorithm
    /// instead of each hand-rolling its own 0.25/0.75 band threshold (ADR
    /// 0004 §1). `None` if `path` is not currently visible.
    ///
    /// Mid-band (`0.25..0.75`) on a branch whose `format != Format::Inline`
    /// (a single-line container has no meaningful "insert into" drop zone) ->
    /// `Into(path)`; an inline branch's whole lower half falls through to
    /// `After(path)`. Bottom band -> `After(path)`. Top band resolves to the
    /// slot immediately preceding this row's own slot(s) in `paste_slots()`'s
    /// flattened order — **not** a tree-sibling computation: for an expanded
    /// branch, `After(that branch)` means "its first child" (`resolve_target`),
    /// so the row before an expanded branch's *next sibling* is that branch's
    /// *last descendant*, not the branch itself. Reusing `paste_slots()`
    /// directly (rather than re-deriving sibling/parent indices by hand)
    /// keeps this provably in sync with the TUI's own arrow-key stepping.
    pub fn pointer_slot(&self, path: &Path, rel_y: f32) -> Option<PasteSlot> {
        let row = self
            .visible_nodes()
            .into_iter()
            .find(|r| &r.node.path == path)?;
        let into_eligible = row.node.is_branch() && row.node.format != Format::Inline;
        if into_eligible && rel_y > 0.25 && rel_y < 0.75 {
            return Some(PasteSlot::Into(path.clone()));
        }
        // Bottom band — and the whole lower half of an inline branch, which
        // has no "insert into" drop zone — is `After` this row.
        if rel_y >= 0.75 || (row.node.is_branch() && rel_y > 0.25) {
            return Some(PasteSlot::After(path.clone()));
        }
        let slots = self.paste_slots();
        // The row's own FIRST slot in `paste_slots()`'s flattened order:
        // `Into` for any branch (`paste_slots` emits it even for inline
        // branches), `After` for a leaf.
        let mine = if row.node.is_branch() {
            PasteSlot::Into(path.clone())
        } else {
            PasteSlot::After(path.clone())
        };
        let mine_idx = slots.iter().position(|s| *s == mine)?;
        Some(slots.get(mine_idx.wrapping_sub(1)).cloned().unwrap_or(mine))
    }

    /// Pointer analogue of the TUI's arrow-key `PasteSlot` stepping: set the
    /// armed clipboard's target directly (Web UI/touch `Intent::SetPasteSlot`,
    /// built from `pointer_slot`). No-op if the slot's path is not currently
    /// visible — mirrors `set_cursor`'s guard, so a stale click (row
    /// scrolled/collapsed away between the pointer event and dispatch) can't
    /// arm a target the tree no longer shows. Also moves `cursor` onto the
    /// slot's row, mirroring `move_paste_slot`'s keyboard-driven sync — no
    /// row is ever painted with the plain cursor style while armed (`body:not(
    /// .paste-mode) .row.cursor`/`.app:not(.paste-mode) .row.cursor` in
    /// `web/style.css`/`web/touch/style.css`), so this is purely functional:
    /// it keeps the auto-scroll-to-cursor behavior (`.row.cursor` in
    /// `web/render.ts`) following the pointer-driven target the same way it
    /// already follows the keyboard-driven one.
    pub fn set_paste_slot(&mut self, slot: PasteSlot) {
        let path = match &slot {
            PasteSlot::Into(p) | PasteSlot::After(p) => p,
        };
        let visible = self.visible_nodes().iter().any(|r| &r.node.path == path);
        if visible {
            self.cursor = path.clone();
            self.paste_slot = Some(slot);
        }
    }

    pub fn is_expanded(&self, path: &Path) -> bool {
        self.expanded.contains(path)
    }

    pub(crate) fn resting_mode(&self) -> Mode {
        if self.filtered_paths.is_some() {
            Mode::FilterResults
        } else {
            Mode::Normal
        }
    }

    /// Returns `true` (and sets a notice) when the clipboard is armed,
    /// signalling the caller to return early — ADR 0005 §5 modal lock.
    pub(crate) fn guard_clipboard_locked(&mut self) -> bool {
        if self.clipboard.is_some() {
            self.set_notice(Notice::core(self.lang, "core.clipboard.action-locked", &[]));
            true
        } else {
            false
        }
    }

    pub fn doc_format(&self) -> DocFormat {
        self.doc.as_ref().map_or(DocFormat::Toml, |d| d.format())
    }

    pub fn enter_filter(&mut self) {
        if self.guard_clipboard_locked() {
            return;
        }
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

    /// Set the whole filter text at once (Web UI live-search `<input>`) and
    /// recompute, instead of replaying `FilterChar`. Non-empty text lands in
    /// `FilterResults`; clearing it drops to the resting mode (still
    /// `FilterResults` if a type filter is narrowing the tree).
    pub fn set_filter(&mut self, text: String) {
        self.filter = text;
        self.filter_cursor = self.filter.chars().count();
        self.recompute_filter();
        if self.filter.is_empty() {
            self.last_filter_applied = self.type_filter.is_active().then_some(FilterLayer::Type);
            self.mode = self.resting_mode();
        } else {
            self.last_filter = self.filter.clone();
            self.last_filter_applied = Some(FilterLayer::Text);
            self.mode = Mode::FilterResults;
        }
    }

    pub fn recompute_filter(&mut self) {
        if self.filter.is_empty() && !self.type_filter.is_active() {
            self.filtered_paths = None;
            return;
        }
        let mut matching: HashSet<Path> = HashSet::new();
        let mut ancestors: HashSet<Path> = HashSet::new();
        let violating: HashSet<Path> = self
            .schema
            .as_ref()
            .map(|s| s.violations.iter().map(|v| v.path.clone()).collect())
            .unwrap_or_default();
        fn walk(
            n: &crate::model::node::Node,
            ancestor_paths: &mut Vec<Path>,
            matching: &mut HashSet<Path>,
            ancestors: &mut HashSet<Path>,
            needle: &str,
            type_filter: &TypeFilter,
            doc: DocFormat,
            violating: &HashSet<Path>,
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
            // A scalar leaf's value is part of the haystack so a search matches
            // values, not just keys/paths/comments.
            let leaf_value = match &n.kind {
                NodeKind::Scalar(_) => n.value.as_deref(),
                _ => None,
            };
            let h = haystack(&path_keys, leaf_value, comment_text);
            let text_ok = fuzzy_match(&h, needle);
            let has_warning = violating.contains(&n.path);
            let has_comment = comment_text.is_some() || n.trailing_comment.is_some();
            let type_ok = type_filter.matches(
                n.key_sign,
                &n.kind,
                n.format,
                doc,
                n.read_only,
                has_warning,
                has_comment,
            );
            if text_ok && type_ok {
                matching.insert(n.path.clone());
                for anc in ancestor_paths.iter() {
                    ancestors.insert(anc.clone());
                }
            }
            // A container that's a deliberate Reverse-exclusion target (its own
            // sign/type facet was selected, so Reverse specifically hid it) has
            // its whole subtree pruned here — otherwise a descendant that
            // legitimately passes the reversed filter would drag this node
            // back in via the ancestor-context rule below, making Reverse look
            // like a no-op on Table/Array (leaves have no children, so this
            // never applied to Scalar/Comment).
            if type_filter.is_reverse_excluded(
                n.key_sign,
                &n.kind,
                n.format,
                doc,
                n.read_only,
                has_warning,
                has_comment,
            ) {
                return;
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
                    violating,
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
            &violating,
        );
        matching.extend(ancestors);
        self.filtered_paths = Some(matching);
    }

    pub fn enter_type_filter(&mut self) {
        if self.guard_clipboard_locked() {
            return;
        }
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

    pub fn open_kind_switch(&mut self) {
        if self.guard_clipboard_locked() {
            return;
        }
        let Some(path) = self
            .visible_nodes()
            .iter()
            .find(|r| r.node.path == self.cursor)
            .map(|r| r.node.path.clone())
        else {
            return;
        };
        let Some(doc) = &self.doc else {
            return;
        };
        let options = doc.kind_options(&path);
        if options.is_empty() {
            self.set_notice(Notice::core(self.lang, "core.kind-switch.unsupported", &[]));
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
                self.on_mutation_success(None);
                self.set_notice(Notice::core(self.lang, "core.kind-switch.converted", &[&label]));
            }
            Err(e) => {
                self.set_notice(Notice::core(self.lang, "core.kind-switch.error", &[&e.to_string()]))
            }
        }
    }

    pub fn exit_kind_switch(&mut self) {
        self.mode = self.resting_mode();
        self.notice = None;
    }

    /// One-shot kind switch for the Web UI (`Intent::CommitKind`): apply
    /// `ConvertKind` directly from an explicit `(path, target)` — the pointer
    /// analogue of `open_kind_switch` + `kind_switch_commit`, with no popup dance.
    /// `target` must come from `kind_options(path)`.
    pub fn commit_kind(&mut self, path: Path, target: crate::model::document::KindTarget) {
        if self.guard_clipboard_locked() {
            return;
        }
        self.mode = self.resting_mode();
        let Some(doc) = self.doc.as_mut() else {
            return;
        };
        match doc.apply(Mutation::ConvertKind { path, target }) {
            Ok(()) => {
                self.on_mutation_success(None);
                self.set_notice(Notice::core(self.lang, "core.kind-switch.converted-generic", &[]));
            }
            Err(e) => {
                self.set_notice(Notice::core(self.lang, "core.kind-switch.error", &[&e.to_string()]))
            }
        }
    }

    pub fn open_convert(&mut self) {
        if self.guard_clipboard_locked() {
            return;
        }
        let Some(is_root) = self
            .visible_nodes()
            .iter()
            .find(|r| r.node.path == self.cursor)
            .map(|r| r.node.path.is_empty())
        else {
            return;
        };
        if !is_root {
            self.set_notice(Notice::core(self.lang, "core.convert.root-only", &[]));
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
            let ext = default_ext(target);
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

    /// Web UI: pick the convert target by value (a `<select>`) rather than by
    /// cursor, and reseed the output path's extension. Mirrors
    /// `convert_pick_format` minus the host-supplied stem.
    pub fn set_convert_format(&mut self, fmt: DocFormat) {
        if let Mode::Convert(st) = &mut self.mode {
            if let Some(i) = st.options.iter().position(|f| *f == fmt) {
                st.cursor = i;
            }
            st.target = fmt;
            st.path = format!("out.{}", default_ext(fmt));
            st.path_cursor = st.path.chars().count();
            st.step = crate::session::state::ConvertStep::Path;
        }
    }

    /// Web UI: set the whole output path at once (an `<input>`), instead of
    /// replaying `ConvertPathChar`.
    pub fn set_convert_path(&mut self, path: String) {
        if let Mode::Convert(st) = &mut self.mode {
            st.path_cursor = path.chars().count();
            st.path = path;
        }
    }

    /// Run the conversion. Returns `Some((output_path, text))` when a write is
    /// needed — the host performs the actual `fs::write`.
    pub fn convert_run(&mut self) -> Option<(String, String)> {
        let (target, path) = match &self.mode {
            Mode::Convert(st) => (st.target, st.path.clone()),
            _ => return None,
        };
        let doc = self.doc.as_ref()?;
        match crate::model::convert::convert(doc, target) {
            Ok(result) => {
                if result.warnings.is_empty() {
                    self.mode = self.resting_mode();
                    Some((path, result.text))
                } else {
                    if let Mode::Convert(st) = &mut self.mode {
                        st.warnings = result.warnings;
                        st.text = result.text;
                        st.step = crate::session::state::ConvertStep::Confirm;
                    }
                    None
                }
            }
            Err(abort) => {
                self.set_notice(Notice::core(self.lang, "core.convert.aborted", &[&abort.to_string()]));
                self.mode = self.resting_mode();
                None
            }
        }
    }

    /// `y` on the Confirm step: signal the host to write the rendered output.
    pub fn convert_confirm(&mut self) -> Option<(String, String)> {
        let (path, text) = match &self.mode {
            Mode::Convert(st) => (st.path.clone(), st.text.clone()),
            _ => return None,
        };
        self.mode = self.resting_mode();
        Some((path, text))
    }

    pub fn exit_convert(&mut self) {
        self.mode = self.resting_mode();
        self.notice = None;
    }

    pub fn toggle_detail(&mut self) {
        if self.guard_clipboard_locked() {
            return;
        }
        if matches!(self.mode, Mode::Detail) {
            self.exit_detail();
        } else {
            self.open_detail();
        }
    }

    pub fn open_detail(&mut self) {
        if self.guard_clipboard_locked() {
            return;
        }
        let rows = self.visible_nodes();
        let node = match rows.iter().find(|r| r.node.path == self.cursor) {
            Some(r) => r.node,
            None => return,
        };
        let dotted = if node.path.is_empty() {
            "(root)".to_string()
        } else {
            let mut s = String::new();
            for seg in &node.path {
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
        let mut detail = if node.is_branch() {
            let (type_str, fmt_str) = branch_type_format(&node.kind);
            let children = node.children.len().to_string();
            [
                tr_args(self.lang, "core.detail.path", &[&dotted]),
                tr_args(self.lang, "core.detail.type", &[type_str]),
                tr_args(self.lang, "core.detail.format", &[fmt_str]),
                tr_args(self.lang, "core.detail.children", &[&children]),
            ]
            .join("\n")
        } else {
            let type_str = match &node.kind {
                NodeKind::Scalar(st) => format!("{st:?}").to_lowercase(),
                other => node_type_label_str(other).to_string(),
            };
            let val_str = node.value.as_deref().unwrap_or("");
            let fmt_str = format_label(node.format).unwrap_or("plain");
            [
                tr_args(self.lang, "core.detail.path", &[&dotted]),
                tr_args(self.lang, "core.detail.type", &[&type_str]),
                tr_args(self.lang, "core.detail.format", &[fmt_str]),
                tr_args(self.lang, "core.detail.value", &[val_str]),
            ]
            .join("\n")
        };
        let sign_str = key_sign_label(node.key_sign);
        detail.push('\n');
        detail.push_str(&tr_args(self.lang, "core.detail.sign", &[sign_str]));
        if let Some(tc) = &node.trailing_comment {
            detail.push('\n');
            detail.push_str(&tr_args(self.lang, "core.detail.comment", &[tc]));
        }
        self.detail_text = Some(detail);
        self.mode = Mode::Detail;
    }

    pub fn exit_detail(&mut self) {
        self.detail_text = None;
        self.mode = self.resting_mode();
    }

    pub fn enter_help(&mut self) {
        if self.guard_clipboard_locked() {
            return;
        }
        self.mode = Mode::Help(HelpTab::Help);
    }

    pub fn exit_help(&mut self) {
        self.mode = Mode::Normal;
    }

    pub fn toggle_help_tab(&mut self) {
        if let Mode::Help(tab) = &mut self.mode {
            *tab = match tab {
                HelpTab::Help => HelpTab::About,
                HelpTab::About => HelpTab::Help,
            };
        }
    }

    pub fn toggle_select(&mut self) {
        if self.clipboard.is_some() {
            return;
        }
        self.selection.toggle(self.cursor.clone());
    }

    /// Pointer analogue of the keyboard selection keys: replace the whole
    /// selection with `paths` (the Web UI resolves click / ⇧-range / ⌘-toggle /
    /// marquee into a final set). Paths not currently visible are dropped, the
    /// set is normalized (a selected descendant of a selected ancestor is
    /// folded away, §6.2), and the cursor follows the focal (last) path.
    pub fn set_selection(&mut self, paths: Vec<Path>) {
        if self.clipboard.is_some() {
            return;
        }
        let visible: std::collections::HashSet<Path> = self.visible_paths().into_iter().collect();
        let kept: Vec<Path> = paths.into_iter().filter(|p| visible.contains(p)).collect();
        if let Some(focal) = kept.last() {
            self.cursor = focal.clone();
        }
        self.selection
            .set_all(crate::session::selection::normalize(kept));
        self.last_action_was_shift_select = false;
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
        if self.selection.is_empty() {
            return self.cursor_row().map(|r| vec![r.path]).unwrap_or_default();
        }
        let paths: Vec<Path> = self.selection.iter().collect();
        crate::session::selection::normalize(paths)
    }

    pub(crate) fn cursor_is_read_only(&self) -> bool {
        self.cursor_row().map(|r| r.read_only).unwrap_or(false)
    }

    pub fn edit_target_kind(&self) -> EditKind {
        let path = match self.cursor_row() {
            Some(r) => r.path,
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
            .map(|d| d.array_member_keys_addressable())
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

    /// Detect an in-file schema hint on the current document. Does **not**
    /// load anything (confy-core is fs-free) — the host resolves the
    /// returned `SchemaSource` (local read or URL fetch) and calls
    /// `apply_schema_text` with the result. Returns `None` (leaving
    /// `self.schema` untouched) when no hint is found — editing proceeds
    /// exactly as before (spec §1).
    pub fn detect_and_request_schema(&mut self) -> Option<crate::schema::SchemaSource> {
        let doc = self.doc.as_ref()?;
        let text = doc.serialize();
        crate::schema::hints::detect_hint(&text, doc.format())
    }

    /// The host resolved `source`'s text (or failed to). `Ok` compiles and
    /// validates; `Err` sets a soft `load_error` — never touches
    /// `self.notice`, and the document stays fully editable either way
    /// (spec §1: "never blocks opening, editing, or saving").
    pub fn apply_schema_text(
        &mut self,
        source: crate::schema::SchemaSource,
        text: Result<String, String>,
    ) {
        // Resolved (success or failure) — mirrors how `ApplyReplace`/
        // `ApplyEditComment` explicitly clear `pending_external_edit` once
        // its outstanding request is answered. Must NOT be drained on every
        // unrelated `dispatch()` call (see `apply()`'s `ApplyOutcome`
        // construction) or a host that issues other dispatches before ever
        // looking at `schema_fetch_request` loses the request outright.
        self.pending_schema_fetch = None;
        let state = match text {
            Ok(text) => match parse_schema_json(&text) {
                Ok(raw) => match jsonschema::Validator::new(&raw) {
                    Ok(compiled) => {
                        let fully_analyzable =
                            crate::schema::dirty_check::is_fully_analyzable(&raw);
                        crate::schema::SchemaState {
                            source,
                            compiled: Some(compiled),
                            raw: Some(raw),
                            fully_analyzable,
                            violations: Vec::new(),
                            warning_ancestors: std::collections::HashSet::new(),
                            load_error: None,
                        }
                    }
                    Err(e) => crate::schema::SchemaState {
                        source,
                        compiled: None,
                        raw: None,
                        fully_analyzable: false,
                        violations: Vec::new(),
                        warning_ancestors: std::collections::HashSet::new(),
                        load_error: Some(format!("invalid schema: {e}")),
                    },
                },
                Err(e) => crate::schema::SchemaState {
                    source,
                    compiled: None,
                    raw: None,
                    fully_analyzable: false,
                    violations: Vec::new(),
                    warning_ancestors: std::collections::HashSet::new(),
                    load_error: Some(format!("schema is not valid JSON: {e}")),
                },
            },
            Err(msg) => crate::schema::SchemaState {
                source,
                compiled: None,
                raw: None,
                fully_analyzable: false,
                violations: Vec::new(),
                warning_ancestors: std::collections::HashSet::new(),
                load_error: Some(msg),
            },
        };
        self.schema = Some(state);
        self.revalidate_schema();
    }

    /// Re-run validation against the current tree. Called after every
    /// successful mutation commit and once right after `apply_schema_text`.
    /// A no-op when no schema is loaded or it failed to compile.
    pub fn revalidate_schema(&mut self) {
        let Some(state) = self.schema.as_mut() else {
            return;
        };
        let Some(compiled) = state.compiled.as_ref() else {
            return;
        };
        let Some(doc) = self.doc.as_ref() else { return };
        let Ok((value, _warnings)) = doc.to_value() else {
            // A YAML opaque node or similar blocks `to_value()` — leave the
            // previous violation list rather than silently clearing it.
            return;
        };
        let (projection, map) = crate::schema::value_bridge::bridge(&self.tree.root, &value);
        state.violations = crate::schema::validate::validate(&projection, compiled, &map);
        state.warning_ancestors = state
            .violations
            .iter()
            .flat_map(|v| (0..v.path.len()).map(|i| v.path[..i].to_vec()))
            .collect();
    }

    /// Current schema violations, each carrying its node's resolved
    /// `text_range` — the native-editor Diagnostics data source (VS Code
    /// schema-hints design). Empty when no schema is loaded or there are no
    /// violations.
    pub fn schema_violations(&self) -> Vec<crate::schema::ViolationView> {
        let Some(state) = self.schema.as_ref() else {
            return Vec::new();
        };
        state
            .violations
            .iter()
            .map(|v| crate::schema::ViolationView {
                path: v.path.clone(),
                pointer: v.pointer.clone(),
                keyword: v.keyword.clone(),
                message: v.message.clone(),
                category: v.category,
                text_range: self
                    .tree
                    .node_at(&v.path)
                    .map(|n| (n.text_range.start as u32, n.text_range.end as u32)),
            })
            .collect()
    }

    /// Resolve the schema-driven editing hint for the node at `path` —
    /// enum/const options, numeric bounds, or `EditHint::None` when
    /// unconstrained or no schema is loaded. Read-only, cheap (same
    /// `resolve_edit_hint` walk `begin_inline_edit_impl`/`nudge` already do),
    /// no I/O. Used by hosts for a hover tooltip (spec §4) and to decide
    /// whether the detail panel should render a schema-select widget before
    /// entering edit mode (spec §2), without a `BeginEdit` round-trip.
    pub fn edit_hint(&self, path: &Path) -> crate::schema::EditHint {
        self.schema
            .as_ref()
            .and_then(|s| s.raw.as_ref())
            .map(|raw| crate::schema::hints_edit::resolve_edit_hint(raw, path))
            .unwrap_or(crate::schema::EditHint::None)
    }

    /// Non-widget descriptive schema info for the node at `path` —
    /// `description`/`type`/`format`/`pattern` from the resolved subschema,
    /// `None` when unresolvable or none of those keywords are present.
    /// Orthogonal to `edit_hint`: that resolves a *widget* (enum/const
    /// picker, numeric bounds) and stays `None` for the common plain-typed
    /// case; this exists so hosts have something to show even then. Same
    /// cheap, read-only, no-I/O shape as `edit_hint` — used by the TUI
    /// Detail popup and the shared web/touch/VS Code detail panel.
    pub fn schema_info(&self, path: &Path) -> Option<String> {
        self.schema
            .as_ref()
            .and_then(|s| s.raw.as_ref())
            .and_then(|raw| crate::schema::hints_edit::resolve_schema_info(raw, path))
    }

    /// After a value commit, surface any resulting schema violation at
    /// `path` as an advisory notice (spec §3). The commit
    /// already succeeded — schema constraints are soft (`CONTEXT.md` §
    /// Schema) — this never blocks or reverts anything, it only informs.
    /// Combines the violation message(s) with a `resolve_edit_hint`-derived
    /// suggestion (valid enum values / numeric bounds) when one applies.
    pub(crate) fn note_schema_violation(&mut self, path: &Path) {
        let Some(state) = self.schema.as_ref() else {
            return;
        };
        let messages: Vec<&str> = state
            .violations
            .iter()
            .filter(|v| &v.path == path)
            .map(|v| v.message.as_str())
            .collect();
        if messages.is_empty() {
            return;
        }
        let mut msg = messages.join("; ");
        if let Some(raw) = state.raw.as_ref() {
            match crate::schema::hints_edit::resolve_edit_hint(raw, path) {
                crate::schema::EditHint::Enum(options) => {
                    let labels: Vec<&str> = options.iter().map(|(l, _)| l.as_str()).collect();
                    if !labels.is_empty() {
                        msg.push_str(&format!(" — valid values: {}", labels.join(", ")));
                    }
                }
                crate::schema::EditHint::Bounded {
                    minimum,
                    maximum,
                    multiple_of,
                } => {
                    let mut parts = Vec::new();
                    match (minimum, maximum) {
                        (Some(min), Some(max)) => parts.push(format!("between {min} and {max}")),
                        (Some(min), None) => parts.push(format!("at least {min}")),
                        (None, Some(max)) => parts.push(format!("at most {max}")),
                        (None, None) => {}
                    }
                    if let Some(m) = multiple_of {
                        parts.push(format!("a multiple of {m}"));
                    }
                    if !parts.is_empty() {
                        msg.push_str(&format!(" — must be {}", parts.join(", ")));
                    }
                }
                crate::schema::EditHint::None => {}
            }
        }
        self.set_notice(Notice::core(self.lang, "core.schema.violation", &[&msg]));
    }

    pub fn schema_enum_move(&mut self, delta: i32) {
        if let crate::session::state::Mode::SchemaEnum(st) = &mut self.mode {
            let len = st.options.len() as i32;
            if len == 0 {
                return;
            }
            st.cursor = ((st.cursor as i32 + delta).rem_euclid(len)) as usize;
        }
    }

    /// Jumps the schema-enum picker cursor by `delta`, clamped to the option
    /// range instead of wrapping (`schema_enum_move`'s ±1 arrow-key step
    /// wraps deliberately; PageUp/PageDown/Home/End should stop at the ends,
    /// same convention as `type_filter::move_cursor`). Callers land exactly
    /// on the first/last option for Home/End by passing a `delta` at least
    /// as large as the option count in either direction — the clamp does
    /// the rest, so callers don't need the exact length.
    pub fn schema_enum_jump(&mut self, delta: i32) {
        if let crate::session::state::Mode::SchemaEnum(st) = &mut self.mode {
            let len = st.options.len() as i32;
            if len == 0 {
                return;
            }
            st.cursor = (st.cursor as i32 + delta).clamp(0, len - 1) as usize;
        }
    }

    /// `touched`: the mutation's target `Path`, when the caller has exactly
    /// one available (most call sites do — `apply_replace`'s value/rename
    /// commit, the dominant "one keystroke" path per the audit's own Critical
    /// finding). `None` for multi-path operations (paste, delete-selected,
    /// structural inserts) — always revalidates, identical to pre-Task-14
    /// behavior. See `schema::dirty_check` for the skip condition itself.
    pub(crate) fn on_mutation_success(&mut self, touched: Option<&Path>) {
        if let Some(doc) = self.doc.as_ref() {
            let snapshot = doc.serialize();
            let tree = doc.project();
            if let Some(h) = self.history.as_mut() {
                h.push(snapshot);
            }
            self.tree = tree;
        }
        self.notice = None;
        let skip_revalidate = match (touched, self.schema.as_ref()) {
            (Some(path), Some(schema)) if schema.fully_analyzable => schema
                .raw
                .as_ref()
                .map(|raw| !crate::schema::dirty_check::path_is_constrained(raw, path))
                .unwrap_or(false),
            _ => false,
        };
        if !skip_revalidate {
            self.revalidate_schema();
        }
        self.sync_schema_hint();
    }

    /// Re-detect the in-document schema hint after a mutation and decide
    /// whether the host needs a (re)fetch. Centralizes the dedup logic that
    /// previously lived only in the VS Code extension's `schemaDedup.ts`
    /// (ADR 0007) so every host gets live hint-change detection for free —
    /// not just the one host that happened to re-run `DetectSchema` on every
    /// reparse. Same source + prior success → no-op (skip a redundant
    /// fetch); same source + prior failure → retry; different source →
    /// request a fetch. No hint detected → clear `self.schema`: every host
    /// loads a schema *because* of a detected hint, so a hint that has
    /// disappeared (deleted, or edited into plain text) must drop the
    /// schema along with it.
    pub(crate) fn sync_schema_hint(&mut self) {
        match self.detect_and_request_schema() {
            Some(source) => match &self.schema {
                Some(state) if state.source == source => {
                    if state.load_error.is_some() {
                        self.pending_schema_fetch = Some(source);
                    }
                }
                _ => self.pending_schema_fetch = Some(source),
            },
            None => {
                self.schema = None;
                self.pending_schema_fetch = None;
            }
        }
    }

    pub fn escape(&mut self) {
        self.notice = None;
        // A pending async external edit (§8.2) lives outside `Mode` — Esc/Cancel
        // from the host's multi-line editor must discard it, else the snapshot's
        // `external_edit` stays set and the host reopens the modal forever.
        if self.pending_external_edit.take().is_some() {
            return;
        }
        match &self.mode {
            Mode::Prompt(_) => {
                self.mode = Mode::Normal;
                self.clipboard = None;
                self.pending_edit = None;
                // Esc on a one-shot (Web panel) prompt returns to the panel.
                if self.prompt_from_commit_edit.take() == Some(true) {
                    self.open_detail();
                }
            }
            Mode::Filter => self.exit_filter(),
            Mode::FilterResults => self.exit_filter_results(),
            Mode::TypeFilter => self.exit_type_filter(),
            Mode::KindSwitch(_) => self.exit_kind_switch(),
            Mode::SchemaEnum(_) => self.schema_enum_cancel(),
            Mode::Convert(_) => self.exit_convert(),
            Mode::Detail => self.exit_detail(),
            Mode::Help(_) => self.exit_help(),
            Mode::Edit(_) => self.edit_cancel(),
            Mode::Normal => {
                if self.clipboard.is_some() {
                    self.clipboard = None;
                    if !self.selection.is_empty() {
                        self.set_notice(Notice::core(self.lang, "core.clipboard.cleared", &[]));
                    }
                } else if !self.selection.is_empty() {
                    self.selection.clear();
                    self.last_action_was_shift_select = false;
                    self.set_notice(Notice::core(self.lang, "core.selection.cleared", &[]));
                }
            }
        }
    }

    pub fn handle_prompt_key(&mut self, c: char) -> bool {
        match &self.mode {
            Mode::Prompt(PromptKind::TypeChange { .. }) => {
                // A prompt raised by a one-shot Web `CommitEdit` must not fall
                // back into `Mode::Edit` (that host has no live editor); when it
                // came from the Detail panel, return there so the panel stays open.
                let one_shot = self.prompt_from_commit_edit.take();
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
                        if one_shot == Some(true) {
                            self.open_detail();
                        }
                    }
                    _ => match (self.pending_edit.take(), one_shot) {
                        (Some(e_pending), None) => self.mode = Mode::Edit(e_pending.0),
                        (_, Some(true)) => {
                            self.notice = None;
                            self.mode = self.resting_mode();
                            self.open_detail();
                        }
                        _ => {
                            self.notice = None;
                            self.mode = self.resting_mode();
                        }
                    },
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
                    self.notice = None;
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
                let cursor_row = match self.cursor_row() {
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
                    self.set_notice(Notice::core(self.lang, "core.paste.cancelled", &[]));
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
                    None => self.notice = None,
                }
                false
            }
            Mode::Prompt(PromptKind::ConfirmQuit) => match c {
                'y' => {
                    self.mode = Mode::Normal;
                    self.clipboard = None;
                    self.notice = None;
                    true // quit
                }
                _ => {
                    self.mode = Mode::Normal;
                    self.clipboard = None;
                    self.notice = None;
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
            false
        } else {
            true
        }
    }

    pub fn serialize(&self) -> Option<String> {
        self.doc.as_ref().map(|d| d.serialize())
    }

    pub fn is_dirty(&self) -> bool {
        self.doc.as_ref().map(|d| d.is_dirty()).unwrap_or(false)
    }

    pub(crate) fn true_sibling_index(&self, path: &Path) -> usize {
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

#[cfg(test)]
mod helper_tests {
    use super::*;
    use crate::model::node::{Format, Path, ScalarType, Seg};

    use super::super::schema_hint::*;
    use super::super::status_fmt::*;

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
    fn schema_clamp_nudge_snaps_to_multiple_of_and_clamps_to_bounds() {
        use crate::model::any_doc::AnyDocument;
        use crate::schema::{SchemaSource, SchemaState};

        let doc = AnyDocument::from_str_as("port = 1\nretry = 1\n", DocFormat::Toml).unwrap();
        let mut s = Session::new(doc);

        // multipleOf: 5, no min/max. Nudging a non-multiple (1 + delta 1
        // => 2) snaps to the nearest multiple of 5.
        s.schema = Some(SchemaState {
            source: SchemaSource::Local("schema.json".into()),
            compiled: None,
            raw: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "port": { "type": "integer", "multipleOf": 5 }
                }
            })),
            fully_analyzable: false,
            violations: Vec::new(),
            warning_ancestors: std::collections::HashSet::new(),
            load_error: None,
        });
        let port: Path = vec![Seg::Key("port".into())];
        assert_eq!(
            s.schema_clamp_nudge(&port, "2").as_deref(),
            Some("0"),
            "2 snaps to nearest multiple of 5 (0)"
        );
        assert_eq!(
            s.schema_clamp_nudge(&port, "8").as_deref(),
            Some("10"),
            "8 snaps to nearest multiple of 5 (10)"
        );

        // minimum/maximum: nudging past a bound clamps to the bound
        // instead of no-op'ing the whole nudge.
        s.schema = Some(SchemaState {
            source: SchemaSource::Local("schema.json".into()),
            compiled: None,
            raw: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "retry": { "type": "integer", "minimum": 0, "maximum": 3 }
                }
            })),
            fully_analyzable: false,
            violations: Vec::new(),
            warning_ancestors: std::collections::HashSet::new(),
            load_error: None,
        });
        let retry: Path = vec![Seg::Key("retry".into())];
        assert_eq!(
            s.schema_clamp_nudge(&retry, "5").as_deref(),
            Some("3"),
            "5 clamps down to maximum 3"
        );
        assert_eq!(
            s.schema_clamp_nudge(&retry, "-2").as_deref(),
            Some("0"),
            "-2 clamps up to minimum 0"
        );

        // no schema loaded: value passes through unchanged.
        s.schema = None;
        assert_eq!(
            s.schema_clamp_nudge(&port, "2").as_deref(),
            Some("2"),
            "no schema => passthrough"
        );

        // path with no Bounded hint: passthrough.
        s.schema = Some(SchemaState {
            source: SchemaSource::Local("schema.json".into()),
            compiled: None,
            raw: Some(serde_json::json!({
                "type": "object",
                "properties": { "port": { "type": "string" } }
            })),
            fully_analyzable: false,
            violations: Vec::new(),
            warning_ancestors: std::collections::HashSet::new(),
            load_error: None,
        });
        assert_eq!(
            s.schema_clamp_nudge(&port, "2").as_deref(),
            Some("2"),
            "non-Bounded hint => passthrough"
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
}
