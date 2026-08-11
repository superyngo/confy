//! The `L` language-picker popup. Split out of `ui.rs` (Task 10, 2026-08-11
//! audit remediation) — pure code motion, no behavior change.
use crate::tui::app::App;
use crate::tui::ui::centered_rect;
use ratatui::prelude::*;
use ratatui::widgets::*;

/// Display label for a picker entry, in that language's own script (the
/// conventional choice — a language name is not itself translated).
fn lang_label(lang: confy_core::session::Lang) -> &'static str {
    match lang {
        confy_core::session::Lang::En => "English (en)",
        confy_core::session::Lang::ZhTw => "繁體中文 (zh-TW)",
    }
}

/// The `L` language-picker popup: a small centered single-select list,
/// mirroring the `K` kind-switch popup's layout. Host-side state
/// (`app.lang_picker`), not a core `Mode`.
pub(crate) fn draw_lang_picker_overlay(f: &mut Frame, app: &App) {
    use confy_core::session::tr;
    use unicode_width::UnicodeWidthStr;
    let Some(st) = &app.lang_picker else {
        return;
    };
    // Column width in display cells (not char count) so a CJK label still
    // aligns the popup's right edge.
    let col = crate::tui::app::LANG_OPTIONS
        .iter()
        .map(|&l| lang_label(l).width())
        .max()
        .unwrap_or(0);
    let lines: Vec<Line> = crate::tui::app::LANG_OPTIONS
        .iter()
        .enumerate()
        .map(|(i, &lang)| {
            let marker = if i == st.cursor { "›" } else { " " };
            let mut style = Style::default();
            if i == st.cursor {
                style = style.add_modifier(Modifier::REVERSED);
            }
            let label = lang_label(lang);
            let pad = " ".repeat(col.saturating_sub(label.width()));
            Line::from(Span::styled(format!(" {marker} {label}{pad}"), style))
        })
        .collect();
    let height = (lines.len() as u16 + 2).min(f.area().height);
    let area = centered_rect(40, height, f.area());
    f.render_widget(Clear, area);
    let title = format!(" {} ", tr(app.session.lang, "tui.lang.picker-title"));
    let block = Block::default()
        .title(title)
        .title_bottom(" ↑↓ move · Enter apply · Esc cancel ")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black).fg(Color::White));
    f.render_widget(Paragraph::new(lines).block(block), area);
}
