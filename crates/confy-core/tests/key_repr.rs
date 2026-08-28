//! Regression suite for the key-representation contract (2026-08-28).
//!
//! One invariant, three backends:
//! - `Node.key` / `Seg::Key` = the **decoded** key — semantic identity, used by
//!   path resolution, collision checks, JSON-Schema lookup and `to_value`.
//! - `Node.key_literal` / `ViewRow.key_literal` = the key **exactly as
//!   authored** — presentation + edit identity (tree row, Path line, rename
//!   buffer). `None` for keyless nodes.
//!
//! Every case here failed before
//! `docs/superpowers/plans/2026-08-28-key-repr-first-class-literal.md` landed.

use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::{ConfigDocument, DocFormat};
use confy_core::model::node::Seg;
use confy_core::session::{Mode, Session};

fn session(src: &str, fmt: DocFormat) -> Session {
    Session::new(AnyDocument::from_str_as(src, fmt).unwrap())
}

fn literal_of(src: &str, fmt: DocFormat) -> (String, Option<String>) {
    let doc = AnyDocument::from_str_as(src, fmt).unwrap();
    let tree = doc.project();
    let child = &tree.root.children[0];
    (child.key.clone(), child.key_literal.clone())
}

// ── Decoded key vs authored spelling ────────────────────────────────────────

#[test]
fn yaml_projects_decoded_key_and_authored_literal() {
    for (src, literal) in [
        ("'a b': 1\n", "'a b'"),
        ("\"a b\": 1\n", "\"a b\""),
        ("\"a\\x20b\": 1\n", "\"a\\x20b\""),
        ("\"a\\u0020b\": 1\n", "\"a\\u0020b\""),
    ] {
        let (key, lit) = literal_of(src, DocFormat::Yaml);
        assert_eq!(key, "a b", "decoded key for {src:?}");
        assert_eq!(lit.as_deref(), Some(literal), "literal for {src:?}");
    }
    // A bare key still reports its (identical) spelling, never `None`.
    let (key, lit) = literal_of("a: 1\n", DocFormat::Yaml);
    assert_eq!(key, "a");
    assert_eq!(lit.as_deref(), Some("a"));
}

#[test]
fn toml_projects_decoded_key_not_the_quote_carrying_token() {
    // taplo lexes a quoted key as an IDENT whose text keeps the quotes; reading
    // it raw used to leak `"a b"` (quotes included) into `Seg::Key`.
    let (key, lit) = literal_of("\"a b\" = 1\n", DocFormat::Toml);
    assert_eq!(key, "a b");
    assert_eq!(lit.as_deref(), Some("\"a b\""));

    let doc = AnyDocument::from_str_as("\"a b\" = 1\n", DocFormat::Toml).unwrap();
    let child = &doc.project().root.children[0];
    assert_eq!(
        child.path,
        vec![Seg::Key("a b".into())],
        "the path segment must be the decoded key"
    );
}

#[test]
fn json_carries_no_literal_because_its_keys_are_always_quoted() {
    let (key, lit) = literal_of("{\"a b\": 1}\n", DocFormat::Json);
    assert_eq!(key, "a b");
    assert_eq!(
        lit, None,
        "JSON keys are unconditionally quoted; a literal would be redundant \
         on every row and would feed `\"key\"` into a re-quoting rename"
    );
}

// ── Display: the authored quote style survives ──────────────────────────────

#[test]
fn path_display_uses_the_authored_quote_style() {
    for (src, fmt, expected) in [
        ("'a b': 1\n", DocFormat::Yaml, "'a b'"),
        ("\"a b\": 1\n", DocFormat::Yaml, "\"a b\""),
        ("\"a b\" = 1\n", DocFormat::Toml, "\"a b\""),
        ("a: 1\n", DocFormat::Yaml, "a"),
    ] {
        let s = session(src, fmt);
        let row = s
            .visible_rows()
            .into_iter()
            .find(|r| !r.key.is_empty())
            .unwrap();
        assert_eq!(row.path_display, expected, "path_display for {src:?}");
    }
}

#[test]
fn a_single_quoted_key_is_never_shown_with_double_quotes() {
    // Three call sites used to hardcode `'"'`, so every quoted key rendered
    // with double quotes regardless of how it was written.
    let s = session("'a b': 1\n", DocFormat::Yaml);
    let row = s
        .visible_rows()
        .into_iter()
        .find(|r| r.key == "a b")
        .unwrap();
    assert_eq!(row.path_display, "'a b'");
    assert_eq!(row.key_literal.as_deref(), Some("'a b'"));
    assert!(
        !row.path_display.contains('"'),
        "no double quote may be synthesized: {}",
        row.path_display
    );
}

#[test]
fn toml_quoted_key_path_display_is_not_double_quoted() {
    let s = session("\"a b\" = 1\n", DocFormat::Toml);
    let row = s
        .visible_rows()
        .into_iter()
        .find(|r| r.key == "a b")
        .unwrap();
    assert_eq!(row.path_display, "\"a b\"");
    assert!(!row.path_display.contains("\"\""), "must not double-wrap");
}

// ── Conversion: the decoded key is what crosses formats ─────────────────────

#[test]
fn toml_quoted_key_converts_to_json_without_baking_in_quotes() {
    let doc = AnyDocument::from_str_as("\"a b\" = 1\n", DocFormat::Toml).unwrap();
    let out = confy_core::model::convert::convert(&doc, DocFormat::Json)
        .expect("TOML→JSON must succeed")
        .text;
    assert!(
        out.contains("\"a b\""),
        "expected a plain `\"a b\"` JSON key: {out}"
    );
    assert!(
        !out.contains("\\\""),
        "the quotes must not be baked into the key: {out}"
    );
}

// ── Editing: the buffer carries the authored spelling ───────────────────────

#[test]
fn rename_buffer_is_seeded_with_the_authored_spelling() {
    for (src, expected) in [("'a b': 1\n", "'a b'"), ("\"a b\": 1\n", "\"a b\"")] {
        let mut s = session(src, DocFormat::Yaml);
        s.cursor = vec![Seg::Key("a b".into())];
        s.begin_inline_rename();
        match &s.mode {
            Mode::Edit(st) => assert_eq!(
                st.buffer, expected,
                "rename buffer for {src:?} must carry the source quotes"
            ),
            _ => panic!("expected Edit mode for {src:?}"),
        }
    }
}

#[test]
fn a_no_op_rename_leaves_the_source_byte_identical() {
    // The buffer round-trips its own spelling, so committing an untouched
    // rename must not restyle the key.
    for src in [
        "'a b': 1\n",
        "\"a b\": 1\n",
        "\"a\\x20b\": 1\n",
        "'it''s': 1\n",
    ] {
        let mut s = session(src, DocFormat::Yaml);
        let key = s.doc.as_ref().unwrap().project().root.children[0]
            .key
            .clone();
        s.cursor = vec![Seg::Key(key)];
        s.begin_inline_rename();
        s.edit_commit();
        assert_eq!(s.serialize().unwrap(), src, "no-op rename altered {src:?}");
    }
}

#[test]
fn a_value_only_edit_does_not_drop_a_quoted_key() {
    let mut s = session("'a b': 1\nz: 9\n", DocFormat::Yaml);
    s.cursor = vec![Seg::Key("a b".into())];
    s.begin_inline_edit();
    s.edit_backspace();
    s.edit_input_char('7');
    s.edit_commit();
    assert_eq!(s.serialize().unwrap(), "'a b': 7\nz: 9\n");
}

// ── Escape decoding drives collision detection ──────────────────────────────

#[test]
fn hex_and_unicode_escapes_decode_so_duplicate_keys_are_caught() {
    // In YAML `"a\x20b"` IS the key `a b`. While the decoder ignored `\x`, the
    // two were different `Seg::Key`s and this rename silently produced a
    // document holding two identical keys.
    let (key, _) = literal_of("\"a\\x20b\": 1\n", DocFormat::Yaml);
    assert_eq!(key, "a b", "\\x20 must decode to a space");

    let src = "\"a\\x20b\": 1\nz: 9\n";
    let mut s = session(src, DocFormat::Yaml);
    s.cursor = vec![Seg::Key("z".into())];
    s.begin_inline_rename();
    s.edit_backspace();
    for c in "a b".chars() {
        s.edit_input_char(c);
    }
    s.edit_commit();
    assert_eq!(
        s.serialize().unwrap(),
        src,
        "renaming into an escape-equal key must be rejected, leaving the source untouched"
    );
    assert!(
        matches!(s.mode, Mode::Edit(_)),
        "a rejected rename stays in Edit mode so the user can correct it"
    );
}

#[test]
fn single_quoted_yaml_doubling_decodes_to_one_quote() {
    let (key, lit) = literal_of("'it''s': 1\n", DocFormat::Yaml);
    assert_eq!(key, "it's");
    assert_eq!(lit.as_deref(), Some("'it''s'"));
}

// ── External edit round-trips under a quoted key ────────────────────────────

#[test]
fn external_edit_round_trips_under_a_quoted_key() {
    for src in ["\"a b\": 1\nz: 9\n", "'a b': 1\nz: 9\n"] {
        let mut s = session(src, DocFormat::Yaml);
        let path = vec![Seg::Key("a b".into())];
        let seed = s.doc.as_ref().unwrap().serialize_fragment(&path);

        // Committing the fragment unchanged must be a no-op.
        s.apply_external_replace(path.clone(), seed.clone());
        assert_eq!(
            s.serialize().unwrap(),
            src,
            "unchanged external edit of {src:?}"
        );

        // Changing only the value must keep the key's spelling and the sibling.
        let mut s2 = session(src, DocFormat::Yaml);
        s2.apply_external_replace(path, seed.replace(": 1", ": 42"));
        let out = s2.serialize().unwrap();
        assert_eq!(out, src.replace(": 1", ": 42"), "value edit of {src:?}");
        assert!(out.contains("z: 9"), "sibling lost: {out}");
    }
}

#[test]
fn external_edit_round_trips_under_a_quoted_container_key() {
    for src in ["\"a b\":\n  c: 1\nz: 9\n", "'a b':\n  c: 1\nz: 9\n"] {
        let mut s = session(src, DocFormat::Yaml);
        let path = vec![Seg::Key("a b".into())];
        let seed = s.doc.as_ref().unwrap().serialize_fragment(&path);
        s.apply_external_replace(path, seed.replace("c: 1", "c: 2"));
        let out = s.serialize().unwrap();
        assert_eq!(
            out,
            src.replace("c: 1", "c: 2"),
            "container edit of {src:?}"
        );
        assert!(out.contains("z: 9"), "sibling lost: {out}");
    }
}

// ── Re-anchoring the path after a rename ────────────────────────────────────
//
// A rename writes `new_key` VERBATIM but a projected path is built from DECODED
// segments. Setting the path's leaf to the raw literal made every later
// `node_at` miss, which surfaced as a spurious type-change prompt followed by
// "path not found" — while the file itself was already correctly modified.

#[test]
fn adding_quotes_to_a_key_leaves_the_cursor_on_the_decoded_path() {
    for (src, fmt, typed, expect) in [
        ("a = 1\n", DocFormat::Toml, "\"a\"", "\"a\" = 1\n"),
        ("a: 1\n", DocFormat::Yaml, "\"a\"", "\"a\": 1\n"),
        ("a: 1\n", DocFormat::Yaml, "'a'", "'a': 1\n"),
    ] {
        let mut s = session(src, fmt);
        s.cursor = vec![Seg::Key("a".into())];
        s.begin_inline_rename();
        for _ in 0..8 {
            s.edit_backspace();
        }
        for c in typed.chars() {
            s.edit_input_char(c);
        }
        s.edit_commit();
        assert_eq!(
            s.serialize().unwrap(),
            expect,
            "output for {src:?} + {typed:?}"
        );
        assert_eq!(
            s.cursor,
            vec![Seg::Key("a".into())],
            "cursor must hold the DECODED key after {typed:?}"
        );
        assert!(
            s.snapshot().error_text().is_none(),
            "unexpected notice for {typed:?}: {:?}",
            s.snapshot().error_text()
        );
    }
}

#[test]
fn panel_rename_adding_quotes_does_not_prompt_or_fail() {
    // The detail panel commits key+value together (not `rename_only`), so it
    // walked straight into the stale-path type check. Every format regressed.
    for (src, fmt, typed, value, expect) in [
        ("a = 1\n", DocFormat::Toml, "\"a\"", "1", "\"a\" = 1\n"),
        ("a: 1\n", DocFormat::Yaml, "\"a\"", "1", "\"a\": 1\n"),
        ("a: 1\n", DocFormat::Yaml, "'a'", "1", "'a': 1\n"),
        (
            "a = \"hi\"\n",
            DocFormat::Toml,
            "\"a\"",
            "\"hi\"",
            "\"a\" = \"hi\"\n",
        ),
    ] {
        let mut s = session(src, fmt);
        s.cursor = vec![Seg::Key("a".into())];
        s.begin_inline_edit();
        s.commit_edit(Some(value.to_string()), Some(typed.to_string()));
        assert!(
            !matches!(s.mode, Mode::Prompt(_)),
            "quoting a key is not a type change; got a prompt for {src:?} + {typed:?}"
        );
        assert_eq!(
            s.snapshot().error_text(),
            None,
            "unexpected error for {src:?} + {typed:?}"
        );
        assert_eq!(
            s.serialize().unwrap(),
            expect,
            "output for {src:?} + {typed:?}"
        );
    }
}

#[test]
fn removing_quotes_and_requoting_both_re_anchor_correctly() {
    // Reverse direction, and quoted -> differently-quoted.
    let mut s = session("\"a\" = 1\n", DocFormat::Toml);
    s.cursor = vec![Seg::Key("a".into())];
    s.begin_inline_rename();
    for _ in 0..8 {
        s.edit_backspace();
    }
    s.edit_input_char('a');
    s.edit_commit();
    assert_eq!(s.serialize().unwrap(), "a = 1\n");
    assert_eq!(s.cursor, vec![Seg::Key("a".into())]);

    let mut y = session("'a b': 1\n", DocFormat::Yaml);
    y.cursor = vec![Seg::Key("a b".into())];
    y.begin_inline_rename();
    for _ in 0..8 {
        y.edit_backspace();
    }
    for c in "'c d'".chars() {
        y.edit_input_char(c);
    }
    y.edit_commit();
    assert_eq!(y.serialize().unwrap(), "'c d': 1\n");
    assert_eq!(
        y.cursor,
        vec![Seg::Key("c d".into())],
        "cursor must follow to the new DECODED key"
    );
}

#[test]
fn rename_key_segs_decodes_without_splitting_a_quoted_dot() {
    use confy_core::model::document::ConfigDocument;
    let toml = AnyDocument::from_str_as("x = 1\n", DocFormat::Toml).unwrap();
    // A dotted rename really is several segments...
    assert_eq!(
        toml.rename_key_segs("a.b"),
        vec!["a".to_string(), "b".to_string()]
    );
    // ...but a quoted key containing a dot is ONE (the old `split('.')` broke
    // this, shattering the path and mangling the written leaf).
    assert_eq!(toml.rename_key_segs("\"a.b\""), vec!["a.b".to_string()]);
    assert_eq!(toml.rename_key_segs("\"a b\""), vec!["a b".to_string()]);

    let yaml = AnyDocument::from_str_as("x: 1\n", DocFormat::Yaml).unwrap();
    assert_eq!(yaml.rename_key_segs("'a.b'"), vec!["a.b".to_string()]);
    assert_eq!(yaml.rename_key_segs("\"a\\x20b\""), vec!["a b".to_string()]);
    // YAML keys carry no structure: a dot is just a character.
    assert_eq!(yaml.rename_key_segs("a.b"), vec!["a.b".to_string()]);
}
