//! Hostile / pathological input must produce a parse error, never a crash.
//! Every backend is recursive descent, so unbounded nesting used to overflow the
//! stack (a hard abort natively, a page-killing trap in wasm). The shared cap is
//! `confy_core::model::MAX_NESTING_DEPTH`; these tests pin the boundary on both
//! sides for load and for the `$EDITOR`-style whole-document Replace.

use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::{ConfigDocument, DocFormat, Mutation};
use confy_core::model::MAX_NESTING_DEPTH;

const OVER: usize = MAX_NESTING_DEPTH + 1;
const OVERFLOW_PROBE: usize = 200_000; // would overflow every host's stack

fn nested(open: &str, close: &str, n: usize) -> String {
    format!("{}{}", open.repeat(n), close.repeat(n))
}

fn cases(n: usize) -> Vec<(DocFormat, String)> {
    vec![
        (DocFormat::Json, nested("[", "]", n)),
        (
            DocFormat::Json,
            format!("{}1{}", "{\"a\":".repeat(n), "}".repeat(n)),
        ),
        (DocFormat::Yaml, format!("a: {}\n", nested("[", "]", n))),
        (DocFormat::Yaml, format!("a: {}\n", nested("{a: ", "}", n))),
        (DocFormat::Toml, format!("a = {}\n", nested("[", "]", n))),
        (
            DocFormat::Toml,
            format!("a = {}1{}\n", "{a = ".repeat(n), "}".repeat(n)),
        ),
    ]
}

#[test]
fn nesting_at_the_cap_loads() {
    for (fmt, src) in cases(MAX_NESTING_DEPTH - 1) {
        if let Err(e) = AnyDocument::from_str_as(&src, fmt) {
            panic!("{fmt:?}: {} levels must load: {e}", MAX_NESTING_DEPTH - 1);
        }
    }
}

#[test]
fn nesting_over_the_cap_is_a_parse_error() {
    for (fmt, src) in cases(OVER) {
        let err = AnyDocument::from_str_as(&src, fmt)
            .err()
            .unwrap_or_else(|| panic!("{fmt:?}: {OVER} levels must be rejected"));
        assert!(
            err.to_string().contains("nesting deeper than"),
            "{fmt:?}: unexpected error text: {err}"
        );
    }
}

#[test]
fn pathological_nesting_does_not_overflow_the_stack() {
    for (fmt, src) in cases(OVERFLOW_PROBE) {
        assert!(AnyDocument::from_str_as(&src, fmt).is_err(), "{fmt:?}");
    }
}

#[test]
fn whole_document_replace_with_pathological_nesting_is_rejected_atomically() {
    let seeds = [
        (DocFormat::Json, "{\"a\": 1}\n"),
        (DocFormat::Yaml, "a: 1\n"),
        (DocFormat::Toml, "a = 1\n"),
    ];
    for (fmt, seed) in seeds {
        let mut doc = AnyDocument::from_str_as(seed, fmt).unwrap();
        let hostile = cases(OVERFLOW_PROBE)
            .into_iter()
            .find(|(f, _)| *f == fmt)
            .unwrap()
            .1;
        let res = doc.apply(Mutation::Replace {
            path: vec![],
            fragment: hostile,
        });
        assert!(res.is_err(), "{fmt:?}: hostile replace must fail");
        assert_eq!(
            doc.serialize(),
            seed,
            "{fmt:?}: failed replace must leave the doc untouched"
        );
        assert!(!doc.is_dirty(), "{fmt:?}");
    }
}
