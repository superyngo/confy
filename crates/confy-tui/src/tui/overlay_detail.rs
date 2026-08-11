//! The `i` Detail popup: full node text + (when the cursor row carries schema
//! violations) an appended `Schema:` section. Split out of `ui.rs` (Task 10,
//! 2026-08-11 audit remediation) — pure code motion, no behavior change.
use crate::tui::app::App;
use crate::tui::state::Mode;
use ratatui::prelude::*;
use ratatui::widgets::*;

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

/// The Detail popup's full rendered text — `detail_text` plus, when the
/// cursor row carries schema violations, an appended `Schema:` section.
/// Shared by `draw_detail_overlay` (sizing + content) and `mod.rs`'s Detail
/// key handler (scroll-clamp), so they can never drift out of sync.
pub(crate) fn detail_full_text(app: &App) -> String {
    let mut text = app.session.detail_text.clone().unwrap_or_default();
    if let Some(msgs) = app
        .cursor_row()
        .and_then(|r| r.violations.as_ref())
        .filter(|msgs| !msgs.is_empty())
    {
        text.push_str("\n\nSchema:\n");
        text.push_str(&msgs.join("\n"));
    }
    text
}

pub(crate) fn draw_detail_overlay(f: &mut Frame, app: &App) {
    if !matches!(app.session.mode, Mode::Detail) {
        return;
    }
    let detail_text = match &app.session.detail_text {
        Some(t) => t.clone(),
        None => return,
    };
    let violations = app
        .cursor_row()
        .and_then(|r| r.violations.as_ref())
        .filter(|msgs| !msgs.is_empty());
    // Size the popup from the FULL rendered text (original + appended Schema
    // section), so violation messages never get clipped.
    let full_text = detail_full_text(app);
    let area = detail_popup_rect(f.area(), &full_text);
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(" Detail (↑/↓ PgUp/PgDn Home/End · Esc) ")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black).fg(Color::White));
    let mut lines: Vec<Line> = detail_text.lines().map(Line::from).collect();
    if let Some(msgs) = violations {
        lines.push(Line::from(""));
        lines.push(Line::from("Schema:"));
        for msg in msgs {
            lines.push(Line::from(Span::styled(
                msg.clone(),
                Style::default().fg(Color::Yellow),
            )));
        }
    }
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0));
    f.render_widget(paragraph, area);
}
