use crate::model::node::Format;
use crate::tui::app::{App, RowSnapshot};
use crate::tui::keys;
use crate::tui::state::{EditState, Mode, PasteSlot, PromptKind};
use ratatui::prelude::*;
use ratatui::widgets::*;

/// Fixed width of the KIND column. The fixed-pitch tag is always exactly
/// 12 columns (`(B) [S:str ]`: 3-char key sign + space + 8-char type slot).
const TYPE_WIDTH: u16 = 12;

/// Width of the NAME column: 40% of the terminal width, floored to 10 columns.
pub(crate) fn name_col_width(total: u16) -> u16 {
    (total * 2 / 5).max(10)
}

/// Collapse a possibly multi-line cell value to a single display line: the first
/// line with non-whitespace content, trimmed, plus a trailing ` …` when any later
/// line also carries content. Single-line values pass through (the trim also
/// strips the leading newline+indent decor a multiline-array element carries, so
/// its value stops rendering blank). Full text stays available in the detail popup.
pub(crate) fn cell_preview(s: &str) -> String {
    let mut lines = s.lines().filter(|l| !l.trim().is_empty());
    let first = lines.next().unwrap_or("").trim().to_string();
    if lines.next().is_some() {
        format!("{first} …")
    } else {
        first
    }
}

/// Compact format suffix for the KIND column. `None` for the single-style
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
        Format::Inf => Some("inf"),
        Format::Nan => Some("nan"),
        Format::Exponent => Some("exp"),
        // Container facets: the branch labels already carry the distinction.
        Format::Inline | Format::Multiline | Format::Scope | Format::Dotted => None,
    }
}

/// TYPE column cell: the precomputed fixed-pitch tag, with per-type colour. On
/// the cursor row (`is_cursor`) we skip colouring so the row's own `fg(White)`
/// wins uncontested.
fn type_col_cell(row: &RowSnapshot, is_cursor: bool) -> Cell<'static> {
    let label = row.type_tag.clone();
    if is_cursor {
        return Cell::from(label);
    }
    let color = match row.type_label.as_str() {
        "string" => Some(Color::Green),
        "integer" | "float" => Some(Color::Cyan),
        "bool" => Some(Color::Yellow),
        "offset-datetime" | "local-datetime" | "local-date" | "local-time" => Some(Color::Magenta),
        "comment" => Some(Color::DarkGray),
        _ => None, // branches: table, array, array-of-tables, inline
    };
    match color {
        Some(c) => Cell::from(label).style(Style::default().fg(c)),
        None => Cell::from(label),
    }
}

/// Width of the VALUE column: leftover after NAME (40%) + KIND (12) + two 1-col gaps.
/// Feeds the inline-editor window, the overflow hint, and the `/` filter input.
pub(crate) fn value_col_width(total: u16) -> usize {
    let name = name_col_width(total);
    (total.saturating_sub(name + TYPE_WIDTH + 2) as usize).max(1)
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

/// Build display spans for `text`, reverse-highlighting the characters that the
/// fuzzy `needle` matched (per-field: run against the cell's own text so the match
/// aligns with what's shown). No match → a single plain span. Consecutive
/// same-style chars are coalesced into one span.
fn highlight_spans(text: &str, needle: &str) -> Vec<Span<'static>> {
    let hl = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    let matched: std::collections::HashSet<usize> =
        match crate::tui::search::fuzzy_indices(text, needle) {
            Some(idx) if !idx.is_empty() => idx.into_iter().collect(),
            _ => return vec![Span::raw(text.to_string())],
        };
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut buf = String::new();
    let mut buf_hl = false;
    for (i, ch) in text.chars().enumerate() {
        let is_hl = matched.contains(&i);
        if is_hl != buf_hl && !buf.is_empty() {
            let s = std::mem::take(&mut buf);
            spans.push(if buf_hl {
                Span::styled(s, hl)
            } else {
                Span::raw(s)
            });
        }
        buf_hl = is_hl;
        buf.push(ch);
    }
    if !buf.is_empty() {
        spans.push(if buf_hl {
            Span::styled(buf, hl)
        } else {
            Span::raw(buf)
        });
    }
    spans
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
    draw_type_filter_overlay(f, app);
    draw_kind_switch_overlay(f, app);
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
        Cell::from("KIND"),
        Cell::from("VALUE"),
    ])
    .style(header_style);
    let table = Table::new(
        std::iter::once(row),
        [
            Constraint::Length(name_col_width(area.width)),
            Constraint::Length(TYPE_WIDTH),
            Constraint::Min(10),
        ],
    )
    .column_spacing(1);
    f.render_widget(table, area);
}

fn draw_tree(f: &mut Frame, area: Rect, app: &App) {
    // In paste mode, the active insertion slot is the cue (not the plain cursor):
    // `Into(i)` fills branch row `i` green (append last child); `After(i)` inserts a
    // standalone green line *below* row `i` (insert as a sibling after it) — a real
    // separator row, so the node's own text is never restyled.
    let active_slot = if app.clipboard.is_some() {
        Some(app.effective_paste_slot())
    } else {
        None
    };
    let mut rows: Vec<Row> = Vec::with_capacity(app.rows.len() + 1);
    // Display index (into `rows`, which may include an inserted green line) of the
    // active paste cue, so the viewport scrolls to it; else the plain cursor.
    let mut selected_display = app.cursor;
    for (i, row) in app.rows.iter().enumerate() {
        {
            let indent = "  ".repeat(row.depth);
            let marker = if row.is_branch {
                // Every branch — including the root/file node (empty path) — shows
                // its real expanded state; the root is seeded open at startup.
                if app.is_expanded(&row.path) {
                    "▾ "
                } else {
                    "▸ "
                }
            } else {
                "  "
            };
            let sel_marker = if app.selection.contains(i) {
                "●"
            } else {
                " "
            };
            let prefix = format!("{sel_marker}{indent}{marker}");
            // Collapse the key to one line (a merged multi-line comment node's key
            // carries newlines) without disturbing the tree prefix/indent.
            let name = format!("{prefix}{}", cell_preview(&row.key));
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
                        let avail = (name_col_width(area.width) as usize)
                            .saturating_sub(prefix.chars().count());
                        let mut spans = vec![Span::raw(prefix)];
                        spans.extend(edit_field_spans(&e.buffer, e.cursor, e.scroll, avail));
                        (
                            Cell::from(Line::from(spans)),
                            Cell::from(cell_preview(row.value.as_deref().unwrap_or(""))),
                        )
                    }
                },
                _ => {
                    // When a filter is active, highlight the fuzzy-matched chars in
                    // the NAME cell only (after the tree prefix) — the filter matches
                    // key/path, not value, so VALUE is never highlighted. Gated on the
                    // query, not the mode, so the highlight survives an inline edit or
                    // detail popup opened from the filtered list.
                    let needle = app.filter.as_str();
                    let val_cell = Cell::from(cell_preview(row.value.as_deref().unwrap_or("")));
                    if needle.is_empty() {
                        (Cell::from(name), val_cell)
                    } else {
                        let mut name_spans = vec![Span::raw(prefix.clone())];
                        name_spans.extend(highlight_spans(&cell_preview(&row.key), needle));
                        (Cell::from(Line::from(name_spans)), val_cell)
                    }
                }
            };
            let is_cursor = i == app.cursor;
            let in_clipboard_source = app
                .clipboard
                .as_ref()
                .is_some_and(|cb| cb.sources.contains(&row.path));
            // Base (non-cursor) appearance: copy source blue, cut source green,
            // multi-select grey.
            let base = if in_clipboard_source {
                let cut = app.clipboard.as_ref().is_some_and(|cb| cb.cut);
                let bg = if cut { Color::Green } else { Color::Blue };
                Style::default().bg(bg).fg(Color::White)
            } else if app.selection.contains(i) {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            let style = match active_slot {
                // Paste mode `Into`: the green branch row (append last child). An
                // invalid target errors on v. `After` restyles nothing — its cue is
                // the inserted green line row below.
                Some(PasteSlot::Into(t)) if t == i => Style::default()
                    .bg(Color::Green)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
                // Clipboard active but this isn't the slot row: no blue cursor.
                _ if active_slot.is_some() => base,
                _ if is_cursor => Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
                _ => base,
            };
            let type_cell = type_col_cell(row, is_cursor);
            if active_slot == Some(PasteSlot::Into(i)) {
                selected_display = rows.len();
            }
            rows.push(Row::new([name_cell, type_cell, value_cell]).style(style));
        }
        // The green insertion line below this row when it's the `After` slot.
        if active_slot == Some(PasteSlot::After(i)) {
            let expanded = app.is_expanded(&row.path);
            selected_display = rows.len();
            rows.push(paste_line_row(row, expanded, area.width));
        }
    }

    let table = Table::new(
        rows,
        [
            Constraint::Length(name_col_width(area.width)),
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
        .with_selected(Some(selected_display));
    f.render_stateful_widget(table, area, &mut state);
    app.table_offset.set(state.offset());
}

/// The standalone green insertion line shown for an `After` paste slot. It is
/// indented to the depth the pasted node will land at — one level deeper than an
/// **expanded** branch (the line reads as "first child"), otherwise the row's own
/// depth (a sibling after it) — matching `resolve_target`.
fn paste_line_row<'a>(row: &RowSnapshot, expanded: bool, width: u16) -> Row<'a> {
    let depth = if row.is_branch && expanded {
        row.depth + 1
    } else {
        row.depth
    };
    let line = format!("{}{}", "  ".repeat(depth), "─".repeat(width as usize));
    Row::new([Cell::from(line), Cell::from(""), Cell::from("")])
        .style(Style::default().fg(Color::Green))
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    // Error messages always take priority — shown with red background regardless
    // of mode or clipboard state so they are never hidden.
    if !matches!(app.mode, Mode::Edit(_)) {
        if let Some(ref msg) = app.error {
            let paragraph = Paragraph::new(format!(" ✗ {msg}")).style(
                Style::default()
                    .bg(Color::Red)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            );
            f.render_widget(paragraph, area);
            return;
        }
    }
    // In filter mode, show the filter input line as an inline text field: a
    // ` /` prefix then the buffer with the char under the caret reverse-
    // highlighted (same treatment as the inline value editor).
    if matches!(app.mode, Mode::Filter) {
        let prefix = " /";
        let avail = (area.width as usize).saturating_sub(prefix.chars().count());
        let mut spans = vec![Span::raw(prefix)];
        spans.extend(edit_field_spans(&app.filter, app.filter_cursor, 0, avail));
        let paragraph = Paragraph::new(Line::from(spans))
            .style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));
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
                // Always show absolute cursor position col/len (1-based).
                let pos_hint = format!("  {}/{}", e.cursor + 1, len);
                let overflow = edit_overflow_hint(e.scroll, len, value_col_width(area.width))
                    .map(|h| format!("  {h}"))
                    .unwrap_or_default();
                let hint = format!("{pos_hint}{overflow}");
                // The field label / Tab hint only applies when there is a name to
                // switch to (array elements have no key).
                let field = if e.is_comment {
                    "comment"
                } else {
                    match e.field {
                        crate::tui::state::EditField::Value => "value",
                        crate::tui::state::EditField::Name => "name",
                    }
                };
                let tab = if e.is_element || e.is_comment || e.rename_only {
                    ""
                } else {
                    "  Tab:name/value"
                };
                let field = if e.rename_only {
                    "name (rename)"
                } else {
                    field
                };
                (
                    format!(
                        " editing {field} — Enter:save  Esc:cancel  ←/→/Home/End:move  Bksp/Del:erase{tab}{hint}"
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
    // In the filtered-result selection mode, surface that the list is still
    // filtered (and how to clear/refine it) rather than the generic hints.
    if matches!(app.mode, Mode::FilterResults) {
        // Tag prefix surfacing each active filter layer (text and/or type).
        let mut tags = String::new();
        if !app.last_filter.is_empty() {
            tags.push_str(&format!("[filter: {}] ", app.last_filter));
        }
        let n_types = app.type_filter.key_signs.len() + app.type_filter.types.len();
        if n_types > 0 {
            tags.push_str(&format!("[type: {n_types}] "));
        }
        let status = if let Some(cb) = &app.clipboard {
            let n = cb.fragments.len();
            let kind = if cb.cut { "cut" } else { "copied" };
            format!(" {tags}{n} {kind} — v:paste  c/x:toggle  Esc:discard")
        } else {
            match &app.status {
                Some(msg) => format!(" {tags}{msg}"),
                None => format!(" {tags}{pos}/{total} | esc:clear  /:refine  f:type"),
            }
        };
        let paragraph =
            Paragraph::new(status).style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));
        f.render_widget(paragraph, area);
        return;
    }
    // When clipboard is loaded, show a sticky hint in place of the normal hints.
    if let Some(cb) = &app.clipboard {
        let n = cb.fragments.len();
        let kind = if cb.cut { "cut" } else { "copied" };
        let text = format!(" {n} node(s) {kind} — v:paste  c/x:toggle  Esc:discard");
        let paragraph =
            Paragraph::new(text).style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));
        f.render_widget(paragraph, area);
        return;
    }
    let mut status = format!(" {pos}/{total} | q:quit ?:help d:x:c:v:r:z/y");
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
        Mode::Prompt(PromptKind::ConfirmQuit) => {
            " Unsaved changes.  y:quit without saving  n:cancel".into()
        }
        Mode::Prompt(PromptKind::TypeChange { from, to }) => {
            format!(" Type will change {from} → {to}.  y:confirm  n:edit")
        }
        Mode::Prompt(PromptKind::ArrayUpgrade { .. }) => {
            " Reformat array to multiline and insert?  y/n".into()
        }
        Mode::Prompt(PromptKind::JsoncUpgrade { .. }) => {
            " Introduce a // comment? This makes the file JSONC.  y/n".into()
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
    let help = keys::help_text(app.doc_format());
    let line_count = help.lines().count() as u16;
    let height = (line_count + 2).min(f.area().height);
    let area = centered_rect(65, height, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(" Help (↑/↓ scroll · ? or Esc) ")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black).fg(Color::White));
    let paragraph = Paragraph::new(help)
        .block(block)
        .scroll((app.help_scroll, 0));
    f.render_widget(paragraph, area);
}

fn draw_type_filter_overlay(f: &mut Frame, app: &App) {
    if !matches!(app.mode, Mode::TypeFilter) {
        return;
    }
    use crate::tui::type_filter::{layout, CheckState, LayoutRow};
    let tf = &app.type_filter;
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
    let height = (lines.len() as u16 + 2).min(f.area().height);
    let area = centered_rect(60, height, f.area());
    let inner_h = area.height.saturating_sub(2);
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

/// The `K` kind-switch popup: a small centered single-select list.
fn draw_kind_switch_overlay(f: &mut Frame, app: &App) {
    let Mode::KindSwitch(st) = &app.mode else {
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

fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
    let popup_width = (r.width * percent_x / 100).min(r.width);
    let h = height.min(r.height);
    let x = (r.width.saturating_sub(popup_width)) / 2;
    let y = (r.height.saturating_sub(h)) / 2;
    Rect::new(x, y, popup_width, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::document::ConfigDocument;
    use crate::tui::app::App;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::io::Write;

    #[test]
    fn highlight_spans_marks_matched_chars() {
        let spans = highlight_spans("server", "svr");
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "server", "all chars preserved in order");
        assert!(
            spans
                .iter()
                .any(|s| s.style.add_modifier.contains(Modifier::UNDERLINED)),
            "matched chars should be highlighted"
        );
    }

    #[test]
    fn highlight_spans_no_match_is_single_plain_span() {
        let spans = highlight_spans("server", "zzz");
        assert_eq!(spans.len(), 1);
        assert!(!spans[0].style.add_modifier.contains(Modifier::UNDERLINED));
    }

    /// Render a real document to a TestBackend and return the buffer as text lines.
    fn render(src: &str, w: u16, h: u16) -> Vec<String> {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(src.as_bytes()).unwrap();
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::load(f.path()).unwrap(),
        );
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
    fn type_filter_popup_renders_with_checkboxes() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"port = 8080\n").unwrap();
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::load(f.path()).unwrap(),
        );
        let mut app = App::new(doc);
        app.enter_type_filter();
        app.type_filter_toggle(); // toggle the focused cell on
        let mut terminal = Terminal::new(TestBackend::new(70, 40)).unwrap();
        terminal.draw(|fr| draw(fr, &app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let joined: String = (0..40)
            .map(|y| (0..70).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("Type filter"),
            "popup title missing: {joined:?}"
        );
        assert!(
            joined.contains("(B) bare"),
            "key-sign cell missing: {joined:?}"
        );
        assert!(
            joined.contains("[x]"),
            "a toggled checkbox should show: {joined:?}"
        );
        assert!(
            joined.contains("[ ]"),
            "an empty checkbox should show: {joined:?}"
        );
    }

    #[test]
    fn type_filter_popup_scrolls_to_keep_cursor_visible() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"port = 8080\n").unwrap();
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::load(f.path()).unwrap(),
        );
        let mut app = App::new(doc);
        app.enter_type_filter();
        app.type_filter_move(1000, 0); // jump to the last (Date) row
                                       // Short terminal: the full menu can't fit, so it must scroll.
        let mut terminal = Terminal::new(TestBackend::new(70, 16)).unwrap();
        terminal.draw(|fr| draw(fr, &app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let joined: String = (0..16)
            .map(|y| (0..70).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("[D:ltim]"),
            "bottom cell should scroll into view: {joined:?}"
        );
        assert!(
            !joined.contains("(B) bare"),
            "top cell should have scrolled off: {joined:?}"
        );
    }

    #[test]
    fn inline_editor_renders_buffer_in_value_column() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"port = 8080\n").unwrap();
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::load(f.path()).unwrap(),
        );
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
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::load(f.path()).unwrap(),
        );
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
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::load(f.path()).unwrap(),
        );
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
    fn type_format_column_shows_fixed_pitch_tag() {
        // A bare-keyed integer renders `(B) [I:dec ]`; a literal string `[S:lit ]`.
        let lines = render("port = 8080\nname = 'x'\n", 60, 8);
        let joined = lines.join("\n");
        assert!(joined.contains("(B) [I:dec ]"), "rows: {joined:?}");
        assert!(joined.contains("(B) [S:lit ]"), "rows: {joined:?}");
        // header reflects both axes
        assert!(lines[1].contains("KIND"), "header: {:?}", lines[1]);
    }

    #[test]
    fn inline_table_tag_differs_from_table_scope() {
        // An inline table reads `[T/I]`; a standard `[table]` scope `[T/S]`.
        let lines = render("pt = { x = 1 }\n[srv]\nport = 8080\n", 60, 8);
        let joined = lines.join("\n");
        assert!(joined.contains("(B) [T/I]"), "rows: {joined:?}");
        assert!(
            joined
                .lines()
                .any(|l| l.contains("srv") && l.contains("[T/S]")),
            "standard table scope tag: {joined:?}"
        );
    }

    /// Render with all branches expanded, returning the joined buffer text.
    fn render_expanded(src: &str, w: u16, h: u16) -> String {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(src.as_bytes()).unwrap();
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::load(f.path()).unwrap(),
        );
        let mut app = App::new(doc);
        app.expand_all();
        app.rebuild_rows();
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|fr| draw(fr, &app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn cell_preview_collapses_multiline() {
        // first content line, trimmed, with an ellipsis when more content follows
        assert_eq!(cell_preview("\n  \"a\""), "\"a\"");
        assert_eq!(cell_preview("# one\n# two"), "# one …");
        assert_eq!(cell_preview("plain"), "plain");
        assert_eq!(cell_preview(""), "");
    }

    #[test]
    fn multiline_array_element_shows_value() {
        // Regression: a multiline-array element carries leading "\n  " decor in its
        // repr, which previously blanked the VALUE cell. cell_preview trims it.
        let joined = render_expanded("arr = [\n  \"a\",\n  \"b\",\n]\n", 60, 10);
        assert!(
            joined.contains("\"a\""),
            "array element value missing from column: {joined:?}"
        );
    }

    #[test]
    fn merged_comment_value_shows_collapsed_in_column() {
        let joined = render_expanded("# one\n# two\na = 1\n", 60, 10);
        assert!(
            joined.contains("# one …"),
            "merged comment not collapsed in column: {joined:?}"
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
        assert!(header.contains("KIND"), "header: {header:?}");
        assert!(header.contains("VALUE"), "header: {header:?}");
        // a data row carries the type tag and value
        let joined = lines.join("\n");
        assert!(joined.contains("port"), "rows: {joined:?}");
        assert!(joined.contains("[I:dec ]"), "type col missing: {joined:?}");
        assert!(joined.contains("8080"), "value col missing: {joined:?}");
    }
}
