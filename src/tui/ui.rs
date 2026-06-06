use crate::model::node::Format;
use crate::tui::app::{App, RowSnapshot};
use crate::tui::keys;
use crate::tui::state::{EditState, Mode, PromptKind};
use ratatui::prelude::*;
use ratatui::widgets::*;

/// Fixed width of the TYPE/FORMAT column. The widest labels are "array-of-tables"
/// and "string/ml-basic", both 15 columns — so 15 is the minimum that avoids
/// truncation and cannot be compressed further without abbreviating those.
const TYPE_WIDTH: u16 = 15;

/// Compact format suffix for the TYPE/FORMAT column. `None` for the single-style
/// `Plain` (bool, float, datetimes, and all branches) so they show type only.
fn format_label(fmt: Format) -> Option<&'static str> {
    match fmt {
        Format::Plain => None,
        Format::BasicString => Some("basic"),
        Format::MultilineBasic => Some("ml-basic"),
        Format::Literal => Some("lit"),
        Format::MultilineLiteral => Some("ml-lit"),
        Format::Decimal => Some("dec"),
        Format::Hex => Some("hex"),
        Format::Octal => Some("oct"),
        Format::Binary => Some("bin"),
    }
}

/// Combined `type/format` label (just `type` when the format is `Plain`).
fn type_format_label(row: &RowSnapshot) -> String {
    match format_label(row.format) {
        Some(fmt) => format!("{}/{}", row.type_label, fmt),
        None => row.type_label.clone(),
    }
}

/// Build the VALUE cell for the inline editor: the buffer rendered with the
/// character at the cursor reverse-highlighted (a trailing space when the cursor
/// is past the end). No glyph is inserted, so characters never shift.
fn edit_value_cell(e: &EditState) -> Cell<'static> {
    let chars: Vec<char> = e.buffer.chars().collect();
    let cur = e.cursor.min(chars.len());
    let rev = Style::default().add_modifier(Modifier::REVERSED);
    let mut spans: Vec<Span> = Vec::with_capacity(chars.len() + 1);
    for (j, ch) in chars.iter().enumerate() {
        let s = ch.to_string();
        if j == cur {
            spans.push(Span::styled(s, rev));
        } else {
            spans.push(Span::raw(s));
        }
    }
    if cur == chars.len() {
        spans.push(Span::styled(" ", rev));
    }
    Cell::from(Line::from(spans))
}

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title bar
            Constraint::Length(1), // column header
            Constraint::Min(1),    // tree table
            Constraint::Length(1), // status bar
        ])
        .split(f.area());

    draw_title(f, chunks[0], app);
    draw_column_header(f, chunks[1]);
    draw_tree(f, chunks[2], app);
    draw_status(f, chunks[3], app);
    draw_prompt_overlay(f, app);
    draw_detail_overlay(f, app);
    draw_help_overlay(f, app);
}

fn draw_title(f: &mut Frame, area: Rect, app: &App) {
    let filename = app.rows.first().map(|r| r.key.as_str()).unwrap_or("");
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    let left = format!("confy — {filename} ");
    let width = area.width as usize;
    // Fill between the left label and the right-aligned version with `─`.
    let used = left.chars().count() + version.chars().count() + 1;
    let fill = "─".repeat(width.saturating_sub(used));
    let line = Line::from(vec![
        Span::styled(left, Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(fill, Style::default().fg(Color::DarkGray)),
        Span::raw(" "),
        Span::styled(version, Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_column_header(f: &mut Frame, area: Rect) {
    let header_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let row = Row::new([
        Cell::from("  NAME"),
        Cell::from("TYPE/FORMAT"),
        Cell::from("VALUE"),
    ])
    .style(header_style);
    let table = Table::new(
        std::iter::once(row),
        [
            Constraint::Min(10),
            Constraint::Length(TYPE_WIDTH),
            Constraint::Min(10),
        ],
    )
    .column_spacing(1);
    f.render_widget(table, area);
}

fn draw_tree(f: &mut Frame, area: Rect, app: &App) {
    let rows: Vec<Row> = app
        .rows
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
            let sel_marker = if app.selection.indices.contains(&i) {
                "●"
            } else {
                " "
            };
            let name = format!("{sel_marker}{indent}{marker}{}", row.key);
            // While inline-editing the cursor row, render the live buffer with the
            // character under the cursor reverse-highlighted. This keeps the cursor
            // visible mid-string without inserting a caret glyph that would shift
            // the surrounding characters.
            let value_cell = match &app.mode {
                Mode::Edit(e) if i == app.cursor => edit_value_cell(e),
                _ => Cell::from(row.value.clone().unwrap_or_default()),
            };
            let style = if i == app.cursor {
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            Row::new([
                Cell::from(name),
                Cell::from(type_format_label(row)),
                value_cell,
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Min(10),
            Constraint::Length(TYPE_WIDTH),
            Constraint::Min(10),
        ],
    )
    .column_spacing(1);
    let mut state = TableState::default();
    state.select(Some(app.cursor));
    f.render_stateful_widget(table, area, &mut state);
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    // In filter mode, show the filter input line.
    if matches!(app.mode, Mode::Filter) {
        let text = format!(" /{}", app.filter);
        let paragraph =
            Paragraph::new(text).style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));
        f.render_widget(paragraph, area);
        return;
    }
    // In the inline editor, show edit-mode hints.
    if matches!(app.mode, Mode::Edit(_)) {
        let text = " editing — Enter:save  Esc:cancel  ←/→/Home/End:move";
        let paragraph =
            Paragraph::new(text).style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));
        f.render_widget(paragraph, area);
        return;
    }
    let total = app.rows.len();
    let pos = if app.rows.is_empty() {
        0
    } else {
        app.cursor + 1
    };
    let mut status = format!(" {pos}/{total} | q:quit ?:help d:x:c:v:m:r:z/y");
    if let Some(ref msg) = app.status {
        status = format!(" {msg}");
    }
    let paragraph =
        Paragraph::new(status).style(Style::default().bg(Color::DarkGray).fg(Color::White));
    f.render_widget(paragraph, area);
}

fn draw_prompt_overlay(f: &mut Frame, app: &App) {
    let text = match &app.mode {
        Mode::Prompt(PromptKind::Collision { key }) => {
            format!(
                " Key '{}' already exists.  o:overwrite  r:rename  c:cancel",
                key
            )
        }
        Mode::Prompt(PromptKind::MoveCollision { key }) => {
            format!(
                " Move collision on '{}' — o:overwrite  r:rename  c:cancel",
                key
            )
        }
        Mode::Prompt(PromptKind::ConfirmQuit) => {
            " Unsaved changes.  y:quit without saving  n:cancel".into()
        }
        Mode::Prompt(PromptKind::TypeChange { from, to }) => {
            format!(" Type will change {from} → {to}.  y:confirm  n:edit")
        }
        _ => return,
    };
    let area = centered_rect(60, 3, f.area());
    f.render_widget(Clear, area);
    let paragraph = Paragraph::new(text).style(
        Style::default()
            .bg(Color::Red)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );
    f.render_widget(paragraph, area);
}

fn draw_detail_overlay(f: &mut Frame, app: &App) {
    if !matches!(app.mode, Mode::Detail) {
        return;
    }
    let detail_text = match &app.detail_text {
        Some(t) => t.clone(),
        None => return,
    };
    let lines: Vec<&str> = detail_text.lines().collect();
    let height = (lines.len() as u16 + 2).min(f.area().height);
    let area = centered_rect(60, height, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(" Detail (Esc to close) ")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black).fg(Color::White));
    let paragraph = Paragraph::new(detail_text).block(block);
    f.render_widget(paragraph, area);
}

fn draw_help_overlay(f: &mut Frame, app: &App) {
    if !matches!(app.mode, Mode::Help) {
        return;
    }
    let help = keys::help_text();
    let line_count = help.lines().count() as u16;
    let height = (line_count + 2).min(f.area().height);
    let area = centered_rect(55, height, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(" Help (? or Esc to close) ")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black).fg(Color::White));
    let paragraph = Paragraph::new(help).block(block);
    f.render_widget(paragraph, area);
}

fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
    let popup_width = r.width * percent_x / 100;
    let x = (r.width.saturating_sub(popup_width)) / 2;
    let y = r.height / 2;
    Rect::new(x, y, popup_width.min(r.width), height.min(r.height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::document::ConfigDocument;
    use crate::tui::app::App;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::io::Write;

    /// Render a real document to a TestBackend and return the buffer as text lines.
    fn render(src: &str, w: u16, h: u16) -> Vec<String> {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(src.as_bytes()).unwrap();
        let doc = crate::model::toml_doc::TomlDocument::load(f.path()).unwrap();
        let app = App::new(doc);
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|fr| draw(fr, &app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn inline_editor_renders_buffer_in_value_column() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"port = 8080\n").unwrap();
        let doc = crate::model::toml_doc::TomlDocument::load(f.path()).unwrap();
        let mut app = App::new(doc);
        app.cursor = 1; // on port
        app.begin_inline_edit();
        for _ in 0..4 {
            app.edit_backspace();
        }
        for c in "9090".chars() {
            app.edit_input_char(c);
        }
        let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
        terminal.draw(|fr| draw(fr, &app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let joined: String = (0..8)
            .map(|y| (0..60).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("9090"),
            "edit buffer not rendered: {joined:?}"
        );
        assert!(
            joined.contains("editing"),
            "edit-mode hint missing: {joined:?}"
        );
        // The cursor is shown by reverse-highlighting a char, not by inserting a
        // caret glyph — so no caret character and no character drift.
        assert!(
            !joined.contains('▏'),
            "caret glyph must not be inserted into the buffer: {joined:?}"
        );
    }

    #[test]
    fn type_format_column_shows_combined_label() {
        // Integer renders as "integer/dec"; a literal string as "string/lit".
        let lines = render("port = 8080\nname = 'x'\n", 60, 8);
        let joined = lines.join("\n");
        assert!(joined.contains("integer/dec"), "rows: {joined:?}");
        assert!(joined.contains("string/lit"), "rows: {joined:?}");
        // header reflects both axes
        assert!(lines[1].contains("TYPE/FORMAT"), "header: {:?}", lines[1]);
    }

    #[test]
    fn title_bar_shows_filename_and_version() {
        let lines = render("port = 8080\n", 60, 8);
        let title = &lines[0];
        assert!(title.starts_with("confy — "), "title was: {title:?}");
        assert!(
            title.contains(&format!("v{}", env!("CARGO_PKG_VERSION"))),
            "title missing version: {title:?}"
        );
    }

    #[test]
    fn column_header_and_type_value_columns_render() {
        let lines = render("port = 8080\n", 60, 8);
        // row 1 is the column header
        let header = &lines[1];
        assert!(header.contains("NAME"), "header: {header:?}");
        assert!(header.contains("TYPE"), "header: {header:?}");
        assert!(header.contains("VALUE"), "header: {header:?}");
        // a data row carries the type label and value
        let joined = lines.join("\n");
        assert!(joined.contains("port"), "rows: {joined:?}");
        assert!(joined.contains("integer"), "type col missing: {joined:?}");
        assert!(joined.contains("8080"), "value col missing: {joined:?}");
    }
}
