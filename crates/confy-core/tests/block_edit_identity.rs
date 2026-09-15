//! Design record §8's sweep invariant: returning a Block's buffer
//! **unmodified** must leave the document byte-identical.
//!
//! The sweep is **exhaustive over the repo's own fixtures × every node in
//! them** (779 Blocks at the time of writing), not a random sample: a
//! fixture's path space is finite and small, so enumerating it is both
//! stronger than sampling and deterministic — a failure names the exact file
//! and path instead of a shrunk case. `roundtrip_proptest.rs`'s fixture list
//! is the corpus.
//!
//! **What this cannot catch, measured by mutation, so nobody trusts it for
//! more than it does:** `block_text` and `apply_block_text` both read
//! `node_text_spans`, so a *symmetric* span error cancels — the text cut out
//! equals the text put back. Adding 1 to every TOML span end leaves this file
//! green. Span *boundaries* are pinned by the per-backend unit tests
//! (`cst_edit::spans`, `json::edit::spans`, `yaml::edit::spans`), which assert
//! the exact slice, and by `block_edit_parity.rs`, which asserts the exact
//! resulting document.
//!
//! What it does catch: anything **asymmetric** — a panic, an out-of-bounds or
//! inverted range, a multi-span set that overlaps or is spliced in the wrong
//! order, and any backend whose `serialize()` is not a fixed point of its own
//! parse.

use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::{ConfigDocument, DocFormat};
use confy_core::model::node::{Node, Path};
use confy_core::session::{Intent, Session};

fn toml_fixtures() -> Vec<(&'static str, &'static str)> {
    vec![
        ("sample.toml", include_str!("fixtures/sample.toml")),
        ("test.toml", include_str!("fixtures/test.toml")),
    ]
}

fn json_fixtures() -> Vec<(&'static str, &'static str)> {
    vec![
        ("sample.json", include_str!("fixtures/sample.json")),
        ("test.json", include_str!("fixtures/test.json")),
        ("edgecases.json", include_str!("fixtures/edgecases.json")),
        ("root_array.json", include_str!("fixtures/root_array.json")),
        ("sample.jsonc", include_str!("fixtures/sample.jsonc")),
        ("comments.jsonc", include_str!("fixtures/comments.jsonc")),
    ]
}

fn yaml_fixtures() -> Vec<(&'static str, &'static str)> {
    // multi-doc.yaml is excluded for the same reason `roundtrip_yaml.rs`
    // excludes it: multi-document YAML is rejected at parse.
    vec![
        ("sample.yaml", include_str!("fixtures/sample.yaml")),
        ("test.yaml", include_str!("fixtures/test.yaml")),
        ("flow-style", include_str!("fixtures/yaml/flow-style.yaml")),
        (
            "github-actions",
            include_str!("fixtures/yaml/github-actions.yaml"),
        ),
        (
            "helm-values",
            include_str!("fixtures/yaml/helm-values.yaml"),
        ),
        ("prometheus", include_str!("fixtures/yaml/prometheus.yaml")),
        ("scalars", include_str!("fixtures/yaml/scalars.yaml")),
        (
            "simple-config",
            include_str!("fixtures/yaml/simple-config.yaml"),
        ),
        (
            "tags-and-anchors",
            include_str!("fixtures/yaml/tags-and-anchors.yaml"),
        ),
        ("comments", include_str!("fixtures/yaml/comments.yaml")),
        ("deployment", include_str!("fixtures/yaml/deployment.yaml")),
        (
            "docker-compose",
            include_str!("fixtures/yaml/docker-compose.yaml"),
        ),
    ]
}

fn collect_paths(n: &Node, out: &mut Vec<Path>) {
    for c in &n.children {
        out.push(c.path.clone());
        collect_paths(c, out);
    }
}

/// Every node path in `src`, in document order.
fn all_paths(src: &str, fmt: DocFormat) -> Vec<Path> {
    let doc = AnyDocument::from_str_as(src, fmt).expect("fixture parses");
    let tree = doc.project();
    let mut out = Vec::new();
    collect_paths(&tree.root, &mut out);
    out
}

fn sweep(fixtures: Vec<(&'static str, &'static str)>, fmt: DocFormat) {
    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0usize;
    for (name, src) in fixtures {
        for path in all_paths(src, fmt) {
            let doc = AnyDocument::from_str_as(src, fmt).expect("fixture parses");
            let mut s = Session::new(doc);
            let buffer = s.block_text(&path);
            if buffer.is_empty() {
                // The node owns no Block (a shape this backend cannot express
                // as text); `apply_block_text` refuses it, so there is nothing
                // to round-trip.
                continue;
            }
            checked += 1;
            s.dispatch(Intent::ApplyBlockText {
                path: path.clone(),
                text: buffer.clone(),
            });
            let got = s.serialize().unwrap_or_default();
            if got != src {
                failures.push(format!(
                    "{name} at {path:?}\n  buffer {buffer:?}\n  first diff at byte {}",
                    got.bytes()
                        .zip(src.bytes())
                        .position(|(a, b)| a != b)
                        .map_or_else(|| "EOF (length differs)".to_string(), |i| i.to_string())
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {checked} Blocks were not byte-identical:\n{}",
        failures.len(),
        failures.join("\n---\n")
    );
    assert!(
        checked > 0,
        "swept nothing — the corpus or `block_text` broke"
    );
}

#[test]
fn toml_blocks_round_trip_byte_identically() {
    sweep(toml_fixtures(), DocFormat::Toml);
}

#[test]
fn json_blocks_round_trip_byte_identically() {
    sweep(json_fixtures(), DocFormat::Json);
}

#[test]
fn yaml_blocks_round_trip_byte_identically() {
    sweep(yaml_fixtures(), DocFormat::Yaml);
}
