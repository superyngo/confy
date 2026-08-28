/// Integration tests: all guarded methods must no-op and set `status` when
/// the clipboard is armed (ADR 0005 §5 — cut/copy modal lock).
use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::DocFormat;
use confy_core::model::node::Seg;
use confy_core::session::{Intent, Mode, Session};

fn armed_session() -> Session {
    // A TOML doc with at least two scalar keys so copy/cut is meaningful.
    let doc = AnyDocument::from_str_as("a = 1\nb = 2\n", DocFormat::Toml).unwrap();
    let mut s = Session::new(doc);
    // Move cursor to first real node ("a") and copy it — arms clipboard.
    s.cursor_down();
    s.copy_selected();
    assert!(s.clipboard.is_some(), "clipboard must be armed after copy");
    s.notice = None; // clear the "copied N node(s)" status
    s
}

fn armed_session_with_table() -> Session {
    let doc = AnyDocument::from_str_as(
        "[server]\nport = 8080\nhost = \"localhost\"\n",
        DocFormat::Toml,
    )
    .unwrap();
    let mut s = Session::new(doc);
    // Expand [server] so its children are visible
    s.cursor_down(); // on "server"
    s.toggle_expand();
    s.cursor_down(); // on "port"
    s.copy_selected();
    assert!(s.clipboard.is_some());
    s.notice = None;
    s
}

fn has_locked_status(s: &Session) -> bool {
    s.snapshot()
        .status_text()
        .map(|st| st.contains("clipboard") || st.contains("剪貼簿"))
        .unwrap_or(false)
}

// ---- 1. add_node / add_child / add_sibling ----

#[test]
fn add_node_locked_while_clipboard_armed() {
    let mut s = armed_session();
    let before = s.visible_rows().len();
    s.add_node();
    assert_eq!(s.visible_rows().len(), before, "no node added");
    assert!(has_locked_status(&s));
}

#[test]
fn add_child_locked_while_clipboard_armed() {
    let mut s = armed_session_with_table();
    let before = s.visible_rows().len();
    s.add_child();
    assert_eq!(s.visible_rows().len(), before, "no child added");
    assert!(has_locked_status(&s));
}

#[test]
fn add_sibling_locked_while_clipboard_armed() {
    let mut s = armed_session();
    let before = s.visible_rows().len();
    s.add_sibling();
    assert_eq!(s.visible_rows().len(), before, "no sibling added");
    assert!(has_locked_status(&s));
}

// ---- 2. delete_selected ----

#[test]
fn delete_selected_locked_while_clipboard_armed() {
    let mut s = armed_session();
    let before = s.visible_rows().len();
    s.delete_selected();
    assert_eq!(s.visible_rows().len(), before, "no deletion");
    assert!(has_locked_status(&s));
}

// ---- 3. nudge ----

#[test]
fn nudge_locked_while_clipboard_armed() {
    let mut s = armed_session();
    let val_before = s
        .visible_rows()
        .iter()
        .find(|r| r.key == "a")
        .unwrap()
        .value
        .clone();
    s.nudge(1);
    let val_after = s
        .visible_rows()
        .iter()
        .find(|r| r.key == "a")
        .unwrap()
        .value
        .clone();
    assert_eq!(val_before, val_after, "value unchanged");
    assert!(has_locked_status(&s));
}

// ---- 4. remark ----

#[test]
fn remark_locked_while_clipboard_armed() {
    let mut s = armed_session();
    s.remark();
    assert!(has_locked_status(&s));
}

// ---- 5. begin_inline_edit / begin_external_edit / begin_inline_rename ----

#[test]
fn begin_inline_edit_locked_while_clipboard_armed() {
    let mut s = armed_session();
    s.begin_inline_edit();
    assert!(!matches!(s.mode, Mode::Edit(_)), "must not enter Edit mode");
    assert!(has_locked_status(&s));
}

#[test]
fn begin_external_edit_locked_while_clipboard_armed() {
    let mut s = armed_session();
    s.apply(Intent::BeginEditExternal);
    assert!(
        s.pending_external_edit.is_none(),
        "must not set pending external edit"
    );
    assert!(has_locked_status(&s));
}

#[test]
fn begin_inline_rename_locked_while_clipboard_armed() {
    let mut s = armed_session();
    s.begin_inline_rename();
    assert!(!matches!(s.mode, Mode::Edit(_)), "must not enter Edit mode");
    assert!(has_locked_status(&s));
}

// ---- 6. open_kind_switch ----

#[test]
fn open_kind_switch_locked_while_clipboard_armed() {
    let mut s = armed_session();
    s.open_kind_switch();
    assert!(
        !matches!(s.mode, Mode::KindSwitch(_)),
        "must not enter KindSwitch"
    );
    assert!(has_locked_status(&s));
}

// ---- 7. open_convert ----

#[test]
fn open_convert_locked_while_clipboard_armed() {
    let mut s = armed_session();
    // Move cursor to root for convert
    s.cursor = vec![];
    s.open_convert();
    assert!(
        !matches!(s.mode, Mode::Convert(_)),
        "must not enter Convert"
    );
    assert!(has_locked_status(&s));
}

// ---- 8. enter_filter / enter_type_filter ----

#[test]
fn enter_filter_locked_while_clipboard_armed() {
    let mut s = armed_session();
    s.enter_filter();
    assert!(!matches!(s.mode, Mode::Filter), "must not enter Filter");
    assert!(has_locked_status(&s));
}

#[test]
fn enter_type_filter_locked_while_clipboard_armed() {
    let mut s = armed_session();
    s.enter_type_filter();
    assert!(
        !matches!(s.mode, Mode::TypeFilter),
        "must not enter TypeFilter"
    );
    assert!(has_locked_status(&s));
}

// ---- 9. undo / redo ----

#[test]
fn undo_locked_while_clipboard_armed() {
    let mut s = armed_session();
    s.undo();
    assert!(has_locked_status(&s));
}

#[test]
fn redo_locked_while_clipboard_armed() {
    let mut s = armed_session();
    s.redo();
    assert!(has_locked_status(&s));
}

// ---- 10. toggle_detail / enter_help ----

#[test]
fn toggle_detail_locked_while_clipboard_armed() {
    let mut s = armed_session();
    s.toggle_detail();
    assert!(!matches!(s.mode, Mode::Detail), "must not enter Detail");
    assert!(has_locked_status(&s));
}

#[test]
fn enter_help_locked_while_clipboard_armed() {
    let mut s = armed_session();
    s.enter_help();
    assert!(!matches!(s.mode, Mode::Help(_)), "must not enter Help");
    assert!(has_locked_status(&s));
}

// ---- 11. move_selection_to ----

#[test]
fn move_selection_to_locked_while_clipboard_armed() {
    let mut s = armed_session_with_table();
    let keys_before: Vec<String> = s.visible_rows().iter().map(|r| r.key.clone()).collect();
    let target = vec![];
    s.move_selection_to(
        vec![vec![Seg::Key("server".into()), Seg::Key("port".into())]],
        target,
        0,
        true,
    );
    let keys_after: Vec<String> = s.visible_rows().iter().map(|r| r.key.clone()).collect();
    assert_eq!(keys_before, keys_after, "tree unchanged");
    assert!(has_locked_status(&s));
}

// ---- 12. toggle_expand STILL works (allowed invariant) ----

#[test]
fn toggle_expand_allowed_while_clipboard_armed() {
    let doc = AnyDocument::from_str_as("[server]\nport = 8080\n", DocFormat::Toml).unwrap();
    let mut s = Session::new(doc);
    s.cursor_down(); // on "server"
    s.copy_selected();
    assert!(s.clipboard.is_some());
    // server is collapsed by default; toggle should expand it
    s.toggle_expand();
    assert!(
        s.visible_rows().len() >= 3,
        "expand must succeed: root + server + port"
    );
}

// ---- 13. commit_kind ----

#[test]
fn commit_kind_locked_while_clipboard_armed() {
    let mut s = armed_session();
    s.commit_kind(
        vec![Seg::Key("a".into())],
        confy_core::model::document::KindTarget::StringBasic,
    );
    assert!(has_locked_status(&s));
}

// ---- 14. commit_edit ----

#[test]
fn commit_edit_locked_while_clipboard_armed() {
    let mut s = armed_session();
    s.commit_edit(Some("new_value".into()), None);
    assert!(has_locked_status(&s));
}
