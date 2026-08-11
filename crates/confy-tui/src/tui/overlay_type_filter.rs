//! The `f` type-filter facet popup. Split out of `ui.rs` (Task 10, 2026-08-11
//! audit remediation) — pure code motion, no behavior change.
use crate::tui::app::App;
use crate::tui::state::Mode;
use crate::tui::ui::centered_rect;
use ratatui::prelude::*;
use ratatui::widgets::*;

/// The `f` type-filter popup's visible inner height for a given terminal
/// size — shared by the renderer (`draw_type_filter_overlay`, to keep the
/// focused row on-screen) and the event loop (`PageUp`/`PageDown` step size,
/// mod.rs), so a page always jumps roughly one screenful of rows.
pub(crate) fn type_filter_inner_height(
    fmt: crate::model::document::DocFormat,
    term_area: Rect,
) -> u16 {
    let total_lines = crate::tui::type_filter::layout(fmt).len() as u16;
    let height = (total_lines + 2).min(term_area.height);
    let area = centered_rect(60, height, term_area);
    area.height.saturating_sub(2)
}

/// How many `type_filter::nav_rows` fit within the `f` popup's visible
/// height — the `PageUp`/`PageDown` step. Distinct from
/// `type_filter_inner_height` (screen *lines*, headers included): headers
/// don't count as cursor stops, so a page of nav rows is smaller than the
/// line height — counting raw lines here would overshoot by roughly 2x.
pub(crate) fn type_filter_page_step(
    fmt: crate::model::document::DocFormat,
    term_area: Rect,
) -> i32 {
    let inner_h = type_filter_inner_height(fmt, term_area) as usize;
    crate::tui::type_filter::layout(fmt)
        .into_iter()
        .take(inner_h)
        .filter(|r| matches!(r, crate::tui::type_filter::LayoutRow::Cells(_)))
        .count()
        .max(1) as i32
}

pub(crate) fn draw_type_filter_overlay(f: &mut Frame, app: &App) {
    if !matches!(app.session.mode, Mode::TypeFilter) {
        return;
    }
    use crate::tui::type_filter::{layout, CheckState, LayoutRow};
    let tf = &app.session.type_filter;
    let fmt = app.doc_format();

    let check = |state: CheckState| match state {
        CheckState::On => "[x]",
        CheckState::Partial => "[~]",
        CheckState::Off => "[ ]",
    };

    // Build the popup body, walking the layout and tracking which navigable row
    // index each cell row is, so the focused cell can be highlighted. We also
    // remember the body line index of the focused row to keep it on-screen when
    // the menu is taller than the terminal.
    let mut lines: Vec<Line> = Vec::new();
    let mut nav_row = 0usize;
    let mut focused_line = 0u16;
    for row in layout(fmt) {
        match row {
            LayoutRow::Header(h) => lines.push(Line::from(Span::styled(
                format!(" {h}"),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ))),
            LayoutRow::Cells(cells) => {
                let mut spans = vec![Span::raw("   ")];
                for (col, cell) in cells.iter().enumerate() {
                    let focused = nav_row == tf.row && col == tf.col;
                    if focused {
                        focused_line = lines.len() as u16;
                    }
                    let state = tf.cell_state(*cell);
                    let text = format!("{} {:<16}", check(state), cell.label());
                    let mut style = Style::default();
                    if state != CheckState::Off {
                        style = style.fg(Color::Green);
                    }
                    if focused {
                        style = style.add_modifier(Modifier::REVERSED);
                    }
                    spans.push(Span::styled(text, style));
                }
                lines.push(Line::from(spans));
                nav_row += 1;
            }
        }
    }

    // Size the popup to its content but cap at the terminal height; when capped,
    // scroll just enough to keep the focused row visible (roughly centered).
    let inner_h = type_filter_inner_height(fmt, f.area());
    let height = (lines.len() as u16 + 2).min(f.area().height);
    let area = centered_rect(60, height, f.area());
    let max_scroll = (lines.len() as u16).saturating_sub(inner_h);
    let scroll = if max_scroll == 0 {
        0
    } else {
        focused_line.saturating_sub(inner_h / 2).min(max_scroll)
    };
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(" Type filter (AND across halves) ")
        .title_bottom(" ↑↓←→ move · Space toggle · Enter apply · Esc clear ")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black).fg(Color::White));
    f.render_widget(Paragraph::new(lines).block(block).scroll((scroll, 0)), area);
}
