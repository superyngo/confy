use crossterm::event::{KeyCode, KeyEvent};

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
    Quit,
    Noop,
}

pub fn map_key(key: KeyEvent) -> KeyAction {
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => KeyAction::CursorDown,
        KeyCode::Up | KeyCode::Char('k') => KeyAction::CursorUp,
        KeyCode::PageUp => KeyAction::PageUp,
        KeyCode::PageDown => KeyAction::PageDown,
        KeyCode::Home => KeyAction::Home,
        KeyCode::End => KeyAction::End,
        KeyCode::Enter | KeyCode::Char(' ') => KeyAction::ToggleExpand,
        KeyCode::Char('0') => KeyAction::CollapseAll,
        KeyCode::Char('9') => KeyAction::ExpandAll,
        KeyCode::Char('q') => KeyAction::Quit,
        _ => KeyAction::Noop,
    }
}
