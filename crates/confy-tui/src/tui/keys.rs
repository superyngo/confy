use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

// `Debug` supplies the variant name the `keymap_doc_*` parity tests compare
// against docs/reference/KEYMAP.md's TUI column.
#[derive(Debug, PartialEq, Eq)]
pub enum KeyAction {
    CursorDown,
    CursorUp,
    PageUp,
    PageDown,
    Home,
    End,
    ToggleExpand,
    CollapseAll,
    ExpandAll,
    ExpandLevel,
    CollapseLevel,
    ToggleSelect,
    ExtendSelectUp,
    ExtendSelectDown,
    Info,
    EditNode,
    EditExternal,
    IncValue,
    DecValue,
    AddNode,
    Delete,
    Copy,
    Cut,
    Paste,
    Remark,
    Save,
    Undo,
    Redo,
    Escape,
    Quit,
    Filter,
    TypeFilter,
    KindSwitch,
    ActionMenu,
    Convert,
    Help,
    Rename,
    LangPicker,
    ToggleDiag,
    Noop,
}

pub fn map_key(key: KeyEvent) -> KeyAction {
    match (key.code, key.modifiers) {
        (KeyCode::Up, m) if m.contains(KeyModifiers::SHIFT) => KeyAction::ExtendSelectUp,
        (KeyCode::Down, m) if m.contains(KeyModifiers::SHIFT) => KeyAction::ExtendSelectDown,
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => KeyAction::CursorDown,
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => KeyAction::CursorUp,
        (KeyCode::PageUp, _) => KeyAction::PageUp,
        (KeyCode::PageDown, _) => KeyAction::PageDown,
        (KeyCode::Home, _) => KeyAction::Home,
        (KeyCode::End, _) => KeyAction::End,
        (KeyCode::Left, _) => KeyAction::DecValue,
        (KeyCode::Right, _) => KeyAction::IncValue,
        (KeyCode::Char(' '), _) => KeyAction::ToggleExpand,
        (KeyCode::Enter, _) => KeyAction::Info,
        (KeyCode::Char('0'), _) => KeyAction::CollapseAll,
        (KeyCode::Char('9'), _) => KeyAction::ExpandAll,
        (KeyCode::Char('1'), _) => KeyAction::ExpandLevel,
        (KeyCode::Char('2'), _) => KeyAction::CollapseLevel,
        (KeyCode::Char('s'), m) if m.contains(KeyModifiers::CONTROL) => KeyAction::Save,
        (KeyCode::Char('s'), _) => KeyAction::ToggleSelect,
        (KeyCode::Char('i'), _) => KeyAction::Info,
        (KeyCode::Char('E'), _) => KeyAction::EditExternal,
        (KeyCode::Char('e'), _) => KeyAction::EditNode,
        (KeyCode::Char('a'), _) => KeyAction::AddNode,
        (KeyCode::Char('d'), _) => KeyAction::Delete,
        (KeyCode::Delete, _) => KeyAction::Delete,
        (KeyCode::Char('c'), _) => KeyAction::Copy,
        (KeyCode::Char('x'), _) => KeyAction::Cut,
        (KeyCode::Char('v'), _) => KeyAction::Paste,
        (KeyCode::Char('r'), _) => KeyAction::Remark,
        (KeyCode::Char('w'), _) => KeyAction::Save,
        (KeyCode::Char('z'), _) => KeyAction::Undo,
        (KeyCode::Char('y'), _) => KeyAction::Redo,
        (KeyCode::Esc, _) => KeyAction::Escape,
        (KeyCode::Char('q'), _) => KeyAction::Quit,
        (KeyCode::Char('/'), _) => KeyAction::Filter,
        (KeyCode::Char('f'), _) => KeyAction::TypeFilter,
        // `k` is vim cursor-up, so kind-switch lives on the capital.
        (KeyCode::Char('K'), _) => KeyAction::KindSwitch,
        (KeyCode::Char('m'), _) => KeyAction::ActionMenu,
        // `c` is copy, so document-convert (Root node) lives on the capital.
        (KeyCode::Char('C'), _) => KeyAction::Convert,
        (KeyCode::Char('?'), _) => KeyAction::Help,
        (KeyCode::F(2), _) => KeyAction::Rename,
        // Language picker — lowercase l (verified unbound; no collision with
        // existing bindings).
        (KeyCode::Char('l'), _) => KeyAction::LangPicker,
        // Diag ring overlay — tilde (verified unbound).
        (KeyCode::Char('~'), _) => KeyAction::ToggleDiag,
        _ => KeyAction::Noop,
    }
}

/// Keybinding help text, displayed in the `?` overlay. Format-specific: the
/// op list and KIND legend differ per backend. Routed through the `tui.*`
/// catalog (i18n Phase 2) -- `en` text is byte-identical to the old
/// `&'static str` consts.
pub fn help_text(
    format: crate::model::document::DocFormat,
    lang: confy_core::session::Lang,
) -> String {
    use crate::model::document::DocFormat;
    use confy_core::session::tr;
    let key = match format {
        DocFormat::Toml => "tui.help.toml",
        DocFormat::Json => "tui.help.json",
        DocFormat::Yaml => "tui.help.yaml",
    };
    tr(lang, key).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use confy_core::session::Lang;

    #[test]
    fn json_help_differs_from_toml() {
        use crate::model::document::DocFormat;
        let j = help_text(DocFormat::Json, Lang::En);
        assert!(j.contains("//"));
        assert!(j.contains("[S:null]"));
        assert!(!j.contains("dotted"));
        assert!(!j.contains("[A/T]"));
        assert_ne!(j, help_text(DocFormat::Toml, Lang::En));
    }

    #[test]
    fn yaml_help_differs_from_toml() {
        use crate::model::document::DocFormat;
        let y = help_text(DocFormat::Yaml, Lang::En);
        assert!(y.contains("[opaq ]"));
        assert!(y.contains("block"));
        assert!(y.contains("flow"));
        assert!(!y.contains("dotted"));
        assert!(!y.contains("[A/T]"));
        assert_ne!(y, help_text(DocFormat::Toml, Lang::En));
    }

    #[test]
    fn help_text_is_translated_for_zh_tw() {
        // Phase 4 completed the zh-TW help-text translation, so the cheatsheet
        // now differs from English while KIND tags and shortcut key names
        // (project vocabulary, deliberately untranslated) still appear in
        // both.
        use crate::model::document::DocFormat;
        let en = help_text(DocFormat::Toml, Lang::En);
        let zh = help_text(DocFormat::Toml, Lang::ZhTw);
        assert_ne!(en, zh);
        assert!(zh.contains("[D:odt ]"));
        assert!(zh.contains("Ctrl+s"));
    }

    #[test]
    fn enter_opens_detail_space_toggles_expand() {
        use crossterm::event::{KeyEvent, KeyModifiers};
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert!(
            matches!(map_key(enter), KeyAction::Info),
            "Enter must route to the same detail-toggle action as `i` (ADR 0005 §4)"
        );
        let space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
        assert!(
            matches!(map_key(space), KeyAction::ToggleExpand),
            "Space must keep ToggleExpand — only Enter's binding reverses"
        );
    }

    #[test]
    fn armed_clipboard_guards_lang_picker_and_edit_node() {
        use crate::model::node::Seg;
        use crate::tui::app::App;
        use confy_core::session::Clipboard;

        let mut app = App::new(crate::model::any_doc::AnyDocument::Toml(
            crate::model::cst_doc::CstDocument::from_str("a = 1\n").unwrap(),
        ));
        app.rebuild_rows();
        app.session.clipboard = Some(Clipboard {
            fragments: vec!["a = 1\n".into()],
            cut: false,
            sources: vec![vec![Seg::Key("a".into())]],
        });

        // Key mapping verification
        use crossterm::event::{KeyEvent, KeyModifiers};
        let key_e_upper = KeyEvent::new(KeyCode::Char('E'), KeyModifiers::NONE);
        assert!(matches!(map_key(key_e_upper), KeyAction::EditExternal));
        let key_l = KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE);
        assert!(matches!(map_key(key_l), KeyAction::LangPicker));

        // Language picker is blocked
        app.open_lang_picker();
        assert!(
            app.lang_picker.is_none(),
            "lang picker should not open when clipboard is armed"
        );
        assert_eq!(
            app.session.notice.as_ref().map(|n| n.text.as_str()),
            Some(confy_core::session::tr(
                app.session.lang,
                "core.clipboard.action-locked"
            ))
        );

        // Edit external / edit node is blocked
        app.session.notice = None;
        app.edit_node();
        assert_eq!(
            app.session.notice.as_ref().map(|n| n.text.as_str()),
            Some(confy_core::session::tr(
                app.session.lang,
                "core.clipboard.action-locked"
            ))
        );
    }

    #[test]
    fn m_opens_action_menu() {
        use crossterm::event::{KeyEvent, KeyModifiers};
        let key_m = KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE);
        assert!(matches!(map_key(key_m), KeyAction::ActionMenu));
    }

    // ---- docs/reference/KEYMAP.md parity (drift guard) ----
    // KEYMAP.md is the single source of truth for the normal-mode keymap on
    // both surfaces. These tests check its **TUI** column against `map_key`;
    // `web/keymap-parity.spec.mjs` checks the **Web** column against
    // `resolveKeyIntent`. Adding/removing/re-pointing a binding without
    // editing the doc fails here.

    struct DocRow {
        key: String,
        tui: String,
        web: String,
        status: String,
    }

    fn keymap_doc() -> Vec<DocRow> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/reference/KEYMAP.md");
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
        let begin = text
            .find("<!-- KEYMAP-TABLE:BEGIN -->")
            .expect("KEYMAP.md is missing the KEYMAP-TABLE:BEGIN marker");
        let end = text
            .find("<!-- KEYMAP-TABLE:END -->")
            .expect("KEYMAP.md is missing the KEYMAP-TABLE:END marker");
        let unwrap = |c: &str| c.trim().trim_matches('`').trim_matches('*').to_string();
        text[begin..end]
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with('|'))
            .map(|l| {
                let inner = l.trim_start_matches('|').trim_end_matches('|');
                inner.split('|').map(str::to_string).collect::<Vec<_>>()
            })
            // Separator detection runs on the *raw* cells: the `-` key row
            // unwraps to a bare "-" and a naive dash test would eat it.
            .filter(|raw| {
                let sep = raw
                    .iter()
                    .all(|c| !c.trim().is_empty() && c.trim().chars().all(|ch| ch == '-' || ch == ':'));
                !sep && raw[0].trim() != "Key"
            })
            .map(|raw| DocRow {
                key: unwrap(&raw[0]),
                tui: unwrap(&raw[1]),
                web: unwrap(&raw[2]),
                status: unwrap(&raw[3]),
            })
            .collect()
    }

    /// Canonical KEYMAP.md key name -> the `KeyEvent` crossterm would deliver.
    fn key_event(name: &str) -> KeyEvent {
        let (mods, base) = if let Some(r) = name.strip_prefix("Shift+") {
            (KeyModifiers::SHIFT, r)
        } else if let Some(r) = name.strip_prefix("Ctrl+") {
            (KeyModifiers::CONTROL, r)
        } else {
            (KeyModifiers::NONE, name)
        };
        let code = match base {
            "ArrowUp" => KeyCode::Up,
            "ArrowDown" => KeyCode::Down,
            "ArrowLeft" => KeyCode::Left,
            "ArrowRight" => KeyCode::Right,
            "Home" => KeyCode::Home,
            "End" => KeyCode::End,
            "PageUp" => KeyCode::PageUp,
            "PageDown" => KeyCode::PageDown,
            "Enter" => KeyCode::Enter,
            "Escape" => KeyCode::Esc,
            "Delete" => KeyCode::Delete,
            "Backspace" => KeyCode::Backspace,
            "Tab" => KeyCode::Tab,
            "Space" => KeyCode::Char(' '),
            s if s.len() > 1 && s.starts_with('F') && s[1..].parse::<u8>().is_ok() => {
                KeyCode::F(s[1..].parse().unwrap())
            }
            s => {
                let mut it = s.chars();
                match (it.next(), it.next()) {
                    (Some(c), None) => KeyCode::Char(c),
                    _ => panic!("KEYMAP.md: unrecognized key name {name:?}"),
                }
            }
        };
        KeyEvent::new(code, mods)
    }

    /// `map_key` result in the doc's encoding (`—` for `Noop`).
    fn tui_binding(name: &str) -> String {
        match map_key(key_event(name)) {
            KeyAction::Noop => "—".to_string(),
            other => format!("{other:?}"),
        }
    }

    #[test]
    fn keymap_doc_tui_column_matches_map_key() {
        let rows = keymap_doc();
        assert!(
            rows.len() > 20,
            "KEYMAP.md table looks truncated: {} rows",
            rows.len()
        );
        for r in &rows {
            assert_eq!(
                tui_binding(&r.key),
                r.tui,
                "KEYMAP.md says `{}` -> `{}` on the TUI, but map_key disagrees. \
                 Update docs/reference/KEYMAP.md in the same commit as the binding change.",
                r.key,
                r.tui
            );
        }
    }

    #[test]
    fn keymap_doc_status_column_is_consistent() {
        for r in &keymap_doc() {
            let expected = match (r.tui != "—", r.web != "—") {
                (true, true) => "both",
                (true, false) => "tui-only",
                (false, true) => "web-only",
                (false, false) => "unbound",
            };
            assert_eq!(
                r.status, expected,
                "KEYMAP.md row `{}` is marked `{}` but its TUI/Web cells derive `{}`",
                r.key, r.status, expected
            );
        }
    }

    #[test]
    fn keymap_doc_covers_every_tui_binding() {
        // Completeness: any key that produces a real action must be documented.
        // Scope (see KEYMAP.md "Scope of the machine-checked table"): unmodified
        // keys, Shift+arrows, and Ctrl+<letter> combinations that are *not* mere
        // wildcard aliases of the unmodified key — `map_key`'s char arms match
        // with a modifier wildcard, so Ctrl+C reaching `Copy` is an alias, while
        // Ctrl+S reaching `Save` (vs `s` -> `ToggleSelect`) is a real binding.
        let documented: std::collections::HashSet<String> =
            keymap_doc().into_iter().map(|r| r.key).collect();

        let mut names: Vec<String> = vec![
            "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End", "PageUp",
            "PageDown", "Enter", "Escape", "Delete", "Backspace", "Tab", "Space",
            "Shift+ArrowUp", "Shift+ArrowDown",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        names.extend((1..=12).map(|n| format!("F{n}")));
        names.extend((0x21u8..0x7f).map(|b| (b as char).to_string()));
        names.extend(
            (b'a'..=b'z')
                .map(|b| format!("Ctrl+{}", b as char))
                .filter(|n| {
                    let bare = n.trim_start_matches("Ctrl+");
                    tui_binding(n) != tui_binding(bare)
                }),
        );

        let missing: Vec<String> = names
            .iter()
            .filter(|n| tui_binding(n) != "—" && !documented.contains(*n))
            .map(|n| format!("{n} -> {}", tui_binding(n)))
            .collect();
        assert!(
            missing.is_empty(),
            "these TUI bindings are missing from docs/reference/KEYMAP.md: {missing:?}"
        );
    }

    #[test]
    fn keymap_doc_unbound_tui_keys_really_are_unbound() {
        for r in keymap_doc().iter().filter(|r| r.tui == "—") {
            assert_eq!(
                tui_binding(&r.key),
                "—",
                "KEYMAP.md lists `{}` as unbound on the TUI, but map_key binds it",
                r.key
            );
        }
    }
}
