pub mod app;
pub mod keys;
pub mod state;
pub mod ui;

use anyhow::Result;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::path::PathBuf;

pub fn run(path: &PathBuf) -> Result<()> {
    use crate::model::document::ConfigDocument;
    let doc = crate::model::toml_doc::TomlDocument::load(path)?;
    let tree = doc.project();
    let mut app = app::App::from_tree(tree);
    app.rebuild_rows();

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let result = run_event_loop(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

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
                keys::KeyAction::Noop => {}
            }
        }
    }
    Ok(())
}
