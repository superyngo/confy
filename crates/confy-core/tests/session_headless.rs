/// Headless Session scripted-Intent tests (§7 exit gate #4).
/// These run entirely in confy-core with no TUI or filesystem dependency.
use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::{ConfigDocument, DocFormat};
use confy_core::model::node::Seg;
use confy_core::session::{EditKind, EditTextOutcome, Host, Mode, Session};

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
    let doc =
        AnyDocument::from_str_as("{\n  \"a\": 1,\n  \"b\": \"x\"\n}\n", DocFormat::Json).unwrap();
    let mut s = Session::new(doc);
    s.expand_all();
    let k = keys(&s);
    assert!(k.iter().any(|k| k == "a"), "a visible: {k:?}");
    assert!(k.iter().any(|k| k == "b"), "b visible: {k:?}");
}

#[test]
fn session_works_with_yaml_backend() {
    let doc = AnyDocument::from_str_as("a: 1\nb: x\n", DocFormat::Yaml).unwrap();
    let s = Session::new(doc);
    let k = keys(&s);
    assert!(k.iter().any(|k| k == "a"), "a visible: {k:?}");
}

// ---- Fake Host $EDITOR flow (§7 exit gate #5) ----
// Proves the multi-line / external-edit path is host-agnostic: no real
// `$EDITOR` is spawned and no terminal is touched. The host's only job is the
// `Host::edit_text` callback; everything else is the pure Session API.

/// A fake host that returns a fixed edited string, recording what it was handed.
struct FakeHost {
    edited: String,
    seen: std::cell::RefCell<Option<String>>,
}

impl Host for FakeHost {
    fn edit_text(&self, initial: String) -> EditTextOutcome {
        *self.seen.borrow_mut() = Some(initial);
        EditTextOutcome::Edited(self.edited.clone())
    }
}

#[test]
fn fake_host_multiline_edit_applies_headlessly() {
    // A multi-line basic string routes to External (not inline) editing.
    let src = "notes = \"\"\"\nline1\nline2\n\"\"\"\n";
    let mut s = toml_session(src);
    s.cursor_down(); // cursor lands on `notes`

    // 1. The routing decision is core-side and pure.
    assert_eq!(s.edit_target_kind(), EditKind::External);

    let cursor_path = s.cursor_row_path().expect("cursor on a row");
    // 2. Core resolves the fragment target (no host needed).
    let (path, wrap) = s.external_edit_path(&cursor_path);
    assert!(!wrap, "keyed multiline scalar is not an element wrap");
    let initial = s.doc.as_ref().unwrap().serialize_fragment(&path);

    // 3. The host callback — the only touch of the outside world.
    let host = FakeHost {
        edited: "notes = \"\"\"\nEDITED\n\"\"\"\n".to_string(),
        seen: std::cell::RefCell::new(None),
    };
    let outcome = host.edit_text(initial.clone());
    let EditTextOutcome::Edited(edited) = outcome else {
        panic!("fake host should report Edited");
    };
    assert_eq!(host.seen.borrow().as_deref(), Some(initial.as_str()));

    // 4. Core applies the edited fragment.
    s.apply_replace(path, edited);
    assert!(s.error.is_none(), "unexpected error: {:?}", s.error);

    let text = s.serialize().unwrap();
    assert!(text.contains("EDITED"), "edited text landed in doc: {text}");
    assert!(!text.contains("line1"), "old content gone: {text}");
}

#[test]
fn fake_host_cancelled_edit_leaves_doc_untouched() {
    let src = "notes = \"\"\"\nline1\n\"\"\"\n";
    let mut s = toml_session(src);
    s.cursor_down();
    let cursor_path = s.cursor_row_path().unwrap();
    let (path, _) = s.external_edit_path(&cursor_path);

    let host = FakeHost {
        edited: String::new(),
        seen: std::cell::RefCell::new(None),
    };
    let _ = host.edit_text(s.doc.as_ref().unwrap().serialize_fragment(&path));
    // Host cancelled — core never receives an apply, so the doc is unchanged.
    let text = s.serialize().unwrap();
    assert!(text.contains("line1"), "doc untouched on cancel: {text}");
}
