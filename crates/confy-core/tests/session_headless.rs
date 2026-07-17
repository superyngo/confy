/// Headless Session scripted-Intent tests (§7 exit gate #4).
/// These run entirely in confy-core with no TUI or filesystem dependency.
use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::{ConfigDocument, DocFormat};
use confy_core::model::node::Seg;
use confy_core::session::{
    EditKind, EditTextOutcome, HelpTab, Host, Intent, Mode, ModeView, Session,
};

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

// ---- dispatch(): the WASM command channel (Stage 2, PORTING §8.4) ----

#[test]
fn dispatch_navigation_updates_cursor_in_snapshot() {
    let mut s = toml_session("a = 1\nb = 2\n");
    let snap = s.dispatch(Intent::CursorDown);
    assert_eq!(snap.cursor, vec![Seg::Key("a".into())]);
    // The cursor row is flagged in the snapshot's rows (full-state transport).
    let cursor_row = snap.rows.iter().find(|r| r.is_cursor).unwrap();
    assert_eq!(cursor_row.key.as_str(), "a");
    assert!(matches!(snap.mode, ModeView::Normal));
}

#[test]
fn dispatch_set_cursor_moves_cursor_by_path() {
    let mut s = toml_session("a = 1\nb = 2\nc = 3\n");
    // Row 0 is the root; 'c' is the third leaf.
    let target = s.visible_paths()[3].clone();
    let snap = s.dispatch(Intent::SetCursor(target.clone()));
    assert_eq!(snap.cursor, target);
    let cursor_row = snap.rows.iter().find(|r| r.is_cursor).unwrap();
    assert_eq!(cursor_row.key.as_str(), "c");

    // An out-of-tree path is ignored (cursor unchanged).
    let snap = s.dispatch(Intent::SetCursor(vec![Seg::Key("nope".into())]));
    assert_eq!(snap.cursor, target);
}

#[test]
fn dispatch_toggle_expand_branch_then_collapse() {
    let mut s = toml_session("[a]\nx = 1\n");
    s.dispatch(Intent::CursorDown); // onto branch 'a'
    let snap = s.dispatch(Intent::ToggleExpand);
    // root + a + x once expanded
    assert_eq!(snap.rows.len(), 3);
    let snap = s.dispatch(Intent::CollapseAll);
    assert_eq!(snap.rows.len(), 2);
}

#[test]
fn dispatch_commit_edit_replaces_value() {
    let mut s = toml_session("a = 1\nb = 2\n");
    let a = s.visible_paths()[1].clone();
    s.dispatch(Intent::SetCursor(a));
    let snap = s.dispatch(Intent::CommitEdit {
        value: Some("42".into()),
        name: None,
    });
    assert!(matches!(snap.mode, ModeView::Normal));
    assert!(s.serialize().unwrap().contains("a = 42"), "value replaced");
    assert!(s.serialize().unwrap().contains("b = 2"), "sibling intact");
}

#[test]
fn dispatch_commit_edit_renames_key() {
    let mut s = toml_session("a = 1\n");
    let a = s.visible_paths()[1].clone();
    s.dispatch(Intent::SetCursor(a));
    s.dispatch(Intent::CommitEdit {
        value: None,
        name: Some("renamed".into()),
    });
    let text = s.serialize().unwrap();
    assert!(
        text.contains("renamed = 1"),
        "key renamed, value kept: {text}"
    );
    assert!(!text.contains("a = 1"), "old key gone");
}

#[test]
fn dispatch_commit_edit_renames_key_inside_scope_table() {
    // Regression: a scoped entry's KEY spells only its own segment; the rename
    // segment index must be end-relative (this errored "path not found").
    let mut s = toml_session("[server]\nhost = \"x\"\n");
    s.dispatch(Intent::ExpandAll);
    s.dispatch(Intent::SetCursor(vec![
        Seg::Key("server".into()),
        Seg::Key("host".into()),
    ]));
    let snap = s.dispatch(Intent::CommitEdit {
        value: None,
        name: Some("hostname".into()),
    });
    assert!(
        snap.status.is_none() && snap.error.is_none(),
        "clean rename: status={:?} error={:?}",
        snap.status,
        snap.error
    );
    assert_eq!(s.serialize().unwrap(), "[server]\nhostname = \"x\"\n");
}

#[test]
fn dispatch_commit_edit_renames_branch_key() {
    // Regression: a branch (table) node has no scalar value, so the Web UI's
    // Detail-panel key rename (`CommitEdit { value: None, name: Some(_) }`)
    // must skip the value-replace step instead of trying to reparse an empty
    // value buffer as a scalar (which failed with "invalid value: …").
    let mut s = toml_session("[server]\nhost = \"x\"\n");
    s.dispatch(Intent::SetCursor(vec![Seg::Key("server".into())]));
    let snap = s.dispatch(Intent::CommitEdit {
        value: None,
        name: Some("svc".into()),
    });
    assert!(
        snap.status.is_none() && snap.error.is_none(),
        "clean rename: status={:?} error={:?}",
        snap.status,
        snap.error
    );
    assert_eq!(s.serialize().unwrap(), "[svc]\nhost = \"x\"\n");
}

#[test]
fn dispatch_commit_edit_rename_from_detail_follows_the_node() {
    // Rename changes the node's path identity — the cursor follows it, so a
    // Detail-origin rename lands back in Detail on the renamed node.
    let mut s = toml_session("[server]\nhost = \"x\"\n");
    s.dispatch(Intent::ExpandAll);
    s.dispatch(Intent::SetCursor(vec![
        Seg::Key("server".into()),
        Seg::Key("host".into()),
    ]));
    s.dispatch(Intent::ToggleDetail);
    let snap = s.dispatch(Intent::CommitEdit {
        value: None,
        name: Some("hostname".into()),
    });
    assert!(matches!(snap.mode, ModeView::Detail), "back in Detail");
    assert_eq!(
        s.cursor,
        vec![Seg::Key("server".into()), Seg::Key("hostname".into())],
        "cursor follows the renamed node"
    );
}

#[test]
fn dispatch_commit_edit_from_detail_returns_to_detail() {
    // A panel-origin (Detail-mode) commit returns to Detail so the host's
    // panel stays open, instead of dropping to Normal.
    let mut s = toml_session("a = 1\n");
    let a = s.visible_paths()[1].clone();
    s.dispatch(Intent::SetCursor(a));
    s.dispatch(Intent::ToggleDetail);
    let snap = s.dispatch(Intent::CommitEdit {
        value: Some("2".into()),
        name: None,
    });
    assert!(matches!(snap.mode, ModeView::Detail), "back in Detail");
    assert_eq!(s.serialize().unwrap(), "a = 2\n");
}

#[test]
fn dispatch_commit_edit_failure_is_one_shot() {
    // A retry branch (invalid value) must not leave a dangling Mode::Edit for
    // the pointer host — it cancels, surfaces the message, and returns to Detail.
    let mut s = toml_session("a = 1\n");
    let a = s.visible_paths()[1].clone();
    s.dispatch(Intent::SetCursor(a));
    s.dispatch(Intent::ToggleDetail);
    let snap = s.dispatch(Intent::CommitEdit {
        value: Some("= not toml =".into()),
        name: None,
    });
    assert!(matches!(snap.mode, ModeView::Detail), "no dangling Edit");
    assert!(snap.error.is_some(), "failure surfaced as error");
    assert_eq!(s.serialize().unwrap(), "a = 1\n", "doc untouched");
}

#[test]
fn dispatch_commit_edit_type_change_prompt_from_detail() {
    // Type-changing value commit defers to the TypeChange prompt; both answers
    // resolve back to Detail (never into Mode::Edit — one-shot host).
    let mut s = toml_session("a = 1\n");
    let a = s.visible_paths()[1].clone();

    // 'y' applies and returns to Detail.
    s.dispatch(Intent::SetCursor(a.clone()));
    s.dispatch(Intent::ToggleDetail);
    let snap = s.dispatch(Intent::CommitEdit {
        value: Some("\"str\"".into()),
        name: None,
    });
    assert!(matches!(snap.mode, ModeView::Prompt { .. }), "prompted");
    let snap = s.dispatch(Intent::PromptKey('y'));
    assert!(matches!(snap.mode, ModeView::Detail), "y → back to Detail");
    assert_eq!(s.serialize().unwrap(), "a = \"str\"\n");

    // 'n' cancels, keeps the doc, and still returns to Detail.
    let snap = s.dispatch(Intent::CommitEdit {
        value: Some("true".into()),
        name: None,
    });
    assert!(matches!(snap.mode, ModeView::Prompt { .. }), "prompted");
    let snap = s.dispatch(Intent::PromptKey('n'));
    assert!(matches!(snap.mode, ModeView::Detail), "n → back to Detail");
    assert_eq!(s.serialize().unwrap(), "a = \"str\"\n", "unchanged");
}

#[test]
fn dispatch_commit_edit_type_change_prompt_from_normal_stays_editing_free() {
    // Outside Detail the one-shot rule still applies: 'n' must not restore
    // Mode::Edit (the pointer host has no live editor to show).
    let mut s = toml_session("a = 1\n");
    let a = s.visible_paths()[1].clone();
    s.dispatch(Intent::SetCursor(a));
    let snap = s.dispatch(Intent::CommitEdit {
        value: Some("\"str\"".into()),
        name: None,
    });
    assert!(matches!(snap.mode, ModeView::Prompt { .. }), "prompted");
    let snap = s.dispatch(Intent::PromptKey('n'));
    assert!(
        matches!(snap.mode, ModeView::Normal),
        "n → Normal, not Edit"
    );
    assert_eq!(s.serialize().unwrap(), "a = 1\n", "unchanged");
}

#[test]
fn dispatch_set_trailing_comment_marks_raw_text() {
    // The Web panel sends raw text (no marker); the session must prepend the
    // backend's comment prefix so the result is a valid trailing comment.
    let mut s = toml_session("a = 1\n");
    let a = s.visible_paths()[1].clone();
    let snap = s.dispatch(Intent::SetTrailing {
        path: a,
        comment: Some("hello".into()),
    });
    assert!(snap.error.is_none(), "no error: {:?}", snap.error);
    assert_eq!(s.serialize().unwrap(), "a = 1  # hello\n");

    // Already-marked text is left as-is (no double "# #").
    let mut s = toml_session("a = 1\n");
    let a = s.visible_paths()[1].clone();
    s.dispatch(Intent::SetTrailing {
        path: a,
        comment: Some("# hi".into()),
    });
    assert_eq!(s.serialize().unwrap(), "a = 1  # hi\n");
}

#[test]
fn dispatch_set_trailing_comment_json_and_yaml() {
    // JSONC uses `//`; YAML uses `#` — both normalized from raw text. The leading
    // `//` comment makes this load as JSONC (comments supported).
    let doc = AnyDocument::from_str_as("{\n  // c\n  \"a\": 1\n}\n", DocFormat::Json).unwrap();
    let mut s = Session::new(doc);
    let a = s
        .visible_rows()
        .iter()
        .find(|r| r.key == "a")
        .unwrap()
        .path
        .clone();
    let snap = s.dispatch(Intent::SetTrailing {
        path: a,
        comment: Some("note".into()),
    });
    assert!(snap.error.is_none(), "json no error: {:?}", snap.error);
    assert!(
        s.serialize().unwrap().contains("// note"),
        "json: {}",
        s.serialize().unwrap()
    );

    let doc = AnyDocument::from_str_as("a: 1\n", DocFormat::Yaml).unwrap();
    let mut s = Session::new(doc);
    let a = s.visible_paths()[1].clone();
    let snap = s.dispatch(Intent::SetTrailing {
        path: a,
        comment: Some("note".into()),
    });
    assert!(snap.error.is_none(), "yaml no error: {:?}", snap.error);
    assert_eq!(s.serialize().unwrap(), "a: 1  # note\n");
}

#[test]
fn dispatch_commit_kind_switches_integer_radix() {
    let mut s = toml_session("n = 255\n");
    let n = s.visible_paths()[1].clone();
    let snap = s.dispatch(Intent::CommitKind {
        path: n,
        target: confy_core::model::document::KindTarget::IntHex,
    });
    assert!(matches!(snap.mode, ModeView::Normal));
    assert!(
        s.serialize().unwrap().contains("0xff"),
        "255 → hex 0xff: {}",
        s.serialize().unwrap()
    );
}

#[test]
fn dispatch_edit_inline_scalar_uses_inline_mode() {
    let mut s = toml_session("a = 1\n");
    s.dispatch(Intent::CursorDown); // onto 'a'
    let snap = s.dispatch(Intent::BeginEdit);
    // Single-line scalar routes inline, not external.
    assert!(
        snap.external_edit.is_none(),
        "scalar should route inline, not external"
    );
    assert!(matches!(snap.mode, ModeView::Edit(_)));
}

#[test]
fn dispatch_edit_inline_table_routes_external() {
    // Web-only `dispatch` routes *every* container to the external popup editor
    // (a branch row has no value cell, so an inline one-line repr is uneditable
    // in the pointer UI). An inline table that the TUI would edit inline must
    // signal external_edit here.
    let mut s = toml_session("a = { x = 1 }\n");
    s.dispatch(Intent::CursorDown); // onto 'a' (the inline table)
    let snap = s.dispatch(Intent::BeginEdit);
    assert!(
        snap.external_edit.is_some(),
        "inline table should route to the external popup editor"
    );
}

#[test]
fn dispatch_add_child_forces_child_into_collapsed_branch() {
    // Web `+` / "Add child": a collapsed branch still receives a child. (The TUI
    // `a`/AddNode would append a root sibling here, because the branch is closed.)
    let mut s = toml_session("[server]\nhost = \"localhost\"\n");
    let snap = s.dispatch(Intent::CursorDown); // onto 'server' (collapsed)
    assert_eq!(snap.cursor.len(), 1, "cursor on the [server] table");
    let snap = s.dispatch(Intent::AddChild);
    // The new node is nested *inside* server (path depth 2), not a root sibling.
    assert_eq!(snap.cursor.len(), 2, "new node nested under server");
    assert_eq!(snap.cursor[0], Seg::Key("server".into()));
}

#[test]
fn dispatch_add_sibling_forces_sibling_off_collapsed_branch() {
    // Web "Append sibling": always a sibling, even on a collapsed branch.
    let mut s = toml_session("[server]\nhost = \"localhost\"\n");
    s.dispatch(Intent::CursorDown); // onto 'server'
    let snap = s.dispatch(Intent::AddSibling);
    // The new placeholder is a root-level sibling (path depth 1), not a child.
    assert_eq!(snap.cursor.len(), 1, "new node is a root sibling");
    assert_ne!(snap.cursor[0], Seg::Key("server".into()));
}

#[test]
fn dispatch_multiline_edit_signals_external_edit_then_applies() {
    // The async-host handshake (PORTING §8.2): BeginEdit on a multi-line scalar
    // returns external_edit in the snapshot; the host returns text via
    // ApplyReplace, which resolves the pending edit.
    let mut s = toml_session("notes = \"\"\"\nline1\n\"\"\"\n");
    s.dispatch(Intent::CursorDown); // onto 'notes'
    let snap = s.dispatch(Intent::BeginEdit);
    let ext = snap.external_edit.expect("multiline routes external");
    assert!(ext.initial.contains("line1"));
    let path = match ext.kind {
        confy_core::session::ExternalEditKind::Value { path } => path,
        other => panic!("expected Value, got {other:?}"),
    };
    // Host edits (async modal) and returns the new fragment.
    let edited = "notes = \"\"\"\nEDITED\n\"\"\"\n".to_string();
    let snap = s.dispatch(Intent::ApplyReplace {
        path: path.clone(),
        text: edited,
    });
    assert!(snap.error.is_none(), "apply should succeed");
    assert!(snap.external_edit.is_none(), "pending cleared after apply");
    let text = s.serialize().unwrap();
    assert!(text.contains("EDITED"), "doc reflects edit: {text}");
    assert!(!text.contains("line1"));
}

#[test]
fn dispatch_escape_cancels_pending_external_edit() {
    // The host's multi-line editor Cancel sends Escape; it must discard the
    // pending external edit so the snapshot stops requesting the modal (else the
    // Web UI reopens it forever — the "Cancel does nothing" bug).
    let mut s = toml_session("notes = \"\"\"\nline1\n\"\"\"\n");
    s.dispatch(Intent::CursorDown); // onto 'notes'
    let snap = s.dispatch(Intent::BeginEdit);
    assert!(snap.external_edit.is_some(), "multiline routes external");
    let snap = s.dispatch(Intent::Escape);
    assert!(
        snap.external_edit.is_none(),
        "Escape clears the pending external edit"
    );
    assert!(!s.is_dirty(), "cancel leaves the document untouched");
}

#[test]
fn dispatch_nudge_increments_scalar_via_snapshot() {
    let mut s = toml_session("a = 1\n");
    s.dispatch(Intent::CursorDown);
    let snap = s.dispatch(Intent::Nudge(1));
    let row = snap.rows.iter().find(|r| r.key == "a").unwrap();
    assert_eq!(row.value.as_deref(), Some("2"));
    assert!(snap.is_dirty, "nudge marks the doc dirty");
}

#[test]
fn dispatch_save_clears_dirty_flag() {
    let mut s = toml_session("a = 1\n");
    s.dispatch(Intent::CursorDown);
    s.dispatch(Intent::Nudge(1));
    assert!(s.is_dirty());
    let snap = s.dispatch(Intent::Save);
    assert!(!snap.is_dirty, "Save clears dirty");
    assert_eq!(snap.status.as_deref(), Some("Saved"));
    // The host obtains bytes separately via serialize(); core stays fs-free.
    assert_eq!(s.serialize().unwrap(), "a = 2\n");
}

#[test]
fn dispatch_set_lang_routes_status_text_through_zh_tw_catalog() {
    let mut s = toml_session("a = 1\n");
    s.dispatch(Intent::CursorDown);
    s.dispatch(Intent::Nudge(1));
    // Default lang (en): Save reports the English "Saved" status.
    assert_eq!(s.dispatch(Intent::Save).status.as_deref(), Some("Saved"));
    // Dirty it again, switch to zh-TW, and confirm the SAME status site now
    // resolves through the zh-TW catalog end-to-end via dispatch/SetLang.
    s.dispatch(Intent::Nudge(1));
    let snap = s.dispatch(Intent::SetLang("zh-TW".into()));
    assert_eq!(snap.lang, "zh-TW");
    let snap = s.dispatch(Intent::Save);
    assert_eq!(
        snap.status.as_deref(),
        Some(confy_core::session::tr(
            confy_core::session::Lang::ZhTw,
            "core.save.saved"
        )),
    );
    assert_ne!(snap.status.as_deref(), Some("Saved"));
}

#[test]
fn dispatch_set_lang_ignores_unknown_code() {
    let mut s = toml_session("a = 1\n");
    let snap = s.dispatch(Intent::SetLang("fr".into()));
    // Unrecognized code leaves the current (default) language unchanged.
    assert_eq!(snap.lang, "en");
}

#[test]
fn dispatch_quit_clean_returns_quit_flag() {
    let mut s = toml_session("a = 1\n");
    let snap = s.dispatch(Intent::QuitRequested);
    assert!(snap.quit, "clean doc quits immediately");
}

#[test]
fn dispatch_quit_dirty_enters_prompt_not_quit() {
    let mut s = toml_session("a = 1\n");
    s.dispatch(Intent::CursorDown);
    s.dispatch(Intent::Nudge(1));
    let snap = s.dispatch(Intent::QuitRequested);
    assert!(!snap.quit, "dirty doc does not quit yet");
    assert!(matches!(snap.mode, ModeView::Prompt { .. }));
    // Confirm 'n' stays; confirm 'y' quits.
    let snap = s.dispatch(Intent::PromptKey('y'));
    assert!(snap.quit, "y confirms quit");
}

#[test]
fn dispatch_snapshot_reflects_filter_mode() {
    let mut s = toml_session("a = 1\nbb = 2\n");
    let snap = s.dispatch(Intent::EnterFilter);
    assert!(matches!(snap.mode, ModeView::Filter { .. }));
    let snap = s.dispatch(Intent::FilterChar('b'));
    if let ModeView::Filter { text, .. } = &snap.mode {
        assert_eq!(text, "b");
    } else {
        panic!("still in Filter mode after FilterChar");
    }
}

#[test]
fn dispatch_type_filter_projects_facet_grid_with_cursor() {
    use confy_core::session::{CheckState, TypeFilterRow, TypeFilterView};
    let mut s = toml_session("a = 1\nb = \"x\"\n");
    let snap = s.dispatch(Intent::EnterTypeFilter);
    let grid = match &snap.mode {
        ModeView::TypeFilter(v) => v,
        _ => panic!("expected TypeFilter mode"),
    };
    // The TOML grid has headers and at least one cell row.
    assert!(grid
        .rows
        .iter()
        .any(|r| matches!(r, TypeFilterRow::Header(_))));
    assert!(grid
        .rows
        .iter()
        .any(|r| matches!(r, TypeFilterRow::Cells(_))));
    // Exactly one cell is the cursor, and nothing is checked yet.
    let cells: Vec<_> = grid
        .rows
        .iter()
        .flat_map(|r| match r {
            TypeFilterRow::Cells(cs) => cs.to_vec(),
            _ => vec![],
        })
        .collect();
    assert_eq!(cells.iter().filter(|c| c.is_cursor).count(), 1);
    assert!(cells.iter().all(|c| c.state == CheckState::Off));
    assert!(!grid.active);

    // Toggle the cursor cell: it goes On and the grid reports active.
    let _ = s.dispatch(Intent::TypeFilterToggle);
    let snap = s.dispatch(Intent::EnterTypeFilter);
    let grid = match &snap.mode {
        ModeView::TypeFilter(v) => v,
        _ => panic!("expected TypeFilter mode after toggle"),
    };
    assert!(grid.active);
    let _ = grid as &TypeFilterView; // type in scope
}

#[test]
fn dispatch_clipboard_count_reflects_copy_then_clears() {
    let mut s = toml_session("a = 1\nb = 2\n");
    // Nothing on the clipboard initially.
    assert_eq!(s.snapshot().clipboard_count, None);
    // Select the 'a' row and copy it.
    s.dispatch(Intent::CursorDown);
    s.dispatch(Intent::ToggleSelect);
    let snap = s.dispatch(Intent::CopySelected);
    assert_eq!(snap.clipboard_count, Some(1));
    // Copy (not cut) exposes the source path so the UI can mark it.
    assert!(!snap.clipboard_cut, "copy is not a cut");
    assert_eq!(snap.clipboard_paths, vec![vec![Seg::Key("a".into())]]);
}

#[test]
fn dispatch_clipboard_cut_flag_and_exit_type_filter() {
    let mut s = toml_session("a = 1\nb = 2\n");
    s.dispatch(Intent::ToggleSelect);
    let snap = s.dispatch(Intent::CutSelected);
    assert!(snap.clipboard_cut, "cut sets the cut flag");
    assert_eq!(snap.clipboard_paths.len(), 1);

    // ExitTypeFilter (the `×`/Esc path) clears facets *and* closes the popup.
    s.dispatch(Intent::EnterTypeFilter);
    s.dispatch(Intent::TypeFilterToggle);
    let snap = s.dispatch(Intent::ExitTypeFilter);
    assert!(
        !matches!(
            snap.mode,
            confy_core::session::view::ModeView::TypeFilter(_)
        ),
        "exit closes the popup"
    );
}

#[test]
fn dispatch_paste_retargets_selection_to_pasted_node() {
    // Copy t1.x, then paste it after t2.y (a different table → no collision). The
    // source selection on t1.x is dropped; the freshly-pasted t2.x becomes the
    // cursor and the sole selection.
    let mut s = toml_session("[t1]\nx = 1\n[t2]\ny = 2\n");
    s.dispatch(Intent::ExpandAll);
    // Navigate onto t1.x (root → t1 → x).
    s.dispatch(Intent::CursorDown); // t1
    s.dispatch(Intent::CursorDown); // x
    s.dispatch(Intent::ToggleSelect); // select t1.x
    s.dispatch(Intent::CopySelected);
    s.dispatch(Intent::CursorDown); // t2
    s.dispatch(Intent::CursorDown); // y
    let snap = s.dispatch(Intent::Paste);
    let cursor = snap.rows.iter().find(|r| r.is_cursor).unwrap();
    assert_eq!(cursor.key, "x", "cursor moved onto the pasted node");
    assert_eq!(
        cursor.path,
        vec![Seg::Key("t2".into()), Seg::Key("x".into())],
        "pasted node lives under t2, not t1"
    );
    let selected: Vec<Vec<Seg>> = snap
        .rows
        .iter()
        .filter(|r| r.selected)
        .map(|r| r.path.clone())
        .collect();
    assert_eq!(
        selected,
        vec![vec![Seg::Key("t2".into()), Seg::Key("x".into())]],
        "only the pasted node is selected; source t1.x is deselected"
    );
}

#[test]
fn dispatch_set_trailing_on_scalar_and_branch() {
    // Web `SetTrailing`: set/clear a node's trailing inline comment, on a leaf
    // scalar and on a branch (TOML `[section]` header).
    let mut s = toml_session("[srv]\nport = 8080\n");
    s.dispatch(Intent::ExpandAll);
    // scalar
    let snap = s.dispatch(Intent::SetTrailing {
        path: vec![Seg::Key("srv".into()), Seg::Key("port".into())],
        comment: Some("# http".into()),
    });
    let port = snap
        .rows
        .iter()
        .find(|r| r.key == "port")
        .expect("port row");
    assert_eq!(port.trailing_comment.as_deref(), Some("# http"));
    // branch header
    let snap = s.dispatch(Intent::SetTrailing {
        path: vec![Seg::Key("srv".into())],
        comment: Some("# the server".into()),
    });
    let srv = snap.rows.iter().find(|r| r.key == "srv").expect("srv row");
    assert_eq!(srv.trailing_comment.as_deref(), Some("# the server"));
    assert!(s.serialize().unwrap().contains("[srv]  # the server"));
    // clear the branch comment again
    let snap = s.dispatch(Intent::SetTrailing {
        path: vec![Seg::Key("srv".into())],
        comment: None,
    });
    let srv = snap.rows.iter().find(|r| r.key == "srv").expect("srv row");
    assert_eq!(srv.trailing_comment, None);
}

// ---- Pointer selection (SetSelection) ----

#[test]
fn dispatch_set_selection_replaces_and_follows_focal() {
    let mut s = toml_session("a = 1\nb = 2\nc = 3\n");
    let pa = vec![Seg::Key("a".into())];
    let pc = vec![Seg::Key("c".into())];
    let snap = s.dispatch(Intent::SetSelection {
        paths: vec![pa, pc],
    });
    let sel: Vec<String> = snap
        .rows
        .iter()
        .filter(|r| r.selected)
        .map(|r| r.key.clone())
        .collect();
    assert_eq!(sel, vec!["a".to_string(), "c".to_string()]);
    // Cursor follows the focal (last) path.
    assert_eq!(snap.rows.iter().find(|r| r.is_cursor).unwrap().key, "c");
    // A fresh SetSelection replaces rather than unions.
    let snap = s.dispatch(Intent::SetSelection {
        paths: vec![vec![Seg::Key("b".into())]],
    });
    let sel: Vec<String> = snap
        .rows
        .iter()
        .filter(|r| r.selected)
        .map(|r| r.key.clone())
        .collect();
    assert_eq!(sel, vec!["b".to_string()]);
}

#[test]
fn dispatch_set_selection_drops_nonvisible_paths() {
    let mut s = toml_session("a = 1\nb = 2\n");
    let snap = s.dispatch(Intent::SetSelection {
        paths: vec![vec![Seg::Key("a".into())], vec![Seg::Key("nope".into())]],
    });
    let sel: Vec<String> = snap
        .rows
        .iter()
        .filter(|r| r.selected)
        .map(|r| r.key.clone())
        .collect();
    assert_eq!(sel, vec!["a".to_string()]);
}

// ---- Pointer drag-reparent (MoveSelectionTo) ----

#[test]
fn dispatch_move_selection_reparents_node() {
    let mut s = toml_session("a = 1\n[t]\nx = 2\n");
    let snap = s.dispatch(Intent::MoveSelectionTo {
        sources: vec![vec![Seg::Key("a".into())]],
        target: vec![Seg::Key("t".into())],
        index: 0,
    });
    assert!(
        snap.error.is_none(),
        "move should succeed: {:?}",
        snap.error
    );
    let text = s.serialize().unwrap();
    let t_at = text.find("[t]").unwrap();
    let a_at = text.find("a = 1").unwrap();
    assert!(a_at > t_at, "'a' reparented under [t]:\n{text}");
}

#[test]
fn dispatch_move_selection_rejects_drop_into_own_subtree() {
    let mut s = toml_session("[t]\nx = 2\n");
    let before = s.serialize().unwrap();
    let snap = s.dispatch(Intent::MoveSelectionTo {
        sources: vec![vec![Seg::Key("t".into())]],
        target: vec![Seg::Key("t".into()), Seg::Key("x".into())],
        index: 0,
    });
    assert!(
        snap.error.is_some(),
        "drop into own subtree must be rejected"
    );
    assert_eq!(s.serialize().unwrap(), before, "document untouched");
}

#[test]
fn dispatch_move_selection_failure_does_not_arm_cut_clipboard() {
    // Regression: a failed drag-move reuses do_paste, whose failure contract
    // restores the (synthetic, cut:true) clipboard — leaving the UI armed in
    // paste-cut mode after a bad drop. The drag must not touch the clipboard.
    let mut s = toml_session("a = 1\nb = 2\n");
    let snap = s.dispatch(Intent::MoveSelectionTo {
        sources: vec![vec![Seg::Key("a".into())]],
        target: vec![Seg::Key("b".into())], // scalar parent → illegal destination
        index: 0,
    });
    assert!(snap.error.is_some(), "move into a scalar must fail");
    assert!(
        snap.clipboard_count.is_none(),
        "failed drag must not arm the clipboard (got cut={})",
        snap.clipboard_cut
    );
}

#[test]
fn dispatch_move_selection_reorders_within_parent() {
    // Move 'a' to AFTER 'b' (b is sibling index 1, so "after" = original index 2).
    // Core adjusts for the removed earlier sibling → b, a, c.
    let mut s = toml_session("a = 1\nb = 2\nc = 3\n");
    s.dispatch(Intent::MoveSelectionTo {
        sources: vec![vec![Seg::Key("a".into())]],
        target: vec![],
        index: 2,
    });
    let t = s.serialize().unwrap();
    assert!(
        t.find("b = 2").unwrap() < t.find("a = 1").unwrap()
            && t.find("a = 1").unwrap() < t.find("c = 3").unwrap(),
        "reordered to b, a, c:\n{t}"
    );
}

#[test]
fn dispatch_move_selection_down_keeps_selection_on_moved_node() {
    // Regression: a same-parent DOWNWARD move shifts the landing slot up by the
    // removed earlier source, so the post-move selection/cursor must follow the
    // moved node — not land on the next row.
    let mut s = toml_session("a = 1\nb = 2\nc = 3\n");
    let snap = s.dispatch(Intent::MoveSelectionTo {
        sources: vec![vec![Seg::Key("a".into())]],
        target: vec![],
        index: 2, // after 'b' → order becomes b, a, c
    });
    assert!(
        snap.error.is_none(),
        "move should succeed: {:?}",
        snap.error
    );
    assert_eq!(
        snap.cursor,
        vec![Seg::Key("a".into())],
        "cursor stays on the moved node 'a', not the next row"
    );
    let row_a = snap.rows.iter().find(|r| r.key == "a").unwrap();
    assert!(
        row_a.is_cursor && row_a.selected,
        "'a' is cursor + selected"
    );
    let row_c = snap.rows.iter().find(|r| r.key == "c").unwrap();
    assert!(
        !row_c.is_cursor && !row_c.selected,
        "the next row 'c' is neither cursor nor selected"
    );
}

#[test]
fn dispatch_move_comment_down_keeps_selection_on_moved_comment() {
    // Regression: a DOWNWARD move of a *comment* node shifted the landing slot
    // up by the removed comment too, but the selection only accounted for node
    // sources — so the moved comment's next row got selected/cursored.
    let mut s = toml_session("# note\na = 1\nb = 2\n");
    // The comment is positional index 0; move it down to after 'b' (index 2).
    let snap = s.dispatch(Intent::MoveSelectionTo {
        sources: vec![vec![Seg::Index(0)]],
        target: vec![],
        index: 2,
    });
    assert!(
        snap.error.is_none(),
        "move should succeed: {:?}",
        snap.error
    );
    // Order is now a, # note, b — the comment landed at index 1 (cursor + select).
    let cur = snap.rows.iter().find(|r| r.is_cursor).unwrap();
    assert!(
        cur.key.contains("note"),
        "cursor stays on the moved comment, not 'b': cursor on {:?}",
        cur.key
    );
    assert!(cur.selected, "the moved comment is selected");
    let row_b = snap.rows.iter().find(|r| r.key == "b").unwrap();
    assert!(
        !row_b.is_cursor && !row_b.selected,
        "the next row 'b' is neither cursor nor selected"
    );
}

#[test]
fn dispatch_move_comment_into_collapsed_table_lands_inside() {
    // Regression (touch drop-into a closed [table] that is NOT the last table):
    // the comment must project as a CHILD of the table, not as a root sibling
    // sitting after it. The "into" drop appends at index = child_count.
    let mut s = toml_session("# note\n[t]\nx = 2\n[u]\nz = 9\n");
    let snap = s.dispatch(Intent::MoveSelectionTo {
        sources: vec![vec![Seg::Index(0)]],
        target: vec![Seg::Key("t".into())],
        index: 1, // child_count of [t]
    });
    assert!(
        snap.error.is_none(),
        "move should succeed: {:?}",
        snap.error
    );
    // The comment is now a child of [t] (path starts with Key("t")), depth 2.
    s.expand_all();
    let rows = s.visible_rows();
    let note = rows
        .iter()
        .find(|r| r.key.contains("note"))
        .expect("comment row visible");
    assert_eq!(
        note.path.first(),
        Some(&Seg::Key("t".into())),
        "comment is a child of [t], not a root sibling: path={:?}",
        note.path
    );
    assert!(
        note.depth >= 2,
        "comment nested under [t]: depth={}",
        note.depth
    );
    // A blank line was inserted so the projection keeps it inside [t].
    let text = s.serialize().unwrap();
    assert!(
        text.contains("x = 2\n# note\n\n[u]"),
        "blank line separates the trailing comment from [u]:\n{text}"
    );
}

#[test]
fn project_blank_line_decides_comment_owner_before_header() {
    // A comment separated from the next header by a blank line trails the
    // preceding table; a comment hugging the header leads the next section.
    let mut s = toml_session("[t]\nx = 1\n# trailing of t\n\n# leading of u\n[u]\nz = 2\n");
    s.expand_all();
    let rows = s.visible_rows();
    let trailing = rows.iter().find(|r| r.key.contains("trailing")).unwrap();
    assert_eq!(
        trailing.path.first(),
        Some(&Seg::Key("t".into())),
        "blank-separated comment trails [t]: {:?}",
        trailing.path
    );
    let leading = rows.iter().find(|r| r.key.contains("leading")).unwrap();
    assert_eq!(
        leading.path.len(),
        1,
        "header-hugging comment stays at root (leads [u]): {:?}",
        leading.path
    );
}

// ---- Pointer filter (SetFilter) ----

#[test]
fn dispatch_set_filter_narrows_then_clears() {
    let mut s = toml_session("alpha = 1\nbeta = 2\n");
    let snap = s.dispatch(Intent::SetFilter("alph".into()));
    assert!(matches!(snap.mode, ModeView::FilterResults));
    let k: Vec<String> = snap.rows.iter().map(|r| r.key.clone()).collect();
    assert!(k.iter().any(|x| x == "alpha"));
    assert!(!k.iter().any(|x| x == "beta"), "beta filtered out: {k:?}");
    // Clearing restores all rows and drops back to Normal.
    let snap = s.dispatch(Intent::SetFilter(String::new()));
    assert!(matches!(snap.mode, ModeView::Normal));
    let k: Vec<String> = snap.rows.iter().map(|r| r.key.clone()).collect();
    assert!(k.iter().any(|x| x == "beta"), "beta back: {k:?}");
}

#[test]
fn dispatch_set_filter_matches_value_not_just_key() {
    let mut s = toml_session("host = \"localhost\"\nport = 8080\n");
    // "localhost" lives only in a value, not a key — the filter must still find it.
    let snap = s.dispatch(Intent::SetFilter("localhost".into()));
    assert!(matches!(snap.mode, ModeView::FilterResults));
    let k: Vec<String> = snap.rows.iter().map(|r| r.key.clone()).collect();
    assert!(
        k.iter().any(|x| x == "host"),
        "host kept by value match: {k:?}"
    );
    assert!(!k.iter().any(|x| x == "port"), "port filtered out: {k:?}");
}

// ---- Pointer convert (SetConvertFormat / SetConvertPath) ----

#[test]
fn dispatch_set_convert_format_seeds_path() {
    let mut s = toml_session("a = 1\n");
    s.dispatch(Intent::SetCursor(vec![]));
    s.dispatch(Intent::OpenConvert);
    let snap = s.dispatch(Intent::SetConvertFormat(DocFormat::Json));
    match snap.mode {
        ModeView::Convert(cv) => {
            assert_eq!(cv.target, DocFormat::Json);
            assert!(cv.path.ends_with(".json"), "path seeded: {}", cv.path);
        }
        m => panic!("expected Convert mode, got {m:?}"),
    }
}

#[test]
fn dispatch_set_convert_path_then_run_writes() {
    let mut s = toml_session("a = 1\n");
    s.dispatch(Intent::SetCursor(vec![]));
    s.dispatch(Intent::OpenConvert);
    s.dispatch(Intent::SetConvertFormat(DocFormat::Json));
    s.dispatch(Intent::SetConvertPath("custom.json".into()));
    let snap = s.dispatch(Intent::ConvertRun);
    let (path, text) = snap.convert_write.expect("convert produced a write");
    assert_eq!(path, "custom.json");
    assert!(text.contains("\"a\""), "json output:\n{text}");
}

// ── comment append-sibling: enter inline editor + Esc-cancel (separate node) ──

#[test]
fn add_comment_sibling_enters_inline_edit_and_separates() {
    let mut s = toml_session("# first\nkey = 1\n");
    s.dispatch(Intent::SetCursor(vec![Seg::Index(0)]));
    let snap = s.dispatch(Intent::AddSibling);
    // A fresh, *separate* single-line comment node opens in the inline editor.
    assert!(
        matches!(snap.mode, ModeView::Edit(ref e) if e.is_comment && !e.buffer.contains('\n')),
        "expected inline comment edit, got {:?}",
        snap.mode
    );
    assert_eq!(snap.cursor, vec![Seg::Index(1)]);
    let text = s.serialize().unwrap();
    assert_eq!(
        text, "# first\n\n# \nkey = 1\n",
        "blank-separated new comment"
    );
}

#[test]
fn add_comment_sibling_commit_keeps_it() {
    let mut s = toml_session("# first\nkey = 1\n");
    s.dispatch(Intent::SetCursor(vec![Seg::Index(0)]));
    s.dispatch(Intent::AddSibling);
    s.dispatch(Intent::CommitEdit {
        value: Some("# hello".into()),
        name: None,
    });
    assert_eq!(s.serialize().unwrap(), "# first\n\n# hello\nkey = 1\n");
}

#[test]
fn add_comment_sibling_escape_removes_it() {
    let src = "# first\nkey = 1\n";
    let mut s = toml_session(src);
    s.dispatch(Intent::SetCursor(vec![Seg::Index(0)]));
    s.dispatch(Intent::AddSibling);
    let snap = s.dispatch(Intent::Escape);
    assert!(matches!(snap.mode, ModeView::Normal));
    assert_eq!(
        s.serialize().unwrap(),
        src,
        "Esc reverts the inserted comment"
    );
}

#[test]
fn add_comment_sibling_yaml() {
    let doc = AnyDocument::from_str_as("# c\na: 1\n", DocFormat::Yaml).unwrap();
    let mut s = Session::new(doc);
    s.dispatch(Intent::SetCursor(vec![Seg::Index(0)]));
    let snap = s.dispatch(Intent::AddSibling);
    assert!(matches!(snap.mode, ModeView::Edit(ref e) if e.is_comment));
    assert_eq!(s.serialize().unwrap(), "# c\n\n# \na: 1\n");
    // Esc reverts.
    s.dispatch(Intent::Escape);
    assert_eq!(s.serialize().unwrap(), "# c\na: 1\n");
}

#[test]
fn add_comment_sibling_jsonc() {
    // The `//` line auto-upgrades the JSON doc to JSONC.
    let doc = AnyDocument::from_str_as("{\n  // c\n  \"a\": 1\n}\n", DocFormat::Json).unwrap();
    let mut s = Session::new(doc);
    let cpath = s
        .visible_rows()
        .iter()
        .find(|r| r.key.starts_with("//"))
        .map(|r| r.path.clone())
        .expect("comment row");
    s.dispatch(Intent::SetCursor(cpath));
    let snap = s.dispatch(Intent::AddSibling);
    assert!(matches!(snap.mode, ModeView::Edit(ref e) if e.is_comment));
    // Two distinct comment rows now (separate nodes, not merged).
    let comment_rows = s
        .visible_rows()
        .iter()
        .filter(|r| r.key.starts_with("//"))
        .count();
    assert_eq!(comment_rows, 2);
    // Esc reverts to the original document.
    s.dispatch(Intent::Escape);
    assert_eq!(s.serialize().unwrap(), "{\n  // c\n  \"a\": 1\n}\n");
}

#[test]
fn enter_help_defaults_to_help_tab_and_toggle_flips_to_about() {
    let mut s = toml_session("a = 1\n");
    s.dispatch(Intent::EnterHelp);
    assert!(matches!(s.mode, Mode::Help(HelpTab::Help)));
    s.dispatch(Intent::ToggleHelpTab);
    assert!(matches!(s.mode, Mode::Help(HelpTab::About)));
    s.dispatch(Intent::ToggleHelpTab);
    assert!(matches!(s.mode, Mode::Help(HelpTab::Help)));
}

#[test]
fn dispatch_snapshot_carries_help_tab() {
    let mut s = toml_session("a = 1\n");
    let snap = s.dispatch(Intent::EnterHelp);
    assert!(matches!(snap.mode, ModeView::Help { tab: HelpTab::Help }));
    let snap = s.dispatch(Intent::ToggleHelpTab);
    assert!(matches!(
        snap.mode,
        ModeView::Help {
            tab: HelpTab::About
        }
    ));
}

#[test]
fn toggle_help_tab_is_noop_outside_help_mode() {
    let mut s = toml_session("a = 1\n");
    s.dispatch(Intent::ToggleHelpTab);
    assert!(matches!(s.mode, Mode::Normal));
}

#[test]
fn escape_exits_help_from_either_tab() {
    let mut s = toml_session("a = 1\n");
    s.dispatch(Intent::EnterHelp);
    s.dispatch(Intent::ToggleHelpTab);
    s.dispatch(Intent::Escape);
    assert!(matches!(s.mode, Mode::Normal));
}

// ---- RevealPath (the "Reveal" operation — breadcrumb mini-tree jump) ----

#[test]
fn reveal_path_expands_ancestors_and_sets_cursor() {
    let mut s = toml_session("[a]\n[a.b]\nx = 1\n");
    // Everything starts collapsed: only root + `a` are visible.
    let target = vec![
        Seg::Key("a".into()),
        Seg::Key("b".into()),
        Seg::Key("x".into()),
    ];
    s.dispatch(Intent::RevealPath(target.clone()));
    let rows = s.visible_rows();
    let cursor_row = rows.iter().find(|r| r.is_cursor).unwrap();
    assert_eq!(cursor_row.path, target);
}

#[test]
fn reveal_path_ignores_unknown_path() {
    let mut s = toml_session("a = 1\n");
    let before = s.visible_rows().len();
    let snap = s.dispatch(Intent::RevealPath(vec![Seg::Key("nope".into())]));
    assert_eq!(s.visible_rows().len(), before, "no expansion happened");
    assert!(snap.status.is_none(), "unknown path is a silent no-op");
    let rows = s.visible_rows();
    let cursor_row = rows.iter().find(|r| r.is_cursor).unwrap();
    assert_eq!(cursor_row.key, "", "cursor stays on root");
}

#[test]
fn reveal_path_hidden_by_filter_expands_and_reports() {
    let mut s = toml_session("port = 8080\n[a]\nx = 1\n");
    s.dispatch(Intent::SetFilter("port".into()));
    // `a.x` exists but the filter hides it: expansion sticks, cursor doesn't
    // move onto it, and the status line says so (grilled decision Q4/C).
    let snap = s.dispatch(Intent::RevealPath(vec![
        Seg::Key("a".into()),
        Seg::Key("x".into()),
    ]));
    let rows = s.visible_rows();
    let cursor_row = rows.iter().find(|r| r.is_cursor).unwrap();
    assert_ne!(cursor_row.key, "x");
    assert!(
        snap.status.is_some(),
        "hidden-by-filter must report on the status line"
    );
}

// ---- children_of (breadcrumb mini-tree lazy query) ----

#[test]
fn children_of_lists_children_of_a_collapsed_branch() {
    let s = toml_session("[a]\nx = 1\ny = 2\n");
    // `a` is collapsed — children_of must not depend on expansion state.
    let kids = s.children_of(&vec![Seg::Key("a".into())]);
    assert_eq!(kids.len(), 2);
    assert_eq!(kids[0].key, "x");
    assert_eq!(kids[0].type_label, "integer");
    assert!(!kids[0].is_branch);
    assert_eq!(
        kids[1].path,
        vec![Seg::Key("a".into()), Seg::Key("y".into())]
    );
    // Unknown path → empty, never a panic.
    assert!(s.children_of(&vec![Seg::Key("nope".into())]).is_empty());
}

#[test]
fn children_of_includes_comments() {
    // Grilled decision Q3/A: the mini-tree shows the same node set as the main
    // tree — a Comment is a first-class child.
    let s = toml_session("# note\na = 1\n");
    let kids = s.children_of(&Vec::new());
    assert_eq!(kids.len(), 2);
    assert_eq!(kids[0].type_label, "comment");
}
