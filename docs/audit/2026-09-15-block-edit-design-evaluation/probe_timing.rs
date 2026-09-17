use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::{ConfigDocument, DocFormat, Mutation};
use confy_core::model::node::Seg;
use std::time::Instant;

fn gen(sections: usize) -> String {
    let mut s = String::new();
    s.push_str("title = \"perf\"\n\n");
    for i in 0..sections {
        s.push_str(&format!("[sec{i}]\n"));
        s.push_str(&format!("name = \"section {i}\"  # note {i}\n"));
        s.push_str(&format!("port = {}\n", 8000 + i));
        s.push_str("flags = [1, 2, 3]\n");
        s.push_str(&format!("[sec{i}.sub]\nk = {i}\n\n"));
    }
    s
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

fn main() {
    let sections: usize = std::env::args()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(5000);
    let src = gen(sections);
    println!(
        "doc: {} sections, {} bytes",
        sections,
        src.len()
    );
    let reps = 7;

    // Mid-document leaf, so neither route benefits from being near the start.
    let mid = sections / 2;
    let path = vec![Seg::Key(format!("sec{mid}")), Seg::Key("port".into())];

    // (a) today's per-node block-edit commit: Replace at the node's path.
    let mut ts = vec![];
    for _ in 0..reps {
        let mut d = AnyDocument::from_str_as(&src, DocFormat::Toml).unwrap();
        let t = Instant::now();
        d.apply(Mutation::Replace {
            path: path.clone(),
            fragment: "port = 1  # note\n".into(),
        })
        .unwrap();
        ts.push(t.elapsed().as_secs_f64() * 1e3);
    }
    println!("(a) apply(Replace @ leaf path)        {:8.2} ms", median(ts));

    // (b) proposed: text-splice the buffer over the node's byte span, then the
    //     existing whole-document Replace (parse + DOM validate + commit).
    //     The span lookup is modelled with a plain text search — the real
    //     implementation reads it off the CST, which is strictly cheaper.
    let needle = format!("port = {}\n", 8000 + mid);
    let mut ts = vec![];
    for _ in 0..reps {
        let mut d = AnyDocument::from_str_as(&src, DocFormat::Toml).unwrap();
        let t = Instant::now();
        let text = d.serialize();
        let start = text.find(&needle).unwrap();
        let end = start + needle.len();
        let mut whole = String::with_capacity(text.len() + 16);
        whole.push_str(&text[..start]);
        whole.push_str("port = 1  # note\n");
        whole.push_str(&text[end..]);
        d.apply(Mutation::Replace {
            path: vec![],
            fragment: whole,
        })
        .unwrap();
        ts.push(t.elapsed().as_secs_f64() * 1e3);
    }
    println!("(b) splice + apply(Replace @ [])      {:8.2} ms", median(ts));

    // (c) the part both routes share afterwards, for scale.
    let d = AnyDocument::from_str_as(&src, DocFormat::Toml).unwrap();
    let mut ts = vec![];
    for _ in 0..reps {
        let t = Instant::now();
        let _ = d.project();
        ts.push(t.elapsed().as_secs_f64() * 1e3);
    }
    println!("(c) project() (paid by both)         {:8.2} ms", median(ts));
}
