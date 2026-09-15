//! The Block-edit contract, one row per case (design record
//! `docs/spec/2026-09-15-block-edit-whole-file-reparse-design.md` §8):
//!
//! > A multi-line edit is accepted if and only if its buffer parses, on its
//! > own, as a legal Node sequence at the edited Node's container level.
//! > Otherwise the whole commit is rejected and the document is untouched.
//! > Backends differ only where the format's grammar differs.
//!
//! Data-driven on purpose: a new shape is a row, not a function, and every
//! rejected row is checked against the same two invariants (document
//! byte-identical, `doc_revision` unmoved — the signal hosts actually read,
//! F4) instead of each test inventing its own.

use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::DocFormat;
use confy_core::model::node::Seg;
use confy_core::session::{Intent, Session};

struct Case {
    /// `<format>/<shape>/<outcome>` — also selects the backend.
    name: &'static str,
    src: &'static str,
    /// `"#N"` is `Seg::Index(N)`; anything else is a key.
    path: &'static [&'static str],
    buffer: &'static str,
    /// `Some(document)` = accepted with exactly this text; `None` = rejected.
    expect: Option<&'static str>,
}

const CASES: &[Case] = &[
    // ---- outcome: an unmodified buffer is byte-identical ----
    Case {
        name: "toml/leaf/unchanged",
        src: "[s]\nk1 = 1\n",
        path: &["s", "k1"],
        buffer: "k1 = 1\n",
        expect: Some("[s]\nk1 = 1\n"),
    },
    // ---- outcome: the key rename the 1:1 fragment mechanism could not do ----
    Case {
        name: "toml/leaf/rename-key",
        src: "[s]\nk1 = 1\nk2 = 2\n",
        path: &["s", "k1"],
        buffer: "renamed = 1\n",
        expect: Some("[s]\nrenamed = 1\nk2 = 2\n"),
    },
    // ---- outcome: N nodes out of one ----
    Case {
        name: "toml/leaf/two-siblings",
        src: "[s]\nk1 = 1\n",
        path: &["s", "k1"],
        buffer: "k1 = 1\nk3 = 3\n",
        expect: Some("[s]\nk1 = 1\nk3 = 3\n"),
    },
    // ---- outcome: Comment -> live node ----
    Case {
        name: "toml/comment/to-live",
        src: "# k = 1\nz = 9\n",
        path: &["#0"],
        buffer: "k = 1\n",
        expect: Some("k = 1\nz = 9\n"),
    },
    // ---- outcome: live node -> Comment ----
    Case {
        name: "toml/leaf/to-comment",
        src: "k = 1\nz = 9\n",
        path: &["k"],
        buffer: "# k = 1\n",
        expect: Some("# k = 1\nz = 9\n"),
    },
    // ---- outcome: rejected, document untouched ----
    Case {
        name: "toml/leaf/duplicate-key",
        src: "[s]\nk1 = 1\nk2 = 2\n",
        path: &["s", "k1"],
        buffer: "k2 = 7\n",
        expect: None,
    },
    Case {
        name: "toml/leaf/unparsable",
        src: "[s]\nk1 = 1\n",
        path: &["s", "k1"],
        buffer: "k1 = = =\n",
        expect: None,
    },
    // ---- outcome: an empty buffer is refused, never a delete ----
    Case {
        name: "toml/leaf/empty-buffer",
        src: "[s]\nk1 = 1\n",
        path: &["s", "k1"],
        buffer: "",
        expect: None,
    },
    Case {
        name: "toml/leaf/whitespace-only-buffer",
        src: "[s]\nk1 = 1\n",
        path: &["s", "k1"],
        buffer: "   \n\n",
        expect: None,
    },
    // ---- shape: branch section ----
    Case {
        name: "toml/section/rename-header",
        src: "[a]\nk = 1\n[b]\nm = 2\n",
        path: &["a"],
        buffer: "[c]\nk = 1\n",
        expect: Some("[c]\nk = 1\n[b]\nm = 2\n"),
    },
    // ---- shape: a scattered table consolidates at its first span ----
    Case {
        name: "toml/scattered-table/consolidates",
        src: "[a]\nk = 1\n[b]\nm = 2\n[a]\nj = 3\n",
        path: &["a"],
        buffer: "[a]\nk = 1\nj = 3\n",
        expect: Some("[a]\nk = 1\nj = 3\n[b]\nm = 2\n"),
    },
    // THREE runs, deliberately: with only two spans the "cut later spans in
    // reverse document order" rule is unobservable (`skip(1)` leaves one), so
    // a two-run case cannot fail when that rule is broken. Verified by
    // mutation: reversing the cut order is missed by every two-span case and
    // caught by this one.
    Case {
        name: "toml/scattered-table/three-runs",
        src: "[a]\nk = 1\n[b]\nm = 2\n[a]\nj = 3\n[c]\nq = 4\n[a]\nr = 5\n",
        path: &["a"],
        buffer: "[a]\nk = 1\nj = 3\nr = 5\n",
        expect: Some("[a]\nk = 1\nj = 3\nr = 5\n[b]\nm = 2\n[c]\nq = 4\n"),
    },
    // ---- shape: dotted table (also multi-span, three member lines) ----
    Case {
        name: "toml/dotted-table/consolidates",
        src: "a.x = 1\nb = 9\na.y = 2\nc = 8\na.z = 3\n",
        path: &["a"],
        buffer: "a.x = 1\na.y = 2\na.z = 3\n",
        expect: Some("a.x = 1\na.y = 2\na.z = 3\nb = 9\nc = 8\n"),
    },
    // ---- shape: array element ----
    Case {
        name: "toml/array-element/edit",
        src: "a = [ 1, 22 ]\n",
        path: &["a", "#1"],
        buffer: "33",
        expect: Some("a = [ 1, 33 ]\n"),
    },
    // ---- shape: flow member ----
    Case {
        name: "toml/inline-member/rename",
        src: "a = { x = 1, y = 2 }\n",
        path: &["a", "x"],
        buffer: "w = 1",
        expect: Some("a = { w = 1, y = 2 }\n"),
    },
    // ---- shape: AoT entry ----
    Case {
        name: "toml/aot-entry/rename-member",
        src: "[[p]]\nn = 1\n[[p]]\nn = 2\n",
        path: &["p", "#0"],
        buffer: "[[p]]\nm = 1\n",
        expect: Some("[[p]]\nm = 1\n[[p]]\nn = 2\n"),
    },
    // ---- format: JSON ----
    Case {
        name: "json/member/unchanged",
        src: "{\n  \"a\": 1,\n  \"b\": 2\n}\n",
        path: &["a"],
        buffer: "\"a\": 1,\n",
        expect: Some("{\n  \"a\": 1,\n  \"b\": 2\n}\n"),
    },
    Case {
        name: "json/member/rename-key",
        src: "{\n  \"a\": 1,\n  \"b\": 2\n}\n",
        path: &["a"],
        buffer: "\"renamed\": 1,\n",
        expect: Some("{\n  \"renamed\": 1,\n  \"b\": 2\n}\n"),
    },
    Case {
        name: "json/member/duplicate-key",
        src: "{\n  \"a\": 1,\n  \"b\": 2\n}\n",
        path: &["a"],
        buffer: "\"b\": 7,\n",
        expect: None,
    },
    Case {
        name: "json/member/two-siblings",
        src: "{\n  \"a\": 1\n}\n",
        path: &["a"],
        buffer: "\"a\": 1,\n  \"c\": 3\n",
        expect: Some("{\n  \"a\": 1,\n  \"c\": 3\n}\n"),
    },
    Case {
        name: "json/member/empty-buffer",
        src: "{\n  \"a\": 1\n}\n",
        path: &["a"],
        buffer: "",
        expect: None,
    },
    // ---- format: YAML ----
    Case {
        name: "yaml/leaf/unchanged",
        src: "a:\n  b: 1\n  c: 2\n",
        path: &["a", "b"],
        buffer: "  b: 1\n",
        expect: Some("a:\n  b: 1\n  c: 2\n"),
    },
    Case {
        name: "yaml/leaf/rename-key-keeps-indentation",
        src: "a:\n  b: 1\n  c: 2\n",
        path: &["a", "b"],
        buffer: "  renamed: 1\n",
        expect: Some("a:\n  renamed: 1\n  c: 2\n"),
    },
    Case {
        name: "yaml/leaf/two-siblings",
        src: "a:\n  b: 1\n",
        path: &["a", "b"],
        buffer: "  b: 1\n  d: 4\n",
        expect: Some("a:\n  b: 1\n  d: 4\n"),
    },
    Case {
        name: "yaml/leaf/empty-buffer",
        src: "a:\n  b: 1\n",
        path: &["a", "b"],
        buffer: "",
        expect: None,
    },
    // The opaque policy: read-only *structurally*, editable as text.
    Case {
        name: "yaml/opaque/text-edit-allowed",
        src: "ref: *anchor\nk: 1\n",
        path: &["ref"],
        buffer: "ref: *anchor  # keep\n",
        expect: Some("ref: *anchor  # keep\nk: 1\n"),
    },
];

fn seg(s: &str) -> Seg {
    match s.strip_prefix('#').and_then(|n| n.parse::<usize>().ok()) {
        Some(i) => Seg::Index(i),
        None => Seg::Key(s.to_string()),
    }
}

fn format_of(name: &str) -> DocFormat {
    match name.split('/').next().unwrap() {
        "toml" => DocFormat::Toml,
        "json" => DocFormat::Json,
        "yaml" => DocFormat::Yaml,
        other => panic!("unknown format prefix `{other}` in case name `{name}`"),
    }
}

#[test]
fn block_edit_matrix() {
    let mut failures: Vec<String> = Vec::new();
    for c in CASES {
        let doc = AnyDocument::from_str_as(c.src, format_of(c.name)).expect(c.name);
        let mut s = Session::new(doc);
        let rev_before = s.doc_revision;
        s.dispatch(Intent::ApplyBlockText {
            path: c.path.iter().copied().map(seg).collect(),
            text: c.buffer.to_string(),
        });
        let got = s.serialize().unwrap_or_default();
        match c.expect {
            Some(want) => {
                if got != want {
                    failures.push(format!(
                        "{}: wrong result\n  got  {got:?}\n  want {want:?}",
                        c.name
                    ));
                }
            }
            None => {
                if got != c.src {
                    failures.push(format!(
                        "{}: rejected but the document changed\n  got  {got:?}\n  want {:?}",
                        c.name, c.src
                    ));
                }
                // The signal hosts read to decide "did it land?" (F4).
                if s.doc_revision != rev_before {
                    failures.push(format!("{}: rejected but doc_revision moved", c.name));
                }
                match &s.notice {
                    Some(n) if n.text.contains("block") => {}
                    other => failures.push(format!(
                        "{}: expected a core.block.* notice, got {other:?}",
                        c.name
                    )),
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases failed:\n{}",
        failures.len(),
        CASES.len(),
        failures.join("\n---\n")
    );
}

/// A key rename makes the pre-edit path vanish, so the commit takes the §5
/// span re-anchor. That re-anchor used to ask the backend for **every** node's
/// spans, and each such query serializes + projects the whole document: this
/// document took 20.9s before, against 26ms for the same edit that keeps its
/// key. The wall-clock assertion is the point of the test — a correct
/// re-anchor is linear, so the bound is loose enough to survive a slow CI box
/// and still fail the quadratic version by three orders of magnitude.
#[test]
fn a_rename_reanchor_stays_linear_on_a_large_document() {
    let mut src = String::from("title = \"perf\"\n\n");
    for i in 0..1000 {
        src.push_str(&format!(
            "[sec{i}]\nname = \"s {i}\"\nport = {}\n\n",
            8000 + i
        ));
    }
    let doc = AnyDocument::from_str_as(&src, DocFormat::Toml).unwrap();
    let mut s = Session::new(doc);
    let path = vec![Seg::Key("sec500".into()), Seg::Key("port".into())];
    // The re-anchor only shows in `cursor` once the row is visible: the
    // trailing `compute_rows` snaps a cursor inside a collapsed branch to the
    // first row, which would mask it.
    s.dispatch(Intent::ExpandAll);
    assert_eq!(s.block_text(&path), "port = 8500\n\n");

    let t = std::time::Instant::now();
    s.dispatch(Intent::ApplyBlockText {
        path,
        text: "renamed = 8500\n\n".to_string(),
    });
    let elapsed = t.elapsed();

    assert!(
        s.serialize().unwrap_or_default().contains("renamed = 8500"),
        "the rename did not land: {:?}",
        s.notice
    );
    // The cursor followed the renamed node (§5's span re-anchor).
    assert_eq!(s.cursor.last(), Some(&Seg::Key("renamed".into())));
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "rename re-anchor took {elapsed:?} — the per-node span query is back"
    );
}
