/// Headless Session scripted-Intent tests (§7 exit gate #4).
/// These run entirely in confy-core with no TUI or filesystem dependency.
use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::{ConfigDocument, DocFormat};
use confy_core::model::node::{Format, Seg};
use confy_core::session::{
    EditKind, EditTextOutcome, HelpTab, Host, Intent, Mode, ModeView, PasteSlot, PromptKind,
    Session,
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

#[test]
fn reverse_type_filter_prunes_excluded_container_subtree() {
    // Reverse + Table facet must hide the whole `[server]` subtree (table +
    // its `port` child), not just fail to match the table itself while a
    // scalar-vs-comment reversal keeps working because leaves have no
    // children to resurrect them via ancestor-context.
    use confy_core::session::type_filter::TypeToken;
    let mut s = toml_session("debug = true\n[server]\nport = 8080\n");
    s.type_filter.types.insert(TypeToken::TableScope);
    s.type_filter.reverse = true;
    s.recompute_filter();
    let fp = s.filtered_paths.clone().unwrap();
    let server_path = vec![Seg::Key("server".into())];
    let port_path = vec![Seg::Key("server".into()), Seg::Key("port".into())];
    let debug_path = vec![Seg::Key("debug".into())];
    assert!(
        !fp.contains(&server_path),
        "excluded table itself stays hidden"
    );
    assert!(
        !fp.contains(&port_path),
        "table's own child must not resurrect the excluded table as ancestor-context"
    );
    assert!(
        fp.contains(&debug_path),
        "root-level scalar outside the excluded table is unaffected"
    );
}

// ---- Mutations via apply_replace ----

#[test]
fn apply_replace_changes_doc() {
    let mut s = toml_session("port = 8080\n");
    let path = vec![Seg::Key("port".into())];
    s.apply_replace(path, "port = 9090\n".into());
    assert!(
        s.snapshot().error_text().is_none(),
        "unexpected error: {:?}",
        s.snapshot().error_text()
    );
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

// ---- cursor_row / view_row_at (O(depth) single-row lookup, Task 9) ----

#[test]
fn cursor_row_matches_full_scan_lookup() {
    let mut s = toml_session("a = 1\nb = 2\nc = 3\n");
    s.cursor_down();
    s.cursor_down(); // cursor on 'b'
    let scanned = s.visible_rows().into_iter().find(|r| r.is_cursor).unwrap();
    let direct = s.cursor_row().expect("cursor row found");
    assert_eq!(direct.path, scanned.path);
    assert_eq!(direct.key, scanned.key);
    assert_eq!(direct.value, scanned.value);
    assert!(direct.is_cursor);
}

#[test]
fn cursor_row_tracks_cursor_across_a_mutation() {
    let mut s = toml_session("a = 1\nb = 2\nc = 3\n");
    s.cursor_down();
    s.cursor_down();
    s.cursor_down(); // cursor on 'c'
    assert_eq!(s.cursor_row().unwrap().key, "c");
    let dead: Vec<Seg> = vec![Seg::Key("c".into())];
    // deletes 'c' (empty selection targets the cursor)
    s.delete_selected();
    // `delete_selected` now snaps the cursor to the deletion point itself, so
    // the cursor is live immediately rather than dangling until the host calls
    // `compute_rows()`. 'c' was the last row, so the snap clamps to 'b'.
    let after = s.cursor_row().expect("cursor snapped to a visible row");
    assert_eq!(after.key, "b", "cursor clamped to the new last row");
    // The invariant this test exists for: a lookup of the just-deleted path
    // must report absence rather than fabricating a stale row.
    assert!(
        s.view_row_at(&dead).is_none(),
        "no false positive for the just-deleted path"
    );
    s.compute_rows(); // hosts still call this after every mutation
    let after = s.cursor_row().expect("cursor still on a visible row");
    assert_ne!(after.key, "c", "deleted row is gone");
    // No staleness: matches a fresh full scan of the post-snap tree.
    let scanned = s.visible_rows().into_iter().find(|r| r.is_cursor).unwrap();
    assert_eq!(after.path, scanned.path);
}

#[test]
fn view_row_at_returns_none_for_a_collapsed_path() {
    let mut s = toml_session("[a]\nx = 1\n");
    s.cursor_down(); // cursor on 'a'
    s.toggle_expand(); // reveal x
    let x_path = s
        .visible_rows()
        .into_iter()
        .find(|r| r.key == "x")
        .unwrap()
        .path;
    assert!(
        s.view_row_at(&x_path).is_some(),
        "x is visible while a is expanded"
    );
    s.toggle_expand(); // collapse 'a' again
    assert!(
        s.view_row_at(&x_path).is_none(),
        "x is hidden once its parent collapses, even though it still exists in the tree"
    );
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
    assert!(
        s.snapshot().error_text().is_none(),
        "unexpected error: {:?}",
        s.snapshot().error_text()
    );

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
fn apply_skips_row_rebuild_dispatch_performs_it() {
    let mut s = toml_session("[a]\nx = 1\n");
    s.dispatch(Intent::ExpandAll);
    s.dispatch(Intent::CursorDown); // a
    s.dispatch(Intent::CursorDown); // x
    let cursor_before = s.cursor.clone();
    assert!(
        s.visible_rows().iter().any(|r| r.path == cursor_before),
        "cursor starts on a visible row"
    );

    // Collapsing 'a' hides 'x'. `apply()` must NOT snap the cursor back onto
    // a visible row -- that's `compute_rows()`'s job, deliberately skipped by
    // the cheap path so navigation-only callers don't pay for it.
    s.apply(Intent::CollapseAll);
    assert_eq!(
        s.cursor, cursor_before,
        "apply() leaves the now-invisible cursor untouched"
    );
    assert!(
        !s.visible_rows().iter().any(|r| r.path == cursor_before),
        "the collapsed child really is hidden"
    );

    // `dispatch()` performs the same mutation plus the row rebuild, which
    // snaps the cursor onto a visible row.
    let mut s2 = toml_session("[a]\nx = 1\n");
    s2.dispatch(Intent::ExpandAll);
    s2.dispatch(Intent::CursorDown);
    s2.dispatch(Intent::CursorDown);
    let snap = s2.dispatch(Intent::CollapseAll);
    assert!(
        snap.rows.iter().any(|r| r.path == s2.cursor),
        "dispatch() snaps the cursor back onto a visible row"
    );
}

#[test]
fn apply_outcome_quit_matches_dispatch_snapshot_quit() {
    // `ApplyOutcome`'s transient signals must carry the exact same values
    // `dispatch()` overlays onto its `SessionSnapshot` -- proven here for
    // `quit`, the simplest of the three (`convert_write`, `schema_fetch_
    // request` share the same overlay code path in `dispatch()`).
    let mut s = toml_session("a = 1\n");
    let outcome = s.apply(Intent::QuitRequested);
    assert!(
        outcome.quit,
        "clean doc quits immediately, same as dispatch()"
    );
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
        snap.status_text().is_none() && snap.error_text().is_none(),
        "clean rename: status={:?} error={:?}",
        snap.status_text(),
        snap.error_text()
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
        snap.status_text().is_none() && snap.error_text().is_none(),
        "clean rename: status={:?} error={:?}",
        snap.status_text(),
        snap.error_text()
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
    // `core.value.invalid` is Warn per the §2.2 severity table (the old
    // status→error one-shot promotion is gone — severity comes from the key).
    assert!(snap.status_text().is_some(), "failure surfaced as a notice");
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
    assert!(
        snap.error_text().is_none(),
        "no error: {:?}",
        snap.error_text()
    );
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
    assert!(
        snap.error_text().is_none(),
        "json no error: {:?}",
        snap.error_text()
    );
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
    assert!(
        snap.error_text().is_none(),
        "yaml no error: {:?}",
        snap.error_text()
    );
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
    assert!(
        matches!(snap.mode, ModeView::AddPicker { .. }),
        "AddChild opens the Add-type picker"
    );
    let snap = s.dispatch(Intent::AddPickerCommit);
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
    assert!(
        matches!(snap.mode, ModeView::AddPicker { .. }),
        "AddSibling opens the Add-type picker"
    );
    let snap = s.dispatch(Intent::AddPickerCommit);
    // The new placeholder is a root-level sibling (path depth 1), not a child.
    assert_eq!(snap.cursor.len(), 1, "new node is a root sibling");
    assert_ne!(snap.cursor[0], Seg::Key("server".into()));
}

// ---- Regression: today's add-child behavior into an AoT group / plain
// array / inline table, spanning the Add-type picker refactor (plan
// `add-node-type-picker-plan.md` step 1). Locked down pre-refactor, then
// updated in step 3/6 once `AddChild`/`AddSibling` route through
// `Mode::AddPicker` (an `AddPickerCommit` now lands the seeded node).
//
// The AoT case caught a real (if minor) pre-existing bug, contrary to the
// plan's initial assumption: `add_node_impl`'s section-ordering clamp (guards
// a scalar landing after a parent's first `[table]`/`[[aot]]` sub-section)
// didn't special-case an `ArrayOfTables` *parent* — inside an AoT group,
// every entry itself projects as a child `Table`, so the clamp's `split`
// (first `Table` child) was always 0, and the intended "append" index (the
// group's child count) got clamped down to 0 every time: the new entry
// landed *first*, not last. `add_picker.rs`'s `insert_seed` fixes this by
// excluding an AoT parent from that clamp — this test now proves the fix
// (the new entry is appended).
// ----

#[test]
fn add_child_into_array_of_tables_group_creates_new_entry_appended() {
    let mut s = toml_session("[[items]]\na = 1\n");
    s.dispatch(Intent::CursorDown); // onto the 'items' AoT group
    s.dispatch(Intent::AddChild);
    s.dispatch(Intent::AddPickerCommit);
    let text = s.serialize().unwrap();
    assert_eq!(
        text, "[[items]]\na = 1\n[[items]]\nnew_field = \"\"\n",
        "the new entry is appended, not prepended: {text:?}"
    );
}

#[test]
fn add_child_into_plain_array_seeds_bare_element() {
    let mut s = toml_session("arr = [1, 2]\n");
    s.dispatch(Intent::CursorDown); // onto 'arr'
    s.dispatch(Intent::AddChild);
    s.dispatch(Intent::AddPickerCommit);
    let text = s.serialize().unwrap();
    assert!(
        text.contains("\"\""),
        "a bare, keyless string element was appended: {text:?}"
    );
}

#[test]
fn add_child_into_inline_table_seeds_keyed_member() {
    let mut s = toml_session("t = { x = 1 }\n");
    s.dispatch(Intent::CursorDown); // onto 't'
    s.dispatch(Intent::AddChild);
    s.dispatch(Intent::AddPickerCommit);
    let text = s.serialize().unwrap();
    assert!(
        text.contains("new_field = \"\""),
        "a keyed member was appended inside the inline table: {text:?}"
    );
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
    assert!(snap.error_text().is_none(), "apply should succeed");
    assert!(snap.external_edit.is_none(), "pending cleared after apply");
    let text = s.serialize().unwrap();
    assert!(text.contains("EDITED"), "doc reflects edit: {text}");
    assert!(!text.contains("line1"));
}

#[test]
fn external_edit_comment_initial_keeps_nested_indent() {
    // The $EDITOR initial for a nested remarked block must be the CST
    // fragment (per-line indent kept). The DOM projection text flattens
    // every line, and an unmodified apply-back used to splice that
    // flattening into the document (quit-without-save corruption).
    let src = "t:\n  # subscribers:\n    # error:\n      # - w@x.com\n";
    let doc = AnyDocument::from_str_as(src, DocFormat::Yaml).unwrap();
    let mut s = Session::new(doc);
    let cpath = vec![Seg::Key("t".into()), Seg::Index(0)];
    s.dispatch(Intent::RevealPath(cpath.clone()));
    let snap = s.dispatch(Intent::BeginEditExternal);
    let ext = snap.external_edit.expect("comment routes external");
    assert_eq!(
        ext.initial, "  # subscribers:\n    # error:\n      # - w@x.com\n",
        "initial must keep per-line indent: {ext:?}"
    );
    // Quitting without saving hands the untouched buffer back; that must
    // not mutate the document.
    s.dispatch(Intent::ApplyEditComment {
        path: cpath,
        text: ext.initial,
    });
    assert_eq!(
        s.serialize().unwrap(),
        src,
        "unmodified apply-back must be a byte-exact no-op"
    );
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
fn dispatch_external_edit_applies_edited_trailing_comment() {
    // Forces the external-edit path (Intent::BeginEditExternal, what the TUI's
    // `E` and the Web's popup-editor button send) on an ordinary scalar that
    // would otherwise edit inline, and proves an edited trailing comment in
    // the host's returned text is applied, not discarded.
    let mut s = toml_session("port = 8080  # http\n");
    s.dispatch(Intent::CursorDown); // onto 'port'
    let snap = s.dispatch(Intent::BeginEditExternal);
    let ext = snap
        .external_edit
        .expect("BeginEditExternal always routes external");
    assert!(
        ext.initial.contains("# http"),
        "initial text shows the trailing comment: {:?}",
        ext.initial
    );
    let path = match ext.kind {
        confy_core::session::ExternalEditKind::Value { path } => path,
        other => panic!("expected Value, got {other:?}"),
    };
    let snap = s.dispatch(Intent::ApplyReplace {
        path,
        text: "port = 9090  # https\n".to_string(),
    });
    assert!(snap.error_text().is_none(), "apply should succeed");
    let text = s.serialize().unwrap();
    assert_eq!(text, "port = 9090  # https\n");
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
    assert_eq!(snap.status_text(), Some("Saved"));
    // The host obtains bytes separately via serialize(); core stays fs-free.
    assert_eq!(s.serialize().unwrap(), "a = 2\n");
}

#[test]
fn dispatch_set_lang_routes_status_text_through_zh_tw_catalog() {
    let mut s = toml_session("a = 1\n");
    s.dispatch(Intent::CursorDown);
    s.dispatch(Intent::Nudge(1));
    // Default lang (en): Save reports the English "Saved" status.
    assert_eq!(s.dispatch(Intent::Save).status_text(), Some("Saved"));
    // Dirty it again, switch to zh-TW, and confirm the SAME status site now
    // resolves through the zh-TW catalog end-to-end via dispatch/SetLang.
    s.dispatch(Intent::Nudge(1));
    let snap = s.dispatch(Intent::SetLang("zh-TW".into()));
    assert_eq!(snap.lang, "zh-TW");
    let snap = s.dispatch(Intent::Save);
    assert_eq!(
        snap.status_text(),
        Some(confy_core::session::tr(
            confy_core::session::Lang::ZhTw,
            "core.save.saved"
        )),
    );
    assert_ne!(snap.status_text(), Some("Saved"));
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

    // The cursor now defaults onto the new "Reverse" cell (row 0, col 0).
    // Toggling it alone must NOT report the grid active — reverse is a no-op
    // until a real sign/type facet is selected.
    let _ = s.dispatch(Intent::TypeFilterToggle);
    let snap = s.dispatch(Intent::EnterTypeFilter);
    let grid = match &snap.mode {
        ModeView::TypeFilter(v) => v,
        _ => panic!("expected TypeFilter mode after reverse toggle"),
    };
    assert!(!grid.active);
    let _ = s.dispatch(Intent::TypeFilterToggle); // untoggle reverse again

    // Move to a real facet cell (row 1, the first "Key sign" row) and toggle
    // it: it goes On and the grid reports active.
    let _ = s.dispatch(Intent::TypeFilterMove(1, 0));
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
fn dispatch_paste_drops_selection_and_moves_only_the_cursor_onto_the_pasted_node() {
    // Copy t1.x, then paste it after t2.y (a different table → no collision). The
    // source selection on t1.x is dropped and NOT replaced by the pasted node --
    // only the cursor follows the paste. Selection is a persistent, opt-in
    // multi-select the user builds explicitly; if paste populated it, the very
    // next unrelated cursor move + copy would silently re-target the old paste.
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
    assert!(
        snap.rows.iter().all(|r| !r.selected),
        "paste must not select the pasted node or leave the source selected"
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
        cut: true,
    });
    assert!(
        snap.error_text().is_none(),
        "move should succeed: {:?}",
        snap.error_text()
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
        cut: true,
    });
    // `core.move.self` is Warn per the §2.2 severity table — the rejection
    // surfaces in the status slot (was the error bucket pre-single-slot).
    assert!(
        snap.status_text().is_some(),
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
        cut: true,
    });
    assert!(snap.error_text().is_some(), "move into a scalar must fail");
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
        cut: true,
    });
    let t = s.serialize().unwrap();
    assert!(
        t.find("b = 2").unwrap() < t.find("a = 1").unwrap()
            && t.find("a = 1").unwrap() < t.find("c = 3").unwrap(),
        "reordered to b, a, c:\n{t}"
    );
}

#[test]
fn dispatch_move_selection_down_keeps_cursor_on_moved_node() {
    // Regression: a same-parent DOWNWARD move shifts the landing slot up by the
    // removed earlier source, so the post-move cursor must follow the moved
    // node — not land on the next row. Selection is NOT auto-populated by a
    // move/paste (a deliberate, opt-in multi-select the user builds
    // explicitly) — see `dispatch_paste_drops_selection_and_moves_only_the_cursor_onto_the_pasted_node`.
    let mut s = toml_session("a = 1\nb = 2\nc = 3\n");
    let snap = s.dispatch(Intent::MoveSelectionTo {
        sources: vec![vec![Seg::Key("a".into())]],
        target: vec![],
        index: 2, // after 'b' → order becomes b, a, c
        cut: true,
    });
    assert!(
        snap.error_text().is_none(),
        "move should succeed: {:?}",
        snap.error_text()
    );
    assert_eq!(
        snap.cursor,
        vec![Seg::Key("a".into())],
        "cursor stays on the moved node 'a', not the next row"
    );
    let row_a = snap.rows.iter().find(|r| r.key == "a").unwrap();
    assert!(
        row_a.is_cursor && !row_a.selected,
        "'a' is cursor, but not auto-selected"
    );
    let row_c = snap.rows.iter().find(|r| r.key == "c").unwrap();
    assert!(
        !row_c.is_cursor && !row_c.selected,
        "the next row 'c' is neither cursor nor selected"
    );
}

#[test]
fn dispatch_move_comment_down_keeps_cursor_on_moved_comment() {
    // Regression: a DOWNWARD move of a *comment* node shifted the landing slot
    // up by the removed comment too, but the cursor-shift math only accounted
    // for node sources — so the moved comment's next row got cursored.
    let mut s = toml_session("# note\na = 1\nb = 2\n");
    // The comment is positional index 0; move it down to after 'b' (index 2).
    let snap = s.dispatch(Intent::MoveSelectionTo {
        sources: vec![vec![Seg::Index(0)]],
        target: vec![],
        index: 2,
        cut: true,
    });
    assert!(
        snap.error_text().is_none(),
        "move should succeed: {:?}",
        snap.error_text()
    );
    // Order is now a, # note, b — the comment landed at index 1 (cursor).
    let cur = snap.rows.iter().find(|r| r.is_cursor).unwrap();
    assert!(
        cur.key.contains("note"),
        "cursor stays on the moved comment, not 'b': cursor on {:?}",
        cur.key
    );
    assert!(!cur.selected, "the moved comment is not auto-selected");
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
        cut: true,
    });
    assert!(
        snap.error_text().is_none(),
        "move should succeed: {:?}",
        snap.error_text()
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
fn move_selection_to_with_cut_false_copies_instead_of_moving() {
    let mut s = toml_session("[a]\nx = 1\n[b]\nc = 2\n");
    s.expand_all();
    let ax = vec![Seg::Key("a".into()), Seg::Key("x".into())];
    let b = vec![Seg::Key("b".into())];
    s.move_selection_to(vec![ax.clone()], b.clone(), 1, false);
    assert!(
        s.snapshot().error_text().is_none(),
        "copy-drag should succeed: {:?}",
        s.snapshot().error_text()
    );
    // Source untouched (copy, not move).
    assert!(
        s.tree.node_at(&ax).is_some(),
        "source `a.x` must survive a copy-drag"
    );
    // Destination gained the copy.
    let bx = vec![Seg::Key("b".into()), Seg::Key("x".into())];
    assert!(s.tree.node_at(&bx).is_some(), "`b` must gain a copy of `x`");
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

#[test]
fn dispatch_convert_run_carries_toml_schema_hint_to_json() {
    let mut s = toml_session("#:schema ./s.json\na = 1\n");
    s.dispatch(Intent::SetCursor(vec![]));
    s.dispatch(Intent::OpenConvert);
    s.dispatch(Intent::SetConvertFormat(DocFormat::Json));
    let snap = s.dispatch(Intent::ConvertRun);
    let (_, text) = snap
        .convert_write
        .expect("convert produced a write, no warnings expected");
    assert!(
        text.contains("\"$schema\": \"./s.json\""),
        "json output:\n{text}"
    );
}

// ── comment append-sibling: enter inline editor + Esc-cancel (separate node) ──

#[test]
fn add_comment_sibling_enters_inline_edit_and_separates() {
    let mut s = toml_session("# first\nkey = 1\n");
    s.dispatch(Intent::SetCursor(vec![Seg::Index(0)]));
    let snap = s.dispatch(Intent::AddSibling);
    assert!(
        matches!(&snap.mode, ModeView::AddPicker { .. }),
        "AddSibling opens the Add-type picker"
    );
    let snap = s.dispatch(Intent::AddPickerCommit);
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
    s.dispatch(Intent::AddPickerCommit);
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
    assert!(matches!(&snap.mode, ModeView::AddPicker { .. }));
    let snap = s.dispatch(Intent::AddPickerCommit);
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
    assert!(matches!(&snap.mode, ModeView::AddPicker { .. }));
    let snap = s.dispatch(Intent::AddPickerCommit);
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
fn remark_never_prompts_on_clean_json() {
    // Remarking a live node on a pure `.json` (no prior comments) applies
    // immediately — no JsoncUpgrade prompt exists anymore.
    let doc = AnyDocument::from_str_as("{\n  \"a\": 1\n}\n", DocFormat::Json).unwrap();
    let mut s = Session::new(doc);
    s.dispatch(Intent::SetCursor(vec![Seg::Key("a".into())]));
    let snap = s.dispatch(Intent::Remark);
    assert!(
        matches!(snap.mode, ModeView::Normal),
        "expected no prompt, got {:?}",
        snap.mode
    );
    assert!(snap.notice.is_none(), "no notice should be set");
    assert!(s.serialize().unwrap().contains("//"));
}

#[test]
fn remark_targets_selection_over_cursor() {
    // With an active multi-select, remark acts on the SELECTED nodes (like
    // delete/copy), not only on the cursor row.
    let mut s = toml_session("a = 1\nb = 2\nc = 3\n");
    s.cursor_down(); // cursor on a
    s.toggle_select(); // select a
    s.cursor_down(); // cursor on b
    s.toggle_select(); // select b (cursor stays on b)
    s.remark();
    let out = s.serialize().unwrap();
    assert!(out.contains("# a = 1"), "a should be remarked: {out:?}");
    assert!(out.contains("# b = 2"), "b should be remarked: {out:?}");
    assert!(out.contains("c = 3"), "c must stay live: {out:?}");
}

#[test]
fn remark_selection_json_remarks_both_members() {
    let doc = AnyDocument::from_str_as(
        "{\n  \"a\": 1,\n  \"b\": 2,\n  \"c\": 3\n}\n",
        DocFormat::Json,
    )
    .unwrap();
    let mut s = Session::new(doc);
    s.dispatch(Intent::SetCursor(vec![Seg::Key("a".into())]));
    s.toggle_select();
    s.dispatch(Intent::SetCursor(vec![Seg::Key("b".into())]));
    s.toggle_select();
    s.remark();
    let out = s.serialize().unwrap();
    assert!(out.contains("// \"a\": 1"), "a remarked: {out:?}");
    assert!(out.contains("// \"b\": 2"), "b remarked: {out:?}");
    assert!(out.contains("\"c\": 3"), "c live: {out:?}");
}

#[test]
fn remark_selection_remaps_to_merged_block_and_back() {
    // Adjacent selected rows collapse into ONE merged comment block; the
    // selection must remap onto that block (not keep the stale Key paths,
    // which silently no-op every later operation).
    let mut s = toml_session("a = 1\nb = 2\nc = 3\n");
    s.cursor_down();
    s.toggle_select();
    s.cursor_down();
    s.toggle_select();
    s.remark();
    assert_eq!(
        s.selected_paths(),
        vec![vec![Seg::Index(0)]],
        "selection should follow the merged block: {:?}",
        s.selected_paths()
    );
    // The block is selected, so remarking again restores BOTH entries.
    s.remark();
    assert_eq!(s.serialize().unwrap(), "a = 1\nb = 2\nc = 3\n");
}

#[test]
fn remark_selection_expands_when_unremarking_merged_block() {
    // Reverse direction: un-remarking a selected merged block splits it back
    // into N live rows; the selection must expand onto all of them.
    let mut s = toml_session("# a = 1\n# b = 2\nc = 3\n");
    s.cursor_down(); // first content row = the merged comment block
    s.toggle_select();
    s.remark();
    let sel = s.selected_paths();
    assert_eq!(sel.len(), 2, "selection expands to restored rows: {sel:?}");
    assert!(
        sel.contains(&vec![Seg::Key("a".into())]),
        "a selected: {sel:?}"
    );
    assert!(
        sel.contains(&vec![Seg::Key("b".into())]),
        "b selected: {sel:?}"
    );
    // Round trip: remarking the expanded selection re-merges both.
    s.remark();
    assert_eq!(s.serialize().unwrap(), "# a = 1\n# b = 2\nc = 3\n");
    assert_eq!(s.selected_paths(), vec![vec![Seg::Index(0)]]);
}

#[test]
fn remark_selection_tracks_scattered_rows() {
    // Non-adjacent selection: each remark is an in-place kind swap
    // (Key<->Index); the selection must follow the swapped addresses so a
    // second remark still resolves (un-comments both).
    let mut s = toml_session("a = 1\nb = 2\nc = 3\n");
    s.cursor_down();
    s.toggle_select(); // a
    s.cursor_down();
    s.cursor_down();
    s.toggle_select(); // c
    s.remark();
    let out = s.serialize().unwrap();
    assert!(
        out.contains("# a = 1") && out.contains("# c = 3") && out.contains("b = 2"),
        "{out:?}"
    );
    s.remark();
    assert_eq!(s.serialize().unwrap(), "a = 1\nb = 2\nc = 3\n");
}

#[test]
fn remark_selection_json_remaps_through_collapse() {
    let doc = AnyDocument::from_str_as(
        "{\n  \"a\": 1,\n  \"b\": 2,\n  \"c\": 3\n}\n",
        DocFormat::Json,
    )
    .unwrap();
    let mut s = Session::new(doc);
    s.dispatch(Intent::SetCursor(vec![Seg::Key("a".into())]));
    s.toggle_select();
    s.dispatch(Intent::SetCursor(vec![Seg::Key("b".into())]));
    s.toggle_select();
    s.remark();
    assert_eq!(
        s.selected_paths(),
        vec![vec![Seg::Index(0)]],
        "json selection follows merged block: {:?}",
        s.selected_paths()
    );
    s.remark();
    let out = s.serialize().unwrap();
    assert!(out.contains("\"a\": 1"), "a restored: {out:?}");
    assert!(out.contains("\"b\": 2"), "b restored: {out:?}");
}

#[test]
fn delete_selected_drops_stale_paths() {
    // After deleting the selected rows the selection must not keep their
    // (now dead) paths — a dead selection silently blocks the next
    // operation until Esc clears it.
    let mut s = toml_session("a = 1\nb = 2\nc = 3\nd = 4\n");
    s.cursor_down();
    s.toggle_select();
    s.cursor_down();
    s.toggle_select();
    s.delete_selected();
    assert_eq!(s.serialize().unwrap(), "c = 3\nd = 4\n");
    // The dead selected paths are dropped (before the fix they stayed in the
    // selection and silently blocked every later operation until Esc).
    // Assert on the selection itself, not `selected_paths()` — the latter
    // falls back to the cursor row when the selection is empty.
    assert!(
        s.selection.is_empty(),
        "dead paths dropped: {:?}",
        s.selection.iter().collect::<Vec<_>>()
    );
    // The cursor snapped back to the deletion point — 'c', the row that took
    // the deleted rows' place — rather than the top of the document, so the
    // next delete acts on content immediately with no re-navigation.
    s.compute_rows();
    s.delete_selected();
    assert_eq!(s.serialize().unwrap(), "d = 4\n");
}

#[test]
fn delete_selected_snaps_cursor_to_deletion_point() {
    // Regression: `delete_selected` computed the topmost deleted row index but
    // never used it, so `compute_rows`'s unresolvable-cursor fallback threw the
    // cursor to row 0. Deleting deep in a large file sent the user to the top.
    let mut s = toml_session("a = 1\nb = 2\nc = 3\nd = 4\ne = 5\n");
    // Select 'c' and 'd' (rows 3 and 4; row 0 is the root).
    for _ in 0..3 {
        s.cursor_down();
    }
    s.toggle_select();
    s.cursor_down();
    s.toggle_select();
    s.delete_selected();
    assert_eq!(s.serialize().unwrap(), "a = 1\nb = 2\ne = 5\n");
    // 'e' now occupies the deleted rows' position — not 'a', and not the root.
    let rows = s.visible_rows();
    let cursor = rows.iter().find(|r| r.is_cursor).expect("a cursor row");
    assert_eq!(
        cursor.key.as_str(),
        "e",
        "cursor snaps to the deletion point"
    );
}

#[test]
fn delete_selected_at_tail_clamps_cursor_to_last_row() {
    // Deleting the final rows leaves `first_idx` past the end of the shortened
    // list; it must clamp instead of panicking or falling back to row 0.
    let mut s = toml_session("a = 1\nb = 2\nc = 3\n");
    for _ in 0..2 {
        s.cursor_down();
    }
    s.toggle_select();
    s.cursor_down();
    s.toggle_select();
    s.delete_selected();
    assert_eq!(s.serialize().unwrap(), "a = 1\n");
    let rows = s.visible_rows();
    let cursor = rows.iter().find(|r| r.is_cursor).expect("a cursor row");
    assert_eq!(cursor.key.as_str(), "a", "clamped to the new last row");
}

#[test]
fn add_comment_sibling_never_blocked_on_clean_json() {
    // A pure `.json` with no comments at load: remark the first node into a
    // comment (unconditionally legal, per the test above), then add a
    // sibling comment next to it — also unconditionally legal, no notice.
    let doc = AnyDocument::from_str_as("{\n  \"a\": 1,\n  \"b\": 2\n}\n", DocFormat::Json).unwrap();
    let mut s = Session::new(doc);
    s.dispatch(Intent::SetCursor(vec![Seg::Key("a".into())]));
    s.dispatch(Intent::Remark);
    // Cursor now sits on the freshly-created comment row (same visible index).
    let snap = s.dispatch(Intent::AddSibling);
    assert!(matches!(&snap.mode, ModeView::AddPicker { .. }));
    let snap = s.dispatch(Intent::AddPickerCommit);
    assert!(matches!(&snap.mode, ModeView::Edit(e) if e.is_comment));
    assert!(snap.notice.is_none(), "no notice should be set");
    s.dispatch(Intent::CommitEdit {
        value: Some("// hi".into()),
        name: None,
    });
    let out = s.serialize().unwrap();
    assert_eq!(out.matches("//").count(), 2, "two comment nodes: {out:?}");
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
    assert!(
        snap.status_text().is_none(),
        "unknown path is a silent no-op"
    );
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
        snap.status_text().is_some(),
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

// ---- History cap (Task 14, 2026-08-11 audit remediation) ----

#[test]
fn history_caps_at_200_entries_evicting_the_oldest() {
    let mut s = toml_session("a = 0\n");
    let path = vec![Seg::Key("a".into())];
    for i in 1..=250 {
        s.apply_replace(path.clone(), format!("a = {i}\n"));
    }
    let history = s.history.as_mut().expect("history present");
    assert_eq!(
        history.depth(),
        200,
        "past is capped at the fixed 200-entry limit, not left to grow to 250"
    );
    // All 200 capped entries must be redeemable...
    for _ in 0..200 {
        assert!(
            history.undo().is_some(),
            "all 200 capped entries must be undoable"
        );
    }
    // ...but the 201st has nothing left: eviction actually dropped the
    // oldest pushes, it didn't just refuse to grow past 200 while secretly
    // keeping them.
    assert!(
        history.undo().is_none(),
        "oldest entries were evicted, not retained beyond the cap"
    );
}

#[test]
fn history_undo_redo_still_works_correctly_at_the_cap_boundary() {
    let mut s = toml_session("a = 0\n");
    let path = vec![Seg::Key("a".into())];
    for i in 1..=205 {
        s.apply_replace(path.clone(), format!("a = {i}\n"));
    }
    assert_eq!(s.serialize().unwrap(), "a = 205\n");
    s.undo();
    assert_eq!(
        s.serialize().unwrap(),
        "a = 204\n",
        "undo restores the immediately-prior value even once the ring buffer has wrapped"
    );
    s.redo();
    assert_eq!(s.serialize().unwrap(), "a = 205\n", "redo re-applies it");
}

// ---- Append into a `[T/D]` table with a nested-inline-table member value ----
//
// Regression for a `resolve_insert_at`/`node_last_root_index` bug (found while
// grilling ADR 0004): `project_inline` also indexes an inline table's own members
// as `Target::Entry`, but their `.index()` is relative to their immediate CST
// parent (the inline table), not the flat ROOT. `node_last_root_index` used to
// recurse past a table member's own backing entry into those nested indices and
// treat them as ROOT-child positions, computing a splice index past the ROOT's
// actual child count and panicking `splice_children`. Triggered by *any* insert
// that appends to a dotted table whose existing member's value contains >= 2
// levels of nested inline tables — not specific to self-paste.

#[test]
fn append_new_key_into_dotted_table_with_nested_inline_member() {
    use confy_core::model::document::{Mutation, OnCollision, Target};
    let mut s = toml_session("t.a = { b = { x = 1 } }\n");
    let doc = s.doc.as_mut().unwrap();
    doc.apply(Mutation::Insert {
        target: Target {
            parent: vec![Seg::Key("t".into())],
            index: 1,
        },
        fragment: "newkey = 1\n".to_string(),
        on_collision: OnCollision::Cancel,
        suggested_key: None,
    })
    .expect("append must not fail");
    assert_eq!(
        s.serialize().unwrap(),
        "t.a = { b = { x = 1 } }\nt.newkey = 1\n"
    );
}

#[test]
fn copy_paste_dotted_table_into_its_own_scope_does_not_panic() {
    use confy_core::session::PasteSlot;
    let mut s = toml_session("t.a = { b = { x = 1 } }\n");
    let t_path = vec![Seg::Key("t".into())];
    s.reveal_path(t_path.clone());
    s.copy_selected();
    // Step from the default `After(t)` slot back to `Into(t)`.
    s.cursor_up();
    assert_eq!(s.effective_paste_slot(), PasteSlot::Into(t_path));
    s.paste();
    assert!(
        s.snapshot().error_text().is_none(),
        "paste must not surface an error: {:?}",
        s.snapshot().error_text()
    );
    assert_eq!(
        s.serialize().unwrap(),
        "t.a = { b = { x = 1 } }\nt.t.a = { b = { x = 1 } }\n"
    );
}

// ---- Stale `self.tree` after a partial multi-fragment paste failure ----
//
// Regression for a second bug found in the same `systematic-debugging` pass:
// `do_paste`'s NODE PHASE loop (copy branch) inserted each grouped fragment via a
// separate `Mutation::Insert`, holding one `doc` borrow across every iteration and
// never calling `on_mutation_success` on its Collision/error early-returns. So a
// multi-node paste whose *first* fragment inserted successfully but whose *second*
// collided left `self.doc` correctly mutated (the first fragment's insert stuck)
// while `self.tree` (and everything addressed through it — visible rows, cursor,
// selection) kept the pre-paste snapshot: silently diverged from the real
// document. The comment phase a few lines below already re-borrows `doc` per
// iteration and calls `on_mutation_success` on its own error paths — the node
// phase was the asymmetric, unfixed twin.

#[test]
fn paste_partial_failure_reprojects_tree_before_returning() {
    use confy_core::model::document::OnCollision;
    use confy_core::session::state::Clipboard;
    use confy_core::session::PasteSlot;
    let mut s = toml_session("a = 1\nb = 2\n[t]\nb = 99\n");
    let t_path = vec![Seg::Key("t".into())];
    s.reveal_path(t_path.clone());
    let target = s.slot_target(PasteSlot::Into(t_path.clone())).unwrap();
    // `a` has no collision under `[t]`; `b` collides with the existing `[t].b`.
    let clipboard = Clipboard {
        fragments: vec!["a = 1\n".to_string(), "b = 2\n".to_string()],
        cut: false,
        sources: vec![vec![Seg::Key("a".into())], vec![Seg::Key("b".into())]],
    };
    s.do_paste(clipboard, target, OnCollision::Cancel, false);
    // A collision no longer sets a notice — it opens the Collision prompt
    // (spec §3: questions moved out of the Notice slot).
    assert!(
        matches!(s.mode, Mode::Prompt(PromptKind::Collision { .. })),
        "the second fragment's collision must surface"
    );
    // The first fragment's insert already committed to the document...
    assert_eq!(s.serialize().unwrap(), "a = 1\nb = 2\n[t]\nb = 99\na = 1\n");
    // ...and `self.tree` must already reflect it, not the pre-paste snapshot.
    let t_node = s.tree.node_at(&t_path).unwrap();
    let a_path = vec![Seg::Key("t".into()), Seg::Key("a".into())];
    assert!(
        t_node.children.iter().any(|c| c.path == a_path),
        "cached tree must see the already-committed t.a, not go stale: {:?}",
        t_node.children.iter().map(|c| &c.path).collect::<Vec<_>>()
    );
}

// ---- `do_paste` didn't expand an `Into` target, stranding the cursor on an
// invisible row ----
//
// Regression for a bug found while chasing a user-reported repro chain
// ("copy a JSON-converted table into itself, rename the nested copy, copy it
// to root" -> `paste error: invalid fragment: fragment is not a value` then
// `delete error: path not found`). Confirmed on the real `confy` TUI binary:
// after pasting a node `Into` a still-collapsed target, the target stayed
// collapsed (`▸`) and F2 (rename) silently did nothing. Root cause:
// `do_paste` set `self.cursor` to the freshly pasted child without expanding
// `target.parent` first -- unlike `add_node_impl`, which already does
// `self.expanded.insert(target.parent.clone())` for exactly this reason. An
// invisible cursor makes every subsequent cursor-relative action (rename,
// the next copy/paste, detail view) silently no-op or resolve against a
// stale path, exactly the kind of confusing chain the user reported.
#[test]
fn paste_into_slot_expands_target_so_pasted_child_is_visible() {
    use confy_core::session::state::Mode;
    use confy_core::session::PasteSlot;
    let mut s = toml_session("t = { a = 1 }\n");
    let t_path = vec![Seg::Key("t".into())];
    s.reveal_path(t_path.clone());
    s.copy_selected();
    // Step the paste slot back from the default `After(t)` to `Into(t)`.
    s.cursor_up();
    assert_eq!(s.effective_paste_slot(), PasteSlot::Into(t_path.clone()));
    s.paste();
    assert!(
        s.snapshot().error_text().is_none(),
        "paste must not surface an error: {:?}",
        s.snapshot().error_text()
    );

    // The pasted child must be immediately visible, not just present in the
    // document -- else the very next cursor-relative action silently no-ops.
    assert!(
        s.cursor_row().is_some(),
        "cursor must land on a visible row after an Into-paste, not an \
         invisible child of a target that stayed collapsed: cursor={:?}",
        s.cursor
    );

    // F2 rename must actually engage on the freshly pasted node.
    s.begin_inline_rename();
    assert!(
        matches!(s.mode, Mode::Edit(_)),
        "rename must enter edit mode on the freshly pasted node, not silently no-op"
    );
}

// ---- `Selection` wasn't remapped after a rename, so a stale selected path
// silently poisoned every action downstream ----
//
// Regression for the user's exact reported repro chain, root-caused *after*
// the `paste_into_slot_expands_target_so_pasted_child_is_visible` fix above
// turned out to be necessary but insufficient -- confirmed on the real TUI
// binary by instrumenting `capture_selected`/`paste`/`delete_selected` with
// temporary debug output (removed once root-caused). The chain: `do_paste`
// selects the freshly pasted node (`self.selection.set_all(...)`, correct);
// F2-renaming that same node then updates `self.cursor` to the new path but
// never touches `self.selection`, which `edit_commit`'s rename-success path
// simply didn't remap. `selected_paths()` prefers a non-empty `self.selection`
// over the cursor, so the *next* copy captured a fragment from the stale,
// now-nonexistent pre-rename path -- silently the wrong (empty/garbage)
// fragment. That's what produced the exact reported errors two steps later:
// pasting the wrong fragment to root failed with `"fragment is not a value"`,
// and deleting the stale path failed with `"path not found"`. Fixed by
// `Selection::remap_prefix`, called alongside the existing cursor remap at
// both rename call sites (`edit_commit`'s plain rename and
// `apply_deferred_rename`'s TypeChange-confirmed dotted rename).
#[test]
fn rename_remaps_stale_selection_so_the_next_copy_targets_the_right_node() {
    use confy_core::session::state::Clipboard;

    let doc = AnyDocument::from_str_as("{}\n", DocFormat::Json).unwrap();
    let mut s = Session::new(doc);

    // Add a field, replace its value with a JSON object literal (triggers the
    // string -> table conversion prompt), confirm it.
    s.add_node();
    s.add_picker_commit();
    // The seeded value buffer for a fresh empty JSON string is `""` -- clear
    // it before typing, exactly like a real user backspacing first.
    for _ in 0..2 {
        s.edit_backspace();
    }
    for c in "{\"a\":1}".chars() {
        s.edit_input_char(c);
    }
    s.edit_commit();
    s.handle_prompt_key('y');
    assert_eq!(s.serialize().unwrap(), "{ \"new_field\": {\"a\":1} }\n");

    // Copy `new_field`, paste it `Into` itself -- lands a nested copy at
    // `new_field.new_field`, cursor follows it (paste no longer auto-selects).
    s.copy_selected();
    s.cursor_up();
    s.paste();
    assert!(
        s.snapshot().error_text().is_none(),
        "into-self paste must not error: {:?}",
        s.snapshot().error_text()
    );
    let nested_path = vec![Seg::Key("new_field".into()), Seg::Key("new_field".into())];
    assert_eq!(s.cursor, nested_path);

    // Explicitly pin the nested copy into `self.selection` (e.g. the user
    // pressed `s` to select it for a later multi-op) -- this is the only way
    // `self.selection` becomes non-empty now that paste itself doesn't do it.
    // A persistent selection like this must survive the rename below.
    s.toggle_select();

    // Rename the nested copy to `inner` -- this is the step that used to
    // leave `self.selection` pointing at the now-stale `new_field.new_field`.
    s.begin_inline_rename();
    for _ in 0.."new_field".chars().count() {
        s.edit_backspace();
    }
    for c in "inner".chars() {
        s.edit_input_char(c);
    }
    s.edit_commit();
    let inner_path = vec![Seg::Key("new_field".into()), Seg::Key("inner".into())];
    assert_eq!(s.cursor, inner_path);

    // Copy again: must capture `inner`'s real fragment, not a stale/garbage
    // fragment resolved from the pre-rename path.
    s.copy_selected();
    match &s.clipboard {
        Some(Clipboard {
            sources, fragments, ..
        }) => {
            assert_eq!(
                sources,
                &vec![inner_path.clone()],
                "copy must target the renamed node's real path"
            );
            assert_eq!(fragments, &vec!["\"inner\": {\"a\":1}".to_string()]);
        }
        None => panic!("copy_selected must arm the clipboard"),
    }

    // Paste the correctly-captured fragment to root, then delete it -- both
    // must now succeed on the first try (previously: "fragment is not a
    // value" then "path not found").
    s.cursor_home();
    s.paste();
    assert!(
        s.snapshot().error_text().is_none(),
        "paste to root must succeed: {:?}",
        s.snapshot().error_text()
    );
    assert_eq!(
        s.serialize().unwrap(),
        "{ \"new_field\": {\"a\":1, \"inner\": {\"a\":1}}, \"inner\": {\"a\":1} }\n"
    );

    s.delete_selected();
    assert!(
        s.snapshot().error_text().is_none(),
        "delete must succeed on the first try: {:?}",
        s.snapshot().error_text()
    );
    assert_eq!(
        s.serialize().unwrap(),
        "{ \"new_field\": {\"a\":1, \"inner\": {\"a\":1}} }\n"
    );
}

// ---- `do_paste` used to leave the freshly-pasted node selected, so a
// subsequent unrelated cursor move + copy silently re-copied the stale paste
// instead of the node under the cursor ----
//
// User-reported: "after pasting and I move the selection and press c again
// trying to copy another node, it actually copies the previous pasted node".
// Root cause: `do_paste` called `self.selection.set_all(...)` on every
// successful paste/move so the pasted node looked "selected" (matching the
// same visual treatment as a manual multi-select). But `self.selection` is a
// persistent, opt-in multi-select that deliberately survives plain cursor
// movement (so `s` / Shift-range selections aren't lost while navigating) --
// and `selected_paths()` prefers a non-empty `self.selection` over the
// cursor. So paste's auto-selection silently hijacked the next `c` regardless
// of where the cursor had since moved. Fixed: `do_paste` now clears
// `self.selection` and only moves the cursor onto the result.
#[test]
fn paste_does_not_leave_a_stale_selection_that_hijacks_the_next_copy() {
    let mut s = toml_session("[t1]\nx = 1\n[t2]\ny = 2\n[t3]\nz = 3\n");
    s.expand_all();
    // Copy `t1.x`, paste it into `t2` (after `y`) -- lands `t2.x`, no
    // collision.
    s.reveal_path(vec![Seg::Key("t1".into()), Seg::Key("x".into())]);
    s.copy_selected();
    s.reveal_path(vec![Seg::Key("t2".into()), Seg::Key("y".into())]);
    s.paste();
    assert!(
        s.snapshot().error_text().is_none(),
        "paste must not surface an error: {:?}",
        s.snapshot().error_text()
    );
    let pasted_x = vec![Seg::Key("t2".into()), Seg::Key("x".into())];
    assert_eq!(s.cursor, pasted_x, "cursor follows the freshly-pasted node");

    // Move away from the pasted node onto an unrelated node ('t3.z') without
    // any selection gesture -- just plain cursor navigation, exactly how a
    // user would look for the next thing to copy.
    s.cursor_down(); // t3
    s.cursor_down(); // z
    let row = s.cursor_row().expect("cursor row");
    assert_eq!(row.key, "z", "cursor now sits on the unrelated node 't3.z'");

    // Copy: must capture 't3.z', not the stale pasted 't2.x'.
    s.copy_selected();
    match &s.clipboard {
        Some(clipboard) => {
            assert_eq!(
                clipboard.sources,
                vec![vec![Seg::Key("t3".into()), Seg::Key("z".into())]],
                "copy must target the node under the cursor, not a stale paste selection"
            );
        }
        None => panic!("copy_selected must arm the clipboard"),
    }
}

// ---- PasteSlot snapshot (ADR 0004 §1) ----

#[test]
fn snapshot_paste_slot_is_none_until_clipboard_armed_then_tracks_effective_slot() {
    let mut s = toml_session("a = 1\n[b]\nc = 2\n");
    assert_eq!(s.snapshot().paste_slot, None);
    s.cursor = vec![Seg::Key("a".into())];
    s.copy_selected();
    // Armed with no explicit `paste_slot` set: falls back to `After(cursor)`,
    // exactly like `effective_paste_slot()`.
    assert_eq!(
        s.snapshot().paste_slot,
        Some(PasteSlot::After(vec![Seg::Key("a".into())]))
    );
}

// ---- pointer_slot / SetPasteSlot (ADR 0004 §1) ----

#[test]
fn pointer_slot_bands_into_vs_after_and_finds_the_preceding_flattened_slot() {
    let mut s = toml_session("a = 1\n[b]\nc = 2\nd = 3\n");
    s.expand_all();
    let a = vec![Seg::Key("a".into())];
    let b = vec![Seg::Key("b".into())];
    let c = vec![Seg::Key("b".into()), Seg::Key("c".into())];
    let d = vec![Seg::Key("b".into()), Seg::Key("d".into())];

    // Mid-band on an expanded, non-inline branch -> Into.
    assert_eq!(s.pointer_slot(&b, 0.5), Some(PasteSlot::Into(b.clone())));
    // Bottom band on a leaf -> After(that leaf).
    assert_eq!(s.pointer_slot(&a, 0.9), Some(PasteSlot::After(a.clone())));
    // Top band on `b`'s first child `c` -> After(b) (== "first child of b",
    // exactly `c`'s own position, via `resolve_target`'s expanded-branch rule).
    assert_eq!(s.pointer_slot(&c, 0.1), Some(PasteSlot::After(b.clone())));
    // Top band on `b`'s second child `d` -> After(c): here the preceding
    // flattened slot and "previous sibling" happen to coincide (`c` is a
    // leaf) — the differentiating case is below.
    assert_eq!(s.pointer_slot(&d, 0.1), Some(PasteSlot::After(c.clone())));
    // Unknown path -> None.
    assert_eq!(s.pointer_slot(&vec![Seg::Key("nope".into())], 0.5), None);
}

#[test]
fn pointer_slot_top_band_skips_into_an_expanded_previous_sibling() {
    // `r`'s previous *sibling* is `s`, an expanded branch with children `x`,
    // `y`. The preceding *flattened* slot before `r` is After(y) (s's last
    // child) — landing visually between `s`'s subtree and `r`, exactly where
    // the top-band click pointed. A sibling-position shortcut would wrongly
    // return After(s), which `slot_target` resolves to "prepend into s's
    // children" (`resolve_target`'s expanded-branch rule) — deep inside s's
    // subtree, nowhere near where the user clicked. This is the regression
    // guard for that bug. (TOML has no back-to-root-scope: a bare `r = 3`
    // after the `[s]` header would parse as `s.r`, so the following sibling
    // is its own `[r]` table instead of a root-level scalar.)
    let mut s = toml_session("[s]\nx = 1\ny = 2\n\n[r]\nz = 3\n");
    s.expand_all();
    let y = vec![Seg::Key("s".into()), Seg::Key("y".into())];
    let r = vec![Seg::Key("r".into())];
    assert_eq!(s.pointer_slot(&r, 0.1), Some(PasteSlot::After(y)));
}

#[test]
fn pointer_slot_withholds_into_for_a_single_line_inline_container() {
    let s = toml_session("t = { x = 1, y = 2 }\n");
    let t = vec![Seg::Key("t".into())];
    assert_eq!(s.tree.node_at(&t).map(|n| n.format), Some(Format::Inline));
    // Mid-band would normally be Into, but a `Format::Inline` branch has no
    // "insert into" drop zone (mirrors the existing web `dnd.ts` comment) —
    // falls through to After.
    assert_eq!(s.pointer_slot(&t, 0.5), Some(PasteSlot::After(t.clone())));
}

#[test]
fn set_paste_slot_ignores_a_slot_whose_path_is_not_visible() {
    let mut s = toml_session("a = 1\n[b]\nc = 2\n");
    // `b` is collapsed by default; `c` is not visible.
    let c = vec![Seg::Key("b".into()), Seg::Key("c".into())];
    s.set_paste_slot(PasteSlot::After(c.clone()));
    assert_eq!(s.paste_slot, None);
    let a = vec![Seg::Key("a".into())];
    s.set_paste_slot(PasteSlot::After(a.clone()));
    assert_eq!(s.paste_slot, Some(PasteSlot::After(a)));
}

#[test]
fn dispatch_set_paste_slot_intent_arms_the_target_for_paste() {
    let mut s = toml_session("a = 1\n[b]\nc = 2\n");
    s.expand_all();
    let a = vec![Seg::Key("a".into())];
    let b = vec![Seg::Key("b".into())];
    s.cursor = a.clone();
    s.copy_selected();
    let snap = s.dispatch(Intent::SetPasteSlot(PasteSlot::Into(b.clone())));
    assert_eq!(snap.paste_slot, Some(PasteSlot::Into(b.clone())));
    // Pointer-driven targeting (desktop click / touch drag) also moves the
    // cursor onto the slot's row, mirroring the TUI's keyboard-driven
    // `PasteSlot` stepping — otherwise the cursor-styled row indicator
    // (`.paste-mode .row.cursor`) goes stale under mouse/touch targeting.
    assert_eq!(
        snap.cursor, b,
        "cursor should follow the pointer-driven target"
    );
}

// ---- AoT-entry move into another `[A/T]` group (ADR 0004 §3) ----

#[test]
fn move_aot_entry_into_another_group_preserves_nested_section() {
    let mut s = toml_session(
        "[[fruit]]\nname = \"apple\"\n\n[fruit.physical]\ncolor = \"red\"\n\n[[items]]\nname = \"seed\"\n",
    );
    let fruit0 = vec![Seg::Key("fruit".into()), Seg::Index(0)];
    s.reveal_path(fruit0.clone());
    s.cursor = fruit0.clone();
    s.cut_selected();
    assert!(
        s.snapshot().error_text().is_none(),
        "cut should succeed: {:?}",
        s.snapshot().error_text()
    );

    let items = vec![Seg::Key("items".into())];
    s.paste_slot = Some(PasteSlot::Into(items.clone()));
    s.paste();
    assert!(
        s.snapshot().error_text().is_none(),
        "paste should succeed: {:?}",
        s.snapshot().error_text()
    );

    // The moved entry lands as `items[1]`, its `name` member traveling with it.
    let name1 = vec![
        Seg::Key("items".into()),
        Seg::Index(1),
        Seg::Key("name".into()),
    ];
    assert_eq!(
        s.tree.node_at(&name1).and_then(|n| n.value.clone()),
        Some("\"apple\"".to_string())
    );

    // `physical` must survive as a real nested table (`[T/S]`, `Format::Scope`)
    // — TOML attaches `[items.physical]` to the most recent `[[items]]` entry,
    // so semantically it is `items[1].physical` (the projector addresses it as
    // a group-level keyed child, the same shape as the source document's
    // `[fruit.physical]`). It must NOT be flattened to a dotted
    // `items[1].physical.color` key (the ADR 0004 §3 bug).
    let physical = vec![Seg::Key("items".into()), Seg::Key("physical".into())];
    let node = s
        .tree
        .node_at(&physical)
        .expect("nested `physical` table survives the atomic move");
    assert_eq!(
        node.format,
        Format::Scope,
        "sub-section stays a real nested table"
    );

    let mut color = physical.clone();
    color.push(Seg::Key("color".into()));
    assert_eq!(
        s.tree.node_at(&color).and_then(|n| n.value.clone()),
        Some("\"red\"".to_string())
    );

    // Moved (cut), so `fruit` no longer has the entry.
    assert!(
        s.tree.node_at(&fruit0).is_none(),
        "cut removed the source entry"
    );
}

// ---- Copy (not cut) of a bare-scalar array element derives
// `<arrayKey>_<index>` as the pasted key (Move + Copy scope parity) ----

#[test]
fn copy_array_element_into_table_derives_array_key_index_name() {
    let mut s = toml_session("[src]\nnums = [10, 20, 30]\n\n[dst]\nkeep = true\n");
    s.expand_all();
    // Cursor on `src.nums[1]` — a bare scalar with no key of its own.
    let nums1 = vec![
        Seg::Key("src".into()),
        Seg::Key("nums".into()),
        Seg::Index(1),
    ];
    s.reveal_path(nums1.clone());
    s.cursor = nums1;
    s.copy_selected();
    assert!(
        s.snapshot().error_text().is_none(),
        "copy should succeed: {:?}",
        s.snapshot().error_text()
    );

    let dst = vec![Seg::Key("dst".into())];
    s.paste_slot = Some(PasteSlot::Into(dst.clone()));
    s.paste();
    assert!(
        s.snapshot().error_text().is_none(),
        "paste should succeed: {:?}",
        s.snapshot().error_text()
    );

    // The pasted key is `nums_1` — derived from the source array's own key +
    // element index, the same name a cut/move of the same element produces —
    // not the generic `placeholder`.
    let pasted = vec![Seg::Key("dst".into()), Seg::Key("nums_1".into())];
    assert_eq!(
        s.tree.node_at(&pasted).and_then(|n| n.value.clone()),
        Some("20".to_string()),
        "pasted scalar must land under the derived `<arrayKey>_<index>` key"
    );

    // Copy (unlike cut) leaves the source array untouched: all 3 original
    // elements survive the copy+paste.
    for (i, v) in [(0usize, "10"), (1, "20"), (2, "30")] {
        let elem = vec![
            Seg::Key("src".into()),
            Seg::Key("nums".into()),
            Seg::Index(i),
        ];
        assert_eq!(
            s.tree.node_at(&elem).and_then(|n| n.value.clone()),
            Some(v.to_string()),
            "source array element {i} must survive the copy"
        );
    }
}

#[test]
fn copy_scalar_out_of_nested_unkeyed_array_falls_back_to_placeholder() {
    // `grid`'s elements are themselves arrays, so the inner element's path
    // ends `[..., Index(0), Index(1)]` — the array holding the scalar has no
    // key of its own, so no `<arrayKey>_<index>` can be derived.
    let mut s = toml_session("[src]\ngrid = [[1, 2], [3, 4]]\n\n[dst]\nkeep = true\n");
    s.expand_all();
    let inner = vec![
        Seg::Key("src".into()),
        Seg::Key("grid".into()),
        Seg::Index(0),
        Seg::Index(1),
    ];
    s.reveal_path(inner.clone());
    s.cursor = inner;
    s.copy_selected();

    let dst = vec![Seg::Key("dst".into())];
    s.paste_slot = Some(PasteSlot::Into(dst.clone()));
    s.paste();
    assert!(
        s.snapshot().error_text().is_none(),
        "paste should succeed: {:?}",
        s.snapshot().error_text()
    );

    let pasted = vec![Seg::Key("dst".into()), Seg::Key("placeholder".into())];
    assert_eq!(
        s.tree.node_at(&pasted).and_then(|n| n.value.clone()),
        Some("2".to_string()),
        "unkeyed/nested-array scalar keeps the generic placeholder key"
    );
}

// ---- YAML quoted-key edit/Path regression tests (2026-08-28 follow-up) ----

#[test]
fn value_only_edit_keeps_quoted_yaml_key_intact() {
    // Editing just the Value of a quoted-key YAML entry must not silently
    // drop the key's quotes from the rebuilt "key: value" fragment — a bug
    // found and fixed as a side effect of the `key_literal_text` rename fix
    // (`begin_inline_edit_impl` previously always seeded `frag_key` with
    // YAML's *decoded* key).
    let doc = AnyDocument::from_str_as("\"a b\": 1\n", DocFormat::Yaml).unwrap();
    let mut s = Session::new(doc);
    s.cursor = vec![Seg::Key("a b".into())];
    s.begin_inline_edit();
    s.edit_backspace();
    for c in "2".chars() {
        s.edit_input_char(c);
    }
    s.edit_commit();
    assert_eq!(s.serialize().unwrap(), "\"a b\": 2\n");
}

#[test]
fn detail_path_line_shows_quotes_for_quoted_yaml_key() {
    let doc = AnyDocument::from_str_as("\"a b\":\n  c: 1\n", DocFormat::Yaml).unwrap();
    let mut s = Session::new(doc);
    s.expanded.insert(vec![Seg::Key("a b".into())]);
    s.cursor = vec![Seg::Key("a b".into()), Seg::Key("c".into())];
    s.open_detail();
    let text = s.detail_text.clone().unwrap();
    assert!(
        text.contains("\"a b\".c"),
        "Path line should show the quoted ancestor key: {text}"
    );
}

#[test]
fn detail_path_line_does_not_double_quote_toml_key() {
    let mut s = toml_session("\"a b\" = 1\n");
    // The path segment is the DECODED key; the quotes live in `key_literal`.
    s.cursor = vec![Seg::Key("a b".into())];
    s.open_detail();
    let text = s.detail_text.clone().unwrap();
    assert!(
        text.contains("\"a b\""),
        "expected single-quoted TOML key: {text}"
    );
    assert!(
        !text.contains("\"\"a b\"\""),
        "TOML key must not be double-quoted: {text}"
    );
}

#[test]
fn detail_path_line_uses_the_authored_quote_style_for_yaml() {
    // A single-quoted key must show single quotes — this is what the old
    // synthesized `'"'` wrap got wrong for every non-double-quoted key.
    let doc = AnyDocument::from_str_as("'a b':\n  c: 1\n", DocFormat::Yaml).unwrap();
    let mut s = Session::new(doc);
    s.expanded.insert(vec![Seg::Key("a b".into())]);
    s.cursor = vec![Seg::Key("a b".into()), Seg::Key("c".into())];
    s.open_detail();
    let text = s.detail_text.clone().unwrap();
    assert!(
        text.contains("'a b'.c"),
        "Path line must use the authored single quotes: {text}"
    );
    assert!(
        !text.contains("\"a b\""),
        "Path line must not synthesize double quotes: {text}"
    );
}

#[test]
fn view_row_path_display_shows_quotes_for_quoted_yaml_key() {
    let doc = AnyDocument::from_str_as("\"a b\": 1\n", DocFormat::Yaml).unwrap();
    let s = Session::new(doc);
    let row = s
        .visible_rows()
        .into_iter()
        .find(|r| r.key == "a b")
        .unwrap();
    assert_eq!(row.path_display, "\"a b\"");
}

#[test]
fn view_row_path_display_does_not_double_quote_toml_key() {
    let s = toml_session("\"a b\" = 1\n");
    let row = s
        .visible_rows()
        .into_iter()
        .find(|r| r.key == "a b")
        .unwrap();
    assert_eq!(row.path_display, "\"a b\"");
    assert_eq!(row.key_literal.as_deref(), Some("\"a b\""));
}

#[test]
fn view_row_path_display_uses_authored_single_quotes_for_yaml() {
    let doc = AnyDocument::from_str_as("'a b': 1\n", DocFormat::Yaml).unwrap();
    let s = Session::new(doc);
    let row = s
        .visible_rows()
        .into_iter()
        .find(|r| r.key == "a b")
        .unwrap();
    assert_eq!(row.path_display, "'a b'");
    assert_eq!(row.key_literal.as_deref(), Some("'a b'"));
}

#[test]
fn view_row_path_display_leaves_bare_yaml_key_unquoted() {
    let doc = AnyDocument::from_str_as("a: 1\n", DocFormat::Yaml).unwrap();
    let s = Session::new(doc);
    let row = s.visible_rows().into_iter().find(|r| r.key == "a").unwrap();
    assert_eq!(row.path_display, "a");
}

// ---- Scripted end-to-end verification: F2 rename on a quoted YAML key ----
// (manual-test substitute — no interactive TUI/browser available here)

#[test]
fn rename_buffer_editing_quote_chars_and_trailing_space_inside_quotes_works() {
    let doc = AnyDocument::from_str_as("\"a b\": 1\n", DocFormat::Yaml).unwrap();
    let mut s = Session::new(doc);
    s.cursor = vec![Seg::Key("a b".into())];
    s.begin_inline_rename();
    assert_eq!(
        match &s.mode {
            Mode::Edit(e) => e.buffer.clone(),
            _ => panic!("expected Edit mode"),
        },
        "\"a b\""
    );
    // Move left past the closing quote and type an intentional trailing
    // space *inside* the quotes — the quote chars are now ordinary,
    // editable buffer content, and protect the inside space from
    // `edit_commit`'s outer `.trim()`.
    s.edit_cursor_left();
    s.edit_input_char(' ');
    s.edit_commit();
    assert_eq!(s.serialize().unwrap(), "\"a b \": 1\n");
}

#[test]
fn commit_unchanged_quoted_yaml_rename_is_a_noop() {
    let doc = AnyDocument::from_str_as("\"a b\": 1\n", DocFormat::Yaml).unwrap();
    let mut s = Session::new(doc);
    s.cursor = vec![Seg::Key("a b".into())];
    let before = s.serialize().unwrap();
    s.begin_inline_rename();
    s.edit_commit(); // no edits made
    assert_eq!(
        s.serialize().unwrap(),
        before,
        "unchanged rename must not rewrite the document"
    );
    assert!(
        !s.is_dirty(),
        "unchanged rename must not mark the document dirty"
    );
}

#[test]
fn rename_collision_check_compares_decoded_names_quoted_or_not() {
    // Typing the new name *with* explicit quotes must still collision-match
    // an existing sibling compared by its decoded name: the rename is
    // rejected (stays in Edit mode with an error notice, document
    // untouched) rather than silently overwriting/renaming past it.
    let doc = AnyDocument::from_str_as("\"a b\": 1\ncd: 2\n", DocFormat::Yaml).unwrap();
    let mut s = Session::new(doc);
    s.cursor = vec![Seg::Key("cd".into())];
    s.begin_inline_rename();
    for _ in 0.."cd".chars().count() {
        s.edit_backspace();
    }
    for c in "\"a b\"".chars() {
        s.edit_input_char(c);
    }
    s.edit_commit();
    assert!(
        matches!(s.mode, Mode::Edit(_)),
        "collision must reject the rename and stay in Edit mode"
    );
    assert_eq!(
        s.serialize().unwrap(),
        "\"a b\": 1\ncd: 2\n",
        "colliding rename must not touch the document"
    );
}

#[test]
fn rename_collision_check_matches_bare_typed_name_too() {
    // Same collision, but typed without quotes.
    let doc = AnyDocument::from_str_as("\"a b\": 1\ncd: 2\n", DocFormat::Yaml).unwrap();
    let mut s = Session::new(doc);
    s.cursor = vec![Seg::Key("cd".into())];
    s.begin_inline_rename();
    for _ in 0.."cd".chars().count() {
        s.edit_backspace();
    }
    for c in "a b".chars() {
        s.edit_input_char(c);
    }
    s.edit_commit();
    assert!(
        matches!(s.mode, Mode::Edit(_)),
        "collision must reject the rename and stay in Edit mode"
    );
    assert_eq!(
        s.serialize().unwrap(),
        "\"a b\": 1\ncd: 2\n",
        "colliding rename must not touch the document"
    );
}

// ---- Add-type picker (Mode::AddPicker) — option filtering per parent kind
// and format, and Escape-inserts-nothing (plan `add-node-type-picker-plan.md`
// step 11) ----

#[test]
fn add_picker_options_filtered_for_inline_table_excludes_table_and_comment() {
    let mut s = toml_session("t = { x = 1 }\n");
    s.dispatch(Intent::CursorDown); // onto 't'
    let snap = s.dispatch(Intent::AddChild);
    let ModeView::AddPicker { options, .. } = snap.mode else {
        panic!("expected AddPicker mode: {:?}", snap.mode);
    };
    let labels: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
    assert!(!labels.contains(&"Table"), "no [table] header inside a flow construct: {labels:?}");
    assert!(!labels.contains(&"Comment"), "no comment inside a flow construct: {labels:?}");
    assert!(labels.contains(&"String"), "scalars stay legal: {labels:?}");
    assert!(labels.contains(&"Inline table"), "nested inline table stays legal: {labels:?}");
    assert!(labels.contains(&"Array"), "array stays legal: {labels:?}");
}

#[test]
fn add_picker_options_for_array_of_tables_group_offers_only_table_entry_and_comment() {
    let mut s = toml_session("[[items]]\na = 1\n");
    s.dispatch(Intent::CursorDown); // onto 'items'
    let snap = s.dispatch(Intent::AddChild);
    let ModeView::AddPicker { options, .. } = snap.mode else {
        panic!("expected AddPicker mode: {:?}", snap.mode);
    };
    let labels: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(labels, vec!["Table entry", "Comment"], "{labels:?}");
}

#[test]
fn add_picker_escape_inserts_nothing() {
    let mut s = toml_session("a = 1\n");
    s.dispatch(Intent::CursorDown); // onto 'a'
    s.dispatch(Intent::AddSibling);
    s.dispatch(Intent::Escape);
    assert_eq!(
        s.serialize().unwrap(),
        "a = 1\n",
        "Esc during the picker leaves the document byte-identical"
    );
}

#[test]
fn add_picker_toml_offers_four_datetime_kinds() {
    let mut s = toml_session("a = 1\n");
    s.dispatch(Intent::CursorDown); // onto 'a'
    let snap = s.dispatch(Intent::AddSibling);
    let ModeView::AddPicker { options, .. } = snap.mode else {
        panic!("expected AddPicker mode: {:?}", snap.mode);
    };
    let labels: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
    for want in ["Offset datetime", "Local datetime", "Local date", "Local time"] {
        assert!(labels.contains(&want), "TOML offers {want:?}: {labels:?}");
    }
    assert!(!labels.contains(&"Null"), "TOML has no null literal: {labels:?}");
}

#[test]
fn add_picker_json_offers_null_not_datetime() {
    let doc = AnyDocument::from_str_as("{\"a\": 1}\n", DocFormat::Json).unwrap();
    let mut s = Session::new(doc);
    s.dispatch(Intent::CursorDown); // onto 'a'
    let snap = s.dispatch(Intent::AddSibling);
    let ModeView::AddPicker { options, .. } = snap.mode else {
        panic!("expected AddPicker mode: {:?}", snap.mode);
    };
    let labels: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
    assert!(labels.contains(&"Null"), "JSON offers Null: {labels:?}");
    assert!(!labels.contains(&"Offset datetime"), "JSON has no datetime scalar: {labels:?}");
}
