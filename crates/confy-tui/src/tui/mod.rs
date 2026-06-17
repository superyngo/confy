pub mod app;
pub mod editor;
pub mod insertion;
pub mod keys;
pub mod search;
pub mod selection;
pub mod state;
pub mod type_filter;
pub mod ui;

use anyhow::Result;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::path::Path;

pub fn run(path: &Path, format: crate::model::document::DocFormat) -> Result<()> {
    let doc = crate::load_document(path, format)?;
    let mut app = app::App::new(doc);
    app.source_path = Some(path.to_path_buf());

    // Restore the terminal even if the event loop panics, so a crash never
    // leaves the user's shell stuck in raw mode / the alternate screen.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
        prev_hook(info);
    }));

    enable_raw_mode()?;
    // If entering the alternate screen or building the terminal fails AFTER raw
    // mode is on, disable raw mode before returning so the shell isn't left stuck
    // (the panic hook only covers panics, not `?`-propagated errors).
    let mut terminal = {
        let setup = (|| -> Result<_> {
            let mut stdout = std::io::stdout();
            execute!(stdout, EnterAlternateScreen)?;
            let backend = ratatui::backend::CrosstermBackend::new(stdout);
            Ok(ratatui::Terminal::new(backend)?)
        })();
        match setup {
            Ok(t) => t,
            Err(e) => {
                let _ = disable_raw_mode();
                return Err(e);
            }
        }
    };

    let result = run_event_loop(&mut terminal, &mut app);

    // Best-effort teardown: never let a cleanup error mask the event-loop result.
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    result
}

fn run_event_loop(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    app: &mut app::App,
) -> Result<()> {
    use crossterm::event::{self, Event, KeyEventKind};
    let mut should_quit = false;
    while !should_quit {
        // Keep the inline editor's horizontal viewport in sync with the cursor at
        // the current terminal width before drawing.
        if let crate::tui::state::Mode::Edit(ref e) = app.mode {
            let total = terminal.size()?.width;
            let w = if e.field == crate::tui::state::EditField::Name {
                ui::name_col_width(total) as usize
            } else {
                ui::value_col_width(total)
            };
            app.edit_clamp_scroll(w);
        }
        terminal.draw(|f| ui::draw(f, app))?;
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            // While a prompt is open, it captures ALL input: char keys go to the
            // prompt handler, Esc dismisses, and every other key is swallowed so
            // navigation can't move the cursor mid-prompt (which would split a
            // multi-fragment paste across two targets). MovePending is deliberately
            // NOT locked — the user navigates to choose the move target there.
            if matches!(app.mode, crate::tui::state::Mode::Prompt(_)) {
                match key.code {
                    crossterm::event::KeyCode::Char(c) => match app.handle_prompt_key(c) {
                        crate::tui::app::PromptOutcome::Quit => should_quit = true,
                        crate::tui::app::PromptOutcome::Consumed => {}
                    },
                    crossterm::event::KeyCode::Esc => app.escape(),
                    _ => {}
                }
                continue;
            }
            // Filter input: an inline text field — type to filter, edit at the
            // caret (Backspace/Del), move it (Left/Right/Home/End). Enter locks in
            // the filtered set (filtered-result selection); Esc clears the filter.
            if matches!(app.mode, crate::tui::state::Mode::Filter) {
                use crossterm::event::KeyCode;
                match key.code {
                    KeyCode::Char(c) => app.filter_char(c),
                    KeyCode::Backspace => app.filter_backspace(),
                    KeyCode::Delete => app.filter_delete(),
                    KeyCode::Left => app.filter_cursor_left(),
                    KeyCode::Right => app.filter_cursor_right(),
                    KeyCode::Home => app.filter_cursor_home(),
                    KeyCode::End => app.filter_cursor_end(),
                    KeyCode::Enter => app.commit_filter(),
                    KeyCode::Esc => app.escape(),
                    _ => {}
                }
                continue;
            }
            // Detail view: scroll with ↑/↓/j/k, PgUp/PgDn, Home/End; Esc/Enter/
            // Space/i dismiss. Height adapts to content; long values scroll within.
            if matches!(app.mode, crate::tui::state::Mode::Detail) {
                use crossterm::event::KeyCode;
                // Compute the popup's inner viewport + content height to clamp scrolling.
                let size = terminal.size()?;
                let text = app.detail_text.clone().unwrap_or_default();
                let rect = ui::detail_popup_rect(
                    ratatui::layout::Rect::new(0, 0, size.width, size.height),
                    &text,
                );
                let inner_h = rect.height.saturating_sub(2);
                let inner_w = rect.width.saturating_sub(2);
                let content_lines = ui::wrapped_line_count(&text, inner_w);
                let max_scroll = (content_lines as u16).saturating_sub(inner_h);
                let page = inner_h.max(1) as i32;
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') => app.detail_scroll_by(1, max_scroll),
                    KeyCode::Up | KeyCode::Char('k') => app.detail_scroll_by(-1, max_scroll),
                    KeyCode::PageDown => app.detail_scroll_by(page, max_scroll),
                    KeyCode::PageUp => app.detail_scroll_by(-page, max_scroll),
                    KeyCode::Home => app.detail_set_scroll(0),
                    KeyCode::End => app.detail_set_scroll(max_scroll),
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('i') | KeyCode::Char(' ') => {
                        app.escape()
                    }
                    _ => {}
                }
                continue;
            }
            // Inline editor: type into the buffer; Enter commits, Esc cancels.
            // Left/Right move the in-buffer cursor (not the value-nudge bindings).
            if matches!(app.mode, crate::tui::state::Mode::Edit(_)) {
                use crossterm::event::KeyCode;
                match key.code {
                    KeyCode::Char(c) => app.edit_input_char(c),
                    KeyCode::Backspace => app.edit_backspace(),
                    KeyCode::Delete => app.edit_delete(),
                    KeyCode::Left => app.edit_cursor_left(),
                    KeyCode::Right => app.edit_cursor_right(),
                    KeyCode::Home => app.edit_cursor_home(),
                    KeyCode::End => app.edit_cursor_end(),
                    KeyCode::Tab | KeyCode::BackTab => app.edit_toggle_field(),
                    KeyCode::Enter => app.edit_commit(),
                    KeyCode::Esc => app.edit_cancel(),
                    _ => {}
                }
                continue;
            }
            // Help overlay: ↑/↓/PgUp/PgDn/Home/End scroll; Esc or ? dismisses.
            if matches!(app.mode, crate::tui::state::Mode::Help) {
                use crossterm::event::KeyCode;
                let help_lines = keys::help_text(app.doc_format()).lines().count() as u16;
                // Approximate visible height: terminal height minus 2 borders.
                let inner_h = terminal.size()?.height.saturating_sub(2);
                let max_scroll = help_lines.saturating_sub(inner_h);
                let page = inner_h.max(1) as i32;
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') => app.help_scroll_by(1, max_scroll),
                    KeyCode::Up | KeyCode::Char('k') => app.help_scroll_by(-1, max_scroll),
                    KeyCode::PageDown => app.help_scroll_by(page, max_scroll),
                    KeyCode::PageUp => app.help_scroll_by(-page, max_scroll),
                    KeyCode::Home => app.help_set_scroll(0),
                    KeyCode::End => app.help_set_scroll(max_scroll),
                    KeyCode::Esc | KeyCode::Char('?') => app.escape(),
                    _ => {}
                }
                continue;
            }
            // Type-filter popup: arrows move the cursor, Space toggles the focused
            // cell, Enter applies (→ FilterResults/Normal), Esc peels the type
            // filter. Every toggle live-updates the filtered background. All other
            // keys are swallowed so the popup is modal.
            if matches!(app.mode, crate::tui::state::Mode::TypeFilter) {
                use crossterm::event::KeyCode;
                match key.code {
                    KeyCode::Up => app.type_filter_move(-1, 0),
                    KeyCode::Down => app.type_filter_move(1, 0),
                    KeyCode::Left => app.type_filter_move(0, -1),
                    KeyCode::Right => app.type_filter_move(0, 1),
                    KeyCode::Char(' ') => app.type_filter_toggle(),
                    KeyCode::Enter => app.commit_type_filter(),
                    KeyCode::Esc => app.escape(),
                    _ => {}
                }
                continue;
            }
            // Kind-switch popup: Up/Down (or j/k) move the selection, Enter
            // applies the conversion, Esc cancels. Modal — other keys swallowed.
            if matches!(app.mode, crate::tui::state::Mode::KindSwitch(_)) {
                use crossterm::event::KeyCode;
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => app.kind_switch_move(-1),
                    KeyCode::Down | KeyCode::Char('j') => app.kind_switch_move(1),
                    KeyCode::Enter => app.kind_switch_commit(),
                    KeyCode::Esc => app.escape(),
                    _ => {}
                }
                continue;
            }
            // Document-conversion flow (modal). The step decides the keys:
            //   Format  — Up/Down (j/k) pick a target, Enter advances, Esc cancels.
            //   Path    — caret text field for the output path, Enter renders.
            //   Confirm — y/Enter writes (lossy), n/Esc cancels.
            if let crate::tui::state::Mode::Convert(ref st) = app.mode {
                use crate::tui::state::ConvertStep;
                use crossterm::event::KeyCode;
                match st.step {
                    ConvertStep::Format => match key.code {
                        KeyCode::Up | KeyCode::Char('k') => app.convert_move(-1),
                        KeyCode::Down | KeyCode::Char('j') => app.convert_move(1),
                        KeyCode::Enter => app.convert_pick_format(),
                        KeyCode::Esc => app.escape(),
                        _ => {}
                    },
                    ConvertStep::Path => match key.code {
                        KeyCode::Char(c) => app.convert_path_char(c),
                        KeyCode::Backspace => app.convert_path_backspace(),
                        KeyCode::Delete => app.convert_path_delete(),
                        KeyCode::Left => app.convert_path_left(),
                        KeyCode::Right => app.convert_path_right(),
                        KeyCode::Home => app.convert_path_home(),
                        KeyCode::End => app.convert_path_end(),
                        KeyCode::Enter => app.convert_run(),
                        KeyCode::Esc => app.escape(),
                        _ => {}
                    },
                    ConvertStep::Confirm => match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                            app.convert_confirm()
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => app.escape(),
                        _ => {}
                    },
                }
                continue;
            }
            let action = keys::map_key(key);
            // Any non-shift-extend action ends the current shift multi-select
            // round, so the next Shift+Arrow begins a fresh one (unioned on top).
            if !matches!(
                action,
                keys::KeyAction::ExtendSelectUp | keys::KeyAction::ExtendSelectDown
            ) {
                app.last_action_was_shift_select = false;
            }
            match action {
                keys::KeyAction::CursorDown => app.cursor_down(),
                keys::KeyAction::CursorUp => app.cursor_up(),
                keys::KeyAction::PageUp => app.page_up(terminal.size()?.height as usize / 2),
                keys::KeyAction::PageDown => app.page_down(terminal.size()?.height as usize / 2),
                keys::KeyAction::Home => app.cursor_home(),
                keys::KeyAction::End => app.cursor_end(),
                keys::KeyAction::ToggleExpand => {
                    if app.clipboard.is_some() {
                        // Paste mode: only the `Into` (on-branch) slot toggles the
                        // branch; the green-line `After` slot is about the gap, not
                        // the branch, so Enter/Space is a no-op there.
                        if matches!(
                            app.effective_paste_slot(),
                            crate::tui::state::PasteSlot::Into(_)
                        ) {
                            app.toggle_expand();
                            app.rebuild_rows();
                            // rebuild reset the slot — keep the user on the branch.
                            app.paste_slot =
                                Some(crate::tui::state::PasteSlot::Into(app.cursor.clone()));
                        }
                    } else if let Some(r) = app.cursor_row() {
                        // Enter/Space: branch toggles expand, leaf opens detail.
                        if r.is_branch {
                            app.toggle_expand();
                            app.rebuild_rows();
                        } else {
                            app.open_detail();
                        }
                    }
                }
                keys::KeyAction::CollapseAll => {
                    app.collapse_all();
                    app.rebuild_rows();
                }
                keys::KeyAction::ExpandAll => {
                    app.expand_all();
                    app.rebuild_rows();
                }
                keys::KeyAction::ExpandLevel => app.expand_level(),
                keys::KeyAction::CollapseLevel => app.collapse_level(),
                keys::KeyAction::Quit => {
                    if app.confirm_quit() {
                        // Already in ConfirmQuit prompt — y/n handled via char
                    } else if app.quit_requested() {
                        should_quit = true;
                    }
                }
                keys::KeyAction::ToggleSelect => app.toggle_select(),
                keys::KeyAction::ExtendSelectUp => {
                    app.extend_select_up();
                }
                keys::KeyAction::ExtendSelectDown => {
                    app.extend_select_down();
                }
                keys::KeyAction::Info => app.toggle_detail(),
                keys::KeyAction::EditNode => {
                    if app.edit_target_kind() == crate::tui::app::EditKind::Inline {
                        app.begin_inline_edit();
                    } else {
                        let _ = disable_raw_mode();
                        let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
                        app.edit_node();
                        let _ = execute!(terminal.backend_mut(), EnterAlternateScreen);
                        let _ = enable_raw_mode();
                        terminal.clear()?;
                    }
                }
                keys::KeyAction::EditExternal => {
                    let _ = disable_raw_mode();
                    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
                    app.edit_node();
                    let _ = execute!(terminal.backend_mut(), EnterAlternateScreen);
                    let _ = enable_raw_mode();
                    terminal.clear()?;
                }
                keys::KeyAction::AddNode => app.add_node(),
                keys::KeyAction::IncValue => app.nudge(1),
                keys::KeyAction::DecValue => app.nudge(-1),
                keys::KeyAction::Delete => app.delete_selected(),
                keys::KeyAction::Copy => app.copy_selected(),
                keys::KeyAction::Cut => app.cut_selected(),
                keys::KeyAction::Paste => app.paste(),
                keys::KeyAction::Remark => app.remark(),
                keys::KeyAction::Save => app.save(),
                keys::KeyAction::Undo => app.undo(),
                keys::KeyAction::Redo => app.redo(),
                keys::KeyAction::Escape => app.escape(),
                keys::KeyAction::Filter => app.enter_filter(),
                keys::KeyAction::TypeFilter => app.enter_type_filter(),
                keys::KeyAction::KindSwitch => app.open_kind_switch(),
                keys::KeyAction::Convert => app.open_convert(),
                keys::KeyAction::Help => app.enter_help(),
                keys::KeyAction::Rename => app.begin_inline_rename(),
                keys::KeyAction::Noop => {}
            }
        }
    }
    Ok(())
}
