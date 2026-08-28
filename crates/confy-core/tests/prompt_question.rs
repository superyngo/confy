//! Task 5 (message-system-integration): `ModeView::Prompt` carries its
//! question text computed per-snapshot from `PromptKind` + `Session::lang`,
//! instead of the host re-rendering it from `tui.prompt.*`/`web.prompt.*`
//! keys. Separate file from `session_headless.rs` for the same reason
//! `session_notice.rs` exists: that file still tracks pre-Task-6 state.
use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::DocFormat;
use confy_core::session::{Intent, ModeView, PromptView, Session};

fn toml_session(src: &str) -> Session {
    let doc = AnyDocument::from_str_as(src, DocFormat::Toml).unwrap();
    Session::new(doc)
}

/// Copy → paste the same key back into its parent: the insert collides and
/// the session opens the Collision prompt for that key.
fn collision_prompt_session() -> Session {
    let mut s = toml_session("port = 8080\n");
    s.cursor_down(); // cursor on 'port'
    s.dispatch(Intent::CopySelected);
    s.dispatch(Intent::Paste); // 'port = 8080' re-inserted at root → collision
    s
}

#[test]
fn prompt_question_renders_from_kind_not_status() {
    let s = collision_prompt_session();
    let snap = s.snapshot();
    match snap.mode {
        ModeView::Prompt { kind, ref question } => {
            assert!(matches!(kind, PromptView::Collision), "kind was {kind:?}");
            assert!(question.contains("port"), "question was {question:?}");
            // `status_text()`/`error_text()` may carry the preceding
            // clipboard notice, but never the prompt question text.
            for text in [snap.status_text(), snap.error_text()]
                .into_iter()
                .flatten()
            {
                assert_ne!(text, question.as_str());
            }
        }
        other => panic!("expected Prompt mode, got {other:?}"),
    }
}

#[test]
fn prompt_question_rerenders_on_language_switch() {
    let mut s = collision_prompt_session();
    let snap = s.dispatch(Intent::SetLang("zh-TW".to_string()));
    match snap.mode {
        ModeView::Prompt { ref question, .. } => {
            // zh-TW collision prose: key '{0}' 發生衝突
            assert!(question.contains("衝突"), "question was {question:?}");
            assert!(question.contains("port"), "question was {question:?}");
        }
        other => panic!("expected Prompt mode, got {other:?}"),
    }
}
