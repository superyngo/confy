use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::DocFormat;
use confy_core::model::node::Seg;
use confy_core::session::{Intent, Session};

// Simulates the external/pop-up editor round trip: the host shows the node's
// current "value  <comment>" bundled text, the user deletes the comment
// portion, and the host dispatches ApplyReplace with the comment-free text
// (mirrors confy-tui's editor.rs / web's openExternalEdit -> ApplyReplace).
fn external_edit_clears_comment(
    src: &str,
    fmt: DocFormat,
    path: Vec<Seg>,
    new_text: &str,
) -> String {
    let doc = AnyDocument::from_str_as(src, fmt).unwrap();
    let mut s = Session::new(doc);
    s.dispatch(Intent::SetCursor(path.clone()));
    s.dispatch(Intent::ApplyReplace {
        path,
        text: new_text.to_string(),
    });
    s.serialize().unwrap()
}

#[test]
fn external_edit_can_clear_trailing_comment() {
    // Regression test for comment-advisory follow-up issue #4: the external
    // pop-up editor's returned text is the authoritative full fragment: if
    // the user deletes the trailing comment from it, `Mutation::Replace`'s
    // "preserve the old comment when the fragment is silent" default (needed
    // for the *inline* editor's value-only fragments) must not silently
    // restore it.
    for (fmt, src, new_text, expect) in [
        (
            DocFormat::Json,
            "{\n  \"a\": 1  // old\n}\n",
            "\"a\": 1",
            "{\n  \"a\": 1\n}\n",
        ),
        (DocFormat::Toml, "a = 1  # old\n", "a = 1", "a = 1\n"),
    ] {
        let out = external_edit_clears_comment(src, fmt, vec![Seg::Key("a".into())], new_text);
        assert_eq!(out, expect, "{fmt:?}: full output: {out:?}");
    }

    // YAML asserted separately: its expectation is "comment absent", not an
    // exact string (existing test's own choice — the CST doesn't guarantee
    // identical whitespace after the splice the way JSON/TOML's exact
    // literal match does).
    let out = external_edit_clears_comment(
        "a: 1  # old\n",
        DocFormat::Yaml,
        vec![Seg::Key("a".into())],
        "a: 1",
    );
    eprintln!("YAML result:\n{out}");
    assert!(
        !out.contains("# old"),
        "YAML: comment should be cleared: {out:?}"
    );
}
