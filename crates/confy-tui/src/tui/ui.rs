use crate::tui::app::{App, RowSnapshot};
use crate::tui::overlay_convert::draw_convert_overlay;
use crate::tui::overlay_detail::draw_detail_overlay;
use crate::tui::overlay_diag::draw_diag_overlay;
use crate::tui::overlay_help::draw_help_overlay;
use crate::tui::overlay_kind_switch::draw_kind_switch_overlay;
use crate::tui::overlay_lang_picker::draw_lang_picker_overlay;
use crate::tui::overlay_schema_enum::draw_schema_enum_overlay;
use crate::tui::overlay_type_filter::draw_type_filter_overlay;
use crate::tui::state::{EditState, Mode, PasteSlot, PromptKind};
use ratatui::prelude::*;
use ratatui::widgets::*;

// Re-exported so `mod.rs`'s existing `ui::X(...)` call sites (event-loop
// scroll-clamp / paging calculations) keep working unchanged after the
// Task 10 overlay-renderer split — the logic didn't move conceptually, just
// its file.
pub(crate) use crate::tui::overlay_detail::{
    detail_full_text, detail_popup_rect, wrapped_line_count,
};
pub(crate) use crate::tui::overlay_schema_enum::schema_enum_page_step;
pub(crate) use crate::tui::overlay_type_filter::type_filter_page_step;

/// Fixed width of the KIND column. The fixed-pitch tag is always exactly
/// 8 columns (the type/notation slot, e.g. `[S:str ]`). The key-sign facet
/// moved to the detail popup's `Sign:` line.
const TYPE_WIDTH: u16 = 8;

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

/// The tree-row label for a key: its **authored spelling** (`key_literal`) when
/// the projection captured one, else the decoded key. Every backend goes through
/// the same path — a quoted YAML key shows its own `"…"`/`'…'`, a quoted TOML key
/// its own single set of quotes, a JSON key its `"…"` — so there are no
/// per-format special cases and no synthesized quote characters here.
pub(crate) fn display_key(key: &str, key_literal: Option<&str>) -> String {
    key_literal.unwrap_or(key).to_string()
}

/// TYPE column cell: the precomputed fixed-pitch tag, with per-type colour. On
/// any row that paints a background fill (`has_fill`: the cursor's blue, a
/// clip source's green/magenta, or the armed paste-target's green `Into`
/// fill) we skip colouring so the row's own fill-appropriate fg wins
/// uncontested — e.g. a Magenta datetime tag on the copy source's Magenta
/// fill, or a Green "string" tag on the paste-target's Green fill, would
/// otherwise be illegible.
fn type_col_cell(row: &RowSnapshot, has_fill: bool) -> Cell<'static> {
    let label = row.type_tag.clone();
    if has_fill {
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

/// Width of the VALUE column: leftover after NAME (40%) + KIND (8) + two 1-col gaps.
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

/// The static VALUE cell: the value preview, plus the node's trailing inline
/// comment (`host: x  # bind`) rendered dimmed after it. Used in Normal mode and
/// while editing the Name field (the Value-field editor renders the live buffer,
/// which already carries the comment). A `comment_advisory` (a `strict_json`
/// document's comment — non-standard JSON confy silently accepts) swaps the
/// dim style for an underlined warn-colored one, the TUI's closest analogue
/// to the web tree's wavy underline (terminals have no hover tooltip; the
/// full advisory text lives in the `i` Detail popup's `Note:` section).
fn value_cell(row: &crate::tui::app::RowSnapshot) -> Cell<'static> {
    let preview = cell_preview(row.value.as_deref().unwrap_or(""));
    let advisory_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::UNDERLINED);
    match &row.trailing_comment {
        // A branch (`[section]  # c`, `key:  # c`) has no value preview, so the
        // comment leads the VALUE cell with no separator; a scalar keeps the gap.
        Some(tc) => {
            let style = if row.comment_advisory.is_some() {
                advisory_style
            } else {
                Style::default().add_modifier(Modifier::DIM)
            };
            let comment = Span::styled(cell_preview(tc), style);
            if preview.is_empty() {
                Cell::from(Line::from(comment))
            } else {
                Cell::from(Line::from(vec![
                    Span::raw(preview),
                    Span::raw("  "),
                    comment,
                ]))
            }
        }
        None if row.comment_advisory.is_some() => {
            Cell::from(Line::from(Span::styled(preview, advisory_style)))
        }
        None => Cell::from(preview),
    }
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
    let take = |a: usize, b: usize| -> String { chars[a..b].iter().collect() };
    // At most three style runs: text before the caret, the caret cell, text
    // after — instead of one Span per visible character.
    let mut spans: Vec<Span> = Vec::with_capacity(3);
    if (start..end).contains(&cur) {
        if cur > start {
            spans.push(Span::raw(take(start, cur)));
        }
        spans.push(Span::styled(chars[cur].to_string(), rev));
        if cur + 1 < end {
            spans.push(Span::raw(take(cur + 1, end)));
        }
    } else {
        if end > start {
            spans.push(Span::raw(take(start, end)));
        }
        // Caret parked just past the last char (append position).
        if cur == len && cur >= start && cur < start + w {
            spans.push(Span::styled(" ", rev));
        }
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
    draw_column_header(f, chunks[1], app);
    draw_tree(f, chunks[2], app);
    draw_status(f, chunks[3], app);
    draw_prompt_overlay(f, app);
    draw_detail_overlay(f, app);
    draw_help_overlay(f, app);
    draw_type_filter_overlay(f, app);
    draw_kind_switch_overlay(f, app);
    draw_convert_overlay(f, app);
    draw_schema_enum_overlay(f, app);
    draw_lang_picker_overlay(f, app);
    draw_diag_overlay(f, app);
}

fn draw_title(f: &mut Frame, area: Rect, app: &App) {
    use confy_core::session::tr_args;
    use unicode_width::UnicodeWidthStr;
    let filename = app.rows.first().map(|r| r.key.as_str()).unwrap_or("");
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    let left = tr_args(app.session.lang, "tui.title", &[filename]);
    let width = area.width as usize;
    // Fill between the left label and the right-aligned version with `─`.
    // Display width (not char count) so a CJK translation of `tui.title`
    // still lands the version flush right.
    let used = left.width() + version.width() + 1;
    let fill = "─".repeat(width.saturating_sub(used));
    let line = Line::from(vec![
        Span::styled(left, Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(fill, Style::default().fg(Color::DarkGray)),
        Span::raw(" "),
        Span::styled(version, Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_column_header(f: &mut Frame, area: Rect, app: &App) {
    use confy_core::session::tr;
    let lang = app.session.lang;
    let header_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let row = Row::new([
        Cell::from(tr(lang, "tui.header.name")),
        Cell::from(tr(lang, "tui.header.kind")),
        Cell::from(tr(lang, "tui.header.value")),
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
    let active_slot = if app.session.clipboard.is_some() {
        Some(app.effective_paste_slot())
    } else {
        None
    };
    // The cursor identity is a path (§3); map it to a visible-row index here, the
    // sole index↔path bridge on the render side.
    let cursor_idx = app.cursor_row_index();
    // Only format rows the viewport can actually show, not every logical row —
    // a large document (thousands of rows) previously paid for building a
    // styled `Row` (indent/highlight/style matching) for every one of them on
    // every single keystroke, even though ratatui only draws ~area.height of
    // them. `start` follows the persisted offset from last frame, nudged just
    // enough to keep the cursor in view (mirrors ratatui's own selection-
    // follow scrolling, which this now takes over — the outer window is our
    // job, not ratatui's, once we're only handing it the visible slice).
    let total = app.rows.len();
    let viewport_h = (area.height as usize).max(1);
    let mut start = app.table_offset.get().min(total.saturating_sub(1));
    if let Some(idx) = cursor_idx {
        if idx < start {
            start = idx;
        } else if idx >= start + viewport_h {
            start = idx + 1 - viewport_h;
        }
    }
    let end = (start + viewport_h).min(total);
    let mut rows: Vec<Row> = Vec::with_capacity(viewport_h + 1);
    // Display index (into `rows`, which may include an inserted green line) of the
    // active paste cue, so the viewport scrolls to it; else the plain cursor —
    // relative to `start` now that `rows` only ever holds the visible window.
    let mut selected_display = cursor_idx.map(|idx| idx - start).unwrap_or(0);
    for (i, row) in app.rows.iter().enumerate().skip(start).take(end - start) {
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
            let sel_marker = if app.session.selection.contains(&row.path) {
                "●"
            } else {
                " "
            };
            // The schema-warning marker rides with the row's own indentation —
            // parked immediately left of the key so its column tracks tree depth
            // instead of sitting in a fixed gutter. Hollow `△` means this branch
            // only *summarizes* a violation somewhere in its subtree; filled `▲`
            // means this exact row violates. A branch that both violates itself
            // and has violating descendants shows the filled glyph — its own
            // problem outranks the summary. Both glyphs are always single-column
            // (unlike `⚠`, whose emoji presentation varies).
            let warn_marker = if row.violations.is_some() {
                "▲"
            } else if row.is_branch && row.has_descendant_violation {
                "△"
            } else {
                " "
            };
            let prefix = format!("{sel_marker}{indent}{marker}{warn_marker} ");
            let disp_key = display_key(&row.key, row.key_literal.as_deref());
            // Collapse the key to one line (a merged multi-line comment node's key
            // carries newlines) without disturbing the tree prefix/indent.
            let name = format!("{prefix}{}", cell_preview(&disp_key));
            // While inline-editing the cursor row, render the live buffer of the
            // focused field (Value or Name) with the char under the cursor
            // reverse-highlighted — no caret glyph, so characters never shift. The
            // NAME field scrolls the same way as VALUE, after the fixed tree prefix.
            let editing = matches!(&app.session.mode, Mode::Edit(_) if Some(i) == cursor_idx);
            let (name_cell, value_cell) = match &app.session.mode {
                Mode::Edit(e) if editing => match e.field {
                    crate::tui::state::EditField::Value => (
                        Cell::from(name),
                        edit_value_cell(e, value_col_width(area.width)),
                    ),
                    crate::tui::state::EditField::Name => {
                        // The rename/edit buffer for a quoted YAML key now
                        // carries the literal quote characters itself (seeded
                        // from `key_literal_text`, see `inline_edit.rs`), so
                        // no separate decoration is drawn here — it would
                        // double the quotes. Plain edit rendering, same as
                        // any other key.
                        let avail = (name_col_width(area.width) as usize)
                            .saturating_sub(prefix.chars().count());
                        let mut spans = vec![Span::raw(prefix)];
                        spans.extend(edit_field_spans(&e.buffer, e.cursor, e.scroll, avail));
                        (Cell::from(Line::from(spans)), value_cell(row))
                    }
                },
                _ => {
                    // When a filter is active, highlight the fuzzy-matched chars in
                    // the NAME cell only (after the tree prefix) — the filter matches
                    // key/path, not value, so VALUE is never highlighted. Gated on the
                    // query, not the mode, so the highlight survives an inline edit or
                    // detail popup opened from the filtered list.
                    let needle = app.session.filter.as_str();
                    let val_cell = value_cell(row);
                    if needle.is_empty() {
                        (Cell::from(name), val_cell)
                    } else {
                        let mut name_spans = vec![Span::raw(prefix.clone())];
                        name_spans.extend(highlight_spans(&cell_preview(&disp_key), needle));
                        (Cell::from(Line::from(name_spans)), val_cell)
                    }
                }
            };
            let is_cursor = Some(i) == cursor_idx;
            let in_clipboard_source = app
                .session
                .clipboard
                .as_ref()
                .is_some_and(|cb| cb.sources.contains(&row.path));
            // Base (non-cursor) appearance: copy source purple, cut source green.
            // Locked selection no longer paints a background — its `sel_marker` glyph
            // (above) is the sole visual cue now, so it composes with the cursor's blue
            // and the clip-source colors instead of being hidden underneath a grey fill
            // (ADR 0005 §2 / ROW_STATE_MODEL.md §3).
            let base = if in_clipboard_source {
                let cut = app.session.clipboard.as_ref().is_some_and(|cb| cb.cut);
                let bg = if cut { Color::Green } else { Color::Magenta };
                Style::default().bg(bg).fg(Color::White)
            } else if row.violations.is_some() {
                // Subdued, not alarming — a soft constraint, never a hard error.
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };
            // Paste slots are now path-keyed (§3); test this row's path against them.
            let into_here = matches!(&active_slot, Some(PasteSlot::Into(p)) if *p == row.path);
            let style = match () {
                // Paste mode `Into`: the green branch row (append last child). An
                // invalid target errors on v. `After` restyles nothing — its cue is
                // the inserted green line row below.
                _ if into_here => Style::default()
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
            let type_cell = type_col_cell(row, is_cursor || in_clipboard_source || into_here);
            if into_here {
                selected_display = rows.len();
            }
            rows.push(Row::new([name_cell, type_cell, value_cell]).style(style));
        }
        // The green insertion line below this row when it's the `After` slot.
        if matches!(&active_slot, Some(PasteSlot::After(p)) if *p == row.path) {
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
    // `rows` is already exactly the visible window, so ratatui gets offset 0 —
    // the windowing above (not ratatui's own selection-follow) now owns
    // scroll position. Persist `start` (+ any residual ratatui-internal
    // adjustment, defensively) as next frame's basis.
    let mut state = TableState::default()
        .with_offset(0)
        .with_selected(Some(selected_display));
    f.render_stateful_widget(table, area, &mut state);
    app.table_offset.set(start + state.offset());
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

/// Maps a non-`Error` `Severity` to its status-line color (design spec §5.1:
/// Success = green, Warn = yellow, Info = default). `Error` is handled by its
/// own red-background branch above `draw_status`'s callers, never via this
/// helper, but is mapped safely regardless.
fn notice_color(severity: confy_core::session::notice::Severity) -> Color {
    use confy_core::session::notice::Severity;
    match severity {
        Severity::Success => Color::Green,
        Severity::Warn => Color::Yellow,
        Severity::Info | Severity::Error => Color::White,
    }
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    use confy_core::session::tr_args;
    // Error messages always take priority — shown with red background regardless
    // of mode or clipboard state so they are never hidden.
    if !matches!(app.session.mode, Mode::Edit(_)) {
        if let Some(notice) = &app.session.notice {
            if notice.severity == confy_core::session::notice::Severity::Error {
                let paragraph = Paragraph::new(format!(" ✗ {}", notice.text)).style(
                    Style::default()
                        .bg(Color::Red)
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                );
                f.render_widget(paragraph, area);
                return;
            }
        }
    }
    // In filter mode, show the filter input line as an inline text field: a
    // ` /` prefix then the buffer with the char under the caret reverse-
    // highlighted (same treatment as the inline value editor).
    if matches!(app.session.mode, Mode::Filter) {
        let prefix = " /";
        let avail = (area.width as usize).saturating_sub(prefix.chars().count());
        let mut spans = vec![Span::raw(prefix)];
        spans.extend(edit_field_spans(
            &app.session.filter,
            app.session.filter_cursor,
            0,
            avail,
        ));
        let paragraph = Paragraph::new(Line::from(spans))
            .style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));
        f.render_widget(paragraph, area);
        return;
    }
    // In the inline editor, show a commit error if there is one (e.g. the value
    // failed the semantic re-parse and could not be saved), otherwise the hints.
    if let Mode::Edit(e) = &app.session.mode {
        let (text, style) = if let Some(notice) = &app.session.notice {
            (
                format!(" {}  (Esc:cancel)", notice.text),
                Style::default()
                    .bg(Color::Red)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
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
        };
        f.render_widget(Paragraph::new(text).style(style), area);
        return;
    }
    let total = app.rows.len();
    let pos = if app.rows.is_empty() {
        0
    } else {
        app.cursor_row_index().unwrap_or(0) + 1
    };
    // In the filtered-result selection mode, surface that the list is still
    // filtered (and how to clear/refine it) rather than the generic hints.
    if matches!(app.session.mode, Mode::FilterResults) {
        // Tag prefix surfacing each active filter layer (text and/or type).
        let mut tags = String::new();
        if !app.session.last_filter.is_empty() {
            tags.push_str(&format!("[filter: {}] ", app.session.last_filter));
        }
        let n_types = app.session.type_filter.key_signs.len() + app.session.type_filter.types.len();
        if n_types > 0 {
            tags.push_str(&format!("[type: {n_types}] "));
        }
        let lang = app.session.lang;
        let (status, status_color) = if let Some(notice) = &app.session.notice {
            (
                tr_args(lang, "tui.status.filter-results-notice", &[&tags, &notice.text]),
                notice_color(notice.severity),
            )
        } else if let Some(cb) = &app.session.clipboard {
            let n = cb.fragments.len().to_string();
            let kind = if cb.cut { "cut" } else { "copied" };
            (
                tr_args(
                    lang,
                    "tui.status.filter-results-clipboard",
                    &[&tags, &n, kind],
                ),
                Color::Yellow,
            )
        } else {
            (
                tr_args(
                    lang,
                    "tui.status.filter-results-default",
                    &[&tags, &pos.to_string(), &total.to_string()],
                ),
                Color::Yellow,
            )
        };
        let paragraph =
            Paragraph::new(status).style(Style::default().bg(Color::DarkGray).fg(status_color));
        f.render_widget(paragraph, area);
        return;
    }
    // When clipboard is loaded, show a sticky hint in place of the normal
    // hints -- unless a notice is pending (e.g. "action disabled while
    // clipboard is armed"), which must stay visible rather than vanish under
    // the very clipboard-armed state it explains (falls through to the
    // generic notice rendering below, mirroring the Edit-mode override).
    if app.session.notice.is_none() {
        if let Some(cb) = &app.session.clipboard {
            let n = cb.fragments.len().to_string();
            let kind = if cb.cut { "cut" } else { "copied" };
            let text = tr_args(app.session.lang, "tui.status.clipboard", &[&n, kind]);
            let paragraph =
                Paragraph::new(text).style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));
            f.render_widget(paragraph, area);
            return;
        }
    }
    let mut status = tr_args(
        app.session.lang,
        "tui.status.default",
        &[&pos.to_string(), &total.to_string()],
    );
    let mut status_color = Color::White;
    if let Some(notice) = &app.session.notice {
        status = format!(" {}", notice.text);
        status_color = notice_color(notice.severity);
    } else if let Some(hint) = app.session.edit_hint(&app.session.cursor).describe() {
        // Dynamic, tooltip-like: appears while the cursor sits on a
        // schema-constrained node, clears the instant it moves off (no
        // explicit session.status set — that always wins, e.g. a just-
        // committed violation message).
        status = format!(" {hint}");
    }
    let violation_count = app
        .session
        .schema
        .as_ref()
        .map(|s| s.violations.len())
        .unwrap_or(0);
    let paragraph = if violation_count > 0 {
        Paragraph::new(Line::from(vec![
            Span::raw(status),
            Span::styled(
                format!(" · {}", tr_args(app.session.lang, "core.schema.count", &[&violation_count.to_string()])),
                Style::default().fg(Color::Yellow),
            ),
        ]))
        .style(Style::default().bg(Color::DarkGray).fg(status_color))
    } else {
        Paragraph::new(status).style(Style::default().bg(Color::DarkGray).fg(status_color))
    };
    f.render_widget(paragraph, area);
}

fn draw_prompt_overlay(f: &mut Frame, app: &App) {
    use confy_core::session::{prompt_question, tr};
    let lang = app.session.lang;
    let Mode::Prompt(ref pk) = app.session.mode else {
        return;
    };
    let question = prompt_question(lang, pk);
    let legend_key = match pk {
        PromptKind::Collision { .. } => "tui.prompt.collision.legend",
        PromptKind::ConfirmQuit => "tui.prompt.confirm-quit.legend",
        PromptKind::TypeChange { .. } => "tui.prompt.type-change.legend",
        PromptKind::ArrayUpgrade { .. } => "tui.prompt.array-upgrade.legend",
    };
    let legend = tr(lang, legend_key);
    let text = format!("{question}\n\n{legend}");
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

pub(crate) fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
    let popup_width = (r.width * percent_x / 100).min(r.width);
    let h = height.min(r.height);
    let x = (r.width.saturating_sub(popup_width)) / 2;
    let y = (r.height.saturating_sub(h)) / 2;
    Rect::new(x, y, popup_width, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::App;
    use crate::tui::state::Clipboard;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// Buffer column where a depth-1 row's key glyph lands in the NAME cell
    /// (1 selection-marker col + 2 indent + 2 branch marker + 1 warning-marker col
    /// + 1 spacing col before the key).
    const KEY_X: u16 = 7;

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
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::from_str(src).unwrap(),
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
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::from_str("port = 8080\n").unwrap(),
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
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::from_str("port = 8080\n").unwrap(),
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
    fn type_filter_page_step_counts_nav_rows_not_all_lines() {
        // Regression guard: the popup mixes Header lines with Cells lines, but
        // `move_cursor`'s delta is in nav-row units (Cells rows only). Feeding
        // it the raw visible *line* count would overshoot by roughly 2x.
        let fmt = crate::model::document::DocFormat::Toml;
        let total_lines = crate::tui::type_filter::layout(fmt).len();
        let total_nav_rows = crate::tui::type_filter::nav_rows(fmt).len();
        assert!(
            total_nav_rows < total_lines,
            "headers must inflate the line count beyond the nav-row count"
        );

        // Terminal tall enough to fit the whole popup: one page covers every
        // nav row.
        let full = Rect::new(0, 0, 80, 100);
        assert_eq!(type_filter_page_step(fmt, full) as usize, total_nav_rows);

        // Short terminal: a page must stay within the panel (never overshoot
        // past the total nav rows) and never degrade to a zero-length jump.
        let short = Rect::new(0, 0, 80, 10);
        let step = type_filter_page_step(fmt, short);
        assert!(step >= 1, "a page must move at least one row");
        assert!(
            (step as usize) < total_nav_rows,
            "a short-terminal page must be smaller than the whole panel"
        );
    }

    #[test]
    fn inline_editor_renders_buffer_in_value_column() {
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::from_str("port = 8080\n").unwrap(),
        );
        let mut app = App::new(doc);
        app.select_row(1); // on port
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
    fn quoted_yaml_key_rename_shows_literal_quotes_in_edit_buffer() {
        let doc = crate::model::any_doc::AnyDocument::Yaml(
            crate::model::yaml::doc::YamlDocument::from_str("\"a b\": 1\n").unwrap(),
        );
        let mut app = App::new(doc);
        app.select_row(1); // on "a b"
        app.begin_inline_rename();
        // The rename buffer itself now carries the literal source text
        // (quotes included), seeded from `key_literal_text` — the quote
        // characters are just ordinary, editable buffer content, mirroring
        // TOML's rename buffer (which already carries its literal quotes).
        assert_eq!(
            match &app.session.mode {
                Mode::Edit(e) => e.buffer.as_str(),
                _ => panic!("should be in rename mode"),
            },
            "\"a b\""
        );
        let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
        terminal.draw(|fr| draw(fr, &app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let joined: String = (0..8)
            .map(|y| (0..60).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        // The rendered row shows the quotes as ordinary buffer chars, not a
        // separate decoration span — no double-quoting (`""a b""`).
        assert!(
            joined.contains("\"a b") || joined.contains("a b\""),
            "quote flank missing from rename buffer render: {joined:?}"
        );
        assert!(
            !joined.contains("\"\"a b\"\""),
            "rename buffer must not be double-quoted: {joined:?}"
        );
    }

    #[test]
    fn inline_commit_error_is_shown_in_status() {
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::from_str("port = 8080\n").unwrap(),
        );
        let mut app = App::new(doc);
        app.select_row(1);
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
            joined.contains("invalid value"),
            "commit error must be visible in the status line: {joined:?}"
        );
    }

    #[test]
    fn notice_severity_drives_status_line_color() {
        // Design spec §5.1: Success => green, Warn => yellow, Info => default
        // (white). Error is covered separately by the red-bg branch above
        // (see the `!matches!(app.session.mode, Mode::Edit(_))` guard).
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::from_str("x = 1\n").unwrap(),
        );
        let mut app = App::new(doc);
        let lang = app.session.lang;
        let cases = [
            ("core.save.saved", confy_core::session::notice::Severity::Success, Color::Green),
            ("core.readonly", confy_core::session::notice::Severity::Warn, Color::Yellow),
            ("core.save.nothing", confy_core::session::notice::Severity::Info, Color::White),
        ];
        for (key, expected_severity, expected_color) in cases {
            let notice = confy_core::session::notice::Notice::core(lang, key, &[]);
            assert_eq!(
                notice.severity, expected_severity,
                "fixture key {key} must map to the severity this test means to exercise"
            );
            app.session.notice = Some(notice);
            let mut terminal = Terminal::new(TestBackend::new(80, 8)).unwrap();
            terminal.draw(|fr| draw(fr, &app)).unwrap();
            let buf = terminal.backend().buffer().clone();
            let fg = buf[(1, 7)].fg; // status bar is always the last row (height - 1)
            assert_eq!(
                fg, expected_color,
                "{key:?} ({expected_severity:?}) must render {expected_color:?}, got {fg:?}"
            );
        }
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
        let long = "x".repeat(400);
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::from_str(&format!("blob = \"{long}\"\n")).unwrap(),
        );
        let mut app = App::new(doc);
        app.select_row(1); // on blob
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
    fn detail_full_text_appends_schema_section_for_cursor_violations() {
        // A schema-violating cursor row must produce a full-text string that
        // includes the appended `Schema:` section — this is what both the popup
        // sizing and the scroll-clamp measure, so they can't drift.
        let schema = r#"{"type":"object","properties":{"port":{"type":"string"}}}"#;
        let mut app = App::new(crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::from_str("port = 1\n").unwrap(),
        ));
        app.session.apply_schema_text(
            confy_core::schema::SchemaSource::Local("/tmp/s.json".into()),
            Ok(schema.to_string()),
        );
        app.rebuild_rows();
        app.select_row(1); // cursor on port
        app.open_detail();
        let full = detail_full_text(&app);
        assert!(full.contains("Schema:"), "section appended: {full:?}");
        assert!(
            full.contains("not of type"),
            "violation msg present: {full:?}"
        );

        // A conforming value produces no violation message — but the
        // Schema section still appears now, carrying schema_info's
        // proactive "Type: string" line (this is the fix: a plain-typed
        // field with no enum/bounded constraint used to show nothing at
        // all outside a violation).
        let mut clean = App::new(crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::from_str("port = \"ok\"\n").unwrap(),
        ));
        clean.session.apply_schema_text(
            confy_core::schema::SchemaSource::Local("/tmp/s.json".into()),
            Ok(schema.to_string()),
        );
        clean.rebuild_rows();
        clean.select_row(1);
        clean.open_detail();
        let clean_full = detail_full_text(&clean);
        assert!(
            clean_full.contains("Type: string"),
            "type info still shown when conforming: {clean_full:?}"
        );
        assert!(
            !clean_full.contains("not of type"),
            "no violation msg when conforming: {clean_full:?}"
        );
    }

    #[test]
    fn detail_full_text_appends_schema_section_for_constraint_without_violation() {
        // A conforming cursor row that still carries a schema constraint (e.g.
        // an `enum`) must get a `Schema:` section too — the panel isn't only a
        // violation channel, it's a general schema-info surface.
        let schema = r#"{"type":"object","properties":{"env":{"enum":["dev","prod"]}}}"#;
        let mut app = App::new(crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::from_str("env = \"dev\"\n").unwrap(),
        ));
        app.session.apply_schema_text(
            confy_core::schema::SchemaSource::Local("/tmp/s.json".into()),
            Ok(schema.to_string()),
        );
        app.rebuild_rows();
        app.select_row(1); // cursor on env
        app.open_detail();
        let full = detail_full_text(&app);
        assert!(full.contains("Schema:"), "section appended: {full:?}");
        assert!(
            full.contains("Valid values:"),
            "constraint hint present: {full:?}"
        );
        assert!(
            !full.contains("not of type") && !full.contains("is not one of"),
            "no violation text for a conforming value: {full:?}"
        );
    }

    #[test]
    fn detail_full_text_appends_schema_section_for_plain_typed_field_with_description() {
        // A field with only `type`/`description` (no enum/bounded, the common
        // real-world case) has no `EditHint` at all, but must still show
        // schema_info's proactive line — this is the gap this feature closes.
        let schema = r#"{"type":"object","properties":{"host":{"type":"string","description":"Bind address"}}}"#;
        let mut app = App::new(crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::from_str("host = \"0.0.0.0\"\n").unwrap(),
        ));
        app.session.apply_schema_text(
            confy_core::schema::SchemaSource::Local("/tmp/s.json".into()),
            Ok(schema.to_string()),
        );
        app.rebuild_rows();
        app.select_row(1); // cursor on host
        app.open_detail();
        let full = detail_full_text(&app);
        assert!(full.contains("Schema:"), "section appended: {full:?}");
        assert!(full.contains("Bind address"), "description shown: {full:?}");
        assert!(full.contains("Type: string"), "type shown: {full:?}");
        assert!(!full.contains("Valid values:"), "no enum hint: {full:?}");
        assert!(!full.contains("not of type"), "no violation: {full:?}");
    }

    #[test]
    fn detail_full_text_appends_note_section_for_comment_advisory() {
        // A comment row in a `strict_json`-flagged document must get an
        // appended `Note:` section carrying `comment_advisory`, independent
        // of any Schema section.
        let doc = crate::model::any_doc::AnyDocument::Json(
            crate::model::json::JsonDocument::from_str("// hi\n{\"a\": 1}\n").unwrap(),
        );
        let mut app = App::new(doc);
        app.session.strict_json = true;
        app.rebuild_rows();
        app.select_row(1); // the leading standalone comment (rows[0] is root)
        let full = detail_full_text(&app);
        assert!(full.contains("Note:"), "section appended: {full:?}");
        assert!(
            !full.contains("Schema:"),
            "no schema loaded, no Schema section: {full:?}"
        );
    }

    #[test]
    fn comment_advisory_renders_underlined_in_value_column() {
        // A `strict_json`-flagged document's trailing comment gets the
        // underlined warn-colored style instead of the plain dim one — the
        // TUI's analogue to the web tree's wavy underline (no hover tooltip
        // in a terminal; the full text lives in the `i` Detail popup).
        let doc = crate::model::any_doc::AnyDocument::Json(
            crate::model::json::JsonDocument::from_str("{\"a\": 1  // note\n}\n").unwrap(),
        );
        let mut app = App::new(doc);
        app.session.strict_json = true;
        app.rebuild_rows();
        let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
        terminal.draw(|fr| draw(fr, &app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let row_y = (0..8)
            .find(|&y| buf[(KEY_X, y)].symbol() == "a")
            .expect("`a` row not found in rendered buffer");
        let value_x = name_col_width(60) + TYPE_WIDTH + 2;
        let underlined = (value_x..60)
            .any(|x| buf[(x, row_y)].modifier.contains(Modifier::UNDERLINED));
        assert!(underlined, "trailing comment_advisory must render underlined");

        // Without `strict_json`, the same document's comment stays plain dim.
        let doc2 = crate::model::any_doc::AnyDocument::Json(
            crate::model::json::JsonDocument::from_str("{\"a\": 1  // note\n}\n").unwrap(),
        );
        let mut plain_app = App::new(doc2);
        plain_app.rebuild_rows();
        let mut terminal2 = Terminal::new(TestBackend::new(60, 8)).unwrap();
        terminal2.draw(|fr| draw(fr, &plain_app)).unwrap();
        let buf2 = terminal2.backend().buffer().clone();
        let row_y2 = (0..8)
            .find(|&y| buf2[(KEY_X, y)].symbol() == "a")
            .expect("`a` row not found in rendered buffer");
        let underlined2 = (value_x..60)
            .any(|x| buf2[(x, row_y2)].modifier.contains(Modifier::UNDERLINED));
        assert!(!underlined2, "plain .jsonc-equivalent doc must not underline");
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
        // An integer renders `[I:dec ]`; a literal string `[S:lit ]`. (The
        // key-sign prefix moved to the detail popup.)
        let lines = render("port = 8080\nname = 'x'\n", 60, 8);
        let joined = lines.join("\n");
        assert!(joined.contains("[I:dec ]"), "rows: {joined:?}");
        assert!(joined.contains("[S:lit ]"), "rows: {joined:?}");
        // header reflects both axes
        assert!(lines[1].contains("KIND"), "header: {:?}", lines[1]);
    }

    #[test]
    fn inline_table_tag_differs_from_table_scope() {
        // An inline table reads `[T/I]`; a standard `[table]` scope `[T/S]`.
        let lines = render("pt = { x = 1 }\n[srv]\nport = 8080\n", 60, 8);
        let joined = lines.join("\n");
        assert!(joined.contains("[T/I]"), "rows: {joined:?}");
        assert!(
            joined
                .lines()
                .any(|l| l.contains("srv") && l.contains("[T/S]")),
            "standard table scope tag: {joined:?}"
        );
    }

    /// Render with all branches expanded, returning the joined buffer text.
    fn render_expanded(src: &str, w: u16, h: u16) -> String {
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::from_str(src).unwrap(),
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
    fn display_key_uses_the_authored_spelling_for_every_backend() {
        // The row label is the literal when projection captured one — the quote
        // characters come from the source, never from this helper.
        assert_eq!(display_key("a b", Some("\"a b\"")), "\"a b\"");
        assert_eq!(display_key("a b", Some("'a b'")), "'a b'");
        assert_eq!(display_key("a", Some("a")), "a");
        // Keyless rows (array elements, comments, root) have no literal.
        assert_eq!(display_key("[0]", None), "[0]");
    }

    #[test]
    fn yaml_quoted_key_shows_quotes_in_tree_row() {
        let doc = crate::model::any_doc::AnyDocument::Yaml(
            crate::model::yaml::doc::YamlDocument::from_str("\"a b\": 1\n").unwrap(),
        );
        let app = App::new(doc);
        let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
        terminal.draw(|fr| draw(fr, &app)).unwrap();
        let joined: String = (0..8)
            .map(|y| {
                (0..60)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("\"a b\""),
            "quoted YAML key must show quote marks in the tree row: {joined:?}"
        );
    }

    #[test]
    fn yaml_single_quoted_key_keeps_single_quotes_in_tree_row() {
        let doc = crate::model::any_doc::AnyDocument::Yaml(
            crate::model::yaml::doc::YamlDocument::from_str("'a b': 1\n").unwrap(),
        );
        let app = App::new(doc);
        let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
        terminal.draw(|fr| draw(fr, &app)).unwrap();
        let joined: String = (0..8)
            .map(|y| {
                (0..60)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("'a b'"),
            "single-quoted YAML key must keep ITS OWN quote style: {joined:?}"
        );
        assert!(
            !joined.contains("\"a b\""),
            "must not synthesize double quotes for a single-quoted key: {joined:?}"
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

    #[test]
    fn draw_tree_windows_to_the_viewport_and_still_follows_the_cursor() {
        // 60 top-level keys — far more than a small terminal can show at once.
        let mut src = String::new();
        for i in 0..60 {
            src.push_str(&format!("k{i:02} = {i}\n"));
        }
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::from_str(&src).unwrap(),
        );
        let mut app = App::new(doc);
        // title(1) + column header(1) + status(1) leaves 7 rows for the tree:
        // root + 6 data rows.
        let mut terminal = Terminal::new(TestBackend::new(60, 10)).unwrap();
        terminal.draw(|fr| draw(fr, &app)).unwrap();
        let joined_initial: String = (0..10)
            .map(|y| {
                (0..60)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined_initial.contains("k00"),
            "top of list visible: {joined_initial:?}"
        );
        assert!(
            !joined_initial.contains("k59"),
            "far-below row must not be drawn while scrolled to the top: {joined_initial:?}"
        );

        // Move the cursor well past the initial window and re-render — the
        // window must follow it (proves the manual offset-tracking replacing
        // ratatui's own selection-follow still works), and the rows that
        // scrolled out of view must be gone from the buffer, not just hidden.
        for _ in 0..40 {
            app.cursor_down();
        }
        app.rebuild_rows();
        terminal.draw(|fr| draw(fr, &app)).unwrap();
        let joined_after: String = (0..10)
            .map(|y| {
                (0..60)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined_after.contains("k39"),
            "cursor row (40th CursorDown = k39) must be visible after scrolling: {joined_after:?}"
        );
        assert!(
            !joined_after.contains("k00"),
            "row scrolled far out of view must not still be drawn: {joined_after:?}"
        );
    }

    #[test]
    fn cursor_selection_and_clip_source_colors_are_distinct_and_composable() {
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::from_str("a = 1\nb = 2\nc = 3\n").unwrap(),
        );
        let mut app = App::new(doc);
        app.select_row(2); // rows[0] is the root; rows[1]=a, rows[2]=b, rows[3]=c — cursor on `b`
        app.session.selection.toggle(app.row_path(2)); // lock-select the cursor row too
        app.session.selection.toggle(app.row_path(3)); // and a second, non-cursor row (`c`)
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
        terminal.draw(|fr| draw(fr, &app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        // Locate a tree row by its NAME-cell key, which starts at column 7 for a
        // depth-1 row (1 selection-marker col + 2 indent + 2 branch marker + 1
        // warning-marker col + 1 spacing col). Scanning the whole line instead
        // would also hit the "confy" title and the status bar.
        let row_y = |needle: &str| -> u16 {
            (0..8)
                .find(|&y| buf[(KEY_X, y)].symbol() == needle)
                .unwrap_or_else(|| panic!("row containing {needle:?} not found in rendered buffer"))
        };
        let cursor_y = row_y("b");
        let locked_only_y = row_y("c");
        assert_eq!(
            buf[(0, cursor_y)].bg,
            Color::Blue,
            "cursor row must be blue, not the retired grey selection fill"
        );
        assert!(
            (0..40).any(|x| buf[(x, cursor_y)].symbol() == "●"),
            "locked-selection glyph must still render on a row that is also the cursor"
        );
        assert_eq!(
            buf[(0, locked_only_y)].bg,
            Color::Reset,
            "a locked-selection row that is not the cursor must not paint any background fill"
        );
    }

    #[test]
    fn paste_target_into_fill_suppresses_kind_tag_color() {
        // The armed paste-slot's `Into` fill (green bg, black fg, ADR 0005 §5)
        // must suppress the KIND column's own type-based colour the same way
        // the cursor's blue and the clip-source colours already do (§3) —
        // otherwise a "string" row's Green KIND tag renders on top of the
        // Into slot's own Green fill and becomes illegible. `paste_slots()`
        // only ever offers `Into` on branch rows (whose KIND tag never gets a
        // colour), so this state is unreachable via normal keyboard/pointer
        // paste-slot cycling; it is however reachable through the WASM
        // `Intent::SetPasteSlot` boundary, which does not re-validate
        // `is_branch` — so this test drives it directly via the session field
        // to pin the render-layer contract regardless of that upstream gate.
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::from_str("s = \"x\"\no = 2\n").unwrap(),
        );
        let mut app = App::new(doc);
        app.select_row(2); // cursor on `o`, so `s` is not the cursor row
        let s_path = app.row_path(1);
        let o_path = app.row_path(2);
        app.session.clipboard = Some(Clipboard {
            fragments: vec!["z = 1\n".into()],
            cut: false,
            sources: vec![o_path], // clip source is `o`, not `s`
        });
        app.session.paste_slot = Some(PasteSlot::Into(s_path));
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
        terminal.draw(|fr| draw(fr, &app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let row_y = (0..8)
            .find(|&y| buf[(KEY_X, y)].symbol() == "s")
            .expect("`s` row not found in rendered buffer");
        let kind_x = name_col_width(40) + 1;
        assert_eq!(
            buf[(kind_x, row_y)].bg,
            Color::Green,
            "Into-target row must show the green paste-target fill"
        );
        assert_eq!(
            buf[(kind_x, row_y)].fg,
            Color::Black,
            "the KIND tag's own colour must be suppressed on the Into fill, matching the row's fg(Black), not painted with type_label's Green"
        );
    }

    #[test]
    fn clip_source_colors_do_not_collide_with_cursor_blue() {
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::from_str("a = 1\nb = 2\n").unwrap(),
        );
        let mut app = App::new(doc);
        let a_path = app.row_path(1); // rows[0] is the root; `a` is the first real row
        app.session.clipboard = Some(Clipboard {
            fragments: vec!["a = 1\n".into()],
            cut: false,
            sources: vec![a_path],
        });
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
        terminal.draw(|fr| draw(fr, &app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let row_y = (0..8)
            .find(|&y| buf[(KEY_X, y)].symbol() == "a")
            .expect("copy-source row not found in rendered buffer");
        assert_eq!(
            buf[(0, row_y)].bg,
            Color::Magenta,
            "copy source must use its own color, not the cursor's blue"
        );
    }

    #[test]
    fn branch_with_descendant_warning_shows_marker_glyph_regardless_of_expand_state() {
        let doc = crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::from_str("[server]\nport = \"nope\"\n").unwrap(),
        );
        let mut app = App::new(doc);
        app.session.apply_schema_text(
            confy_core::schema::SchemaSource::Local("/tmp/s.json".into()),
            Ok(r#"{"type":"object","properties":{"server":{"type":"object","properties":{"port":{"type":"integer"}}}}}"#.to_string()),
        );
        let server_path: crate::model::node::Path = vec![crate::model::node::Seg::Key("server".into())];
        // Collapsed: the marker must show.
        app.session.expanded.remove(&server_path);
        app.rebuild_rows();
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
        terminal.draw(|fr| draw(fr, &app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        assert!(
            (0..40).any(|x| (0..8).any(|y| buf[(x, y)].symbol() == "△")),
            "collapsed branch with only a hidden descendant violation must show the hollow △ marker"
        );
        assert!(
            !(0..40).any(|x| (0..8).any(|y| buf[(x, y)].symbol() == "▲")),
            "the violating leaf is hidden, so nothing is filled yet"
        );
        // Expanded: the summary markers must still show on root/server — a stable
        // visual cue that doesn't vanish the moment the violating child becomes
        // visible. And the leaf that actually violates (`port`) must show its own
        // filled ▲, unified with the branch-summary case rather than relying on the
        // yellow text alone. Root and server only summarize a descendant violation
        // (hollow △); only `port` itself violates (filled ▲).
        app.session.expanded.insert(server_path);
        app.rebuild_rows();
        let mut terminal2 = Terminal::new(TestBackend::new(40, 8)).unwrap();
        terminal2.draw(|fr| draw(fr, &app)).unwrap();
        let buf2 = terminal2.backend().buffer().clone();
        let hollow_count = (0..40)
            .flat_map(|x| (0..8).map(move |y| (x, y)))
            .filter(|&(x, y)| buf2[(x, y)].symbol() == "△")
            .count();
        let filled_count = (0..40)
            .flat_map(|x| (0..8).map(move |y| (x, y)))
            .filter(|&(x, y)| buf2[(x, y)].symbol() == "▲")
            .count();
        assert_eq!(
            hollow_count, 2,
            "root and server only summarize a descendant violation"
        );
        assert_eq!(
            filled_count, 1,
            "only port itself violates"
        );
    }
}
