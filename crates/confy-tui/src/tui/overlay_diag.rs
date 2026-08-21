//! The `~` diag ring overlay. Split following the overlay separation pattern
//! established in Task 10 (2026-08-11 audit remediation).
use crate::tui::app::App;
use crate::tui::ui::centered_rect;
use ratatui::prelude::*;
use ratatui::widgets::*;

/// The `~` diag ring overlay: a centered read-only list of diagnostic events,
/// newest last. Host-side state (`app.diag_overlay_open`), not a core `Mode`.
/// Simple bounded list (no scroll state for Phase 2 — the ring is capped at
/// 256 events and the popup shows all that fit).
pub(crate) fn draw_diag_overlay(f: &mut Frame, app: &App) {
    if !app.diag_overlay_open {
        return;
    }

    let events: Vec<_> = app.session.diag.iter().collect();
    
    let lines: Vec<Line> = events
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

    // Size: 80% width, up to 20 lines of content (+ 2 for borders)
    let content_height = lines.len().min(20) as u16;
    let height = (content_height + 2).min(f.area().height);
    let area = centered_rect(80, height, f.area());
    
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(" Diagnostics ")
        .title_bottom(" ~ or Esc to close ")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black).fg(Color::White));
    f.render_widget(Paragraph::new(lines).block(block), area);
}
