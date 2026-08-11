//! The `C` convert-document popup. Split out of `ui.rs` (Task 10, 2026-08-11
//! audit remediation) — pure code motion, no behavior change.
use crate::tui::app::App;
use crate::tui::state::{ConvertStep, Mode};
use crate::tui::ui::centered_rect;
use ratatui::prelude::*;
use ratatui::widgets::*;

pub(crate) fn draw_convert_overlay(f: &mut Frame, app: &App) {
    let Mode::Convert(st) = &app.session.mode else {
        return;
    };
    let (lines, footer): (Vec<Line>, &str) = match st.step {
        ConvertStep::Format => {
            let lines = st
                .options
                .iter()
                .enumerate()
                .map(|(i, fmt)| {
                    let marker = if i == st.cursor { "›" } else { " " };
                    let mut style = Style::default();
                    if i == st.cursor {
                        style = style.add_modifier(Modifier::REVERSED);
                    }
                    Line::from(Span::styled(format!(" {marker} {:<28}", fmt.name()), style))
                })
                .collect();
            (lines, " ↑↓ move · Enter next · Esc cancel ")
        }
        ConvertStep::Path => {
            let lines = vec![
                Line::from(format!(" Target: {}", st.target.name())),
                Line::from(""),
                Line::from(" Output path:"),
                Line::from(Span::styled(
                    format!(" {} ", st.path),
                    Style::default().add_modifier(Modifier::REVERSED),
                )),
            ];
            (lines, " type path · Enter convert · Esc cancel ")
        }
        ConvertStep::Confirm => {
            let mut lines = vec![Line::from(Span::styled(
                format!(" Converting to {} normalizes (lossy):", st.target.name()),
                Style::default().fg(Color::Yellow),
            ))];
            for w in &st.warnings {
                lines.push(Line::from(format!("   • {w}")));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(format!(" Write {} ?", st.path)));
            (lines, " y write · n/Esc cancel ")
        }
    };
    let height = (lines.len() as u16 + 2).min(f.area().height);
    let area = centered_rect(60, height, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(" Convert document ")
        .title_bottom(footer)
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black).fg(Color::White));
    f.render_widget(Paragraph::new(lines).block(block), area);
}
