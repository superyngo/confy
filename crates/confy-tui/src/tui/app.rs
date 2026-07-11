use std::cell::Cell;
use std::path::PathBuf;

use confy_core::session::Session;
pub use confy_core::session::{EditKind, FilterLayer, PendingCommit};

use crate::model::document::ConfigDocument;
#[cfg(test)]
use crate::model::document::{OnCollision, Target};
use crate::model::node::{Format, NodeKind, NodeTree, Path};
#[cfg(test)]
use crate::tui::state::Clipboard;
use crate::tui::state::{Mode, PasteSlot};

pub struct App {
    pub session: Session,
    /// Render projection of the visible tree — rebuilt by `rebuild_rows`.
    pub rows: Vec<RowSnapshot>,
    /// The source file path (interactive mode). `None` in headless tests.
    pub source_path: Option<PathBuf>,
    /// Vertical scroll offset (in display rows) of the detail popup.
    pub detail_scroll: u16,
    /// Vertical scroll offset (in display rows) of the help overlay.
    pub help_scroll: u16,
    /// Persisted vertical scroll offset (top visible row) of the main tree table.
    pub table_offset: Cell<usize>,
    /// The `l` language picker popup, when open. Host-side mini-mode (not a
    /// core `Mode` variant) — language choice is a host concern since
    /// selecting one also writes the config file (§i18n Phase 2).
    pub lang_picker: Option<LangPickerState>,
}

/// In-flight `l` language-picker state: just the cursor over `LANG_OPTIONS`.
pub struct LangPickerState {
    pub cursor: usize,
}

/// The languages offered by the picker, in display order.
pub const LANG_OPTIONS: [confy_core::session::Lang; 2] = [
    confy_core::session::Lang::En,
    confy_core::session::Lang::ZhTw,
];

/// Host-side view model for ratatui: augments ViewRow with fixed-pitch type_tag.
#[derive(Clone)]
pub struct RowSnapshot {
    pub key: String,
    pub path: Path,
    pub depth: usize,
    pub is_branch: bool,
    pub value: Option<String>,
    pub scalar_type: Option<String>,
    /// Word label for the node's type — used by the detail popup and type-change detection.
    pub type_label: String,
    /// Fixed-pitch TYPE-column tag, e.g. `[S:str ]` (always 8 chars).
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
        let session = Session::new(doc);
        let mut app = App {
            session,
            rows: Vec::new(),
            source_path: None,
            detail_scroll: 0,
            help_scroll: 0,
            table_offset: Cell::new(0),
            lang_picker: None,
        };
        app.rebuild_rows();
        app
    }

    /// Construct a headless App from a pre-built NodeTree (used in unit tests).
    pub fn from_tree(tree: NodeTree) -> Self {
        let session = Session::from_tree(tree);
        let mut app = App {
            session,
            rows: Vec::new(),
            source_path: None,
            detail_scroll: 0,
            help_scroll: 0,
            table_offset: Cell::new(0),
            lang_picker: None,
        };
        app.rebuild_rows();
        app
    }

    /// Rebuild the host's render rows from the session's current view.
    pub fn rebuild_rows(&mut self) {
        let doc_fmt = self.session.doc_format();
        let view_rows = self.session.compute_rows();
        self.rows = view_rows
            .into_iter()
            .map(|vr| {
                // `type_label`/`read_only` already ride on the ViewRow; the tree
                // lookup is needed only for `type_tag`'s NodeKind.
                let type_tag = self
                    .session
                    .tree
                    .node_at(&vr.path)
                    .map(|n| type_tag(&n.kind, vr.format, doc_fmt, n.read_only))
                    .unwrap_or_default();
                let scalar_type = vr.scalar_type.map(|st| format!("{st:?}").to_lowercase());
                RowSnapshot {
                    key: vr.key,
                    path: vr.path,
                    depth: vr.depth,
                    is_branch: vr.is_branch,
                    value: vr.value,
                    scalar_type,
                    type_label: vr.type_label,
                    type_tag,
                    format: vr.format,
                    trailing_comment: vr.trailing_comment,
                }
            })
            .collect();
    }

    // ---- HOST row accessors ----

    pub fn visible_keys(&self) -> Vec<String> {
        self.rows.iter().map(|r| r.key.clone()).collect()
    }

    pub fn visible_paths(&self) -> Vec<Path> {
        self.rows.iter().map(|r| r.path.clone()).collect()
    }

    pub fn cursor_row(&self) -> Option<&RowSnapshot> {
        self.rows.iter().find(|r| r.path == self.session.cursor)
    }

    pub fn cursor_row_index(&self) -> Option<usize> {
        self.rows.iter().position(|r| r.path == self.session.cursor)
    }

    #[cfg(test)]
    pub(crate) fn select_row(&mut self, i: usize) {
        self.session.cursor = self.rows[i].path.clone();
    }

    #[cfg(test)]
    pub(crate) fn row_path(&self, i: usize) -> Path {
        self.rows[i].path.clone()
    }

    // ---- Navigation delegates ----

    pub fn cursor_down(&mut self) {
        self.session.cursor_down();
    }
    pub fn cursor_up(&mut self) {
        self.session.cursor_up();
    }
    pub fn toggle_expand(&mut self) {
        self.session.toggle_expand();
    }
    pub fn collapse_all(&mut self) {
        self.session.collapse_all();
    }
    pub fn expand_all(&mut self) {
        self.session.expand_all();
    }
    pub fn expand_level(&mut self) {
        self.session.expand_level();
        self.rebuild_rows();
    }
    pub fn collapse_level(&mut self) {
        self.session.collapse_level();
        self.rebuild_rows();
    }
    pub fn page_up(&mut self, page_size: usize) {
        self.session.page_up(page_size);
    }
    pub fn page_down(&mut self, page_size: usize) {
        self.session.page_down(page_size);
    }
    pub fn cursor_home(&mut self) {
        self.session.cursor_home();
    }
    pub fn cursor_end(&mut self) {
        self.session.cursor_end();
    }

    // ---- Paste-mode insertion slots ----

    pub fn paste_slots(&self) -> Vec<PasteSlot> {
        self.session.paste_slots()
    }
    pub fn effective_paste_slot(&self) -> PasteSlot {
        self.session.effective_paste_slot()
    }
    #[cfg(test)]
    fn slot_target(&self, slot: PasteSlot) -> Option<Target> {
        self.session.slot_target(slot)
    }
    pub fn is_expanded(&self, path: &Path) -> bool {
        self.session.is_expanded(path)
    }

    // ---- Filter (/) ----

    pub fn enter_filter(&mut self) {
        self.session.enter_filter();
        self.rebuild_rows();
    }
    pub fn commit_filter(&mut self) {
        self.session.commit_filter();
        self.rebuild_rows();
    }
    pub fn exit_filter_results(&mut self) {
        self.session.exit_filter_results();
        self.rebuild_rows();
    }
    pub fn exit_filter(&mut self) {
        self.session.exit_filter();
        self.rebuild_rows();
    }
    pub fn filter_char(&mut self, c: char) {
        self.session.filter_char(c);
        self.rebuild_rows();
    }
    pub fn filter_backspace(&mut self) {
        self.session.filter_backspace();
        self.rebuild_rows();
    }
    pub fn filter_delete(&mut self) {
        self.session.filter_delete();
        self.rebuild_rows();
    }
    pub fn filter_cursor_left(&mut self) {
        self.session.filter_cursor_left();
    }
    pub fn filter_cursor_right(&mut self) {
        self.session.filter_cursor_right();
    }
    pub fn filter_cursor_home(&mut self) {
        self.session.filter_cursor_home();
    }
    pub fn filter_cursor_end(&mut self) {
        self.session.filter_cursor_end();
    }
    #[cfg(test)]
    fn recompute_filter(&mut self) {
        self.session.recompute_filter();
        self.rebuild_rows();
    }

    // ---- Type filter (f) ----

    pub fn enter_type_filter(&mut self) {
        self.session.enter_type_filter();
        self.rebuild_rows();
    }
    pub fn type_filter_move(&mut self, dr: i32, dc: i32) {
        self.session.type_filter_move(dr, dc);
    }
    pub fn type_filter_toggle(&mut self) {
        self.session.type_filter_toggle();
        self.rebuild_rows();
    }
    pub fn commit_type_filter(&mut self) {
        self.session.commit_type_filter();
        self.rebuild_rows();
    }
    pub fn exit_type_filter(&mut self) {
        self.session.exit_type_filter();
        self.rebuild_rows();
    }

    // ---- Format ----

    pub fn doc_format(&self) -> crate::model::document::DocFormat {
        self.session.doc_format()
    }

    // ---- Kind switch (K) ----

    pub fn open_kind_switch(&mut self) {
        self.session.open_kind_switch();
    }
    pub fn kind_switch_move(&mut self, delta: i32) {
        self.session.kind_switch_move(delta);
    }
    pub fn kind_switch_commit(&mut self) {
        self.session.kind_switch_commit();
        self.rebuild_rows();
    }
    pub fn exit_kind_switch(&mut self) {
        self.session.exit_kind_switch();
    }

    // ---- Document conversion (C) ----

    pub fn open_convert(&mut self) {
        self.session.open_convert();
    }
    pub fn convert_move(&mut self, delta: i32) {
        self.session.convert_move(delta);
    }
    pub fn convert_pick_format(&mut self) {
        let stem = self
            .source_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());
        self.session.convert_pick_format(stem);
    }
    pub fn convert_path_char(&mut self, c: char) {
        self.session.convert_path_char(c);
    }
    pub fn convert_path_backspace(&mut self) {
        self.session.convert_path_backspace();
    }
    pub fn convert_path_delete(&mut self) {
        self.session.convert_path_delete();
    }
    pub fn convert_path_left(&mut self) {
        self.session.convert_path_left();
    }
    pub fn convert_path_right(&mut self) {
        self.session.convert_path_right();
    }
    pub fn convert_path_home(&mut self) {
        self.session.convert_path_home();
    }
    pub fn convert_path_end(&mut self) {
        self.session.convert_path_end();
    }
    pub fn convert_run(&mut self) {
        if let Some((path, text)) = self.session.convert_run() {
            self.convert_write(&path, &text);
        }
        self.rebuild_rows();
    }
    pub fn convert_confirm(&mut self) {
        if let Some((path, text)) = self.session.convert_confirm() {
            self.convert_write(&path, &text);
        }
        self.rebuild_rows();
    }
    fn convert_write(&mut self, path: &str, text: &str) {
        match std::fs::write(path, text) {
            Ok(()) => {
                self.session.status = Some(format!("converted → {path}"));
                self.session.mode = if self.session.filtered_paths.is_some() {
                    Mode::FilterResults
                } else {
                    Mode::Normal
                };
            }
            Err(e) => {
                self.session.error = Some(format!("convert write failed: {e}"));
                self.session.mode = Mode::Normal;
            }
        }
    }
    pub fn exit_convert(&mut self) {
        self.session.exit_convert();
    }

    // ---- Detail view ----

    pub fn toggle_detail(&mut self) {
        self.session.toggle_detail();
    }
    pub fn open_detail(&mut self) {
        self.session.open_detail();
    }
    pub fn detail_scroll_by(&mut self, delta: i32, max: u16) {
        let v = (self.detail_scroll as i32 + delta).clamp(0, max as i32);
        self.detail_scroll = v as u16;
    }
    pub fn detail_set_scroll(&mut self, v: u16) {
        self.detail_scroll = v;
    }
    pub fn exit_detail(&mut self) {
        self.session.exit_detail();
    }

    // ---- Help ----

    pub fn enter_help(&mut self) {
        self.help_scroll = 0;
        self.session.enter_help();
    }
    pub fn help_scroll_by(&mut self, delta: i32, max: u16) {
        let v = (self.help_scroll as i32 + delta).clamp(0, max as i32);
        self.help_scroll = v as u16;
    }
    pub fn help_set_scroll(&mut self, v: u16) {
        self.help_scroll = v;
    }
    pub fn exit_help(&mut self) {
        self.session.exit_help();
    }

    /// The About-tab body: the core's translated `about_text(lang)`, plus two
    /// host-only lines (`Config:`/`Language:`) that must NOT live in the core
    /// catalog since the config path is filesystem-specific to this host.
    pub fn about_text(&self) -> String {
        use confy_core::session::{state::about_text as core_about_text, tr_args};
        let lang = self.session.lang;
        let mut s = core_about_text(lang).to_string();
        s.push('\n');
        s.push_str(&tr_args(
            lang,
            "tui.about.config",
            &[&crate::config::config_path().display().to_string()],
        ));
        s.push('\n');
        s.push_str(&tr_args(lang, "tui.about.language", &[lang.code()]));
        s.push('\n');
        s
    }

    // ---- Language picker (l) ----

    /// Open the popup with the cursor on the currently active language.
    pub fn open_lang_picker(&mut self) {
        let cursor = LANG_OPTIONS
            .iter()
            .position(|&l| l == self.session.lang)
            .unwrap_or(0);
        self.lang_picker = Some(LangPickerState { cursor });
    }
    pub fn lang_picker_move(&mut self, delta: i32) {
        if let Some(st) = &mut self.lang_picker {
            let n = LANG_OPTIONS.len() as i32;
            st.cursor = (st.cursor as i32 + delta).rem_euclid(n) as usize;
        }
    }
    /// Apply the highlighted language: switches the session's live `lang`,
    /// then best-effort persists it to the config file. A save failure is
    /// surfaced as a status message, never a crash — the session-level
    /// switch already succeeded either way.
    pub fn lang_picker_commit(&mut self) {
        let Some(st) = self.lang_picker.take() else {
            return;
        };
        let lang = LANG_OPTIONS[st.cursor];
        self.session.set_lang(lang);
        let cfg = crate::config::Config {
            lang: Some(lang.code().to_string()),
        };
        self.session.status = Some(match crate::config::save_config(&cfg) {
            Ok(()) => confy_core::session::tr_args(lang, "tui.lang.saved", &[lang.code()]),
            Err(e) => confy_core::session::tr_args(lang, "tui.lang.save-failed", &[&e.to_string()]),
        });
    }
    pub fn exit_lang_picker(&mut self) {
        self.lang_picker = None;
    }

    // ---- Selection ----

    pub fn toggle_select(&mut self) {
        self.session.toggle_select();
    }
    pub fn extend_select_up(&mut self) {
        self.session.extend_select_up();
    }
    pub fn extend_select_down(&mut self) {
        self.session.extend_select_down();
    }
    pub fn selected_paths(&self) -> Vec<Path> {
        self.session.selected_paths()
    }

    fn cursor_is_read_only(&self) -> bool {
        self.cursor_row()
            .and_then(|r| self.session.tree.node_at(&r.path))
            .map(|n| n.read_only)
            .unwrap_or(false)
    }

    // ---- Edit routing ----

    /// `e` — edit the cursor node. Comments and containers go to $EDITOR; single-line
    /// scalars and comment nodes use the inline editor. HOST SPLIT: spawns $EDITOR.
    pub fn edit_node(&mut self) {
        if self.cursor_is_read_only() {
            self.session.status = Some("read-only node (block comment)".into());
            return;
        }
        let cursor_row = match self.cursor_row() {
            Some(r) => r.clone(),
            None => return,
        };
        if let Some(node) = self.session.tree.node_at(&cursor_row.path) {
            if let NodeKind::Comment(text) = &node.kind {
                if self.session.no_array_ancestor(&cursor_row.path) {
                    let initial = format!("{text}\n");
                    let edited = match crate::tui::editor::edit_text(&initial) {
                        Ok(t) => t,
                        Err(e) => {
                            self.session.error = Some(format!("editor error: {e}"));
                            return;
                        }
                    };
                    self.apply_edit_comment(cursor_row.path.clone(), edited);
                    return;
                }
            }
        }
        let (path, wrap_element) = self.external_edit_path(&cursor_row.path);
        let fragment = match self.session.doc.as_ref() {
            Some(d) => d.serialize_fragment(&path),
            None => return,
        };
        let edited = match crate::tui::editor::edit_text(&fragment) {
            Ok(t) => t,
            Err(e) => {
                self.session.error = Some(format!("editor error: {e}"));
                return;
            }
        };
        let edited = if wrap_element {
            match self.session.doc.as_ref() {
                Some(d) => d.scalar_fragment(None, edited.trim_end_matches('\n')),
                None => return,
            }
        } else {
            edited
        };
        self.apply_replace(path, edited);
    }

    pub fn edit_target_kind(&self) -> EditKind {
        self.session.edit_target_kind()
    }
    pub(crate) fn external_edit_path(&self, path: &Path) -> (Path, bool) {
        self.session.external_edit_path(path)
    }
    pub fn begin_inline_edit(&mut self) {
        self.session.begin_inline_edit();
    }
    pub fn begin_inline_rename(&mut self) {
        self.session.begin_inline_rename();
    }
    pub fn edit_toggle_field(&mut self) {
        self.session.edit_toggle_field();
    }
    pub fn edit_clamp_scroll(&mut self, width: usize) {
        self.session.edit_clamp_scroll(width);
    }
    pub fn edit_input_char(&mut self, c: char) {
        self.session.edit_input_char(c);
    }
    pub fn edit_backspace(&mut self) {
        self.session.edit_backspace();
    }
    pub fn edit_delete(&mut self) {
        self.session.edit_delete();
    }
    pub fn edit_cursor_left(&mut self) {
        self.session.edit_cursor_left();
    }
    pub fn edit_cursor_right(&mut self) {
        self.session.edit_cursor_right();
    }
    pub fn edit_cursor_home(&mut self) {
        self.session.edit_cursor_home();
    }
    pub fn edit_cursor_end(&mut self) {
        self.session.edit_cursor_end();
    }
    pub fn edit_cancel(&mut self) {
        self.session.edit_cancel();
        self.rebuild_rows();
    }
    pub fn edit_commit(&mut self) {
        self.session.edit_commit();
        self.rebuild_rows();
    }

    // ---- Mutations ----

    pub(crate) fn apply_replace(&mut self, path: Path, edited: String) {
        self.session.apply_replace(path, edited);
        self.rebuild_rows();
    }
    pub(crate) fn apply_edit_comment(&mut self, path: Path, text: String) {
        self.session.apply_edit_comment(path, text);
        self.rebuild_rows();
    }
    #[cfg(test)]
    pub(crate) fn apply_insert(&mut self, target: Target, edited: String) {
        self.session.apply_insert(target, edited);
        self.rebuild_rows();
    }

    pub fn nudge(&mut self, delta: i64) {
        self.session.nudge(delta);
        self.rebuild_rows();
    }
    pub fn add_node(&mut self) {
        self.session.add_node();
        self.rebuild_rows();
    }
    pub fn delete_selected(&mut self) {
        self.session.delete_selected();
        self.rebuild_rows();
    }
    pub fn copy_selected(&mut self) {
        self.session.copy_selected();
    }
    pub fn cut_selected(&mut self) {
        self.session.cut_selected();
    }
    pub fn paste(&mut self) {
        self.session.paste();
        self.rebuild_rows();
    }
    #[cfg(test)]
    pub(crate) fn do_paste(
        &mut self,
        clipboard: Clipboard,
        target: Target,
        on_collision: OnCollision,
        allow_upgrade: bool,
    ) {
        self.session
            .do_paste(clipboard, target, on_collision, allow_upgrade);
        self.rebuild_rows();
    }
    pub fn remark(&mut self) {
        self.session.remark();
        self.rebuild_rows();
    }

    // ---- Save (HOST fs write) ----

    pub fn save(&mut self) {
        let Some(ref path) = self.source_path else {
            self.session.error = Some("no save path set".into());
            return;
        };
        let path = path.clone();
        let doc = match self.session.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        if !doc.is_dirty() {
            self.session.status = Some("no changes to save".into());
            return;
        }
        let text = doc.serialize();
        match std::fs::write(&path, text) {
            Ok(()) => {
                doc.mark_saved();
                self.session.status = Some("Saved".into());
            }
            Err(e) => self.session.error = Some(format!("save error: {e}")),
        }
    }

    // ---- Undo / redo ----

    pub fn undo(&mut self) {
        self.session.undo();
        self.rebuild_rows();
    }
    pub fn redo(&mut self) {
        self.session.redo();
        self.rebuild_rows();
    }

    // ---- Escape / quit ----

    pub fn escape(&mut self) {
        self.session.escape();
        self.rebuild_rows();
    }
    pub fn confirm_quit(&self) -> bool {
        self.session.confirm_quit()
    }
    pub fn quit_requested(&mut self) -> bool {
        self.session.quit_requested()
    }

    // ---- Prompt ----

    pub fn handle_prompt_key(&mut self, c: char) -> PromptOutcome {
        if self.session.handle_prompt_key(c) {
            PromptOutcome::Quit
        } else {
            self.rebuild_rows();
            PromptOutcome::Consumed
        }
    }
}

/// Fixed-pitch TYPE-column tag: always 8 columns. The `(kind, format, doc,
/// read_only)` decision lives once in `classify`; this only maps its
/// `TypeToken` to the column glyph, so the tag list can't drift from the
/// type-filter.
fn type_tag(
    kind: &NodeKind,
    format: Format,
    doc: crate::model::document::DocFormat,
    read_only: bool,
) -> String {
    use confy_core::session::{classify, TypeToken};
    let slot: &str = match classify(kind, format, doc, read_only) {
        TypeToken::Root => "[G]",
        TypeToken::Comment => "[C]",
        TypeToken::Opaque => "[opaq ]",
        TypeToken::SeqBlock => "[A/B]",
        TypeToken::SeqFlow => "[A/F]",
        TypeToken::ArrayMultiline => "[A/M]",
        TypeToken::ArrayInline => "[A/I]",
        TypeToken::Aot => "[A/T]",
        TypeToken::MapFlow => "[T/F]",
        TypeToken::InlineTable => "[T/I]",
        TypeToken::MapBlock => "[T/B]",
        TypeToken::TableMultiline => "[T/M]",
        TypeToken::TableDotted => "[T/D]",
        TypeToken::TableScope => "[T/S]",
        TypeToken::StrMBasic => "[S:mstr]",
        TypeToken::StrLit | TypeToken::StrLiteralBlock => "[S:lit ]",
        TypeToken::StrMLit => "[S:mlit]",
        TypeToken::StrSingle => "[S:sq  ]",
        TypeToken::StrDouble => "[S:dq  ]",
        TypeToken::StrFolded => "[S:fold]",
        TypeToken::StrBasic => "[S:str ]",
        TypeToken::IntHex => "[I:hex ]",
        TypeToken::IntOct => "[I:oct ]",
        TypeToken::IntBin => "[I:bin ]",
        TypeToken::IntDec => "[I:dec ]",
        TypeToken::FloatInf => "[F:inf ]",
        TypeToken::FloatNan => "[F:nan ]",
        TypeToken::FloatExp => "[F:exp ]",
        TypeToken::FloatPlain => "[F:flt ]",
        TypeToken::Bool => "[B:bool]",
        TypeToken::Null => "[S:null]",
        TypeToken::Odt => "[D:odt ]",
        TypeToken::Ldt => "[D:ldt ]",
        TypeToken::LDate => "[D:ldat]",
        TypeToken::LTime => "[D:ltim]",
    };
    format!("{slot:<8}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::*;
    use crate::tui::state::PromptKind;

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
        let mut app = App::from_tree(NodeTree { root });
        // Populate the render rows so path-keyed `select_row`/`row_path` resolve
        // (pre-§3 these tests set `app.session.cursor = 1` directly on empty rows).
        app.rebuild_rows();
        app
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
        assert!(app.session.filtered_paths.is_none());

        // Type only: integers -> keep `port` (+ root ancestor), drop `host`.
        let mut app = typed_sample();
        app.session.type_filter.types.insert(TypeToken::IntDec);
        app.recompute_filter();
        let fp = app.session.filtered_paths.clone().unwrap();
        assert!(fp.contains(&port_path));
        assert!(!fp.contains(&host_path));
        assert!(fp.contains(&Vec::<Seg>::new()), "root ancestor kept");

        // Text only: "host" -> keep `host`, drop `port`.
        let mut app = typed_sample();
        app.session.filter = "host".into();
        app.recompute_filter();
        let fp = app.session.filtered_paths.clone().unwrap();
        assert!(fp.contains(&host_path));
        assert!(!fp.contains(&port_path));

        // AND: text "port" + type string -> intersection is empty (no leaf passes).
        let mut app = typed_sample();
        app.session.filter = "port".into();
        app.session.type_filter.types.insert(TypeToken::StrBasic);
        app.recompute_filter();
        let fp = app.session.filtered_paths.clone().unwrap();
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
    fn rebuild_preserves_path_keyed_selection() {
        // Selection is path-keyed (Slice 3+); expand/collapse does not invalidate
        // paths, so the selection must survive a rebuild after structural changes.
        let mut app = sample();
        app.rebuild_rows();
        app.cursor_down(); // on `a`
        app.toggle_select(); // select `a`
        assert!(!app.session.selection.is_empty());
        app.toggle_expand();
        app.rebuild_rows(); // structure changed — selection should survive
        assert!(
            !app.session.selection.is_empty(),
            "path-keyed selection must survive rebuild"
        );
    }

    #[test]
    fn selection_ops_are_blocked_while_clipboard_active() {
        let mut app = sample();
        // Move cursor to a leaf so we have something selectable.
        app.select_row(1);
        // Load a clipboard (simulates copy).
        app.session.clipboard = Some(Clipboard {
            fragments: vec!["x = 1\n".into()],
            cut: false,
            sources: vec![vec![Seg::Key("a".into()), Seg::Key("x".into())]],
        });
        // toggle_select must be a no-op while clipboard is active.
        app.toggle_select();
        assert!(
            app.session.selection.is_empty(),
            "s should not select when clipboard active"
        );
        // extend_select_down must not alter selection either.
        app.extend_select_down();
        assert!(
            app.session.selection.is_empty(),
            "Shift+Down should not select when clipboard active"
        );
        // extend_select_up must not alter selection either.
        app.extend_select_up();
        assert!(
            app.session.selection.is_empty(),
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
        app.select_row(app.visible_keys().iter().position(|k| k == "a").unwrap());
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
        assert_eq!(app.cursor_row().unwrap().key, "a");
    }

    #[test]
    fn collapse_level_in_place_on_open_branch_else_ascends() {
        let mut app = app_with("[a]\np = 1\n[a.b]\nq = 2\n");
        let below = |app: &App| app.visible_keys()[1..].to_vec();
        app.collapse_all();
        app.rebuild_rows();
        app.select_row(app.visible_keys().iter().position(|k| k == "a").unwrap());
        app.expand_level();
        assert_eq!(below(&app), vec!["a", "p", "b"]);
        // Cursor on the open branch `a` -> collapse in place, cursor stays.
        app.collapse_level();
        assert_eq!(below(&app), vec!["a"]);
        assert_eq!(app.cursor_row().unwrap().key, "a");
        // Reopen, drop cursor on leaf `p` -> collapse ascends to parent `a`.
        app.expand_level();
        app.select_row(app.visible_keys().iter().position(|k| k == "p").unwrap());
        app.collapse_level();
        assert_eq!(below(&app), vec!["a"]);
        assert_eq!(app.cursor_row().unwrap().key, "a");
    }

    #[test]
    fn shift_rounds_union_across_a_plain_move_and_esc_clears() {
        use std::collections::HashSet;
        let mut app = app_with("a = 1\nb = 2\nc = 3\nd = 4\ne = 5\n");
        app.rebuild_rows();
        // rows: f.toml(0) a(1) b(2) c(3) d(4) e(5)
        app.select_row(1);
        app.extend_select_down(); // round 1 -> {1,2}
                                  // a non-shift key (handled in the event loop) resets the flag:
        app.session.last_action_was_shift_select = false;
        app.select_row(4);
        app.extend_select_down(); // round 2 from a fresh anchor -> {4,5}
                                  // Selection is path-keyed (§3); map back to row indices for the assertion.
        let sel: HashSet<usize> = app
            .session
            .selection
            .iter()
            .filter_map(|p| app.rows.iter().position(|r| r.path == p))
            .collect();
        assert_eq!(
            sel,
            HashSet::from([1, 2, 4, 5]),
            "second round must union, not extend from round 1's anchor"
        );
        app.escape(); // Esc in normal mode clears the selection
        assert!(app.session.selection.is_empty());
    }

    #[test]
    fn external_edit_fragment_is_clean_node_text() {
        // CST backend: a `[t]` opened in `$EDITOR` is just the table's own section
        // text — no leading blank, and no adjacent comment (comments are independent
        // nodes now, edited on their own row).
        let app = app_with("a = 1\n\n# c\n[t]\nx = 1\n");
        let doc = app.session.doc.as_ref().unwrap();
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
        let doc = app.session.doc.as_ref().unwrap();
        let frag = doc.serialize_fragment(&[Seg::Key("port".into())]);
        assert_eq!(frag, "port = 8080\n", "got: {frag:?}");
    }

    // --- e/n apply-path tests (post-editor logic, no $EDITOR spawned) ---

    fn app_with(src: &str) -> App {
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::from_str(src).unwrap(),
        );
        App::new(doc)
    }

    #[test]
    fn kind_switch_converts_scalar_via_popup() {
        let mut app = app_with("a = \"42\"\nb = 1\n");
        app.select_row(app.rows.iter().position(|r| r.key == "a").unwrap());
        app.open_kind_switch();
        let Mode::KindSwitch(st) = &app.session.mode else {
            panic!("popup should be open");
        };
        // A basic string offers the other three string notations.
        assert_eq!(st.options[0].0, "literal string  '…'");
        assert_eq!(st.options.len(), 3);
        app.kind_switch_commit();
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            "a = '42'\nb = 1\n"
        );
        assert!(matches!(app.session.mode, Mode::Normal));
    }

    #[test]
    fn convert_flow_opens_only_on_root() {
        let mut app = app_with("a = 1\n");
        // On a non-root node it refuses.
        app.select_row(app.rows.iter().position(|r| r.key == "a").unwrap());
        app.open_convert();
        assert!(matches!(app.session.mode, Mode::Normal));
        assert!(app.session.error.as_deref().unwrap_or("").contains("root"));
        // On the root node it opens with the other two formats offered.
        app.session.error = None;
        app.select_row(app.rows.iter().position(|r| r.path.is_empty()).unwrap());
        app.open_convert();
        let Mode::Convert(st) = &app.session.mode else {
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
        app.select_row(app.rows.iter().position(|r| r.path.is_empty()).unwrap());
        app.open_convert();
        // Pick JSON.
        if let Mode::Convert(st) = &mut app.session.mode {
            st.cursor = st
                .options
                .iter()
                .position(|f| *f == crate::model::document::DocFormat::Json)
                .unwrap();
        }
        app.convert_pick_format();
        // Path step seeded with the .json extension.
        if let Mode::Convert(st) = &app.session.mode {
            assert!(st.path.ends_with(".json"));
        } else {
            panic!("should be on the path step");
        }
        // Overwrite the path with the temp file and run (lossless → writes at once).
        if let Mode::Convert(st) = &mut app.session.mode {
            st.path = out.path().to_string_lossy().into_owned();
        }
        app.convert_run();
        assert!(matches!(app.session.mode, Mode::Normal));
        let written = std::fs::read_to_string(out.path()).unwrap();
        assert_eq!(written, "{\n  \"a\": 1,\n  \"b\": \"x\"\n}\n");
        // The open document is untouched.
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            "a = 1\nb = \"x\"\n"
        );
    }

    #[test]
    fn convert_flow_lossy_requires_confirm() {
        let mut app = app_with("n = 0xFF\n");
        let out = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        app.select_row(app.rows.iter().position(|r| r.path.is_empty()).unwrap());
        app.open_convert();
        if let Mode::Convert(st) = &mut app.session.mode {
            st.cursor = st
                .options
                .iter()
                .position(|f| *f == crate::model::document::DocFormat::Json)
                .unwrap();
        }
        app.convert_pick_format();
        if let Mode::Convert(st) = &mut app.session.mode {
            st.path = out.path().to_string_lossy().into_owned();
        }
        app.convert_run();
        // A lossy conversion stops at the confirm step (warning shown, no write yet).
        let Mode::Convert(st) = &app.session.mode else {
            panic!("should pause at confirm");
        };
        assert!(matches!(st.step, crate::tui::state::ConvertStep::Confirm));
        assert!(st.warnings.iter().any(|w| w.contains("non-decimal")));
        app.convert_confirm();
        assert!(matches!(app.session.mode, Mode::Normal));
        assert_eq!(
            std::fs::read_to_string(out.path()).unwrap(),
            "{\n  \"n\": 255\n}\n"
        );
    }

    #[test]
    fn kind_switch_rejects_bool_scalar() {
        let mut app = app_with("a = true\n");
        app.select_row(app.rows.iter().position(|r| r.key == "a").unwrap());
        app.open_kind_switch();
        assert!(
            matches!(app.session.mode, Mode::Normal),
            "popup must not open"
        );
        assert!(app
            .session
            .error
            .as_deref()
            .unwrap_or("")
            .contains("cannot"));
    }

    #[test]
    fn kind_switch_rejects_non_convertible_node() {
        let mut app = app_with("# c\na = 1\n");
        app.session.cursor = app
            .rows
            .iter()
            .find(|r| r.key.starts_with('#'))
            .unwrap()
            .path
            .clone();
        app.open_kind_switch();
        assert!(
            matches!(app.session.mode, Mode::Normal),
            "popup must not open"
        );
        assert!(app
            .session
            .error
            .as_deref()
            .unwrap_or("")
            .contains("cannot"));
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
        app.select_row(ai);
        app.cut_selected();
        let di = app
            .rows
            .iter()
            .find(|r| r.key == "dest")
            .unwrap()
            .path
            .clone();
        app.session.paste_slot = Some(crate::tui::app::PasteSlot::Into(di.clone()));
        app.session.cursor = di;
        app.paste();
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            "[dest]\nz = 0\na.x = 1\na.y = 2\n",
            "status={:?}",
            app.session.status
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
                app.session
                    .tree
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
        assert!(
            app.session.status.is_none(),
            "unexpected status: {:?}",
            app.session.status
        );
        let s = app.session.doc.as_ref().unwrap().serialize();
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
        let before = app.session.doc.as_ref().unwrap().serialize();
        let cpath = app.rows[1].path.clone();
        app.apply_edit_comment(cpath, "not a comment\n".into());
        assert!(
            app.session.error.is_some(),
            "invalid comment must surface in error"
        );
        assert_eq!(app.session.doc.as_ref().unwrap().serialize(), before);
    }

    #[test]
    fn single_line_comment_edits_inline() {
        let mut app = app_with("# old\nx = 1\n");
        app.expand_all();
        app.rebuild_rows();
        app.select_row(1); // the comment node
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        app.begin_inline_edit();
        let e = match &app.session.mode {
            Mode::Edit(e) => e,
            _ => panic!("expected inline edit mode"),
        };
        assert!(e.is_comment, "comment edit must set is_comment");
        assert_eq!(e.buffer, "# old", "buffer seeded with raw comment text");
        // Tab is a no-op for a comment (no name field).
        app.edit_toggle_field();
        assert!(
            matches!(&app.session.mode, Mode::Edit(e) if e.field == crate::tui::state::EditField::Value)
        );
        // Commit an edited comment → EditComment round-trips into the doc.
        if let Mode::Edit(ref mut e) = app.session.mode {
            e.buffer = "# new".into();
        }
        app.edit_commit();
        assert!(matches!(app.session.mode, Mode::Normal));
        let s = app.session.doc.as_ref().unwrap().serialize();
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
        app.select_row(pos);
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        app.begin_inline_edit();
        if let Mode::Edit(ref mut e) = app.session.mode {
            assert!(e.is_comment);
            e.buffer = "# changed".into();
        } else {
            panic!("expected inline edit mode");
        }
        app.edit_commit();
        assert!(matches!(app.session.mode, Mode::Normal));
        let s = app.session.doc.as_ref().unwrap().serialize();
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
        app.select_row(pos);
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        app.begin_inline_edit();
        if let Mode::Edit(ref mut e) = app.session.mode {
            assert!(e.is_comment);
            e.buffer = "#321".into();
        } else {
            panic!("expected inline edit mode");
        }
        app.edit_commit();
        let s = app.session.doc.as_ref().unwrap().serialize();
        assert_eq!(s, "[[product]]\n#321\nname = \"Hammer\"\n");
    }

    #[test]
    fn multiline_comment_routes_external() {
        let mut app = app_with("# a\n# b\nx = 1\n");
        app.expand_all();
        app.rebuild_rows();
        app.select_row(1); // merged multi-line comment node
        assert_eq!(app.edit_target_kind(), EditKind::External);
    }

    #[test]
    fn inline_comment_commit_rejects_non_comment_and_stays_in_editor() {
        let mut app = app_with("# keep\nx = 1\n");
        let before = app.session.doc.as_ref().unwrap().serialize();
        app.expand_all();
        app.rebuild_rows();
        app.select_row(1);
        app.begin_inline_edit();
        if let Mode::Edit(ref mut e) = app.session.mode {
            e.buffer = "not a comment".into();
        }
        app.edit_commit();
        assert!(
            matches!(app.session.mode, Mode::Edit(_)),
            "stay in editor on error"
        );
        assert!(app.session.status.is_some(), "error surfaced in status");
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            before,
            "doc unchanged"
        );
    }

    #[test]
    fn apply_replace_invalid_toml_sets_status_and_leaves_doc() {
        let mut app = app_with("port = 8080\n");
        let before = app.session.doc.as_ref().unwrap().serialize();
        app.apply_replace(vec![Seg::Key("port".into())], "port = = nope".into());
        assert!(
            app.session.error.is_some(),
            "invalid TOML must surface in error"
        );
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            before,
            "doc unchanged"
        );
    }

    #[test]
    fn apply_replace_valid_pushes_history_and_rebuilds() {
        let mut app = app_with("port = 8080\n");
        app.apply_replace(vec![Seg::Key("port".into())], "port = 9090\n".into());
        assert!(app.session.status.is_none());
        assert!(app
            .session
            .doc
            .as_ref()
            .unwrap()
            .serialize()
            .contains("9090"));
        // history advanced: undo restores the pre-edit snapshot
        let restored = app.session.history.as_mut().unwrap().undo().unwrap();
        assert!(restored.contains("8080"));
    }

    #[test]
    fn apply_insert_collision_sets_status_and_leaves_doc() {
        let mut app = app_with("port = 8080\n");
        let before = app.session.doc.as_ref().unwrap().serialize();
        app.apply_insert(
            crate::model::document::Target {
                parent: vec![],
                index: 1,
            },
            "port = 1\n".into(),
        );
        assert!(
            app.session.error.is_some(),
            "collision must surface in error"
        );
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            before,
            "doc unchanged"
        );
    }

    #[test]
    fn apply_insert_invalid_toml_sets_status_and_leaves_doc() {
        // §10 rejection path for `n`: invalid fragment -> Fragment -> error, no change.
        let mut app = app_with("port = 8080\n");
        let before = app.session.doc.as_ref().unwrap().serialize();
        app.apply_insert(
            crate::model::document::Target {
                parent: vec![],
                index: 1,
            },
            "= = nope".into(),
        );
        assert!(
            app.session.error.is_some(),
            "invalid TOML must surface in error"
        );
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
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
        assert!(app.session.status.is_none());
        assert!(app
            .session
            .doc
            .as_ref()
            .unwrap()
            .serialize()
            .contains("host = \"x\""));
        // reproject + rebuild surfaced the new key as a visible row
        assert!(app.visible_keys().contains(&"host".to_string()));
        let restored = app.session.history.as_mut().unwrap().undo().unwrap();
        assert!(!restored.contains("host"));
    }

    #[test]
    fn cut_then_paste_moves_node() {
        let mut app = app_with("a = 1\n[dest]\n");
        // cursor on `a` (row 1, after root)
        app.select_row(1);
        // cut
        app.cut_selected();
        assert!(app.session.clipboard.is_some());
        assert!(app.session.clipboard.as_ref().unwrap().cut);
        let s_before_paste = app.session.doc.as_ref().unwrap().serialize();
        assert!(
            s_before_paste.contains("a = 1"),
            "cut defers deletion until paste"
        );

        // navigate into [dest] — expand root + dest, cursor on dest
        app.expand_all();
        app.rebuild_rows();
        let dest_idx = app.rows.iter().position(|r| r.key == "dest").unwrap();
        app.select_row(dest_idx);

        // paste
        app.paste();
        let s = app.session.doc.as_ref().unwrap().serialize();
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
        app.select_row(1); // on `a`
        app.delete_selected();
        let s = app.session.doc.as_ref().unwrap().serialize();
        assert!(!s.contains("a = 1"));
        assert!(s.contains("b = 2"));
    }

    #[test]
    fn undo_restores_after_delete() {
        let mut app = app_with("a = 1\n");
        app.select_row(1);
        app.delete_selected();
        assert!(!app
            .session
            .doc
            .as_ref()
            .unwrap()
            .serialize()
            .contains("a = 1"));
        app.undo();
        assert!(
            app.session
                .doc
                .as_ref()
                .unwrap()
                .serialize()
                .contains("a = 1"),
            "undo restores deleted node"
        );
    }

    #[test]
    fn redo_reapplies_after_undo() {
        let mut app = app_with("a = 1\n");
        app.select_row(1);
        app.delete_selected();
        app.undo();
        assert!(app
            .session
            .doc
            .as_ref()
            .unwrap()
            .serialize()
            .contains("a = 1"));
        app.redo();
        assert!(
            !app.session
                .doc
                .as_ref()
                .unwrap()
                .serialize()
                .contains("a = 1"),
            "redo re-applies deletion"
        );
    }

    #[test]
    fn remark_toggles_comment() {
        let mut app = app_with("port = 8080\n");
        app.select_row(1); // on port
        app.remark();
        let s = app.session.doc.as_ref().unwrap().serialize();
        assert!(
            s.contains("# port = 8080"),
            "remark should comment out: {s:?}"
        );
    }

    #[test]
    fn pure_json_remark_prompts_then_upgrades() {
        let doc = crate::model::any_doc::AnyDocument::from_str_as(
            "{\n  \"a\": 1\n}\n",
            crate::model::document::DocFormat::Json,
        )
        .unwrap();
        let mut app = App::new(doc);

        // Expand the root so "a" appears as a row, then position the cursor on it.
        app.expand_level();
        app.rebuild_rows();
        let ai = app.rows.iter().position(|r| r.key == "a").unwrap();
        app.select_row(ai);

        // Remark on a live node in a pure .json must show the JSONC-upgrade prompt.
        app.remark();
        assert!(
            matches!(
                app.session.mode,
                Mode::Prompt(PromptKind::JsoncUpgrade { .. })
            ),
            "expected JsoncUpgrade prompt, got {:?}",
            std::mem::discriminant(&app.session.mode)
        );
        // Document must be unchanged at this point.
        assert!(
            !app.session.doc.as_ref().unwrap().is_dirty(),
            "doc must be clean while prompt is pending"
        );

        // Confirm with 'y' — should enable comments and apply the remark.
        app.handle_prompt_key('y');
        let s = app.session.doc.as_ref().unwrap().serialize();
        assert!(
            s.contains("//"),
            "after upgrade the serialized output must contain a // comment: {s:?}"
        );
        assert!(
            app.session.doc.as_ref().unwrap().supports_comments(),
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
        app.select_row(0); // root
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
            app.session.mode,
            Mode::Prompt(PromptKind::Collision { .. })
        ));
        let cb = app
            .session
            .clipboard
            .as_ref()
            .expect("clipboard must be set");
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
        app.session.mode = Mode::Prompt(PromptKind::ConfirmQuit);
        let outcome = app.handle_prompt_key('y');
        assert!(matches!(outcome, PromptOutcome::Quit));
        assert!(matches!(app.session.mode, Mode::Normal));
    }

    #[test]
    fn confirm_quit_n_returns_consumed() {
        let mut app = app_with("a = 1\n");
        app.session.mode = Mode::Prompt(PromptKind::ConfirmQuit);
        let outcome = app.handle_prompt_key('n');
        assert!(matches!(outcome, PromptOutcome::Consumed));
        assert!(matches!(app.session.mode, Mode::Normal));
    }

    // --- Filter must match by scalar VALUE (Batch 1 #1) ---

    #[test]
    fn filter_matches_value() {
        let mut app = app_with("port = 8080\nhost = \"localhost\"\n");
        app.expand_all();
        app.rebuild_rows();
        // A scalar's value (`8080`) is part of the haystack, so searching the
        // value surfaces the node it belongs to.
        app.enter_filter();
        for c in "8080".chars() {
            app.filter_char(c);
        }
        let keys = app.visible_keys();
        assert!(
            keys.iter().any(|k| k == "port"),
            "value 8080 must match the `port` node, got: {keys:?}"
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
        assert!(matches!(app.session.mode, Mode::FilterResults));
        assert!(
            app.session.filtered_paths.is_some(),
            "filter stays applied after commit"
        );
        let keys = app.visible_keys();
        assert!(keys.iter().any(|k| k == "port"));
        assert!(!keys.iter().any(|k| k == "host"), "host filtered out");
        // Esc unfilters back to the full list but remembers the keyword.
        app.escape();
        assert!(matches!(app.session.mode, Mode::Normal));
        assert!(app.session.filtered_paths.is_none());
        assert_eq!(app.session.last_filter, "port");
        let keys = app.visible_keys();
        assert!(keys.iter().any(|k| k == "host"), "full list restored");
        // Re-entering the filter restores the remembered query + live results.
        app.enter_filter();
        assert_eq!(app.session.filter, "port");
        assert_eq!(app.session.filter_cursor, 4);
        assert!(app.session.filtered_paths.is_some());
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
        assert!(matches!(app.session.mode, Mode::FilterResults));
        // Detail popup: open then close returns to the filtered selection.
        app.open_detail();
        assert!(matches!(app.session.mode, Mode::Detail));
        app.exit_detail();
        assert!(matches!(app.session.mode, Mode::FilterResults));
        assert!(app.session.filtered_paths.is_some());
        assert_eq!(
            app.session.filter, "port",
            "filter (and its highlight) survives detail"
        );
        // Inline edit: cancel returns to the filtered selection too.
        app.select_row(app.rows.iter().position(|r| r.key == "port").unwrap());
        app.begin_inline_edit();
        assert!(matches!(app.session.mode, Mode::Edit(_)));
        app.edit_cancel();
        assert!(matches!(app.session.mode, Mode::FilterResults));
        assert_eq!(app.session.filter, "port");
    }

    #[test]
    fn edit_delete_removes_char_at_cursor() {
        let mut app = app_with("port = 8080\n");
        app.rebuild_rows();
        app.select_row(app.rows.iter().position(|r| r.key == "port").unwrap());
        app.begin_inline_edit();
        app.edit_cursor_home(); // caret before "8080"
        app.edit_delete(); // remove the '8'
        if let Mode::Edit(ref e) = app.session.mode {
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
        assert_eq!(app.session.filter, "port");
        assert_eq!(app.session.filter_cursor, 2);
        // Home then Del removes the leading 'p'.
        app.filter_cursor_home();
        app.filter_delete();
        assert_eq!(app.session.filter, "ort");
        assert_eq!(app.session.filter_cursor, 0);
        // Backspace at the start is a no-op.
        app.filter_backspace();
        assert_eq!(app.session.filter, "ort");
        // End then Backspace removes the trailing 't'.
        app.filter_cursor_end();
        app.filter_backspace();
        assert_eq!(app.session.filter, "or");
        assert_eq!(app.session.filter_cursor, 2);
    }

    // --- Blocker 2: detail must show type and value ---

    // --- Task 19: save + dirty-aware quit ---

    #[test]
    fn save_writes_to_file_and_resets_dirty() {
        // Keep the NamedTempFile alive so the path isn't deleted.
        let f = tempfile::NamedTempFile::new().unwrap();
        let path = f.path().to_path_buf();
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::from_str("port = 8080\n").unwrap(),
        );
        let mut app = App::new(doc);
        // The host owns the save target: the path the TUI loaded from.
        app.source_path = Some(path.clone());
        // Mutate to make dirty
        app.apply_replace(vec![Seg::Key("port".into())], "port = 9090\n".into());
        assert!(
            app.session.doc.as_ref().unwrap().is_dirty(),
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
            !app.session.doc.as_ref().unwrap().is_dirty(),
            "must not be dirty after save"
        );
        assert!(
            app.session.status.as_deref() == Some("Saved"),
            "status must be 'Saved'"
        );
    }

    #[test]
    fn quit_when_dirty_enters_confirm_quit() {
        let mut app = app_with("a = 1\n");
        app.apply_replace(vec![Seg::Key("a".into())], "a = 2\n".into());
        assert!(app.session.doc.as_ref().unwrap().is_dirty());
        let should_quit = app.quit_requested();
        assert!(!should_quit, "should NOT quit immediately when dirty");
        assert!(
            matches!(app.session.mode, Mode::Prompt(PromptKind::ConfirmQuit)),
            "must enter ConfirmQuit prompt"
        );
    }

    #[test]
    fn quit_when_clean_signals_quit() {
        let mut app = app_with("a = 1\n");
        assert!(
            !app.session.doc.as_ref().unwrap().is_dirty(),
            "fresh doc must be clean"
        );
        let should_quit = app.quit_requested();
        assert!(should_quit, "must return true (quit) when clean");
        assert!(
            matches!(app.session.mode, Mode::Normal),
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
        app.select_row(idx_of(&app, "port"));
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        // scalar directly under a [table] → inline
        app.select_row(idx_of(&app, "host"));
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        // a [table] branch → external
        app.select_row(idx_of(&app, "server"));
        assert_eq!(app.edit_target_kind(), EditKind::External);
        // a single-line array / inline table → inline (edited as its one-line repr)
        app.select_row(idx_of(&app, "arr"));
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        app.select_row(idx_of(&app, "pt"));
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        // scalar element directly in a top-level array → inline
        app.select_row(idx_of(&app, "[0]"));
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        // scalar member of an inline table → inline (value Replace + key Rename
        // both address it via an all-`Key` path)
        app.select_row(idx_of(&app, "y"));
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
        app.select_row(app.rows.iter().position(|r| r.path == p_arr).unwrap());
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        app.select_row(app.rows.iter().position(|r| r.path == p_inl).unwrap());
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
    }

    #[test]
    fn edit_target_kind_routes_multiline_string_external() {
        let mut app = app_with("ml = \"\"\"\nline1\nline2\n\"\"\"\nsingle = \"x\"\n");
        app.expand_all();
        app.rebuild_rows();
        // multiline string scalar → external (inline editor is single-line)
        app.select_row(idx_of(&app, "ml"));
        assert_eq!(app.edit_target_kind(), EditKind::External);
        // single-line string scalar → inline (control)
        app.select_row(idx_of(&app, "single"));
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
        app.select_row(idx_of(&app, "[0]"));
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
        app.select_row(pos);
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
    }

    #[test]
    fn serialize_aot_entry_emits_single_block() {
        // `E` on an AoT entry serializes just that `[[product]]` block (not the
        // whole array-of-tables) for external editing.
        let app = app_with("[[product]]\nname = \"Hammer\"\n[[product]]\nname = \"Nail\"\n");
        let doc = app.session.doc.as_ref().unwrap();
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
        assert!(
            app.session.status.is_none(),
            "unexpected status: {:?}",
            app.session.status
        );
        let s = app.session.doc.as_ref().unwrap().serialize();
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
        app.select_row(pos);
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
        app.select_row(pos);
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
    }

    #[test]
    fn nudge_writes_back_through_replace() {
        let mut app = app_with("port = 8080\n");
        app.select_row(1); // on port
        app.nudge(1);
        assert!(app
            .session
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
        app.session.pending_trailing = Some(Some("# leak".into()));
        app.edit_cancel();
        assert!(app.session.pending_trailing.is_none());
        // A subsequent nudge writes only the value, no stray comment.
        app.select_row(1);
        app.nudge(1);
        let out = app.session.doc.as_ref().unwrap().serialize();
        assert!(
            !out.contains("# leak"),
            "stale comment leaked into nudge: {out}"
        );
    }

    #[test]
    fn inline_commit_same_type_applies_replace() {
        let mut app = app_with("port = 8080\n");
        app.select_row(1);
        app.begin_inline_edit();
        for _ in 0..4 {
            app.edit_backspace();
        }
        for c in "9090".chars() {
            app.edit_input_char(c);
        }
        app.edit_commit();
        assert!(matches!(app.session.mode, Mode::Normal));
        assert!(app
            .session
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
        app.select_row(1);
        app.begin_inline_edit();
        // Tab switches to the Name field (active buffer becomes the key "port").
        app.edit_toggle_field();
        assert!(matches!(&app.session.mode, Mode::Edit(e) if e.field == EditField::Name));
        for _ in 0..4 {
            app.edit_backspace(); // clear "port"
        }
        for c in "addr".chars() {
            app.edit_input_char(c);
        }
        app.edit_commit();
        assert!(matches!(app.session.mode, Mode::Normal));
        // key renamed, value preserved, no stray old key
        let s = app.session.doc.as_ref().unwrap().serialize();
        assert_eq!(s, "addr = 8080\n");
    }

    #[test]
    fn inline_rename_to_dotted_confirms_and_converts_to_table() {
        // Editing a scalar's Name to `foo.x` asks "integer → table"; `y` applies the
        // rename, turning it into a `[T/D]` table (issue 4).
        use crate::tui::state::EditField;
        let mut app = app_with("foo = 1\n");
        app.select_row(1);
        app.begin_inline_edit();
        app.edit_toggle_field();
        assert!(matches!(&app.session.mode, Mode::Edit(e) if e.field == EditField::Name));
        for c in ".x".chars() {
            app.edit_input_char(c); // "foo" -> "foo.x"
        }
        app.edit_commit();
        assert!(
            matches!(
                app.session.mode,
                Mode::Prompt(PromptKind::TypeChange { .. })
            ),
            "dotted rename must confirm the type change"
        );
        app.handle_prompt_key('y');
        assert!(matches!(app.session.mode, Mode::Normal));
        assert_eq!(app.session.doc.as_ref().unwrap().serialize(), "foo.x = 1\n");
    }

    #[test]
    fn inline_rename_to_dotted_cancel_leaves_doc_untouched() {
        let mut app = app_with("foo = 1\n");
        app.select_row(1);
        app.begin_inline_edit();
        app.edit_toggle_field();
        for c in ".x".chars() {
            app.edit_input_char(c);
        }
        app.edit_commit();
        app.handle_prompt_key('n'); // decline
        assert_eq!(app.session.doc.as_ref().unwrap().serialize(), "foo = 1\n");
    }

    #[test]
    fn inline_tab_is_noop_for_array_element() {
        use crate::tui::state::EditField;
        let mut app = app_with("arr = [1, 2]\n");
        app.expand_all();
        app.rebuild_rows();
        app.select_row(idx_of(&app, "[0]"));
        app.begin_inline_edit();
        app.edit_toggle_field(); // array element has no name → stays on Value
        assert!(matches!(&app.session.mode, Mode::Edit(e) if e.field == EditField::Value));
    }

    #[test]
    fn inline_commit_type_change_enters_prompt_then_confirms() {
        let mut app = app_with("port = 8080\n");
        app.select_row(1);
        app.begin_inline_edit();
        for _ in 0..4 {
            app.edit_backspace();
        }
        for c in "\"hi\"".chars() {
            app.edit_input_char(c);
        }
        app.edit_commit();
        assert!(
            matches!(
                app.session.mode,
                Mode::Prompt(PromptKind::TypeChange { .. })
            ),
            "changing integer→string must confirm"
        );
        assert!(app.session.pending_edit.is_some());
        app.handle_prompt_key('y');
        assert!(matches!(app.session.mode, Mode::Normal));
        assert!(app
            .session
            .doc
            .as_ref()
            .unwrap()
            .serialize()
            .contains("port = \"hi\""));
    }

    #[test]
    fn inline_commit_invalid_toml_keeps_editor_open() {
        let mut app = app_with("port = 8080\n");
        let before = app.session.doc.as_ref().unwrap().serialize();
        app.select_row(1);
        app.begin_inline_edit();
        for _ in 0..4 {
            app.edit_backspace();
        }
        for c in "= nope".chars() {
            app.edit_input_char(c);
        }
        app.edit_commit();
        assert!(
            matches!(app.session.mode, Mode::Edit(_)),
            "stay in editor to fix"
        );
        assert!(app.session.status.is_some());
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            before,
            "doc unchanged"
        );
    }

    #[test]
    fn inline_editor_home_end_move_cursor() {
        let mut app = app_with("port = 8080\n");
        app.select_row(1);
        app.begin_inline_edit();
        // buffer is "8080", cursor starts at end (4)
        app.edit_cursor_home();
        if let Mode::Edit(ref e) = app.session.mode {
            assert_eq!(e.cursor, 0);
        } else {
            panic!("not in edit mode");
        }
        app.edit_cursor_end();
        if let Mode::Edit(ref e) = app.session.mode {
            assert_eq!(e.cursor, e.buffer.chars().count());
        } else {
            panic!("not in edit mode");
        }
    }

    #[test]
    fn add_node_inserts_empty_string_and_enters_edit() {
        let mut app = app_with("a = 1\n");
        app.select_row(1); // on a
        app.add_node();
        assert!(
            matches!(app.session.mode, Mode::Edit(_)),
            "add should open the inline editor"
        );
        assert!(
            app.session
                .doc
                .as_ref()
                .unwrap()
                .serialize()
                .contains("new_field = \"\""),
            "placeholder inserted: {}",
            app.session.doc.as_ref().unwrap().serialize()
        );
    }

    #[test]
    fn add_on_collapsed_table_adds_sibling_table() {
        use crate::tui::state::EditField;
        // idea 3 / idea 5: `a` on a collapsed `[t]` adds a sibling `[placeholder]`
        // (a scalar there would be captured by `[t]`). A keyed container sibling
        // opens in rename Edit mode so Esc cancels the just-added node.
        let mut app = app_with("[t]\nx = 1\n");
        app.select_row(app.rows.iter().position(|r| r.key == "t").unwrap()); // collapsed
        app.add_node();
        assert!(
            matches!(&app.session.mode, Mode::Edit(e) if e.field == EditField::Name),
            "structured add: rename edit"
        );
        let s = app.session.doc.as_ref().unwrap().serialize();
        assert!(s.contains("[placeholder]"), "serialize: {s:?}");
        // It is a sibling of [t], not nested inside it.
        assert!(s.contains("[t]") && s.contains("[placeholder]"));
    }

    #[test]
    fn add_on_collapsed_dotted_table_adds_table_sibling() {
        use crate::tui::state::EditField;
        // Same-kind model: `a` on a `[T/D]` table (a Table node) adds a sibling
        // table — an empty `[placeholder]` scope table, in rename Edit mode.
        // `[T/D]` tables start collapsed, so `a` is a collapsed branch.
        let mut app = app_with("a.b = 1\n");
        app.select_row(app.rows.iter().position(|r| r.key == "a").unwrap());
        app.add_node();
        assert!(
            matches!(&app.session.mode, Mode::Edit(e) if e.field == EditField::Name),
            "table add: rename edit"
        );
        let s = app.session.doc.as_ref().unwrap().serialize();
        assert!(s.contains("[placeholder]"), "serialize: {s:?}");
    }

    #[test]
    fn add_on_collapsed_array_adds_array_sibling() {
        use crate::tui::state::EditField;
        // Same-kind model: `a` on a collapsed array adds an empty array sibling
        // right after it in the same scope — no stray scalar two rows up. The
        // keyed sibling opens in rename Edit mode (Esc cancels).
        let mut app = app_with("nums = [1, 2]\nname = \"x\"\n");
        app.select_row(app.rows.iter().position(|r| r.key == "nums").unwrap());
        app.add_node();
        assert!(
            matches!(&app.session.mode, Mode::Edit(e) if e.field == EditField::Name),
            "array add: rename edit"
        );
        let s = app.session.doc.as_ref().unwrap().serialize();
        assert_eq!(s, "nums = [1, 2]\nplaceholder = []\nname = \"x\"\n");
    }

    #[test]
    fn add_on_toml_array_element_seeds_keyless_bare() {
        // Item 1: adding beside an array *element* seeds a keyless bare scalar
        // (`""`), not a `{ __elem__ = "" }` inline table — uniform with JSON/YAML.
        let mut app = app_with("nums = [1, 2]\n");
        app.session.expanded.insert(vec![Seg::Key("nums".into())]);
        app.rebuild_rows();
        app.session.cursor = app
            .rows
            .iter()
            .find(|r| r.path == vec![Seg::Key("nums".into()), Seg::Index(0)])
            .unwrap()
            .path
            .clone();
        app.add_node();
        assert!(
            matches!(app.session.mode, Mode::Edit(_)),
            "scalar element: inline"
        );
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            "nums = [1, \"\", 2]\n"
        );
    }

    #[test]
    fn esc_after_add_rolls_the_insert_back() {
        // Item 2a: `a` opens the inline editor on a seed; Esc removes the seed and
        // leaves the document (and undo history) exactly as before the add.
        let mut app = app_with("a = 1\nb = 2\n");
        app.select_row(app.rows.iter().position(|r| r.key == "a").unwrap());
        app.add_node();
        assert!(matches!(app.session.mode, Mode::Edit(_)));
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            "a = 1\nnew_field = \"\"\nb = 2\n"
        );
        app.edit_cancel();
        assert!(!matches!(app.session.mode, Mode::Edit(_)));
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            "a = 1\nb = 2\n"
        );
        // No undo/redo crumb: the add never happened.
        app.undo();
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            "a = 1\nb = 2\n"
        );
    }

    #[test]
    fn esc_after_normal_edit_keeps_node() {
        // A cancelled edit of an *existing* node leaves it intact (created_on_add
        // is false), so the rollback path must not fire.
        let mut app = app_with("a = 1\nb = 2\n");
        app.select_row(app.rows.iter().position(|r| r.key == "a").unwrap());
        app.begin_inline_edit();
        app.edit_cancel();
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            "a = 1\nb = 2\n"
        );
    }

    #[test]
    fn add_on_scalar_leaf_adds_scalar_sibling_after() {
        // `a` on a scalar leaf adds an empty-string scalar sibling immediately
        // after it (inline edit), in the same scope.
        let mut app = app_with("a = 1\nb = 2\n");
        app.select_row(app.rows.iter().position(|r| r.key == "a").unwrap());
        app.add_node();
        assert!(
            matches!(app.session.mode, Mode::Edit(_)),
            "scalar add opens inline"
        );
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            "a = 1\nnew_field = \"\"\nb = 2\n"
        );
    }

    #[test]
    fn add_on_expanded_table_appends_scalar_child() {
        // idea 3: `a` on an expanded `[t]` appends a scalar as its last child.
        let mut app = app_with("[t]\nx = 1\n");
        app.session.expanded.insert(vec![Seg::Key("t".into())]);
        app.rebuild_rows();
        app.select_row(app.rows.iter().position(|r| r.key == "t").unwrap());
        app.add_node();
        assert!(
            matches!(app.session.mode, Mode::Edit(_)),
            "scalar add opens inline"
        );
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            "[t]\nx = 1\nnew_field = \"\"\n"
        );
    }

    #[test]
    fn add_root_scalar_lands_before_first_table() {
        // D5 clamp: `a` on the root appends a scalar, clamped to before `[t]`.
        let mut app = app_with("a = 1\n[t]\nx = 1\n");
        app.select_row(0); // root
        app.add_node();
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            "a = 1\nnew_field = \"\"\n[t]\nx = 1\n"
        );
    }

    #[test]
    fn toggle_detail_on_branch_shows_kind_and_child_count() {
        let mut app = app_with("[server]\nhost = \"h\"\nport = 8080\n");
        app.expand_all();
        app.rebuild_rows();
        app.select_row(app.rows.iter().position(|r| r.key == "server").unwrap());
        app.toggle_detail();
        assert!(matches!(app.session.mode, Mode::Detail));
        let d = app.session.detail_text.clone().unwrap();
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
        assert!(matches!(app.session.mode, Mode::Normal));
        assert!(app.session.detail_text.is_none());
    }

    #[test]
    fn detail_distinguishes_inline_table_format() {
        // `{ }` inline table reads as Type table / Format inline; a standard
        // `[table]` reads as Type table / Format table.
        let mut app = app_with("pt = { x = 1 }\n[srv]\nport = 8080\n");
        app.expand_all();
        app.rebuild_rows();
        app.select_row(app.rows.iter().position(|r| r.key == "pt").unwrap());
        app.open_detail();
        let d = app.session.detail_text.clone().unwrap();
        assert!(d.contains("Format:") && d.contains("inline"), "inline: {d}");

        app.exit_detail();
        app.select_row(app.rows.iter().position(|r| r.key == "srv").unwrap());
        app.open_detail();
        let d = app.session.detail_text.clone().unwrap();
        assert!(
            d.contains("Format:") && d.contains("table"),
            "standard: {d}"
        );
    }

    #[test]
    fn detail_scroll_clamps_to_range() {
        let mut app = app_with("port = 8080\n");
        app.select_row(1);
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
        app.select_row(1); // on port (row 0 is root)
        app.open_detail();
        let detail = app
            .session
            .detail_text
            .as_ref()
            .expect("detail should be set");
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
        app.select_row(1); // on the merged comment node (row 0 is root)
        app.open_detail();
        let detail = app
            .session
            .detail_text
            .as_ref()
            .expect("detail should be set");
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
        app.session.expanded.insert(vec![Seg::Key("hosts".into())]);
        app.rebuild_rows();
        app.select_row(3); // hosts[1] = "b" (root=0, hosts=1, [0]=2, [1]=3)
        app.open_detail();
        let detail = app
            .session
            .detail_text
            .as_ref()
            .expect("detail should be set");
        assert!(
            detail.contains("hosts[1]"),
            "detail path should include the element index, got: {detail}"
        );
    }

    #[test]
    fn esc_from_clipboard_with_selection_clears_clipboard_first() {
        let mut app = sample();
        app.select_row(1);
        // Simulate: user selected row 1 then pressed 'c'
        app.session.selection.toggle(app.row_path(1));
        app.session.clipboard = Some(Clipboard {
            fragments: vec!["x = 1\n".into()],
            cut: false,
            sources: vec![vec![Seg::Key("a".into()), Seg::Key("x".into())]],
        });
        // First Esc: should clear clipboard, leave selection intact.
        app.escape();
        assert!(
            app.session.clipboard.is_none(),
            "first Esc must clear clipboard"
        );
        assert!(
            !app.session.selection.is_empty(),
            "first Esc must leave selection intact"
        );
        // Second Esc: should clear selection.
        app.escape();
        assert!(
            app.session.selection.is_empty(),
            "second Esc must clear selection"
        );
    }

    #[test]
    fn esc_from_clipboard_without_selection_clears_in_one_step() {
        let mut app = sample();
        // No selection — cursor-only clipboard.
        app.session.clipboard = Some(Clipboard {
            fragments: vec!["x = 1\n".into()],
            cut: false,
            sources: vec![vec![Seg::Key("a".into()), Seg::Key("x".into())]],
        });
        app.escape();
        assert!(
            app.session.clipboard.is_none(),
            "single Esc must clear clipboard"
        );
        assert!(
            app.session.selection.is_empty(),
            "selection must stay empty"
        );
    }

    #[test]
    fn paste_slots_interleave_into_then_after() {
        let mut app = app_with("a = 1\n[t]\nx = 1\n");
        app.session.expanded.insert(vec![Seg::Key("t".into())]);
        app.rebuild_rows();
        // rows: 0 root(branch), 1 a(leaf), 2 [t](branch), 3 t.x(leaf)
        assert_eq!(
            app.paste_slots(),
            vec![
                PasteSlot::Into(app.row_path(0)),
                PasteSlot::After(app.row_path(0)),
                PasteSlot::After(app.row_path(1)),
                PasteSlot::Into(app.row_path(2)),
                PasteSlot::After(app.row_path(2)),
                PasteSlot::After(app.row_path(3)),
            ]
        );
    }

    #[test]
    fn default_paste_slot_is_after_cursor() {
        let mut app = app_with("a = 1\nb = 2\n");
        app.select_row(1);
        assert_eq!(
            app.effective_paste_slot(),
            PasteSlot::After(app.row_path(1))
        );
    }

    #[test]
    fn into_slot_targets_last_child_of_branch() {
        let mut app = app_with("[t]\nx = 1\ny = 2\n");
        app.session.expanded.insert(vec![Seg::Key("t".into())]);
        app.rebuild_rows();
        // rows: 0 root, 1 [t], 2 t.x, 3 t.y
        let target = app.slot_target(PasteSlot::Into(app.row_path(1))).unwrap();
        assert_eq!(target.parent, vec![Seg::Key("t".into())]);
        assert_eq!(target.index, 2, "append after both existing children");
    }

    #[test]
    fn paste_navigation_steps_slots_and_syncs_cursor() {
        let mut app = app_with("a = 1\nb = 2\n");
        // rows: 0 root, 1 a, 2 b → slots [Into(0),After(0),After(1),After(2)]
        app.select_row(0);
        let (p0, p1) = (app.row_path(0), app.row_path(1));
        app.session.clipboard = Some(Clipboard {
            fragments: vec!["c = 3\n".into()],
            cut: false,
            sources: vec![vec![Seg::Key("a".into())]],
        });
        assert_eq!(app.effective_paste_slot(), PasteSlot::After(p0.clone()));
        app.cursor_down();
        assert_eq!(app.effective_paste_slot(), PasteSlot::After(p1.clone()));
        assert_eq!(
            app.cursor_row_index(),
            Some(1),
            "cursor follows the slot's row"
        );
        app.cursor_up();
        assert_eq!(app.effective_paste_slot(), PasteSlot::After(p0));
    }

    #[test]
    fn paste_into_collapsed_branch_appends_as_child() {
        // [t] collapsed; paste with the Into(t) slot lands inside it (idea 2),
        // not as a top-level sibling.
        let mut app = app_with("[t]\nx = 1\n");
        // rows: 0 root, 1 [t] (collapsed by default)
        app.select_row(1);
        app.session.clipboard = Some(Clipboard {
            fragments: vec!["y = 9\n".into()],
            cut: false,
            sources: vec![vec![Seg::Key("y".into())]],
        });
        app.session.paste_slot = Some(PasteSlot::Into(app.row_path(1)));
        app.paste();
        assert!(
            app.session.status.is_none(),
            "unexpected status: {:?}",
            app.session.status
        );
        let s = app.session.doc.as_ref().unwrap().serialize();
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
        app.session.clipboard = Some(Clipboard {
            fragments: vec!["z = 9\n".into()],
            cut: false,
            sources: vec![vec![Seg::Key("a".into())]],
        });
        // Aim the slot at "after [t]" (root append, past the header).
        app.session.paste_slot = Some(PasteSlot::After(app.row_path(2)));
        app.paste();
        assert!(
            app.session.clipboard.is_some(),
            "clipboard must survive an illegal paste"
        );
        assert!(
            app.session
                .error
                .as_deref()
                .unwrap_or("")
                .contains("paste error"),
            "error: {:?}",
            app.session.error
        );
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            "a = 1\n[t]\nx = 1\n",
            "document must be untouched"
        );
    }

    #[test]
    fn cut_comment_paste_into_multiline_array_moves_it() {
        // #6e: a cut comment can be moved into a multiline array (append slot).
        let mut app = app_with("# top\narr = [\n  1,\n  2,\n]\n");
        let crow = comment_row(&app);
        app.select_row(crow);
        app.cut_selected();
        let arow = app
            .rows
            .iter()
            .find(|r| r.key == "arr")
            .unwrap()
            .path
            .clone();
        app.session.paste_slot = Some(PasteSlot::Into(arow));
        app.paste();
        assert!(
            app.session.status.is_none(),
            "unexpected status: {:?}",
            app.session.status
        );
        let s = app.session.doc.as_ref().unwrap().serialize();
        assert_eq!(s, "arr = [\n  1,\n  2,\n  # top\n]\n", "got: {s:?}");
    }

    #[test]
    fn cut_comment_into_single_line_array_prompts_for_upgrade() {
        // A comment pasted into a single-line array no longer errors: it enters
        // the ArrayUpgrade y/n prompt, non-destructively (clipboard + doc intact).
        let mut app = app_with("# note\narr = [1]\n");
        let crow = comment_row(&app);
        app.select_row(crow);
        app.cut_selected();
        let arow = app
            .rows
            .iter()
            .find(|r| r.key == "arr")
            .unwrap()
            .path
            .clone();
        app.session.paste_slot = Some(PasteSlot::Into(arow.clone()));
        app.paste();
        assert!(
            matches!(
                app.session.mode,
                Mode::Prompt(PromptKind::ArrayUpgrade { .. })
            ),
            "should prompt for the multiline upgrade"
        );
        assert!(app.session.clipboard.is_some(), "clipboard must be kept");
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            "# note\narr = [1]\n",
            "nothing mutated before confirmation"
        );

        // 'n' cancels: clipboard kept, document untouched, back to Normal.
        app.handle_prompt_key('n');
        assert!(matches!(app.session.mode, Mode::Normal));
        assert!(app.session.clipboard.is_some(), "clipboard survives a 'n'");
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            "# note\narr = [1]\n"
        );

        // Retry and confirm with 'y': the array upgrades to multiline, the
        // comment lands inside, and the cut source is deleted.
        app.session.paste_slot = Some(PasteSlot::Into(arow));
        app.paste();
        assert!(matches!(
            app.session.mode,
            Mode::Prompt(PromptKind::ArrayUpgrade { .. })
        ));
        app.handle_prompt_key('y');
        assert!(matches!(app.session.mode, Mode::Normal));
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            "arr = [\n  1,\n  # note\n]\n",
            "upgrade + insert + cut-source delete"
        );
        assert!(
            app.session.clipboard.is_none(),
            "paste consumed the clipboard"
        );
    }

    #[test]
    fn paste_bare_value_into_table_synthesizes_placeholder_key() {
        // C2 / key+: pasting a bare element value into a Table/Root synthesizes a
        // `placeholder` key instead of erroring.
        let mut app = app_with("a = 1\n");
        app.select_row(0); // root
        app.session.clipboard = Some(Clipboard {
            fragments: vec!["42\n".into()],
            cut: false,
            sources: vec![vec![Seg::Key("a".into())]],
        });
        app.paste();
        assert!(
            app.session.status.is_none(),
            "unexpected status: {:?}",
            app.session.status
        );
        let s = app.session.doc.as_ref().unwrap().serialize();
        assert!(s.contains("placeholder = 42"), "serialize: {s:?}");
    }

    #[test]
    fn cut_paste_same_scope_moves_without_collision() {
        let mut app = app_with("a = 1\nb = 2\n");
        app.rebuild_rows();
        app.select_row(1); // on `a`
        app.cut_selected();
        assert!(app.session.clipboard.is_some());
        app.select_row(2); // on `b`
        app.paste();
        assert!(
            matches!(app.session.mode, Mode::Normal),
            "no collision prompt expected"
        );
        let out = app.session.doc.as_ref().unwrap().serialize();
        assert_eq!(out.matches("a =").count(), 1, "exactly one `a`: {out:?}");
        assert_eq!(out.matches("b =").count(), 1, "exactly one `b`: {out:?}");
        assert!(
            app.session.clipboard.is_none(),
            "clipboard consumed on successful move"
        );
    }

    #[test]
    fn copy_paste_comment_node() {
        let mut app = app_with("# note\na = 1\nb = 2\n");
        app.rebuild_rows();
        let cpos = comment_row(&app);
        app.select_row(cpos);
        app.copy_selected();
        assert!(app.session.clipboard.is_some());
        let bpos = app
            .rows
            .iter()
            .position(|r| matches!(r.path.last(), Some(Seg::Key(k)) if k == "b"))
            .unwrap();
        app.select_row(bpos);
        app.paste();
        let out = app.session.doc.as_ref().unwrap().serialize();
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
        app.select_row(cpos);
        app.cut_selected();
        let bpos = app
            .rows
            .iter()
            .position(|r| matches!(r.path.last(), Some(Seg::Key(k)) if k == "b"))
            .unwrap();
        app.select_row(bpos);
        app.paste();
        let out = app.session.doc.as_ref().unwrap().serialize();
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
        let doc = app.session.doc.as_ref().unwrap();
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
        let doc = app.session.doc.as_ref().unwrap();
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
        app.session.cursor = app
            .rows
            .iter()
            .find(|r| matches!(r.path.last(), Some(Seg::Key(k)) if k == "x"))
            .unwrap()
            .path
            .clone();
        app.cut_selected();
        app.select_row(comment_row(&app));
        app.paste();
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            "# note\nx = 1\ny = 2\n"
        );
    }

    #[test]
    fn cut_comment_pastes_elsewhere_without_vanishing() {
        // Regression: cutting a comment and pasting it elsewhere must move it, not
        // lose it.
        let mut app = app_with("# note\na = 1\nb = 2\nc = 3\n");
        app.rebuild_rows();
        app.select_row(comment_row(&app));
        app.cut_selected();
        app.session.cursor = app
            .rows
            .iter()
            .find(|r| matches!(r.path.last(), Some(Seg::Key(k)) if k == "c"))
            .unwrap()
            .path
            .clone();
        app.paste();
        let out = app.session.doc.as_ref().unwrap().serialize();
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
        app.select_row(comment_row(&app));
        app.cut_selected();
        app.session.cursor = app
            .rows
            .iter()
            .find(|r| matches!(r.path.last(), Some(Seg::Key(k)) if k == "a"))
            .unwrap()
            .path
            .clone();
        app.paste();
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            "a = 1\n# c\nb = 2\n"
        );
    }

    #[test]
    fn cut_comment_moves_down_without_overshoot_jsonc() {
        let mut app = app_with_jsonc("{\n  // c\n  \"a\": 1,\n  \"b\": 2\n}\n");
        app.rebuild_rows();
        app.select_row(comment_row(&app));
        app.cut_selected();
        app.session.cursor = app
            .rows
            .iter()
            .find(|r| matches!(r.path.last(), Some(Seg::Key(k)) if k == "a"))
            .unwrap()
            .path
            .clone();
        app.paste();
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            "{\n  \"a\": 1,\n  // c\n  \"b\": 2\n}\n"
        );
    }

    #[test]
    fn cut_comment_moves_down_without_overshoot_yaml() {
        let mut app = app_with_yaml("# c\na: 1\nb: 2\n");
        app.rebuild_rows();
        app.select_row(comment_row(&app));
        app.cut_selected();
        app.session.cursor = app
            .rows
            .iter()
            .find(|r| matches!(r.path.last(), Some(Seg::Key(k)) if k == "a"))
            .unwrap()
            .path
            .clone();
        app.paste();
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            "a: 1\n# c\nb: 2\n"
        );
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
        app.select_row(comment_row(&app));
        app.cut_selected();
        app.session.cursor = app
            .rows
            .iter()
            .find(|r| matches!(r.path.last(), Some(Seg::Key(k)) if k == "multiline_literal"))
            .unwrap()
            .path
            .clone();
        app.paste();
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
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
        app.select_row(comment_row(&app));
        app.copy_selected();
        app.session.cursor = app
            .rows
            .iter()
            .find(|r| matches!(r.path.last(), Some(Seg::Key(k)) if k == "multiline_literal"))
            .unwrap()
            .path
            .clone();
        app.paste();
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
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
                app.session
                    .tree
                    .node_at(&r.path)
                    .map(|n| matches!(n.kind, NodeKind::Comment(_)))
                    .unwrap_or(false)
            })
            .map(|r| r.path.clone())
            .collect();
        assert_eq!(cpaths.len(), 2);
        let doc = app.session.doc.as_ref().unwrap();
        let fragments: Vec<String> = cpaths.iter().map(|p| doc.serialize_fragment(p)).collect();
        app.session.clipboard = Some(Clipboard {
            fragments,
            cut: false,
            sources: cpaths,
        });
        // Paste onto `y` (so the copies land together, after the originals).
        app.session.cursor = app
            .rows
            .iter()
            .find(|r| matches!(r.path.last(), Some(Seg::Key(k)) if k == "y"))
            .unwrap()
            .path
            .clone();
        app.paste();
        let out = app.session.doc.as_ref().unwrap().serialize();
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
        // Mimic the host's `.jsonc`-extension comment-enable.
        let mut doc = crate::model::any_doc::AnyDocument::from_str_as(
            src,
            crate::model::document::DocFormat::Json,
        )
        .unwrap();
        doc.enable_comments();
        App::new(doc)
    }

    fn app_with_yaml(src: &str) -> App {
        let doc = crate::model::any_doc::AnyDocument::from_str_as(
            src,
            crate::model::document::DocFormat::Yaml,
        )
        .unwrap();
        App::new(doc)
    }

    fn app_with_json(src: &str) -> App {
        let doc = crate::model::any_doc::AnyDocument::from_str_as(
            src,
            crate::model::document::DocFormat::Json,
        )
        .unwrap();
        App::new(doc)
    }

    /// Move the cursor to the first row whose path ends in the given key segment,
    /// expanding everything first.
    fn cursor_to_key(app: &mut App, key: &str) {
        app.expand_all();
        app.rebuild_rows();
        app.session.cursor = app
            .rows
            .iter()
            .find(|r| r.key == key)
            .unwrap_or_else(|| panic!("row {key:?} not found"))
            .path
            .clone();
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
            app.session.doc.as_ref().unwrap().serialize(),
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
        assert!(
            matches!(app.session.mode, Mode::Edit(_)),
            "member add opens inline"
        );
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            "arr = [\n  { a = 1, new_field = \"\" },\n]\n"
        );
    }

    #[test]
    fn toml_inline_table_array_element_edits_inline_keeping_comment() {
        // Group B items 2b.1 + 7: the `[T/I]` element itself edits inline as its
        // one-liner, and its trailing comment survives the edit.
        let mut app = app_with("arr = [\n  { a = 1 },  # note\n]\n");
        app.session.expanded.insert(vec![Seg::Key("arr".into())]);
        app.rebuild_rows();
        app.session.cursor = app
            .rows
            .iter()
            .find(|r| r.path == vec![Seg::Key("arr".into()), Seg::Index(0)])
            .unwrap()
            .path
            .clone();
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        // The editor seeds the buffer as `value  # comment`; editing the value while
        // keeping the comment must preserve it on commit.
        app.begin_inline_edit();
        let seeded = match &app.session.mode {
            Mode::Edit(e) => e.buffer.clone(),
            _ => panic!("inline editor open"),
        };
        assert_eq!(seeded, "{ a = 1 }  # note", "comment seeded into buffer");
        if let Mode::Edit(e) = &mut app.session.mode {
            e.buffer = "{ a = 2 }  # note".to_string();
            e.cursor = e.buffer.chars().count();
        }
        app.edit_commit();
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
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
        app.session.cursor = app
            .rows
            .iter()
            .find(|r| r.path == vec![Seg::Key("arr".into()), Seg::Index(0)])
            .unwrap()
            .path
            .clone();
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        // And the one-liner edit applies through Replace.
        inline_set_value(&mut app, "{ \"a\": 1 }");
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
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
        assert!(matches!(app.session.mode, Mode::Edit(_)));
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
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
            app.session.doc.as_ref().unwrap().serialize(),
            "{\n  \"arr\": [\n    { \"a\": 5, \"b\": 2 }\n  ]\n}\n"
        );
    }

    /// Drive the inline editor to set the Value field to `new_value` and commit.
    fn inline_set_value(app: &mut App, new_value: &str) {
        app.begin_inline_edit();
        if let Mode::Edit(e) = &mut app.session.mode {
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
        app.select_row(name_row);
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        let port_row = app.rows.iter().position(|r| r.key == "port").unwrap();
        app.select_row(port_row);
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        // And the edit actually applies through Replace.
        inline_set_value(&mut app, "9090");
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
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
            .session
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
            app.session.doc.as_ref().unwrap().serialize(),
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
            app.session.doc.as_ref().unwrap().serialize(),
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
        let wrapped = app
            .session
            .doc
            .as_ref()
            .unwrap()
            .scalar_fragment(None, "\"z\"");
        app.apply_replace(path, wrapped);
        assert!(
            app.session.status.is_none() && app.session.error.is_none(),
            "status {:?} error {:?}",
            app.session.status,
            app.session.error
        );
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
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
        app.session.cursor = app
            .rows
            .iter()
            .find(|r| r.path == p)
            .expect("vals[0] visible")
            .path
            .clone();
        assert_eq!(
            app.edit_target_kind(),
            EditKind::Inline,
            "deep array element edits inline"
        );
        inline_set_value(&mut app, "999");
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
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
        app.session.cursor = app
            .rows
            .iter()
            .find(|r| r.path == p)
            .expect("vals[0] visible")
            .path
            .clone();
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        inline_set_value(&mut app, "999");
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
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
            .session
            .doc
            .as_ref()
            .unwrap()
            .scalar_fragment(None, "{ \"a\": 9 }");
        app.apply_replace(path, wrapped);
        assert!(
            app.session.status.is_none() && app.session.error.is_none(),
            "status {:?} error {:?}",
            app.session.status,
            app.session.error
        );
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
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
            app.session.status.is_none() && app.session.error.is_none(),
            "status {:?} error {:?}",
            app.session.status,
            app.session.error
        );
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
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
            app.session.status.is_none() && app.session.error.is_none(),
            "status {:?} error {:?}",
            app.session.status,
            app.session.error
        );
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
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
        app.select_row(row);
        assert_eq!(app.edit_target_kind(), EditKind::Inline);
        inline_set_value(&mut app, "1  // first");
        assert!(matches!(app.session.mode, Mode::Normal), "should commit");
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
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
        app.select_row(row);
        let before = app.session.doc.as_ref().unwrap().serialize();
        inline_set_value(&mut app, "1  // nope");
        assert!(matches!(app.session.mode, Mode::Edit(_)), "stays in editor");
        assert!(app
            .session
            .status
            .as_deref()
            .unwrap_or("")
            .contains("inline collection"));
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            before,
            "doc untouched"
        );
    }

    #[test]
    fn yaml_nudge_preserves_inline_comment() {
        // The ←/→ value nudge must keep a YAML trailing comment (YAML Replace drops
        // it, so the nudge re-asserts it like the editor does).
        let mut app = app_with_yaml("port: 8081  # bind\n");
        app.select_row(app.rows.iter().position(|r| r.key == "port").unwrap());
        app.nudge(1);
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            "port: 8082  # bind\n"
        );
    }

    #[test]
    fn yaml_value_edit_preserves_inline_comment() {
        // ④ YAML Replace swaps the whole entry (dropping the comment); the editor
        // must re-assert the unchanged comment so it survives a value-only edit.
        let mut app = app_with_yaml("host: x  # bind\n");
        app.select_row(app.rows.iter().position(|r| r.key == "host").unwrap());
        // Edit only the value; keep the comment text the same.
        inline_set_value(&mut app, "y  # bind");
        assert!(
            matches!(app.session.mode, Mode::Normal),
            "should commit cleanly"
        );
        assert_eq!(
            app.session.doc.as_ref().unwrap().serialize(),
            "host: y  # bind\n"
        );
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
        app.select_row(ci);
        // Verify the node is read_only before mutating.
        assert!(app.cursor_is_read_only(), "block comment must be read_only");
        app.delete_selected();
        assert!(
            app.session
                .status
                .as_deref()
                .unwrap_or("")
                .contains("read-only"),
            "expected read-only status, got: {:?}",
            app.session.status
        );
        use crate::model::document::ConfigDocument;
        assert!(
            app.session
                .doc
                .as_ref()
                .unwrap()
                .serialize()
                .contains("/* ro */"),
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
        app.select_row(ci);
        app.edit_node();
        assert!(
            app.session
                .status
                .as_deref()
                .unwrap_or("")
                .contains("read-only"),
            "expected read-only status, got: {:?}",
            app.session.status
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
        app.select_row(ci);
        app.cut_selected();
        assert!(
            app.session
                .status
                .as_deref()
                .unwrap_or("")
                .contains("read-only"),
            "expected read-only status, got: {:?}",
            app.session.status
        );
        assert!(
            app.session.clipboard.is_none(),
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
        app.select_row(ci);
        app.remark();
        assert!(
            app.session
                .status
                .as_deref()
                .unwrap_or("")
                .contains("read-only"),
            "expected read-only status, got: {:?}",
            app.session.status
        );
        use crate::model::document::ConfigDocument;
        assert!(
            app.session
                .doc
                .as_ref()
                .unwrap()
                .serialize()
                .contains("/* ro */"),
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
        app.select_row(ci);
        assert!(app.cursor_is_read_only(), "block comment must be read_only");
        app.copy_selected();
        assert!(
            app.session.clipboard.is_some(),
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
        app.select_row(ci);
        app.begin_inline_edit();
        // Clear the value buffer and type a new JSON string literal `"b"`.
        if let Mode::Edit(ref mut e) = app.session.mode {
            e.buffer.clear();
            e.cursor = 0;
        }
        for c in "\"b\"".chars() {
            app.edit_input_char(c);
        }
        app.edit_commit();
        assert!(
            app.session.error.is_none(),
            "unexpected error: {:?}",
            app.session.error
        );
        assert!(
            app.session.status.as_deref() != Some("invalid value"),
            "edit should not have failed validation"
        );
        assert!(
            app.session
                .doc
                .as_ref()
                .unwrap()
                .serialize()
                .contains("\"tags\": \"b\""),
            "value must be updated: {}",
            app.session.doc.as_ref().unwrap().serialize()
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
        app.select_row(ci);
        app.nudge(1);
        assert!(
            app.session.error.is_none(),
            "unexpected error: {:?}",
            app.session.error
        );
        assert!(
            app.session
                .doc
                .as_ref()
                .unwrap()
                .serialize()
                .contains("\"port\": 8081"),
            "nudged value must be 8081: {}",
            app.session.doc.as_ref().unwrap().serialize()
        );
    }
}
