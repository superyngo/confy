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
pub(crate) fn format_label(fmt: Format) -> Option<&'static str> {
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

/// Combined `type/format` label for the TYPE/FORMAT column. Scalars append their
/// compact format (omitted for `Plain`). Branches stay one word, except an inline
/// table — which is a `table` written `{ inline }` — reads as `table/inline` so
/// the writing style is visible (a standard `[table]` stays plain `table`).
fn type_format_label(row: &RowSnapshot) -> String {
    if row.is_branch {
        match row.type_label.as_str() {
            "inline" => "table/inline".to_string(),
            other => other.to_string(),
        }
    } else {
        match format_label(row.format) {
            Some(fmt) => format!("{}/{}", row.type_label, fmt),
            None => row.type_label.clone(),
        }
    }
}

/// Approximate width (columns) of the VALUE column for a given total terminal
/// width. The tree Table uses `[Min(10), Length(TYPE_WIDTH), Min(10)]` with
/// `column_spacing(1)` (two gaps), so NAME and VALUE split the leftover equally.
pub(crate) fn value_col_width(total: u16) -> usize {
    ((total.saturating_sub(2 + TYPE_WIDTH) / 2) as usize).max(1)
}

/// Build the VALUE cell for the inline editor: the buffer window starting at the
/// editor's persistent `scroll` offset (the event loop keeps the cursor inside
/// it), with the character at the cursor reverse-highlighted (a trailing space
/// when the cursor is past the end). No glyph is inserted, so characters never
/// shift.
fn edit_value_cell(e: &EditState, width: usize) -> Cell<'static> {
    Cell::from(Line::from(edit_field_spans(
        &e.buffer, e.cursor, e.scroll, width,
    )))
}

/// Reverse-highlighted window of `buffer` starting at `scroll`, `width` columns
/// wide, with the char at `cursor` highlighted (trailing space when past the end).
/// Shared by the VALUE and (editable) NAME cells.
fn edit_field_spans(
    buffer: &str,
    cursor: usize,
    scroll: usize,
    width: usize,
) -> Vec<Span<'static>> {
    let chars: Vec<char> = buffer.chars().collect();
    let len = chars.len();
    let cur = cursor.min(len);
    let w = width.max(1);
    let start = scroll.min(len);
    let end = (start + w).min(len);
    let rev = Style::default().add_modifier(Modifier::REVERSED);
    let mut spans: Vec<Span> = Vec::with_capacity(end - start + 1);
    for (j, ch) in chars[start..end].iter().enumerate() {
        let s = ch.to_string();
        if start + j == cur {
            spans.push(Span::styled(s, rev));
        } else {
            spans.push(Span::raw(s));
        }
    }
    if cur == len && cur >= start && cur < start + w {
        spans.push(Span::styled(" ", rev));
    }
    spans
}

/// Compact "position / proportion" hint for an overflowing inline edit:
/// `⟨start–end/len⟩` (1-based visible char range over total) for the window at
/// `scroll`. `None` when the whole buffer fits, so it only appears on overflow.
fn edit_overflow_hint(scroll: usize, len: usize, width: usize) -> Option<String> {
    if len < width {
        return None;
    }
    let start = scroll.min(len);
    let end = (start + width.max(1)).min(len);
    Some(format!("⟨{}–{}/{}⟩", start + 1, end, len))
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
            let prefix = format!("{sel_marker}{indent}{marker}");
            let name = format!("{prefix}{}", row.key);
            // While inline-editing the cursor row, render the live buffer of the
            // focused field (Value or Name) with the char under the cursor
            // reverse-highlighted — no caret glyph, so characters never shift. The
            // NAME field scrolls the same way as VALUE, after the fixed tree prefix.
            let editing = matches!(&app.mode, Mode::Edit(e) if i == app.cursor);
            let (name_cell, value_cell) = match &app.mode {
                Mode::Edit(e) if editing => match e.field {
                    crate::tui::state::EditField::Value => (
                        Cell::from(name),
                        edit_value_cell(e, value_col_width(area.width)),
                    ),
                    crate::tui::state::EditField::Name => {
                        let avail =
                            value_col_width(area.width).saturating_sub(prefix.chars().count());
                        let mut spans = vec![Span::raw(prefix)];
                        spans.extend(edit_field_spans(&e.buffer, e.cursor, e.scroll, avail));
                        (
                            Cell::from(Line::from(spans)),
                            Cell::from(row.value.clone().unwrap_or_default()),
                        )
                    }
                },
                _ => (
                    Cell::from(name),
                    Cell::from(row.value.clone().unwrap_or_default()),
                ),
            };
            let style = if i == app.cursor {
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            Row::new([name_cell, Cell::from(type_format_label(row)), value_cell]).style(style)
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
    // Seed the table's scroll offset from the persisted value so ratatui only
    // scrolls when the cursor would leave the viewport (a fresh default state
    // would re-derive the offset each frame and pin the cursor to an edge), then
    // store the post-render offset back for the next frame.
    let mut state = TableState::default()
        .with_offset(app.table_offset.get())
        .with_selected(Some(app.cursor));
    f.render_stateful_widget(table, area, &mut state);
    app.table_offset.set(state.offset());
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
    // In the inline editor, show a commit error if there is one (e.g. the value
    // failed the semantic re-parse and could not be saved), otherwise the hints.
    if let Mode::Edit(e) = &app.mode {
        let (text, style) = match &app.status {
            Some(msg) => (
                format!(" {msg}  (Esc:cancel)"),
                Style::default()
                    .bg(Color::Red)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            None => {
                // When the value overflows the VALUE column, append a compact
                // hint of which char range is visible out of the total.
                let len = e.buffer.chars().count();
                let hint = edit_overflow_hint(e.scroll, len, value_col_width(area.width))
                    .map(|h| format!("  {h}"))
                    .unwrap_or_default();
                // The field label / Tab hint only applies when there is a name to
                // switch to (array elements have no key).
                let field = match e.field {
                    crate::tui::state::EditField::Value => "value",
                    crate::tui::state::EditField::Name => "name",
                };
                let tab = if e.is_element { "" } else { "  Tab:name/value" };
                (
                    format!(
                        " editing {field} — Enter:save  Esc:cancel  ←/→/Home/End:move{tab}{hint}"
                    ),
                    Style::default().bg(Color::DarkGray).fg(Color::Yellow),
                )
            }
        };
        f.render_widget(Paragraph::new(text).style(style), area);
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

/// Centered rect for the Detail popup. Width is a fixed 70%; height flexes to fit
/// the (wrapped) content within `[5, 80% of screen]`, so small popups stay small
/// and large values scroll inside the capped pane. Shared with the event loop's
/// scroll clamping so both agree on geometry.
pub(crate) fn detail_popup_rect(r: Rect, text: &str) -> Rect {
    let w = (r.width * 70 / 100).clamp(20.min(r.width), r.width);
    let content = wrapped_line_count(text, w.saturating_sub(2)) as u16;
    let min_h = 5.min(r.height);
    let max_h = (r.height * 80 / 100).max(min_h);
    let h = (content + 2).clamp(min_h, max_h).min(r.height);
    let x = (r.width.saturating_sub(w)) / 2;
    let y = (r.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

/// Number of display rows `text` occupies when char-wrapped to `width`. Used to
/// clamp the detail popup's scroll. Approximates ratatui's word wrap closely
/// enough for clamping (each logical line takes ⌈chars/width⌉ rows, min 1).
pub(crate) fn wrapped_line_count(text: &str, width: u16) -> usize {
    let w = (width.max(1)) as usize;
    text.lines()
        .map(|l| {
            let n = l.chars().count();
            if n == 0 {
                1
            } else {
                n.div_ceil(w)
            }
        })
        .sum()
}

fn draw_detail_overlay(f: &mut Frame, app: &App) {
    if !matches!(app.mode, Mode::Detail) {
        return;
    }
    let detail_text = match &app.detail_text {
        Some(t) => t.clone(),
        None => return,
    };
    let area = detail_popup_rect(f.area(), &detail_text);
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(" Detail (↑/↓ PgUp/PgDn Home/End · Esc) ")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black).fg(Color::White));
    let paragraph = Paragraph::new(detail_text)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0));
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
    fn inline_commit_error_is_shown_in_status() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"port = 8080\n").unwrap();
        let doc = crate::model::toml_doc::TomlDocument::load(f.path()).unwrap();
        let mut app = App::new(doc);
        app.cursor = 1;
        app.begin_inline_edit();
        for _ in 0..4 {
            app.edit_backspace();
        }
        for c in "= nope".chars() {
            app.edit_input_char(c);
        }
        app.edit_commit(); // invalid: stays in Edit mode with an error status
        let mut terminal = Terminal::new(TestBackend::new(80, 8)).unwrap();
        terminal.draw(|fr| draw(fr, &app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let joined: String = (0..8)
            .map(|y| (0..80).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("invalid TOML"),
            "commit error must be visible in the status line: {joined:?}"
        );
    }

    #[test]
    fn detail_popup_height_adapts_within_range() {
        let screen = Rect::new(0, 0, 80, 40);
        // Short content clamps up to the minimum height (5).
        let short = detail_popup_rect(screen, "a\nb");
        assert_eq!(short.width, 56, "width is a fixed 70%");
        assert_eq!(short.height, 5, "short content uses the minimum height");
        // Tall content clamps down to the maximum (80% of 40 = 32).
        let tall = detail_popup_rect(screen, &"x\n".repeat(100));
        assert_eq!(tall.height, 32, "tall content caps at 80% of the screen");
    }

    #[test]
    fn wrapped_line_count_counts_char_wrapped_rows() {
        assert_eq!(wrapped_line_count("abc", 10), 1);
        assert_eq!(wrapped_line_count("abcdefghij", 5), 2);
        assert_eq!(wrapped_line_count("a\nbb\n", 5), 2);
        // a long single line wraps into several rows
        assert_eq!(wrapped_line_count(&"x".repeat(25), 10), 3);
    }

    #[test]
    fn detail_popup_scrolls_long_value() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        let long = "x".repeat(400);
        f.write_all(format!("blob = \"{long}\"\n").as_bytes())
            .unwrap();
        let doc = crate::model::toml_doc::TomlDocument::load(f.path()).unwrap();
        let mut app = App::new(doc);
        app.cursor = 1; // on blob
        app.open_detail();
        let render_detail = |app: &App| -> String {
            let mut t = Terminal::new(TestBackend::new(60, 20)).unwrap();
            t.draw(|fr| draw(fr, app)).unwrap();
            let buf = t.backend().buffer().clone();
            (0..20)
                .map(|y| (0..60).map(|x| buf[(x, y)].symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        };
        // At the top, the Path line is visible.
        assert!(render_detail(&app).contains("Path:"), "top shows Path line");
        // After scrolling down, the Path line scrolls out of the popup.
        app.detail_set_scroll(6);
        assert!(
            !render_detail(&app).contains("Path:"),
            "Path line should scroll away"
        );
    }

    #[test]
    fn overflow_hint_only_appears_when_value_exceeds_width() {
        // fits entirely → no hint
        assert_eq!(edit_overflow_hint(0, 4, 10), None);
        // overflow: window at scroll=11 shows chars 12–20 of 20
        assert_eq!(
            edit_overflow_hint(11, 20, 10).as_deref(),
            Some("⟨12–20/20⟩")
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
    fn inline_table_column_shows_two_segment_label() {
        // An inline table reads as `table/inline`; a standard table stays `table`.
        let lines = render("pt = { x = 1 }\n[srv]\nport = 8080\n", 60, 8);
        let joined = lines.join("\n");
        assert!(joined.contains("table/inline"), "rows: {joined:?}");
        assert!(
            joined
                .lines()
                .any(|l| l.contains("srv") && l.contains("table")),
            "standard table stays plain `table`: {joined:?}"
        );
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
