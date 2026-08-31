//! The schema-constrained enum/const picker popup. Split out of `ui.rs`
//! (Task 10, 2026-08-11 audit remediation) — pure code motion, no behavior
//! change.
use crate::tui::app::App;
use crate::tui::state::Mode;
use crate::tui::ui::centered_rect;
use ratatui::prelude::*;
use ratatui::widgets::*;

/// The schema-constrained enum/const picker: reuses the `K` kind-switch
/// popup's exact shape (spec §3/§5). Long option lists scroll to keep the
/// cursor on-screen (`scroll_offset`, computed the same follow-the-cursor
/// way as the tree's own row scrolling) — `PageUp`/`PageDown` jump a
/// screenful via `schema_enum_page_step`, which must stay in lockstep with
/// this function's `inner_h` so a page always matches what's on screen.
pub(crate) fn draw_schema_enum_overlay(f: &mut Frame, app: &App) {
    let Mode::SchemaEnum(st) = &app.session.mode else {
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
    let scroll_offset = schema_enum_scroll_offset(st.cursor, lines.len(), inner_h);
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(if st.from_schema {
            " Schema value "
        } else {
            " Value "
        })
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

/// Smallest scroll offset that keeps `cursor` inside the `inner_h`-tall
/// visible window — scrolls up the instant the cursor would go above it,
/// down the instant it would go below, otherwise holds still.
fn schema_enum_scroll_offset(cursor: usize, option_count: usize, inner_h: u16) -> u16 {
    let inner_h = inner_h.max(1) as usize;
    if option_count <= inner_h {
        return 0;
    }
    let max_offset = option_count - inner_h;
    cursor.saturating_sub(inner_h - 1).min(max_offset) as u16
}

/// How many options fit within the picker's visible height for a given
/// option count/terminal size — the `PageUp`/`PageDown` step (mod.rs),
/// kept in lockstep with `draw_schema_enum_overlay`'s own height calc.
pub(crate) fn schema_enum_page_step(option_count: usize, term_area: Rect) -> i32 {
    let height = (option_count as u16 + 2).min(term_area.height);
    let area = centered_rect(40, height, term_area);
    area.height.saturating_sub(2).max(1) as i32
}
