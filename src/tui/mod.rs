pub mod app;
pub mod keys;
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
    let tree = doc.project();
    let mut app = app::App::from_tree(tree);
    app.rebuild_rows();

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
            if key.kind != KeyEventKind::Press { continue; }
            match keys::map_key(key) {
                keys::KeyAction::CursorDown => app.cursor_down(),
                keys::KeyAction::CursorUp => app.cursor_up(),
                keys::KeyAction::PageUp => app.page_up(terminal.size()?.height as usize / 2),
                keys::KeyAction::PageDown => app.page_down(terminal.size()?.height as usize / 2),
                keys::KeyAction::Home => app.cursor_home(),
                keys::KeyAction::End => app.cursor_end(),
                keys::KeyAction::ToggleExpand => {
                    app.toggle_expand();
                    app.rebuild_rows();
                }
                keys::KeyAction::CollapseAll => {
                    app.collapse_all();
                    app.rebuild_rows();
                }
                keys::KeyAction::ExpandAll => {
                    app.expand_all();
                    app.rebuild_rows();
                }
                keys::KeyAction::Quit => should_quit = true,
                keys::KeyAction::ToggleSelect => app.toggle_select(),
                keys::KeyAction::ExtendSelectUp => {
                    app.extend_select_up();
                }
                keys::KeyAction::ExtendSelectDown => {
                    app.extend_select_down();
                }
                keys::KeyAction::Noop => {}
            }
        }
    }
    Ok(())
}
