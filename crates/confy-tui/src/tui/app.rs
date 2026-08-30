use std::cell::Cell;
use std::path::PathBuf;

pub use confy_core::session::{EditKind, FilterLayer, PendingCommit};
use confy_core::session::{Intent, Session};

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
    /// The `~` diag ring overlay, when open. Host-side mini-mode (not a core
    /// `Mode` variant) — purely read-only TUI visualization of `Session.diag`.
    pub diag_overlay_open: bool,
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
    /// Key-sign label (`bare`/`quoted`/`dotted`/`none`) — the Detail popup's
    /// "Sign" field only; never used to reconstruct a key's spelling.
    pub key_sign: String,
    /// The key's authored spelling, or `None` for keyless rows — renders the
    /// tree-row label via `ui.rs::display_key`.
    pub key_literal: Option<String>,
    /// Writing style of a scalar leaf (`Plain` for branches/comments).
    pub format: Format,
    pub trailing_comment: Option<String>,
    pub violations: Option<Vec<String>>,
    pub has_descendant_violation: bool,
    pub comment_advisory: Option<String>,
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
            diag_overlay_open: false,
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
            diag_overlay_open: false,
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
                let type_tag = if vr.violations.is_some() {
                    // The KIND column is a fixed 8 cols; the tag's padding lives
                    // *inside* the brackets (e.g. `[I:dec ]`), so `trim_end` is a
                    // no-op. Swap that internal space for `!` to stay in budget.
                    // Tags with no padding space (e.g. `[B:bool]`) keep their glyph;
                    // the row's yellow accent is the primary cue then.
                    type_tag.replacen(' ', "!", 1)
                } else {
                    type_tag
                };
                let scalar_type = vr.scalar_type.map(|st| format!("{st:?}").to_lowercase());
                RowSnapshot {
                    key: vr.key,
                    path: vr.path,
                    depth: vr.depth,
                    is_branch: vr.is_branch,
                    value: vr.value,
                    scalar_type,
                    type_label: vr.type_label.into_owned(),
                    type_tag,
                    format: vr.format,
                    key_sign: vr.key_sign.into_owned(),
                    key_literal: vr.key_literal,
                    trailing_comment: vr.trailing_comment,
                    violations: vr.violations.clone(),
                    has_descendant_violation: vr.has_descendant_violation,
                    comment_advisory: vr.comment_advisory,
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
        self.session.apply(Intent::CursorDown);
    }
    pub fn cursor_up(&mut self) {
        self.session.apply(Intent::CursorUp);
    }
    pub fn toggle_expand(&mut self) {
        self.session.toggle_expand();
    }
    pub fn collapse_all(&mut self) {
        self.session.apply(Intent::CollapseAll);
        self.rebuild_rows();
    }
    pub fn expand_all(&mut self) {
        self.session.apply(Intent::ExpandAll);
        self.rebuild_rows();
    }
    pub fn expand_level(&mut self) {
        self.session.apply(Intent::ExpandLevel);
        self.rebuild_rows();
    }
    pub fn collapse_level(&mut self) {
        self.session.apply(Intent::CollapseLevel);
        self.rebuild_rows();
    }
    pub fn page_up(&mut self, page_size: usize) {
        self.session.apply(Intent::PageUp(page_size));
    }
    pub fn page_down(&mut self, page_size: usize) {
        self.session.apply(Intent::PageDown(page_size));
    }
    pub fn cursor_home(&mut self) {
        self.session.apply(Intent::CursorHome);
    }
    pub fn cursor_end(&mut self) {
        self.session.apply(Intent::CursorEnd);
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
        self.session.apply(Intent::EnterFilter);
        self.rebuild_rows();
    }
    pub fn commit_filter(&mut self) {
        self.session.apply(Intent::CommitFilter);
        self.rebuild_rows();
    }
    pub fn exit_filter_results(&mut self) {
        self.session.apply(Intent::ExitFilterResults);
        self.rebuild_rows();
    }
    pub fn exit_filter(&mut self) {
        self.session.apply(Intent::ExitFilter);
        self.rebuild_rows();
    }
    pub fn filter_char(&mut self, c: char) {
        self.session.apply(Intent::FilterChar(c));
        self.rebuild_rows();
    }
    pub fn filter_backspace(&mut self) {
        self.session.apply(Intent::FilterBackspace);
        self.rebuild_rows();
    }
    pub fn filter_delete(&mut self) {
        self.session.apply(Intent::FilterDelete);
        self.rebuild_rows();
    }
    pub fn filter_cursor_left(&mut self) {
        self.session.apply(Intent::FilterCursorLeft);
    }
    pub fn filter_cursor_right(&mut self) {
        self.session.apply(Intent::FilterCursorRight);
    }
    pub fn filter_cursor_home(&mut self) {
        self.session.apply(Intent::FilterCursorHome);
    }
    pub fn filter_cursor_end(&mut self) {
        self.session.apply(Intent::FilterCursorEnd);
    }
    #[cfg(test)]
    fn recompute_filter(&mut self) {
        self.session.recompute_filter();
        self.rebuild_rows();
    }

    // ---- Type filter (f) ----

    pub fn enter_type_filter(&mut self) {
        self.session.apply(Intent::EnterTypeFilter);
        self.rebuild_rows();
    }
    pub fn type_filter_move(&mut self, dr: i32, dc: i32) {
        self.session.apply(Intent::TypeFilterMove(dr, dc));
    }
    pub fn type_filter_toggle(&mut self) {
        self.session.apply(Intent::TypeFilterToggle);
        self.rebuild_rows();
    }
    pub fn commit_type_filter(&mut self) {
        self.session.apply(Intent::CommitTypeFilter);
        self.rebuild_rows();
    }
    pub fn exit_type_filter(&mut self) {
        self.session.apply(Intent::ExitTypeFilter);
        self.rebuild_rows();
    }

    // ---- Format ----

    pub fn doc_format(&self) -> crate::model::document::DocFormat {
        self.session.doc_format()
    }

    // ---- Kind switch (K) ----

    pub fn open_kind_switch(&mut self) {
        self.session.apply(Intent::OpenKindSwitch);
    }
    pub fn kind_switch_move(&mut self, delta: i32) {
        self.session.apply(Intent::KindSwitchMove(delta));
    }
    pub fn kind_switch_commit(&mut self) {
        self.session.apply(Intent::KindSwitchCommit);
        self.rebuild_rows();
    }
    pub fn exit_kind_switch(&mut self) {
        self.session.apply(Intent::ExitKindSwitch);
    }

    // ---- Document conversion (C) ----

    pub fn open_convert(&mut self) {
        self.session.apply(Intent::OpenConvert);
    }
    pub fn convert_move(&mut self, delta: i32) {
        self.session.apply(Intent::ConvertMove(delta));
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
        self.session.apply(Intent::ConvertPathChar(c));
    }
    pub fn convert_path_backspace(&mut self) {
        self.session.apply(Intent::ConvertPathBackspace);
    }
    pub fn convert_path_delete(&mut self) {
        self.session.apply(Intent::ConvertPathDelete);
    }
    pub fn convert_path_left(&mut self) {
        self.session.apply(Intent::ConvertPathLeft);
    }
    pub fn convert_path_right(&mut self) {
        self.session.apply(Intent::ConvertPathRight);
    }
    pub fn convert_path_home(&mut self) {
        self.session.apply(Intent::ConvertPathHome);
    }
    pub fn convert_path_end(&mut self) {
        self.session.apply(Intent::ConvertPathEnd);
    }
    /// Toggle the output path's extension between `.json` and `.jsonc` on the
    /// Convert Path step (item 1: TUI-side JSONC option — approach A from the
    /// design discussion. The Format step's picker is core-cursor-driven with
    /// exactly 3 entries (`DocFormat` has no `Jsonc` variant; `.json`/`.jsonc`
    /// both compile to `DocFormat::Json` by design, see CONTEXT.md "Comment
    /// advisory"), so a 4th list row would desync the core cursor bounds.
    /// Editing the path extension directly avoids touching that cursor math.
    /// Bound to `Tab` on the Path step (mod.rs) — free there since the field
    /// only otherwise consumes printable chars + cursor/edit keys. A no-op
    /// when the target isn't Json or the path doesn't end in `.json`/`.jsonc`.
    pub fn convert_toggle_jsonc_ext(&mut self) {
        let crate::tui::state::Mode::Convert(st) = &self.session.mode else {
            return;
        };
        if st.target != crate::model::document::DocFormat::Json {
            return;
        }
        let path = st.path.clone();
        let lower = path.to_ascii_lowercase();
        let new_path = if lower.ends_with(".jsonc") {
            path[..path.len() - 1].to_string()
        } else if lower.ends_with(".json") {
            format!("{path}c")
        } else {
            return;
        };
        self.session.apply(Intent::SetConvertPath(new_path));
    }
    pub fn convert_run(&mut self) {
        let outcome = self.session.apply(Intent::ConvertRun);
        if let Some((path, text)) = outcome.convert_write {
            self.convert_write(&path, &text);
        }
        self.rebuild_rows();
    }
    pub fn convert_confirm(&mut self) {
        let outcome = self.session.apply(Intent::ConvertConfirm);
        if let Some((path, text)) = outcome.convert_write {
            self.convert_write(&path, &text);
        }
        self.rebuild_rows();
    }
    fn convert_write(&mut self, path: &str, text: &str) {
        match std::fs::write(path, text) {
            Ok(()) => {
                self.session
                    .dispatch(confy_core::session::Intent::SetHostNotice {
                        key: "tui.host.convert-success".to_string(),
                        args: vec![path.to_string()],
                        source: confy_core::session::notice::NoticeSource::HostTui,
                    });
                self.session.mode = if self.session.filtered_paths.is_some() {
                    Mode::FilterResults
                } else {
                    Mode::Normal
                };
            }
            Err(e) => {
                self.session
                    .dispatch(confy_core::session::Intent::SetHostNotice {
                        key: "tui.host.convert-write-failed".to_string(),
                        args: vec![e.to_string()],
                        source: confy_core::session::notice::NoticeSource::HostTui,
                    });
                self.session.mode = Mode::Normal;
            }
        }
    }
    pub fn exit_convert(&mut self) {
        self.session.apply(Intent::ExitConvert);
    }

    // ---- Detail view ----

    pub fn toggle_detail(&mut self) {
        self.session.apply(Intent::ToggleDetail);
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
        self.session.apply(Intent::ExitDetail);
    }

    // ---- Help ----

    pub fn enter_help(&mut self) {
        self.help_scroll = 0;
        self.session.apply(Intent::EnterHelp);
    }
    pub fn help_scroll_by(&mut self, delta: i32, max: u16) {
        let v = (self.help_scroll as i32 + delta).clamp(0, max as i32);
        self.help_scroll = v as u16;
    }
    pub fn help_set_scroll(&mut self, v: u16) {
        self.help_scroll = v;
    }
    pub fn exit_help(&mut self) {
        self.session.apply(Intent::ExitHelp);
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
        if self.session.clipboard.is_some() {
            self.session
                .dispatch(confy_core::session::Intent::SetHostNotice {
                    key: "core.clipboard.action-locked".to_string(),
                    args: vec![],
                    source: confy_core::session::notice::NoticeSource::HostTui,
                });
            return;
        }
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
        match crate::config::save_config(&cfg) {
            Ok(()) => {
                self.session
                    .dispatch(confy_core::session::Intent::SetHostNotice {
                        key: "tui.lang.saved".to_string(),
                        args: vec![lang.code().to_string()],
                        source: confy_core::session::notice::NoticeSource::HostTui,
                    });
            }
            Err(e) => {
                self.session
                    .dispatch(confy_core::session::Intent::SetHostNotice {
                        key: "tui.lang.save-failed".to_string(),
                        args: vec![e.to_string()],
                        source: confy_core::session::notice::NoticeSource::HostTui,
                    });
            }
        }
    }
    pub fn exit_lang_picker(&mut self) {
        self.lang_picker = None;
    }

    // ---- Selection ----

    pub fn toggle_select(&mut self) {
        self.session.apply(Intent::ToggleSelect);
    }
    pub fn extend_select_up(&mut self) {
        self.session.apply(Intent::ExtendSelectUp);
    }
    pub fn extend_select_down(&mut self) {
        self.session.apply(Intent::ExtendSelectDown);
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
        if self.session.clipboard.is_some() {
            self.session
                .dispatch(confy_core::session::Intent::SetHostNotice {
                    key: "core.clipboard.action-locked".to_string(),
                    args: vec![],
                    source: confy_core::session::notice::NoticeSource::HostTui,
                });
            return;
        }
        if self.cursor_is_read_only() {
            self.session
                .dispatch(confy_core::session::Intent::SetHostNotice {
                    key: "tui.host.readonly-comment".to_string(),
                    args: vec![],
                    source: confy_core::session::notice::NoticeSource::HostTui,
                });
            return;
        }
        let cursor_row = match self.cursor_row() {
            Some(r) => r.clone(),
            None => return,
        };
        if let Some(node) = self.session.tree.node_at(&cursor_row.path) {
            if let NodeKind::Comment(_) = &node.kind {
                if self.session.no_array_ancestor(&cursor_row.path) {
                    // $EDITOR initial = the CST fragment (raw block text with
                    // per-line indent), NOT the DOM projection text: the
                    // projection's comment merge drops each line's leading
                    // INDENT, which flattened a nested remarked block on open.
                    let fragment = match self.session.doc.as_ref() {
                        Some(d) => d.serialize_fragment(&cursor_row.path),
                        None => return,
                    };
                    if fragment.is_empty() {
                        return;
                    }
                    let initial = format!("{fragment}\n");
                    let edited = match crate::tui::editor::edit_text(&initial) {
                        Ok(t) => t,
                        Err(e) => {
                            self.session
                                .dispatch(confy_core::session::Intent::SetHostNotice {
                                    key: "tui.host.editor-error".to_string(),
                                    args: vec![e.to_string()],
                                    source: confy_core::session::notice::NoticeSource::HostTui,
                                });
                            return;
                        }
                    };
                    // Unmodified buffer = quit without saving: cancel instead
                    // of splicing the text back (which would dirty the doc).
                    if edited == initial {
                        return;
                    }
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
                self.session
                    .dispatch(confy_core::session::Intent::SetHostNotice {
                        key: "tui.host.editor-error".to_string(),
                        args: vec![e.to_string()],
                        source: confy_core::session::notice::NoticeSource::HostTui,
                    });
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
        self.session.apply(Intent::BeginRename);
    }
    pub fn edit_toggle_field(&mut self) {
        self.session.apply(Intent::EditToggleField);
    }
    pub fn edit_clamp_scroll(&mut self, width: usize) {
        self.session.edit_clamp_scroll(width);
    }
    pub fn edit_input_char(&mut self, c: char) {
        self.session.apply(Intent::EditChar(c));
    }
    pub fn edit_backspace(&mut self) {
        self.session.apply(Intent::EditBackspace);
    }
    pub fn edit_delete(&mut self) {
        self.session.apply(Intent::EditDelete);
    }
    pub fn edit_cursor_left(&mut self) {
        self.session.apply(Intent::EditCursorLeft);
    }
    pub fn edit_cursor_right(&mut self) {
        self.session.apply(Intent::EditCursorRight);
    }
    pub fn edit_cursor_home(&mut self) {
        self.session.apply(Intent::EditCursorHome);
    }
    pub fn edit_cursor_end(&mut self) {
        self.session.apply(Intent::EditCursorEnd);
    }
    pub fn edit_cancel(&mut self) {
        self.session.apply(Intent::EditCancel);
        self.rebuild_rows();
    }
    pub fn edit_commit(&mut self) {
        self.session.apply(Intent::EditCommit);
        self.rebuild_rows();
    }

    // ---- Mutations ----

    /// $EDITOR commit (`edit_node`'s external-edit branch). `session`'s
    /// `apply_external_replace` (not the bare `apply_replace` the inline
    /// editor's commit path uses) treats `edited` as the fragment's complete,
    /// authoritative text so an explicit trailing-comment deletion in the
    /// popped-open editor sticks (comment-advisory follow-up issue #4).
    pub(crate) fn apply_replace(&mut self, path: Path, edited: String) {
        self.session.apply_external_replace(path, edited);
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
        self.session.apply(Intent::Nudge(delta));
        self.rebuild_rows();
    }
    pub fn add_node(&mut self) {
        self.session.apply(Intent::AddNode);
        self.rebuild_rows();
    }
    pub fn delete_selected(&mut self) {
        self.session.apply(Intent::DeleteSelected);
        self.rebuild_rows();
    }
    pub fn copy_selected(&mut self) {
        self.session.apply(Intent::CopySelected);
    }
    pub fn cut_selected(&mut self) {
        self.session.apply(Intent::CutSelected);
    }
    pub fn paste(&mut self) {
        self.session.apply(Intent::Paste);
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
        self.session.apply(Intent::Remark);
        self.rebuild_rows();
    }

    // ---- Save (HOST fs write) ----

    pub fn save(&mut self) {
        let Some(ref path) = self.source_path else {
            self.session
                .dispatch(confy_core::session::Intent::SetHostNotice {
                    key: "tui.host.no-save-path".to_string(),
                    args: vec![],
                    source: confy_core::session::notice::NoticeSource::HostTui,
                });
            return;
        };
        let path = path.clone();
        let doc = match self.session.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        if !doc.is_dirty() {
            self.session
                .dispatch(confy_core::session::Intent::SetHostNotice {
                    key: "tui.host.no-changes".to_string(),
                    args: vec![],
                    source: confy_core::session::notice::NoticeSource::HostTui,
                });
            return;
        }
        let text = doc.serialize();
        match std::fs::write(&path, text) {
            Ok(()) => {
                doc.mark_saved();
                self.session
                    .dispatch(confy_core::session::Intent::SetHostNotice {
                        key: "tui.host.saved".to_string(),
                        args: vec![],
                        source: confy_core::session::notice::NoticeSource::HostTui,
                    });
            }
            Err(e) => {
                self.session
                    .dispatch(confy_core::session::Intent::SetHostNotice {
                        key: "tui.host.save-error".to_string(),
                        args: vec![e.to_string()],
                        source: confy_core::session::notice::NoticeSource::HostTui,
                    });
            }
        }
    }

    // ---- Undo / redo ----

    pub fn undo(&mut self) {
        self.session.apply(Intent::Undo);
        self.rebuild_rows();
    }
    pub fn redo(&mut self) {
        self.session.apply(Intent::Redo);
        self.rebuild_rows();
    }

    // ---- Escape / quit ----

    pub fn escape(&mut self) {
        self.session.apply(Intent::Escape);
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
        let outcome = self.session.apply(Intent::PromptKey(c));
        if outcome.quit {
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
pub(crate) fn type_tag(
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
#[path = "tests.rs"]
mod tests;
