use confy_core::model::document::{ConfigDocument, Mutation};
use confy_core::model::node::Seg;
use confy_core::model::yaml::YamlDocument;

fn dump(n: &confy_core::model::node::Node, depth: usize) {
    println!(
        "{}{} kind={:?} fmt={:?} val={:?} trailing={:?} ro={}",
        "  ".repeat(depth),
        n.key,
        n.kind,
        n.format,
        n.value,
        n.trailing_comment,
        n.read_only
    );
    for c in &n.children {
        dump(c, depth + 1);
    }
}

fn load_str(src: &str) -> anyhow::Result<YamlDocument> {
    YamlDocument::from_str(src)
}

#[test]
fn scratch_sample() {
    let src = std::fs::read_to_string("tests/fixtures/sample.yaml").unwrap();
    match load_str(&src) {
        Ok(doc) => dump(&doc.project().root, 0),
        Err(e) => println!("LOAD ERROR: {e}"),
    }
}

#[test]
fn scratch_nested_flow() {
    let src = "server: {host: \"0.0.0.0\", port: 8080, inner: {host2: \"0.0.0.0\", port2: 8080}}\n";
    match load_str(src) {
        Ok(doc) => dump(&doc.project().root, 0),
        Err(e) => println!("LOAD ERROR: {e}"),
    }
}

#[test]
fn scratch_edit_first_comment() {
    let src = std::fs::read_to_string("tests/fixtures/sample.yaml").unwrap();
    let mut doc = load_str(&src).unwrap();
    // First comment is the root's child index 0.
    let r = doc.apply(Mutation::EditComment {
        path: vec![Seg::Index(0)],
        text: "# confy sample EDITED".to_string(),
    });
    println!("apply result: {r:?}");
    println!(
        "---- serialized after edit comment ----\n{}",
        doc.serialize()
    );
}

#[test]
fn scratch_replace_block_array() {
    let src = "flags:\n  - fast\n  - safe\nafter: 1\n";
    let mut doc = load_str(src).unwrap();
    let frag = doc.serialize_fragment(&[Seg::Key("flags".to_string())]);
    println!("---- fragment for flags ----\n{frag}\n----");
    let r = doc.apply(Mutation::Replace {
        path: vec![Seg::Key("flags".to_string())],
        fragment: frag,
    });
    println!("replace(block array) result: {r:?}");
}

#[test]
fn scratch_roundtrip_sample() {
    let src = std::fs::read_to_string("tests/fixtures/sample.yaml").unwrap();
    let doc = load_str(&src).unwrap();
    let out = doc.serialize();
    println!("byte-identical: {}", out == src);
    if out != src {
        println!("---- serialized (no edit) ----\n{out}");
    }
}

#[test]
fn scratch_min_comment_flow() {
    let src = "# c\nflags: [fast, safe]\nratio: {x: 1.5}\n";
    let mut doc = load_str(src).unwrap();
    doc.apply(Mutation::EditComment {
        path: vec![Seg::Index(0)],
        text: "# c2".to_string(),
    })
    .unwrap();
    let out = doc.serialize();
    println!("EXPECT: # c2\\nflags: [fast, safe]\\nratio: {{x: 1.5}}\\n");
    println!("GOT:\n{out}");
}

#[test]
fn scratch_edit_comment_roundtrip_check() {
    let src = std::fs::read_to_string("tests/fixtures/sample.yaml").unwrap();
    let mut doc = load_str(&src).unwrap();
    doc.apply(Mutation::EditComment {
        path: vec![Seg::Index(0)],
        text: "# confy sample".to_string(), // same text -> should be byte-identical
    })
    .unwrap();
    let out = doc.serialize();
    println!("byte-identical after no-op comment edit: {}", out == src);
    for (i, (a, b)) in src.lines().zip(out.lines()).enumerate() {
        if a != b {
            println!("line {i}: SRC={a:?}  OUT={b:?}");
        }
    }
}

#[test]
fn scratch_direct_parse() {
    let src = std::fs::read_to_string("tests/fixtures/sample.yaml").unwrap();
    // Replace just the first comment line with a longer one.
    let rest = src.split_once('\n').unwrap().1;
    let new_doc = format!("# confy sample EDITED\n{rest}");
    let doc = load_str(&new_doc).unwrap();
    let out = doc.serialize();
    println!("direct byte-identical: {}", out == new_doc);
    for (i, (a, b)) in new_doc.lines().zip(out.lines()).enumerate() {
        if a != b {
            println!("line {i}: IN={a:?} OUT={b:?}");
        }
    }
}

#[test]
fn scratch_flags_isolate() {
    let src = std::fs::read_to_string("tests/fixtures/sample.yaml").unwrap();
    let doc = load_str(&src).unwrap();
    let ser = doc.serialize();
    println!("load+serialize identical: {}", ser == src);
    println!("has block flags in serialize: {}", ser.contains("  - fast"));
    println!(
        "has inline flags in serialize: {}",
        ser.contains("[fast, safe]")
    );
}

#[test]
fn scratch_parse_block_seq_fragment() {
    for (label, s) in [
        ("bare block seq", "flags:\n  - fast\n  - safe\n"),
        (
            "block seq + dedent",
            "flags:\n  - fast\n  - safe\nafter: 1\n",
        ),
        ("bare block map", "server:\n  host: a\n  port: 1\n"),
        ("nested key only", "a:\n  b: 1\n"),
    ] {
        match load_str(s) {
            Ok(_) => println!("{label}: OK"),
            Err(e) => println!("{label}: ERR {e}"),
        }
    }
}

#[test]
fn scratch_replace_block_map() {
    let src = "server:\n  host: a\n  port: 1\nafter: 1\n";
    let mut doc = load_str(src).unwrap();
    let frag = doc.serialize_fragment(&[Seg::Key("server".to_string())]);
    let r = doc.apply(Mutation::Replace {
        path: vec![Seg::Key("server".to_string())],
        fragment: frag,
    });
    println!("replace(block map) result: {r:?}");
    println!("after:\n{}", doc.serialize());
}
