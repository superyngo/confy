/// Notice slot + diag ring headless tests (message-system Phase 1, Tasks 3+4).
/// Kept in a dedicated file — independent of `session_headless.rs`, whose
/// legacy `status`/`error` assertions are migrated to the `notice` model in
/// Task 6 — so the Task 3 contract stays verifiable and green through that
/// migration.
use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::DocFormat;
use confy_core::session::{Intent, Lang, Notice, Session, Severity};

fn toml_session(src: &str) -> Session {
    let doc = AnyDocument::from_str_as(src, DocFormat::Toml).unwrap();
    Session::new(doc)
}

#[test]
fn set_notice_taps_the_diag_ring() {
    let mut session = toml_session("port = 1\n");
    session.set_notice(Notice::core(Lang::En, "core.save.saved", &[]));
    let last = session.diag.iter().last().expect("diag event recorded");
    assert_eq!(last.kind, "notice");
    // `Notice` carries no catalog key — the tap captures the rendered text
    // verbatim (spec §7), so assert on it (En renders "Saved").
    assert!(last.detail.contains("Saved"), "detail was: {}", last.detail);
    assert_eq!(session.notice.as_ref().unwrap().severity, Severity::Success);
}

#[test]
fn set_lang_clears_notice() {
    let mut session = toml_session("port = 1\n");
    session.set_notice(Notice::core(Lang::En, "core.save.saved", &[]));
    session.dispatch(Intent::SetLang("zh-TW".to_string()));
    assert!(session.notice.is_none());
}
