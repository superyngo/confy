pub mod app;
pub mod editor;
pub mod insertion;
pub mod keys;
pub mod search;
pub mod selection;
pub mod state;
pub mod ui;

use anyhow::Result;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::path::Path;

pub fn run(path: &Path) -> Result<()> {
    use crate::model::document::ConfigDocument;
    let doc = crate::model::toml_doc::TomlDocument::load(path)?;
    let mut app = app::App::new(doc);

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
            // Filter mode: capture chars for the filter string, Esc clears.
            if matches!(app.mode, crate::tui::state::Mode::Filter) {
                match key.code {
                    crossterm::event::KeyCode::Char(c) => app.filter_char(c),
                    crossterm::event::KeyCode::Backspace => app.filter_backspace(),
                    crossterm::event::KeyCode::Esc => app.escape(),
                    _ => {}
                }
                continue;
            }
            // Detail view: Esc or Enter/Space dismisses.
            if matches!(app.mode, crate::tui::state::Mode::Detail) {
                match key.code {
                    crossterm::event::KeyCode::Esc
                    | crossterm::event::KeyCode::Enter
                    | crossterm::event::KeyCode::Char(' ') => app.escape(),
                    _ => {}
                }
                continue;
            }
            // Help overlay: Esc or ? dismisses.
            if matches!(app.mode, crate::tui::state::Mode::Help) {
                match key.code {
                    crossterm::event::KeyCode::Esc
                    | crossterm::event::KeyCode::Char('?') => app.escape(),
                    _ => {}
                }
                continue;
            }
            match keys::map_key(key) {
                keys::KeyAction::CursorDown => app.cursor_down(),
                keys::KeyAction::CursorUp => app.cursor_up(),
                keys::KeyAction::PageUp => app.page_up(terminal.size()?.height as usize / 2),
                keys::KeyAction::PageDown => app.page_down(terminal.size()?.height as usize / 2),
                keys::KeyAction::Home => app.cursor_home(),
                keys::KeyAction::End => app.cursor_end(),
                keys::KeyAction::ToggleExpand => {
                    // Enter/Space: branch toggles expand, leaf opens detail.
                    if let Some(r) = app.rows.get(app.cursor) {
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
                keys::KeyAction::EditNode => {
                    let _ = disable_raw_mode();
                    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
                    app.edit_node();
                    let _ = execute!(terminal.backend_mut(), EnterAlternateScreen);
                    let _ = enable_raw_mode();
                    terminal.clear()?;
                }
                keys::KeyAction::NewNode => {
                    let _ = disable_raw_mode();
                    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
                    app.new_node();
                    let _ = execute!(terminal.backend_mut(), EnterAlternateScreen);
                    let _ = enable_raw_mode();
                    terminal.clear()?;
                }
                keys::KeyAction::Delete => app.delete_selected(),
                keys::KeyAction::Copy => app.copy_selected(),
                keys::KeyAction::Cut => app.cut_selected(),
                keys::KeyAction::Paste => app.paste(),
                keys::KeyAction::Move => app.move_pressed(),
                keys::KeyAction::Remark => app.remark(),
                keys::KeyAction::Undo => app.undo(),
                keys::KeyAction::Redo => app.redo(),
                keys::KeyAction::Escape => app.escape(),
                keys::KeyAction::Filter => app.enter_filter(),
                keys::KeyAction::Help => app.enter_help(),
                keys::KeyAction::Noop => {}
            }
        }
    }
    Ok(())
}
