use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
    Convert,
    Help,
    Rename,
    LangPicker,
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
        (KeyCode::Enter, _) | (KeyCode::Char(' '), _) => KeyAction::ToggleExpand,
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
        // `c` is copy, so document-convert (Root node) lives on the capital.
        (KeyCode::Char('C'), _) => KeyAction::Convert,
        (KeyCode::Char('?'), _) => KeyAction::Help,
        (KeyCode::F(2), _) => KeyAction::Rename,
        // Language picker — lowercase l (verified unbound; no collision with
        // existing bindings).
        (KeyCode::Char('l'), _) => KeyAction::LangPicker,
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
}
