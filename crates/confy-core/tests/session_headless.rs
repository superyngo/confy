/// Headless Session scripted-Intent tests (§7 exit gate #4).
/// These run entirely in confy-core with no TUI or filesystem dependency.
use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::DocFormat;
use confy_core::session::{Session, Mode, EditKind};
use confy_core::model::node::Seg;

fn toml_session(src: &str) -> Session {
    let doc = AnyDocument::from_str_as(src, DocFormat::Toml).unwrap();
    Session::new(doc)
}

fn keys(s: &Session) -> Vec<String> {
    s.visible_rows().iter().map(|r| r.key.clone()).collect()
}

// ---- Navigation ----

#[test]
fn cursor_down_advances_to_next_row() {
    let mut s = toml_session("a = 1\nb = 2\n");
    // rows: [root(key=""), a, b]
    s.cursor_down(); // on 'a'
    s.cursor_down(); // on 'b'
    let rows = s.visible_rows();
    let cursor_row = rows.iter().find(|r| r.is_cursor).unwrap();
    assert_eq!(cursor_row.key.as_str(), "b");
}

#[test]
fn expand_collapse_works_headlessly() {
    let mut s = toml_session("[a]\nx = 1\n");
    assert_eq!(s.visible_rows().len(), 2, "before expand: root + a");
    s.cursor_down(); // on 'a'
    s.toggle_expand();
    assert_eq!(s.visible_rows().len(), 3, "after expand: root, a, x");
    s.collapse_all();
    assert_eq!(s.visible_rows().len(), 2);
}

// ---- Filter ----

#[test]
fn filter_narrows_visible_rows() {
    let mut s = toml_session("port = 8080\nhost = \"localhost\"\n");
    s.enter_filter();
    for c in "port".chars() {
        s.filter_char(c);
    }
    let k = keys(&s);
    assert!(k.iter().any(|k| k == "port"), "port visible: {k:?}");
    assert!(!k.iter().any(|k| k == "host"), "host filtered: {k:?}");
}

// ---- Mutations via apply_replace ----

#[test]
fn apply_replace_changes_doc() {
    let mut s = toml_session("port = 8080\n");
    let path = vec![Seg::Key("port".into())];
    s.apply_replace(path, "port = 9090\n".into());
    assert!(s.error.is_none(), "unexpected error: {:?}", s.error);
    let text = s.serialize().unwrap();
    assert!(text.contains("9090"), "new value in doc: {text}");
}

// ---- Undo / Redo ----

#[test]
fn undo_redo_cycle() {
    let mut s = toml_session("a = 1\n");
    let path = vec![Seg::Key("a".into())];
    s.apply_replace(path, "a = 2\n".into());
    assert!(s.serialize().unwrap().contains("a = 2"));
    s.undo();
    assert!(s.serialize().unwrap().contains("a = 1"), "undo restored");
    s.redo();
    assert!(s.serialize().unwrap().contains("a = 2"), "redo re-applied");
}

// ---- Edit kind routing ----

#[test]
fn edit_target_kind_inline_for_simple_scalar() {
    let mut s = toml_session("port = 8080\n");
    s.cursor_down(); // on 'port'
    assert_eq!(s.edit_target_kind(), EditKind::Inline);
}

#[test]
fn edit_target_kind_external_for_root() {
    let s = toml_session("port = 8080\n");
    // cursor is on root (default)
    assert_eq!(s.edit_target_kind(), EditKind::External);
}

// ---- Quit flow ----

#[test]
fn quit_requested_returns_true_when_clean() {
    let mut s = toml_session("a = 1\n");
    assert!(s.quit_requested(), "clean doc quits immediately");
}

#[test]
fn quit_requested_prompts_when_dirty() {
    let mut s = toml_session("a = 1\n");
    let path = vec![Seg::Key("a".into())];
    s.apply_replace(path, "a = 99\n".into());
    assert!(!s.quit_requested(), "dirty doc shows prompt");
    assert!(matches!(s.mode, Mode::Prompt(_)));
    let quit = s.handle_prompt_key('y');
    assert!(quit, "y confirms quit");
}

// ---- visible_rows bakes in selection + cursor ----

#[test]
fn visible_rows_marks_cursor_and_selection() {
    let mut s = toml_session("a = 1\nb = 2\n");
    s.cursor_down(); // cursor on 'a'
    s.toggle_select(); // select 'a'
    s.cursor_down(); // cursor on 'b'
    let rows = s.visible_rows();
    let a_row = rows.iter().find(|r| r.key == "a").unwrap();
    let b_row = rows.iter().find(|r| r.key == "b").unwrap();
    assert!(a_row.selected, "a should be selected");
    assert!(!a_row.is_cursor, "a is not the cursor");
    assert!(!b_row.selected, "b not selected");
    assert!(b_row.is_cursor, "b is the cursor");
}

// ---- Copy / cut ----

#[test]
fn copy_selected_loads_clipboard() {
    let mut s = toml_session("a = 1\nb = 2\n");
    s.cursor_down(); // on 'a'
    s.copy_selected();
    assert!(s.clipboard.is_some(), "clipboard loaded");
    let cb = s.clipboard.as_ref().unwrap();
    assert!(!cb.cut);
    assert!(!cb.fragments.is_empty());
}

// ---- visible_rows across all 3 backends ----

#[test]
fn session_works_with_json_backend() {
    let doc = AnyDocument::from_str_as(
        "{\n  \"a\": 1,\n  \"b\": \"x\"\n}\n",
        DocFormat::Json,
    ).unwrap();
    let mut s = Session::new(doc);
    s.expand_all();
    let k = keys(&s);
    assert!(k.iter().any(|k| k == "a"), "a visible: {k:?}");
    assert!(k.iter().any(|k| k == "b"), "b visible: {k:?}");
}

#[test]
fn session_works_with_yaml_backend() {
    let doc = AnyDocument::from_str_as(
        "a: 1\nb: x\n",
        DocFormat::Yaml,
    ).unwrap();
    let s = Session::new(doc);
    let k = keys(&s);
    assert!(k.iter().any(|k| k == "a"), "a visible: {k:?}");
}
