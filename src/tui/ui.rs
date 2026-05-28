use crate::tui::app::App;
use ratatui::prelude::*;
use ratatui::widgets::*;

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // tree area
            Constraint::Length(1), // status bar
        ])
        .split(f.area());

    draw_tree(f, chunks[0], app);
    draw_status(f, chunks[1], app);
}

fn draw_tree(f: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app.rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let indent = "  ".repeat(row.depth);
            let marker = if row.is_branch {
                // The Root (depth 0) is always shown expanded by flatten, so its
                // marker must reflect that rather than the (always-absent) set.
                if row.depth == 0 || app.is_expanded(&row.path) {
                    "▾ "
                } else {
                    "▸ "
                }
            } else {
                "  "
            };
            let sel_marker = if app.selection.indices.contains(&i) { "●" } else { " " };
            let text = format!("{sel_marker}{indent}{marker}{}", row.key);
            let style = if i == app.cursor {
                Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(text).style(style)
        })
        .collect();

    let list = List::new(items);
    let mut state = ListState::default();
    state.select(Some(app.cursor));
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    let total = app.rows.len();
    let pos = if app.rows.is_empty() { 0 } else { app.cursor + 1 };
    let status = format!(" {pos}/{total} | q:quit ?:help 0/9:collapse/expand");
    let paragraph = Paragraph::new(status)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));
    f.render_widget(paragraph, area);
}
