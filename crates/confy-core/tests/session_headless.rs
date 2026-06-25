/// Headless Session scripted-Intent tests (§7 exit gate #4).
/// These run entirely in confy-core with no TUI or filesystem dependency.
use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::{ConfigDocument, DocFormat};
use confy_core::model::node::Seg;
use confy_core::session::{EditKind, EditTextOutcome, Host, Intent, Mode, ModeView, Session};

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
