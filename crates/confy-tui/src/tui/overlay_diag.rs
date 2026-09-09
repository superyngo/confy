//! The `~` diag ring overlay. Split following the overlay separation pattern
//! established in Task 10 (2026-08-11 audit remediation).
use crate::tui::app::App;
use crate::tui::ui::centered_rect;
use ratatui::prelude::*;
use ratatui::widgets::*;

/// The `~` diag ring overlay: a centered read-only list of the **most recent**
/// diagnostic events. Host-side state (`app.diag_overlay_open`), not a core
/// `Mode`. No scroll state (Phase 2): the window is the tail of the ring, since
/// an operator opens this to see what just happened. The title reports how much
/// of the ring is off-screen so a clipped view is never mistaken for the whole.
pub(crate) fn draw_diag_overlay(f: &mut Frame, app: &App) {
    if !app.diag_overlay_open {
        return;
    }

    let events: Vec<_> = app.session.diag.iter().collect();
    let total = events.len();

    let mut lines: Vec<Line> = events
        .iter()
        .map(|e| {
            let level_str = format!("{:?}", e.level);
            let text = format!(" [{:5}] {} {}", level_str, e.kind, e.detail);
            let style = match e.level {
                confy_core::session::diag::DiagLevel::Error => Style::default().fg(Color::Red),
                confy_core::session::diag::DiagLevel::Warn => Style::default().fg(Color::Yellow),
                confy_core::session::diag::DiagLevel::Info => Style::default().fg(Color::Cyan),
                confy_core::session::diag::DiagLevel::Debug => Style::default().fg(Color::DarkGray),
            };
            Line::from(Span::styled(text, style))
        })
        .collect();

    // Size: 80% width, the last 20 events (+ 2 for borders), further clamped to
    // whatever the terminal can actually show — a `Paragraph` renders from its
    // first line, so anything that does not fit is clipped off the *bottom*.
    // Slicing before rendering is what keeps the newest events on screen.
    let room = (f.area().height.saturating_sub(2)) as usize;
    let window = lines.len().min(20).min(room.max(1));
    let start = lines.len() - window;
    let shown = lines.split_off(start);
    let height = (window.max(1) as u16 + 2).min(f.area().height);
    let area = centered_rect(80, height, f.area());

    // An empty ring still deserves a box that says so, rather than a 2-row
    // borders-only sliver with nothing between them.
    let shown = if shown.is_empty() {
        vec![Line::from(Span::styled(
            " (no events yet)",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        shown
    };

    let title = if total > window {
        format!(" Diagnostics — last {window} of {total} ")
    } else {
        format!(" Diagnostics — {total} ")
    };

    f.render_widget(Clear, area);
    let block = Block::default()
        .title(title)
        .title_bottom(" ~ or Esc to close ")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black).fg(Color::White));
    f.render_widget(Paragraph::new(shown).block(block), area);
}
