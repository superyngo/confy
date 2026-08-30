//! The `m` Action menu popup (design doc `docs/superpowers/specs/2026-08-30-action-menu-design.md`
//! §8, ADR 0009) — mirrors `overlay_kind_switch.rs`'s shape.
use crate::tui::app::App;
use crate::tui::state::Mode;
use crate::tui::ui::centered_rect;
use ratatui::prelude::*;
use ratatui::widgets::*;

/// The `m` Action menu popup: a small centered list, disabled items dimmed
/// (not skipped), `Delete` separated by a rule and shown in red.
pub(crate) fn draw_action_menu_overlay(f: &mut Frame, app: &App) {
    let Mode::ActionMenu { cursor } = &app.session.mode else {
        return;
    };
    let items = app.session.action_menu_items();
    let (_, target_label) = app.session.action_menu_targets();
    let mut lines: Vec<Line> = Vec::with_capacity(items.len() + 1);
    for (i, it) in items.iter().enumerate() {
        if it.separator_before {
            lines.push(Line::from(Span::raw("  ────────────────────────")));
        }
        let marker = if i == *cursor { "›" } else { " " };
        let mut style = Style::default();
        if !it.enabled {
            style = style.fg(Color::DarkGray);
        }
        if it.danger && it.enabled {
            style = style.fg(Color::Red);
        }
        if i == *cursor {
            style = style.add_modifier(Modifier::REVERSED);
        }
        lines.push(Line::from(Span::styled(
            format!(" {marker} {:<28}", it.label),
            style,
        )));
    }
    let height = (lines.len() as u16 + 2).min(f.area().height);
    let area = centered_rect(40, height, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(format!(" {target_label} "))
        .title_bottom(" ↑↓ move · Enter apply · Esc cancel ")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black).fg(Color::White));
    f.render_widget(Paragraph::new(lines).block(block), area);
}
