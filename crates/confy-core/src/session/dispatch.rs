//! The single command channel for non-TUI hosts (WASM / Web UI).
//!
//! `Session::dispatch(Intent) -> SessionSnapshot` is the one entry point the Web
//! UI uses: it serializes one [`super::intent::Intent`] and re-renders from the
//! returned [`super::view::SessionSnapshot`]. The routing mirrors the TUI event
//! loop (`confy_tui::tui::run_event_loop`), but expressed as a direct
//! `Intent → Session method` map. The TUI itself is unchanged — it still calls
//! `Session` methods directly; this is the new, independently-tested WASM
//! contract (PORTING §8.4).

use crate::model::document::ConfigDocument;
use crate::model::node::NodeKind;
use crate::session::intent::Intent;
use crate::session::state::{EditKind, KindSwitchState, Mode, PendingExternalEdit, PromptKind};
use crate::session::type_filter::{layout, LayoutRow};
use crate::session::view::{
    ConvertView, EditView, ExternalEdit, ExternalEditKind, KindOptionView, ModeView, PromptView,
    SessionSnapshot, TypeFilterCellView, TypeFilterRow, TypeFilterView,
};

impl super::Session {
    /// The one command channel. Map a key (in the host) to an [`Intent`], call
    /// this, and re-render from the returned [`SessionSnapshot`].
    ///
    /// Full-state transport (PORTING §8.3): the snapshot carries the entire
    /// visible tree + modal surfaces + transient signals (`external_edit`,
    /// `convert_write`, `quit`). No structured row diff yet.
    pub fn dispatch(&mut self, intent: Intent) -> SessionSnapshot {
        // Any non-shift-extend action ends the current shift multi-select round,
        // so the next Shift+Arrow begins a fresh one (mirrors the TUI loop).
        if !matches!(intent, Intent::ExtendSelectUp | Intent::ExtendSelectDown) {
            self.last_action_was_shift_select = false;
        }

        // Transient signals that only the dispatch return carries (not persistent
        // session state), overlaid on the snapshot after routing.
        let mut convert_write = None;
        let mut quit = false;

        match intent {
            // ---- Navigation ----
            Intent::CursorDown => self.cursor_down(),
            Intent::CursorUp => self.cursor_up(),
            Intent::CursorHome => self.cursor_home(),
            Intent::CursorEnd => self.cursor_end(),
            Intent::PageUp(n) => self.page_up(n),
            Intent::PageDown(n) => self.page_down(n),
            Intent::ToggleExpand => {
                // Enter/Space on a branch toggles expand; on a leaf opens detail
                // (mirrors the TUI). Paste-mode corner cases are host-advanced.
                let is_branch = self
                    .cursor_row_path()
                    .and_then(|p| self.tree.node_at(&p))
                    .map(|n| n.is_branch())
                    .unwrap_or(false);
                if is_branch {
                    self.toggle_expand();
                } else {
                    self.open_detail();
                }
            }
            Intent::CollapseAll => self.collapse_all(),
            Intent::ExpandAll => self.expand_all(),
            Intent::ExpandLevel => self.expand_level(),
            Intent::CollapseLevel => self.collapse_level(),

            // ---- Pointer (Web UI) ----
            Intent::SetCursor(path) => self.set_cursor(path),
            Intent::CommitEdit { value, name } => self.commit_edit(value, name),
            Intent::CommitKind { path, target } => self.commit_kind(path, target),
            Intent::SetSelection { paths } => self.set_selection(paths),
            Intent::SetTrailing { path, comment } => self.set_trailing_comment(path, comment),
            Intent::MoveSelectionTo {
                sources,
                target,
                index,
            } => self.move_selection_to(sources, target, index),

            // ---- Selection ----
            Intent::ToggleSelect => self.toggle_select(),
            Intent::ExtendSelectUp => self.extend_select_up(),
            Intent::ExtendSelectDown => self.extend_select_down(),

            // ---- Filter (/) ----
            Intent::EnterFilter => self.enter_filter(),
            Intent::CommitFilter => self.commit_filter(),
            Intent::ExitFilter => self.exit_filter(),
            Intent::ExitFilterResults => self.exit_filter_results(),
            Intent::SetFilter(text) => self.set_filter(text),
            Intent::FilterChar(c) => self.filter_char(c),
            Intent::FilterBackspace => self.filter_backspace(),
            Intent::FilterDelete => self.filter_delete(),
            Intent::FilterCursorLeft => self.filter_cursor_left(),
            Intent::FilterCursorRight => self.filter_cursor_right(),
            Intent::FilterCursorHome => self.filter_cursor_home(),
            Intent::FilterCursorEnd => self.filter_cursor_end(),

            // ---- Type filter (f) ----
            Intent::EnterTypeFilter => self.enter_type_filter(),
            Intent::CommitTypeFilter => self.commit_type_filter(),
            Intent::ExitTypeFilter => self.exit_type_filter(),
            Intent::TypeFilterMove(dr, dc) => self.type_filter_move(dr, dc),
            Intent::TypeFilterToggle => self.type_filter_toggle(),

            // ---- Kind switch (K) ----
            Intent::OpenKindSwitch => self.open_kind_switch(),
            Intent::KindSwitchMove(d) => self.kind_switch_move(d),
            Intent::KindSwitchCommit => self.kind_switch_commit(),
            Intent::ExitKindSwitch => self.exit_kind_switch(),

            // ---- Convert (C) ----
            Intent::OpenConvert => self.open_convert(),
            Intent::ConvertMove(d) => self.convert_move(d),
            // The session is fs-free; the host owns the source path/stem. `None`
            // seeds `out.<ext>`; the user edits the path in the Path step.
            Intent::ConvertPickFormat => self.convert_pick_format(None),
            Intent::SetConvertFormat(fmt) => self.set_convert_format(fmt),
            Intent::SetConvertPath(path) => self.set_convert_path(path),
            Intent::ConvertPathChar(c) => self.convert_path_char(c),
            Intent::ConvertPathBackspace => self.convert_path_backspace(),
            Intent::ConvertPathDelete => self.convert_path_delete(),
            Intent::ConvertPathLeft => self.convert_path_left(),
            Intent::ConvertPathRight => self.convert_path_right(),
            Intent::ConvertPathHome => self.convert_path_home(),
            Intent::ConvertPathEnd => self.convert_path_end(),
            Intent::ConvertRun => convert_write = self.convert_run(),
            Intent::ConvertConfirm => convert_write = self.convert_confirm(),
            Intent::ExitConvert => self.exit_convert(),

            // ---- Detail popup (i) ----
            Intent::ToggleDetail => self.toggle_detail(),
            Intent::ExitDetail => self.exit_detail(),
            // Core holds no scroll state — the Web UI scrolls the DOM natively.
            Intent::DetailScrollBy(..) | Intent::DetailSetScroll(..) => {}

            // ---- Help (?) ----
            Intent::EnterHelp => self.enter_help(),
            Intent::ExitHelp => self.exit_help(),
            Intent::HelpScrollBy(..) | Intent::HelpSetScroll(..) => {}

            // ---- Inline edit ----
            Intent::BeginEdit => {
                // Smart `e`: scalars/comments edit inline; **container nodes always
                // open the external popup editor** in the Web UI. A branch row has no
                // value cell, so an inline one-line repr (inline table/array) is
                // uneditable in the pointer UI — routing every container to the modal
                // makes all branches editable uniformly. (Web-only: the TUI calls
                // `edit_target_kind`/`begin_inline_edit` directly, so BEHAVIOR_MATRIX
                // §6 inline-table editing there is untouched.)
                let is_branch = self
                    .cursor_row_path()
                    .and_then(|p| self.tree.node_at(&p).map(|n| n.kind.clone()))
                    .map(|k| {
                        matches!(
                            k,
                            NodeKind::Table
                                | NodeKind::InlineTable
                                | NodeKind::Array
                                | NodeKind::ArrayOfTables
                        )
                    })
                    .unwrap_or(false);
                if !is_branch && self.edit_target_kind() == EditKind::Inline {
                    self.begin_inline_edit();
                } else {
                    self.begin_external_edit();
                }
            }
            Intent::BeginRename => self.begin_inline_rename(),
            Intent::EditToggleField => self.edit_toggle_field(),
            // Horizontal viewport clamp is host-owned (terminal width); no-op headlessly.
            Intent::EditClampScroll(_) => {}
            Intent::EditChar(c) => self.edit_input_char(c),
            Intent::EditBackspace => self.edit_backspace(),
            Intent::EditDelete => self.edit_delete(),
            Intent::EditCursorLeft => self.edit_cursor_left(),
            Intent::EditCursorRight => self.edit_cursor_right(),
            Intent::EditCursorHome => self.edit_cursor_home(),
            Intent::EditCursorEnd => self.edit_cursor_end(),
            Intent::EditCommit => self.edit_commit(),
            Intent::EditCancel => self.edit_cancel(),

            // ---- External edit resolution (host returned edited text) ----
            Intent::ApplyReplace { path, text } => {
                let wrap = self
                    .pending_external_edit
                    .as_ref()
                    .map(|p| p.wrap_element)
                    .unwrap_or(false);
                self.pending_external_edit = None;
                let text = if wrap {
                    self.doc
                        .as_ref()
                        .map(|d| d.scalar_fragment(None, text.trim_end_matches('\n')))
                        .unwrap_or(text)
                } else {
                    text
                };
                self.apply_replace(path, text);
            }
            Intent::ApplyEditComment { path, text } => {
                self.pending_external_edit = None;
                self.apply_edit_comment(path, text);
            }

            // ---- Mutations ----
            Intent::Nudge(d) => self.nudge(d),
            Intent::AddNode => self.add_node(),
            Intent::AddChild => self.add_child(),
            Intent::AddSibling => self.add_sibling(),
            Intent::DeleteSelected => self.delete_selected(),
            Intent::CopySelected => self.copy_selected(),
            Intent::CutSelected => self.cut_selected(),
            Intent::Paste => self.paste(),
            Intent::Remark => self.remark(),

            // ---- Undo / Redo ----
            Intent::Undo => self.undo(),
            Intent::Redo => self.redo(),

            // ---- Lifecycle ----
            Intent::Escape => self.escape(),
            Intent::PromptKey(c) => quit = self.handle_prompt_key(c),
            Intent::QuitRequested => {
                // Mirrors the TUI Quit action: if already in the confirm prompt,
                // the yes/no is handled via PromptKey; otherwise request quit
                // (which enters the prompt if there are unsaved changes).
                if !self.confirm_quit() {
                    quit = self.quit_requested();
                }
            }
            Intent::Save => {
                // FS-free: the host obtains bytes via `serialize()` and writes/
                // downloads. Core just clears the dirty flag + reports status.
                if let Some(d) = self.doc.as_mut() {
                    if d.is_dirty() {
                        d.mark_saved();
                        self.status = Some("Saved".into());
                    } else {
                        self.status = Some("no changes to save".into());
                    }
                }
            }
        }

        // Snap the cursor onto a visible row and drop a stale paste slot after
        // any structural change (delete/collapse/filter), mirroring the TUI's
        // `App::rebuild_rows`. `snapshot()` is `&self` and can't do this itself.
        self.compute_rows();
        let mut snap = self.snapshot();
        snap.convert_write = convert_write;
        snap.quit = quit;
        snap
    }

    /// The full renderable state, on demand (no mutation). The Web UI pulls this
    /// after each `dispatch`, or to resync.
    pub fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            doc_format: self.doc_format(),
            is_dirty: self.is_dirty(),
            mode: self.mode_view(),
            rows: self.visible_rows(),
            cursor: self.cursor.clone(),
            status: self.status.clone(),
            error: self.error.clone(),
            detail_text: self.detail_text.clone(),
            external_edit: self.external_edit_view(),
            convert_write: None,
            clipboard_count: self
                .clipboard
                .as_ref()
                .map(|c| c.fragments.len())
                .filter(|n| *n > 0),
            clipboard_cut: self.clipboard.as_ref().map(|c| c.cut).unwrap_or(false),
            clipboard_paths: self
                .clipboard
                .as_ref()
                .map(|c| c.sources.clone())
                .unwrap_or_default(),
            quit: false,
        }
    }

    /// Resolve an edit intent that routed external: record the target so the
    /// follow-up `ApplyReplace`/`ApplyEditComment` can complete. Mirrors
    /// `App::edit_node` minus the spawn (§8.2).
    fn begin_external_edit(&mut self) {
        let cursor_path = match self.cursor_row_path() {
            Some(p) => p,
            None => return,
        };
        if self
            .tree
            .node_at(&cursor_path)
            .map(|n| n.read_only)
            .unwrap_or(false)
        {
            self.status = Some("read-only node (block comment)".into());
            return;
        }
        if let Some(node) = self.tree.node_at(&cursor_path) {
            if let NodeKind::Comment(_) = &node.kind {
                if self.no_array_ancestor(&cursor_path) {
                    self.pending_external_edit = Some(PendingExternalEdit {
                        path: cursor_path,
                        wrap_element: false,
                        is_comment: true,
                    });
                    return;
                }
            }
        }
        let (path, wrap_element) = self.external_edit_path(&cursor_path);
        self.pending_external_edit = Some(PendingExternalEdit {
            path,
            wrap_element,
            is_comment: false,
        });
    }

    fn mode_view(&self) -> ModeView {
        match &self.mode {
            Mode::Normal => ModeView::Normal,
            Mode::Prompt(pk) => ModeView::Prompt {
                kind: prompt_view(pk),
            },
            Mode::Filter => ModeView::Filter {
                text: self.filter.clone(),
                cursor: self.filter_cursor,
            },
            Mode::FilterResults => ModeView::FilterResults,
            Mode::TypeFilter => ModeView::TypeFilter(self.type_filter_view()),
            Mode::KindSwitch(KindSwitchState {
                cursor, options, ..
            }) => ModeView::KindSwitch {
                cursor: *cursor,
                options: options
                    .iter()
                    .map(|(label, target)| KindOptionView {
                        label: label.clone(),
                        target: *target,
                    })
                    .collect(),
            },
            Mode::Convert(st) => ModeView::Convert(ConvertView {
                step: st.step,
                cursor: st.cursor,
                options: st.options.clone(),
                target: st.target,
                path: st.path.clone(),
                path_cursor: st.path_cursor,
                warnings: st.warnings.clone(),
            }),
            Mode::Detail => ModeView::Detail,
            Mode::Help => ModeView::Help,
            Mode::Edit(e) => ModeView::Edit(EditView {
                field: e.field,
                buffer: e.buffer.clone(),
                cursor: e.cursor,
                key: e.key.clone(),
                is_element: e.is_element,
                is_comment: e.is_comment,
                rename_only: e.rename_only,
            }),
        }
    }

    /// Build the `f` type-filter facet grid from the authoritative `layout` +
    /// the live filter state. The host renders this verbatim — it never
    /// re-derives the per-format facet set (PORTING §5 type_filter SPLIT).
    fn type_filter_view(&self) -> TypeFilterView {
        let fmt = self.doc_format();
        let tf = &self.type_filter;
        let mut rows = Vec::new();
        // `nav_rows` indexing matches `tf.row`, so we track the cell-row index
        // separately to mark the cursor cell within the flattened layout.
        let mut cell_row_idx = 0usize;
        for lr in layout(fmt) {
            match lr {
                LayoutRow::Header(h) => rows.push(TypeFilterRow::Header(h.to_string())),
                LayoutRow::Cells(cells) => {
                    let is_cursor_row = cell_row_idx == tf.row;
                    let views: Vec<TypeFilterCellView> = cells
                        .iter()
                        .enumerate()
                        .map(|(col, cell)| TypeFilterCellView {
                            label: cell.label().to_string(),
                            state: tf.cell_state(*cell),
                            is_cursor: is_cursor_row && col == tf.col,
                        })
                        .collect();
                    rows.push(TypeFilterRow::Cells(views));
                    cell_row_idx += 1;
                }
            }
        }
        TypeFilterView {
            rows,
            cursor_row: tf.row,
            cursor_col: tf.col,
            active: tf.is_active(),
        }
    }

    fn external_edit_view(&self) -> Option<ExternalEdit> {
        let pe = self.pending_external_edit.as_ref()?;
        if pe.is_comment {
            let initial = match self.tree.node_at(&pe.path).map(|n| &n.kind) {
                Some(NodeKind::Comment(t)) => format!("{t}\n"),
                _ => String::new(),
            };
            Some(ExternalEdit {
                initial,
                kind: ExternalEditKind::Comment {
                    path: pe.path.clone(),
                },
            })
        } else {
            let initial = self
                .doc
                .as_ref()
                .map(|d| d.serialize_fragment(&pe.path))
                .unwrap_or_default();
            Some(ExternalEdit {
                initial,
                kind: ExternalEditKind::Value {
                    path: pe.path.clone(),
                },
            })
        }
    }
}

fn prompt_view(pk: &PromptKind) -> PromptView {
    match pk {
        PromptKind::ConfirmQuit => PromptView::ConfirmQuit,
        PromptKind::Collision { .. } => PromptView::Collision,
        PromptKind::TypeChange { .. } => PromptView::TypeChange,
        PromptKind::ArrayUpgrade { .. } => PromptView::ArrayUpgrade,
        PromptKind::JsoncUpgrade { .. } => PromptView::JsoncUpgrade,
    }
}
