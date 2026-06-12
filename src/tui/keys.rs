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
        (KeyCode::Char('1'), _) => KeyAction::ExpandLevel,
        (KeyCode::Char('2'), _) => KeyAction::CollapseLevel,
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
        (KeyCode::Char('?'), _) => KeyAction::Help,
        _ => KeyAction::Noop,
    }
}

/// Keybinding help text, displayed in the `?` overlay.
pub fn help_text() -> &'static str {
    "\
 j/k/Arrows  Move cursor       PgUp/PgDn  Page up/down
 Home/End     First/last row    0/9         Collapse/expand all
 1/2          Expand/collapse one level (subtree / ascend)
 Enter/Space  Expand branch or open leaf detail
 s            Toggle select     Shift+Up/Dn Range select
 i            Detail/info popup (any node)
 e            Edit (inline/$EDITOR)  E       Force $EDITOR
 ←/→          Toggle bool / ±1 number    a   Add node
 d            Delete            x/c/v       Cut/copy/paste
 r            Remark toggle     z/y         Undo/redo
 K            Kind switch (scalar type / table & array notation)
 /            Fuzzy filter      f           Type filter (checkbox menu)
 /…Enter      Lock in filtered list   Esc   Clear filter / selection
 w/Ctrl+s     Save              q           Quit
 ?            This help

 ── KIND column ──────────────────────────────────────────────────
 Key sign (first 3 chars):
   (B) bare key   (Q) quoted key   (D) dotted key   (-) no key

 Containers:
   [G]     root/file node
   [C]     comment node
   [A/I]   inline array        [A/M]  multiline array
   [A/T]   array-of-tables     [T/I]  inline table
   [T/S]   table scope (standard [header])
   [T/D]   dotted-key table (a.b.c = …)

 Scalars  [type:format]:
   [S:str ] basic string        [S:mstr] multiline basic string
   [S:lit ] literal string      [S:mlit] multiline literal string
   [I:dec ] decimal integer     [I:hex ] hex integer
   [I:oct ] octal integer       [I:bin ] binary integer
   [F:flt ] float               [F:inf ] infinity  [F:nan ] NaN
   [B:bool] boolean
   [D:odt ] offset datetime     [D:ldt ] local datetime
   [D:ldat] local date          [D:ltim] local time
 "
}
