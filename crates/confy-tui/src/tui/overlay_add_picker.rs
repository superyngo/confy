//! The Add-type picker popup (`Mode::AddPicker`) — the type-selection menu
//! `a`/AddChild/AddSibling now open instead of inserting directly. Reuses
//! `overlay_schema_enum.rs`'s exact shape (scroll + page-step), since the
//! TOML option list runs to a dozen rows.
use crate::tui::app::App;
use crate::tui::state::Mode;
use crate::tui::ui::centered_rect;
use ratatui::prelude::*;
use ratatui::widgets::*;

pub(crate) fn draw_add_picker_overlay(f: &mut Frame, app: &App) {
    let Mode::AddPicker(st) = &app.session.mode else {
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
    let inner_h = area.height.saturating_sub(2);
    let scroll_offset = add_picker_scroll_offset(st.cursor, lines.len(), inner_h);
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(" Add node ")
        .title_bottom(" ↑↓ move · PgUp/PgDn · Home/End · Enter apply · Esc cancel ")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black).fg(Color::White));
    f.render_widget(
        Paragraph::new(lines)
            .block(block)
            .scroll((scroll_offset, 0)),
        area,
    );
}

/// Smallest scroll offset keeping `cursor` inside the `inner_h`-tall visible
/// window — identical logic to `overlay_schema_enum::schema_enum_scroll_offset`.
fn add_picker_scroll_offset(cursor: usize, option_count: usize, inner_h: u16) -> u16 {
    let inner_h = inner_h.max(1) as usize;
    if option_count <= inner_h {
        return 0;
    }
    let max_offset = option_count - inner_h;
    cursor.saturating_sub(inner_h - 1).min(max_offset) as u16
}

/// How many options fit within the picker's visible height — the
/// `PageUp`/`PageDown` step (mod.rs), kept in lockstep with
/// `draw_add_picker_overlay`'s own height calc.
pub(crate) fn add_picker_page_step(option_count: usize, term_area: Rect) -> i32 {
    let height = (option_count as u16 + 2).min(term_area.height);
    let area = centered_rect(40, height, term_area);
    area.height.saturating_sub(2).max(1) as i32
}
