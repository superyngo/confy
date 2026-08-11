//! The `?` Help | About popup. Split out of `ui.rs` (Task 10, 2026-08-11
//! audit remediation) — pure code motion, no behavior change.
use crate::tui::app::App;
use crate::tui::keys;
use crate::tui::overlay_detail::wrapped_line_count;
use crate::tui::state::Mode;
use crate::tui::ui::centered_rect;
use ratatui::prelude::*;
use ratatui::widgets::*;

pub(crate) fn draw_help_overlay(f: &mut Frame, app: &App) {
    if !matches!(app.session.mode, Mode::Help(_)) {
        return;
    }
    let tab = match app.session.mode {
        Mode::Help(t) => t,
        _ => unreachable!(),
    };
    use crate::tui::state::HelpTab;
    let (title, text) = match tab {
        HelpTab::Help => (
            " Help | About (Tab to switch · ↑/↓ scroll · ? or Esc) ",
            keys::help_text(app.doc_format(), app.session.lang),
        ),
        HelpTab::About => (
            " About | Help (Tab to switch · ↑/↓ scroll · ? or Esc) ",
            app.about_text(),
        ),
    };
    let popup_width = (f.area().width * 65 / 100).min(f.area().width);
    let line_count = wrapped_line_count(&text, popup_width.saturating_sub(2)) as u16;
    let height = (line_count + 2).min(f.area().height);
    let area = centered_rect(65, height, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black).fg(Color::White));
    let paragraph = Paragraph::new(text)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.help_scroll, 0));
    f.render_widget(paragraph, area);
}
