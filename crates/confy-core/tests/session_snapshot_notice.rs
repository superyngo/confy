//! `SessionSnapshot` notice slot-occupancy tests (design spec §10/§11 Q7
//! follow-up, §8 phase-1 row) plus the `Intent::SetHostNotice` dispatch path
//! and the diag `dispatch`/`mutation` taps (spec §7). These exercise the
//! single-slot model's new behavior, distinct from the migrated legacy
//! `status`/`error` assertions in `session_headless.rs`.
use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::DocFormat;
use confy_core::model::node::Seg;
use confy_core::session::{DiagLevel, Intent, NoticeSource, Session, Severity};

fn toml_session(src: &str) -> Session {
    let doc = AnyDocument::from_str_as(src, DocFormat::Toml).unwrap();
    Session::new(doc)
}

#[test]
fn warn_notice_occupies_status_not_error() {
    // Arming the clipboard modal-locks the other surfaces (ADR 0005 §5):
    // EnterFilter surfaces `core.clipboard.action-locked` — Warn per the
    // §2.2 severity table, so it must land in the status slot only.
    let mut s = toml_session("a = 1\nb = 2\n");
    s.dispatch(Intent::CursorDown);
    s.dispatch(Intent::CopySelected);
    let snap = s.dispatch(Intent::EnterFilter);
    assert!(
        snap.error_text().is_none(),
        "Warn must not occupy the error slot"
    );
    assert!(
        snap.status_text().is_some(),
        "Warn must occupy the status slot"
    );
    assert_eq!(snap.notice.as_ref().unwrap().severity, Severity::Warn);
}

#[test]
fn error_notice_occupies_error_not_status() {
    // Drag-moving a node onto a scalar parent is an illegal destination —
    // `core.paste.error` (Error per §2.2), so it must land in the error
    // slot only.
    let mut s = toml_session("a = 1\nb = 2\n");
    let snap = s.dispatch(Intent::MoveSelectionTo {
        sources: vec![vec![Seg::Key("a".into())]],
        target: vec![Seg::Key("b".into())], // scalar parent → illegal destination
        index: 0,
        cut: true,
    });
    assert!(
        snap.error_text().is_some(),
        "Error must occupy the error slot"
    );
    assert!(
        snap.status_text().is_none(),
        "Error must not occupy the status slot"
    );
    assert_eq!(snap.notice.as_ref().unwrap().severity, Severity::Error);
}

#[test]
fn no_notice_is_both_none() {
    let s = toml_session("a = 1\n");
    let snap = s.snapshot();
    assert!(snap.error_text().is_none());
    assert!(snap.status_text().is_none());
    assert!(snap.notice.is_none());
}

#[test]
fn set_host_notice_intent_goes_through_dispatch() {
    // Hosts report their own notices through the sole dispatch path
    // (§12 Q6): severity resolves through the shared `severity_of` table,
    // the source stamp comes from the Intent.
    let mut s = toml_session("a = 1\n");
    let snap = s.dispatch(Intent::SetHostNotice {
        key: "core.clipboard.action-locked".into(),
        args: vec![],
        source: NoticeSource::HostTui,
    });
    assert_eq!(snap.notice.as_ref().unwrap().source, NoticeSource::HostTui);
    assert_eq!(snap.notice.as_ref().unwrap().severity, Severity::Warn);
    // And it round-trips through the status slot like any non-Error notice.
    assert!(snap.status_text().is_some());
    assert!(snap.error_text().is_none());
}

#[test]
fn set_host_notice_core_source_is_a_defensive_no_op() {
    // `NoticeSource::Core` in a SetHostNotice is a caller bug (core code
    // never dispatches host notices) — it must not panic in release and
    // must not fabricate a Core-stamped host notice.
    let mut s = toml_session("a = 1\n");
    let snap = s.dispatch(Intent::SetHostNotice {
        key: "core.clipboard.action-locked".into(),
        args: vec![],
        source: NoticeSource::Core,
    });
    assert!(snap.notice.is_none(), "Core source must not set a notice");
}

#[test]
fn dispatch_and_mutation_diag_taps_fire() {
    let mut s = toml_session("a = 1\nb = 2\n");
    s.dispatch(Intent::CursorDown);
    let events: Vec<_> = s.diag.iter().collect();
    // The dispatch tap (Debug, every intent) precedes everything else.
    assert!(
        events.iter().any(|e| e.kind == "dispatch"
            && e.level == DiagLevel::Debug
            && e.detail.contains("CursorDown")),
        "dispatch tap missing: {events:?}"
    );
    // The mutation tap records success at Info.
    assert!(
        events.iter().any(|e| e.kind == "mutation"
            && e.level == DiagLevel::Info
            && e.detail.contains("CursorDown")),
        "success mutation tap missing: {events:?}"
    );
    // And failure at Error, once an intent actually fails.
    s.dispatch(Intent::MoveSelectionTo {
        sources: vec![vec![Seg::Key("a".into())]],
        target: vec![Seg::Key("b".into())], // scalar parent → illegal destination
        index: 0,
        cut: true,
    });
    assert!(
        s.diag.iter().any(|e| e.kind == "mutation"
            && e.level == DiagLevel::Error
            && e.detail.contains("MoveSelectionTo")),
        "failure mutation tap missing"
    );
}

#[test]
fn session_snapshot_has_no_legacy_status_error_fields() {
    let src = std::fs::read_to_string("src/session/view.rs").unwrap();
    assert!(
        !src.contains("pub status: Option<String>"),
        "SessionSnapshot still has legacy 'status' field"
    );
    assert!(
        !src.contains("pub error: Option<String>"),
        "SessionSnapshot still has legacy 'error' field"
    );
}
