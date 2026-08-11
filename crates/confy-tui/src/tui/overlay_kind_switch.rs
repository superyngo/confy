//! The `K` kind-switch popup. Split out of `ui.rs` (Task 10, 2026-08-11 audit
//! remediation) — pure code motion, no behavior change.
use crate::tui::app::App;
use crate::tui::state::Mode;
use crate::tui::ui::centered_rect;
use ratatui::prelude::*;
use ratatui::widgets::*;

/// The `K` kind-switch popup: a small centered single-select list.
pub(crate) fn draw_kind_switch_overlay(f: &mut Frame, app: &App) {
    let Mode::KindSwitch(st) = &app.session.mode else {
        return;
    };
    let lines: Vec<Line> = st
        .options
        .iter()
        .enumerate()
        .map(|(i, (label, _))| {
            let marker = if i == st.cursor { "›" } else { " " };
            let mut style = Style::default();
            if i == st.cursor {
                style = style.add_modifier(Modifier::REVERSED);
            }
            Line::from(Span::styled(format!(" {marker} {label:<28}"), style))
        })
        .collect();
    let height = (lines.len() as u16 + 2).min(f.area().height);
    let area = centered_rect(40, height, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(" Switch kind ")
        .title_bottom(" ↑↓ move · Enter apply · Esc cancel ")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black).fg(Color::White));
    f.render_widget(Paragraph::new(lines).block(block), area);
}
