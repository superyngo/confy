use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::DocFormat;
use confy_core::session::{Intent, Session};

#[test]
fn schema_fetch_request_survives_unrelated_dispatches_until_resolved() {
    // A JSON doc with a $schema hint. Session::new() eagerly computes
    // pending_schema_fetch. If the host issues ANY other dispatch (e.g.
    // SetLang, SetHostNotice for the comment-advisory toast) before ever
    // consuming the FIRST dispatch's schema_fetch_request, later snapshots
    // must still carry it -- otherwise the host never resolves+applies the
    // schema, and violations never show up until an edit happens to
    // re-trigger detection via sync_schema_hint(). Regression test for
    // comment-advisory follow-up issue #2.
    let src = "{\n  \"$schema\": \"./schema.json\",\n  \"a\": 1\n}\n";
    let doc = AnyDocument::from_str_as(src, DocFormat::Json).unwrap();
    let mut s = Session::new(doc);

    // Sanity: a hint was actually detected at construction.
    assert!(
        s.pending_schema_fetch.is_some(),
        "sanity: Session::new should have detected the $schema hint"
    );

    // Host does what web/ui.ts's openText() does: SetLang, then (for a
    // strict_json file with pre-existing comments) SetHostNotice.
    s.strict_json = true;
    let snap1 = s.dispatch(Intent::SetLang("en".to_string()));

    let snap2 = s.dispatch(Intent::SetHostNotice {
        key: "web.host.json-comments-detected".to_string(),
        args: vec![],
        source: confy_core::session::notice::NoticeSource::HostWeb,
    });

    assert!(
        snap1.schema_fetch_request.is_some(),
        "sanity: the first dispatch's snapshot carries the request"
    );
    assert!(
        snap2.schema_fetch_request.is_some(),
        "the schema fetch request must still be visible on the snapshot the \
         host actually renders from (snap2), not just the first dispatch's \
         snapshot (snap1) that nobody looked at"
    );

    // Once the host resolves it via SchemaLoaded, it must actually clear —
    // otherwise the host would re-fetch the same schema forever.
    let snap3 = s.dispatch(Intent::SchemaLoaded {
        source: confy_core::schema::SchemaSource::Local("./schema.json".into()),
        text: Ok("{}".into()),
    });
    assert!(
        snap3.schema_fetch_request.is_none(),
        "a resolved fetch request must not keep re-surfacing"
    );
}
