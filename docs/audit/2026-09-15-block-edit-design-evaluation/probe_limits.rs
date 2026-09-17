use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::DocFormat;
use confy_core::model::node::Seg;
use confy_core::session::{Intent, Session};

fn k(s: &str) -> Seg {
    Seg::Key(s.into())
}

/// Simulate the host's popup / $EDITOR round trip: open the buffer the core
/// produces, hand back `edited`, print what landed.
fn probe(label: &str, src: &str, fmt: DocFormat, path: Vec<Seg>, edited: &str) {
    let doc = AnyDocument::from_str_as(src, fmt).unwrap();
    let mut s = Session::new(doc);
    s.dispatch(Intent::SetCursor(path.clone()));
    let buf = s.multiline_edit_initial(&path);
    let snap = s.dispatch(Intent::ApplyReplace {
        path,
        text: edited.to_string(),
    });
    let out = s.serialize().unwrap();
    println!("=== {label} [{fmt:?}]");
    println!("--- src\n{src}");
    println!("--- buffer opened\n{buf}");
    println!("--- edited buffer\n{edited}");
    println!("--- result\n{out}");
    println!(
        "--- notice: {:?}",
        snap.notice.as_ref().map(|n| format!("{n:?}"))
    );
    println!();
}

fn main() {
    // 1. Leaf: user renames the key inside the popup buffer.
    probe(
        "leaf key rename in buffer",
        "a = 1\nb = 2\n",
        DocFormat::Toml,
        vec![k("a")],
        "renamed = 1\n",
    );
    probe(
        "leaf key rename in buffer",
        "a: 1\nb: 2\n",
        DocFormat::Yaml,
        vec![k("a")],
        "renamed: 1\n",
    );
    probe(
        "leaf key rename in buffer",
        "{\n  \"a\": 1,\n  \"b\": 2\n}\n",
        DocFormat::Json,
        vec![k("a")],
        "\"renamed\": 1",
    );

    // 2. Leaf: user adds a sibling line inside the popup buffer.
    probe(
        "leaf + sibling line in buffer",
        "a = 1\nb = 2\n",
        DocFormat::Toml,
        vec![k("a")],
        "a = 1\nnew = 9\n",
    );
    probe(
        "leaf + sibling line in buffer",
        "a: 1\nb: 2\n",
        DocFormat::Yaml,
        vec![k("a")],
        "a: 1\nnew: 9\n",
    );
    probe(
        "leaf + sibling line in buffer",
        "{\n  \"a\": 1,\n  \"b\": 2\n}\n",
        DocFormat::Json,
        vec![k("a")],
        "\"a\": 1,\n  \"new\": 9",
    );

    // 3. Section: rename the header to a fresh name (single contiguous span).
    probe(
        "section header rename in buffer",
        "[a]\nx = 1\n\n[b]\ny = 2\n",
        DocFormat::Toml,
        vec![k("a")],
        "[c]\nx = 1\n",
    );

    // 4. Section: add a new sibling *section* inside the buffer.
    probe(
        "section + sibling section in buffer",
        "[a]\nx = 1\n\n[b]\ny = 2\n",
        DocFormat::Toml,
        vec![k("a")],
        "[a]\nx = 1\n\n[z]\nq = 3\n",
    );

    // 5. Branch (table) whose buffer adds a child — the documented-legal case.
    probe(
        "section + new child in buffer",
        "[a]\nx = 1\n",
        DocFormat::Toml,
        vec![k("a")],
        "[a]\nx = 1\nnew = 2\n",
    );

    // 6. YAML block map branch: rename its key + add a sibling map.
    probe(
        "block-map branch rename in buffer",
        "srv:\n  host: a\nother: 1\n",
        DocFormat::Yaml,
        vec![k("srv")],
        "renamed:\n  host: a\n",
    );
    probe(
        "block-map branch + sibling in buffer",
        "srv:\n  host: a\nother: 1\n",
        DocFormat::Yaml,
        vec![k("srv")],
        "srv:\n  host: a\nextra: 7\n",
    );

    // 7. Comment node: add sibling comment lines (documented-legal), then try
    //    turning a comment line into a live entry.
    let doc = AnyDocument::from_str_as("# one\nkey = 1\n", DocFormat::Toml).unwrap();
    let mut s = Session::new(doc);
    let path = vec![Seg::Index(0)];
    s.dispatch(Intent::SetCursor(path.clone()));
    let snap = s.dispatch(Intent::ApplyEditComment {
        path: path.clone(),
        text: "# one\n# two\n".to_string(),
    });
    println!("=== comment node + sibling comment");
    println!("--- result\n{}", s.serialize().unwrap());
    println!("--- notice: {:?}\n", snap.notice.map(|n| format!("{n:?}")));

    let doc = AnyDocument::from_str_as("# one\nkey = 1\n", DocFormat::Toml).unwrap();
    let mut s = Session::new(doc);
    s.dispatch(Intent::SetCursor(path.clone()));
    let snap = s.dispatch(Intent::ApplyEditComment {
        path: path.clone(),
        text: "live = 1\n".to_string(),
    });
    println!("=== comment node -> live entry");
    println!("--- result\n{}", s.serialize().unwrap());
    println!("--- notice: {:?}\n", snap.notice.map(|n| format!("{n:?}")));
}
