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
    Move,
    Remark,
    Save,
    Undo,
    Redo,
    Escape,
    Quit,
    Filter,
    Help,
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
        (KeyCode::Char('s'), m) if m.contains(KeyModifiers::CONTROL) => KeyAction::Save,
        (KeyCode::Char('s'), _) => KeyAction::ToggleSelect,
        (KeyCode::Char('i'), _) => KeyAction::Info,
        (KeyCode::Char('E'), _) => KeyAction::EditExternal,
        (KeyCode::Char('e'), _) => KeyAction::EditNode,
        (KeyCode::Char('a'), _) => KeyAction::AddNode,
        (KeyCode::Char('d'), _) => KeyAction::Delete,
        (KeyCode::Char('c'), _) => KeyAction::Copy,
        (KeyCode::Char('x'), _) => KeyAction::Cut,
        (KeyCode::Char('v'), _) => KeyAction::Paste,
        (KeyCode::Char('m'), _) => KeyAction::Move,
        (KeyCode::Char('r'), _) => KeyAction::Remark,
        (KeyCode::Char('w'), _) => KeyAction::Save,
        (KeyCode::Char('z'), _) => KeyAction::Undo,
        (KeyCode::Char('y'), _) => KeyAction::Redo,
        (KeyCode::Esc, _) => KeyAction::Escape,
        (KeyCode::Char('q'), _) => KeyAction::Quit,
        (KeyCode::Char('/'), _) => KeyAction::Filter,
        (KeyCode::Char('?'), _) => KeyAction::Help,
        _ => KeyAction::Noop,
    }
}

/// Keybinding help text, displayed in the `?` overlay.
pub fn help_text() -> &'static str {
    "\
 j/k/Arrows  Move cursor       PgUp/PgDn  Page up/down
 Home/End     First/last row    0/9         Collapse/expand all
 Enter/Space  Expand branch or open leaf detail
 s            Toggle select     Shift+Up/Dn Range select
 i            Detail/info popup (any node)
 e            Edit (inline/$EDITOR)  E       Force $EDITOR
 ←/→          Toggle bool / ±1 number    a   Add node
 d            Delete            x/c/v       Cut/copy/paste
 m            Move (2-press)    r           Remark toggle
 z/y          Undo/redo         /           Fuzzy filter
 w/Ctrl+s     Save              q           Quit
 ?            This help
 "
}
