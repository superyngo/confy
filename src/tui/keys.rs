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
    ToggleSelect,
    ExtendSelectUp,
    ExtendSelectDown,
    Quit,
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
        (KeyCode::Enter, _) | (KeyCode::Char(' '), _) => KeyAction::ToggleExpand,
        (KeyCode::Char('0'), _) => KeyAction::CollapseAll,
        (KeyCode::Char('9'), _) => KeyAction::ExpandAll,
        (KeyCode::Char('s'), _) => KeyAction::ToggleSelect,
        (KeyCode::Char('q'), _) => KeyAction::Quit,
        _ => KeyAction::Noop,
    }
}
