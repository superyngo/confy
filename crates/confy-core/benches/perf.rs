//! Dependency-free perf harness for the hot paths the 2026-08-29 optimization
//! audit identified. Deliberately **not** criterion: this crate keeps a tight
//! dependency graph (pinned `rowan`, single-version core deps), and comparative
//! before/after numbers do not need criterion's statistical machinery.
//!
//! Run with:
//!   cargo bench -p confy-core
//!   cargo bench -p confy-core --bench perf -- --nodes 5000
//!
//! `--bench perf` is required when passing `--nodes`: without it the args reach
//! the lib test binary first, which rejects the flag.
//!
//! Each case reports the median of N iterations. Compare runs, not absolutes —
//! the numbers are only meaningful against another run on the same machine.

use std::time::{Duration, Instant};

use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::{ConfigDocument, DocFormat, Mutation};
use confy_core::model::node::Seg;
use confy_core::session::session::Session;

/// A synthetic TOML document with `sections` tables of 8 scalars each, plus a
/// multiline array and a comment per section — shaped to exercise the
/// projection (nested paths, decor, mixed scalar types) rather than a flat
/// best case.
fn gen_toml(sections: usize) -> String {
    let mut s = String::new();
    for i in 0..sections {
        s.push_str(&format!("# section {i}\n[svc_{i}]\n"));
        s.push_str(&format!("name = \"service-{i}\"\n"));
        s.push_str(&format!("port = {}\n", 8000 + i));
        s.push_str("enabled = true\n");
        s.push_str(&format!("ratio = {}.5  # tuned\n", i % 7));
        s.push_str(&format!("tag = 'literal-{i}'\n"));
        s.push_str(&format!("hex = 0x{:x}\n", i));
        s.push_str(&format!("note = \"a longer string value for {i}\"\n"));
        s.push_str(&format!("retries = {}\n", i % 5));
        s.push_str("hosts = [\n  \"alpha\",\n  \"beta\",\n  \"gamma\",\n]\n\n");
    }
    s
}

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

/// Time `f` `iters` times and report the median.
fn bench(label: &str, iters: usize, mut f: impl FnMut()) {
    // One untimed warm-up pass so first-call effects (allocator growth, cache
    // population) do not land in the reported median.
    f();
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t = Instant::now();
        f();
        samples.push(t.elapsed());
    }
    let m = median(samples);
    println!("  {label:<44} {:>10.3?}", m);
}

fn main() {
    // `cargo bench` passes --bench; accept an optional --nodes override.
    let args: Vec<String> = std::env::args().collect();
    let sections: usize = args
        .iter()
        .position(|a| a == "--nodes")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);

    let src = gen_toml(sections);
    let doc = AnyDocument::from_str_as(&src, DocFormat::Toml).expect("generated TOML must parse");
    let node_count = {
        let t = doc.project();
        fn count(n: &confy_core::model::node::Node) -> usize {
            1 + n.children.iter().map(count).sum::<usize>()
        }
        count(&t.root)
    };
    println!(
        "\nconfy-core perf — {sections} sections, {} bytes, {node_count} projected nodes\n",
        src.len()
    );

    // ---- load + parse + project -------------------------------------------
    println!("load / project:");
    bench("AnyDocument::from_str_as (parse)", 20, || {
        let _ = AnyDocument::from_str_as(&src, DocFormat::Toml).unwrap();
    });
    bench("project() (rowan tree -> NodeTree)", 20, || {
        let _ = doc.project();
    });
    bench("serialize() (tree -> String)", 50, || {
        let _ = doc.serialize();
    });

    // ---- is_dirty (audit A1) ----------------------------------------------
    // Measured on a document that has been edited, which is the state the
    // `clean` fast-path flag stops covering — i.e. the real editing case.
    println!("\ndirty tracking:");
    let mut edited = AnyDocument::from_str_as(&src, DocFormat::Toml).unwrap();
    edited
        .apply(Mutation::Replace {
            path: vec![Seg::Key("svc_0".into()), Seg::Key("port".into())],
            fragment: edited.scalar_fragment(Some("port"), "9999"),
        })
        .expect("replace a scalar");
    bench("is_dirty() on an edited doc", 50, || {
        let _ = edited.is_dirty();
    });

    // ---- mutation hot path -------------------------------------------------
    println!("\nmutation (apply):");
    let mid = sections / 2;
    let scalar_path = vec![Seg::Key(format!("svc_{mid}")), Seg::Key("port".into())];
    let mut n = 0u32;
    bench("apply(Replace scalar)", 20, || {
        n = n.wrapping_add(1);
        let mut d = AnyDocument::from_str_as(&src, DocFormat::Toml).unwrap();
        d.apply(Mutation::Replace {
            path: scalar_path.clone(),
            fragment: d.scalar_fragment(Some("port"), &format!("{}", 9000 + n)),
        })
        .expect("replace must succeed");
    });
    bench("apply(Rename key)", 20, || {
        let mut d = AnyDocument::from_str_as(&src, DocFormat::Toml).unwrap();
        d.apply(Mutation::Rename {
            path: vec![Seg::Key(format!("svc_{mid}")), Seg::Key("name".into())],
            new_key: "renamed".into(),
        })
        .expect("rename must succeed");
    });

    // ---- session view projection (audit A4/B2/B3) --------------------------
    println!("\nsession view:");
    let mut sess = Session::new(AnyDocument::from_str_as(&src, DocFormat::Toml).unwrap());
    sess.expand_all();
    let visible = sess.visible_rows().len();
    println!("  ({visible} visible rows after expand_all)");
    bench("visible_rows() fully expanded", 30, || {
        let _ = sess.visible_rows();
    });

    let collapsed = Session::new(AnyDocument::from_str_as(&src, DocFormat::Toml).unwrap());
    bench("visible_rows() collapsed (top level)", 30, || {
        let _ = collapsed.visible_rows();
    });

    println!();
}
