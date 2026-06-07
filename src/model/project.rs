use crate::model::node::{Format, Node, NodeKind, NodeTree, ScalarType, Seg};
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value};

/// Group standalone comment lines from a decor prefix/trailing string into
/// blocks. A run of consecutive `#`-prefixed lines becomes one block (joined by
/// `\n`); any blank or non-`#` line breaks the current block. Each returned
/// block becomes a single (possibly multi-line) Comment node.
fn comment_blocks(decor_text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for line in decor_text.lines() {
        let t = line.trim();
        if t.starts_with('#') {
            current.push(t.to_string());
        } else if !current.is_empty() {
            blocks.push(current.join("\n"));
            current.clear();
        }
    }
    if !current.is_empty() {
        blocks.push(current.join("\n"));
    }
    blocks
}

fn comment_node(text: &str, parent: &[Seg], ordinal: usize) -> Node {
    let mut path = parent.to_vec();
    path.push(Seg::Key(format!("#comment:{ordinal}")));
    Node {
        key: text.to_string(),
        path,
        kind: NodeKind::Comment(text.to_string()),
        children: Vec::new(),
        value: Some(text.to_string()),
        format: Format::Plain,
        trailing_comment: None,
    }
}

fn trailing_comment_from(table: &Table, key: &str) -> Option<String> {
    table
        .key(key)
        .and_then(|k| k.leaf_decor().suffix().and_then(|r| r.as_str()))
        .map(|s| s.trim().to_string())
        .filter(|s| s.starts_with('#'))
}

pub fn project(doc: &DocumentMut, filename: &str) -> NodeTree {
    let mut root = Node::branch(filename.to_string(), NodeKind::Root);
    root.children = project_table(doc.as_table(), &[]);
    // Handle document-level trailing comments (comment-only files and end-of-doc comments)
    if let Some(trailing) = doc.trailing().as_str() {
        let base_ordinal = root.children.len();
        for (i, c) in comment_blocks(trailing).into_iter().enumerate() {
            root.children.push(comment_node(&c, &[], base_ordinal + i));
        }
    }
    NodeTree { root }
}

pub(crate) fn scalar_type(v: &Value) -> ScalarType {
    match v {
        Value::String(_) => ScalarType::String,
        Value::Integer(_) => ScalarType::Integer,
        Value::Float(_) => ScalarType::Float,
        Value::Boolean(_) => ScalarType::Bool,
        Value::Datetime(d) => {
            // A TOML datetime is one of four types depending on which components
            // are present (toml_datetime::Datetime { date, time, offset }).
            let dt = d.value();
            match (dt.date.is_some(), dt.time.is_some(), dt.offset.is_some()) {
                (_, _, true) => ScalarType::OffsetDatetime,
                (true, true, false) => ScalarType::LocalDatetime,
                (true, false, false) => ScalarType::LocalDate,
                (false, true, false) => ScalarType::LocalTime,
                // date+time absent is not representable; default keeps us total.
                _ => ScalarType::LocalDatetime,
            }
        }
        Value::Array(_) | Value::InlineTable(_) => unreachable!("handled by item dispatch"),
    }
}

/// Derive the writing style (§format) of a scalar from its rendered repr. The
/// repr is `Value::to_string()` (already stored in `Node.value`) — for strings
/// it carries the surrounding quotes, for integers the base prefix.
fn detect_format(st: ScalarType, repr: &str) -> Format {
    let s = repr.trim();
    match st {
        ScalarType::String => {
            if s.starts_with("\"\"\"") {
                Format::MultilineBasic
            } else if s.starts_with("'''") {
                Format::MultilineLiteral
            } else if s.starts_with('\'') {
                Format::Literal
            } else {
                Format::BasicString
            }
        }
        ScalarType::Integer => {
            // A leading sign is only valid on decimal integers, so prefix-match
            // after an optional sign isn't needed: 0x/0o/0b never carry a sign.
            if s.starts_with("0x") {
                Format::Hex
            } else if s.starts_with("0o") {
                Format::Octal
            } else if s.starts_with("0b") {
                Format::Binary
            } else {
                Format::Decimal
            }
        }
        _ => Format::Plain,
    }
}

fn project_table(table: &Table, base: &[Seg]) -> Vec<Node> {
    let mut out = Vec::new();
    let mut ordinal = 0usize;
    for (key, item) in table.iter() {
        // Standalone comments before a leaf/inline key live in the key's
        // leaf_decor prefix.
        if let Some(k) = table.key(key) {
            if let Some(prefix) = k.leaf_decor().prefix().and_then(|r| r.as_str()) {
                for c in comment_blocks(prefix) {
                    out.push(comment_node(&c, base, ordinal));
                    ordinal += 1;
                }
            }
        }
        // Standalone comments before a `[table]` or `[[array_of_tables]]` header
        // live in the inner table's own decor prefix — leaf_decor is empty there.
        // (Verified against toml_edit 0.22 via §7.1 review-probe.)
        match item {
            Item::Table(t) => {
                if let Some(prefix) = t.decor().prefix().and_then(|r| r.as_str()) {
                    for c in comment_blocks(prefix) {
                        out.push(comment_node(&c, base, ordinal));
                        ordinal += 1;
                    }
                }
            }
            Item::ArrayOfTables(aot) => {
                if let Some(first) = aot.iter().next() {
                    if let Some(prefix) = first.decor().prefix().and_then(|r| r.as_str()) {
                        for c in comment_blocks(prefix) {
                            out.push(comment_node(&c, base, ordinal));
                            ordinal += 1;
                        }
                    }
                }
            }
            _ => {}
        }
        let mut path = base.to_vec();
        path.push(Seg::Key(key.to_string()));
        match item {
            // Only flatten implicit tables created by dotted *keys* (`a.b.c = 1`,
            // which toml_edit marks `is_dotted()`). A dotted table *header*
            // (`[x.a]` with no `[x]`) also yields an implicit `x`, but it is
            // NOT dotted — project it as a real branch so its children nest.
            Item::Table(t) if t.is_implicit() && t.is_dotted() => {
                flatten_dotted(t, key, &path, &mut out);
            }
            _ => {
                out.push(project_item(
                    key,
                    item,
                    path,
                    trailing_comment_from(table, key),
                ));
            }
        }
    }
    out
}

/// Re-join implicit tables created by toml_edit for dotted keys (e.g. `a.b.c = 1`)
/// into a single leaf node per §4. The node's *display key* is the dotted join,
/// but its *path* keeps one Seg::Key per segment so the node stays navigable for
/// mutation (the path resolver walks the real `doc["a"]["b"]["c"]` structure).
fn flatten_dotted(table: &Table, prefix: &str, seg_path: &[Seg], out: &mut Vec<Node>) {
    for (key, item) in table.iter() {
        // Standalone comments before a dotted-key leaf live on the INNER key's
        // leaf_decor inside the implicit table (toml_edit 0.22 — review-probed).
        if let Some(k) = table.key(key) {
            if let Some(prefix_s) = k.leaf_decor().prefix().and_then(|r| r.as_str()) {
                let base_ord = out.len();
                for (j, c) in comment_blocks(prefix_s).into_iter().enumerate() {
                    out.push(comment_node(&c, seg_path, base_ord + j));
                }
            }
        }
        let dotted_key = format!("{prefix}.{key}");
        let mut path = seg_path.to_vec();
        path.push(Seg::Key(key.to_string()));
        match item {
            Item::Table(t) if t.is_implicit() && t.is_dotted() => {
                flatten_dotted(t, &dotted_key, &path, out);
            }
            _ => {
                out.push(project_item(
                    &dotted_key,
                    item,
                    path,
                    trailing_comment_from(table, key),
                ));
            }
        }
    }
}

fn project_item(key: &str, item: &Item, path: Vec<Seg>, trailing_comment: Option<String>) -> Node {
    match item {
        Item::Value(Value::Array(arr)) => project_array(key, arr, path),
        Item::Value(Value::InlineTable(it)) => project_inline(key, it, path),
        Item::Value(v) => {
            let st = scalar_type(v);
            let repr = v.to_string();
            let mut n = Node::leaf(key.to_string(), NodeKind::Scalar(st));
            n.path = path;
            n.format = detect_format(st, &repr);
            n.value = Some(repr);
            n.trailing_comment = trailing_comment;
            n
        }
        Item::Table(t) => {
            let mut n = Node::branch(key.to_string(), NodeKind::Table);
            n.path = path.clone();
            n.children = project_table(t, &path);
            n
        }
        Item::ArrayOfTables(aot) => project_aot(key, aot, path),
        Item::None => {
            let mut n = Node::leaf(key.to_string(), NodeKind::Scalar(ScalarType::String));
            n.path = path;
            n
        }
    }
}

fn project_array(key: &str, arr: &Array, path: Vec<Seg>) -> Node {
    let mut n = Node::branch(key.to_string(), NodeKind::Array);
    n.path = path.clone();
    for (i, v) in arr.iter().enumerate() {
        let mut p = path.clone();
        p.push(Seg::Index(i));
        n.children.push(project_value(&format!("[{i}]"), v, p));
    }
    n
}

fn project_inline(key: &str, it: &InlineTable, path: Vec<Seg>) -> Node {
    let mut n = Node::branch(key.to_string(), NodeKind::InlineTable);
    n.path = path.clone();
    for (k, v) in it.iter() {
        let mut p = path.clone();
        p.push(Seg::Key(k.to_string()));
        n.children.push(project_value(k, v, p));
    }
    n
}

fn project_aot(key: &str, aot: &ArrayOfTables, path: Vec<Seg>) -> Node {
    let mut n = Node::branch(key.to_string(), NodeKind::ArrayOfTables);
    n.path = path.clone();
    for (i, t) in aot.iter().enumerate() {
        // For i >= 1, comments between successive [[key]] entries live in this
        // entry's decor().prefix(). (Entry 0's prefix is captured at the parent
        // level by project_table — it sits as a sibling BEFORE the AoT branch
        // per §7.1 row 1, not as a child of it.)
        if i > 0 {
            if let Some(prefix) = t.decor().prefix().and_then(|r| r.as_str()) {
                let base_ord = n.children.len();
                for (j, c) in comment_blocks(prefix).into_iter().enumerate() {
                    n.children.push(comment_node(&c, &path, base_ord + j));
                }
            }
        }
        let mut p = path.clone();
        p.push(Seg::Index(i));
        let mut child = Node::branch(format!("[{i}]"), NodeKind::Table);
        child.path = p.clone();
        child.children = project_table(t, &p);
        n.children.push(child);
    }
    n
}

fn project_value(key: &str, v: &Value, path: Vec<Seg>) -> Node {
    match v {
        Value::Array(a) => project_array(key, a, path),
        Value::InlineTable(it) => project_inline(key, it, path),
        other => {
            let st = scalar_type(other);
            let repr = other.to_string();
            let mut n = Node::leaf(key.to_string(), NodeKind::Scalar(st));
            n.path = path;
            n.format = detect_format(st, &repr);
            n.value = Some(repr);
            n
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::{NodeKind, ScalarType, Seg};
    use toml_edit::DocumentMut;

    fn tree(src: &str) -> crate::model::node::NodeTree {
        let doc = src.parse::<DocumentMut>().unwrap();
        project(&doc, "f.toml")
    }

    fn keys_of(t: &crate::model::node::NodeTree) -> Vec<String> {
        t.root.children.iter().map(|n| n.key.clone()).collect()
    }

    #[test]
    fn comment_before_item() {
        // row 1: comment before an item -> sibling immediately before it
        let t = tree("# lead\nport = 8080\n");
        assert_eq!(keys_of(&t), vec!["# lead".to_string(), "port".to_string()]);
        assert_eq!(t.root.children[0].kind, NodeKind::Comment("# lead".into()));
    }

    #[test]
    fn comment_between_items() {
        let t = tree("a = 1\n# mid\nb = 2\n");
        assert_eq!(
            keys_of(&t),
            vec!["a".to_string(), "# mid".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn comment_at_end_of_document() {
        // row 4: trailing comment with no following item -> last top-level sibling
        let t = tree("a = 1\n# tail\n");
        assert_eq!(keys_of(&t), vec!["a".to_string(), "# tail".to_string()]);
    }

    #[test]
    fn comment_at_start_of_document() {
        let t = tree("# top\na = 1\n");
        assert_eq!(keys_of(&t), vec!["# top".to_string(), "a".to_string()]);
    }

    #[test]
    fn consecutive_comments_merge_into_one_node() {
        // Consecutive `#` lines merge into a single multi-line Comment node whose
        // key/value carry the joined block.
        let t = tree("# one\n# two\na = 1\n");
        assert_eq!(
            keys_of(&t),
            vec!["# one\n# two".to_string(), "a".to_string()]
        );
        let c = &t.root.children[0];
        assert_eq!(c.kind, NodeKind::Comment("# one\n# two".into()));
        assert_eq!(c.value.as_deref(), Some("# one\n# two"));
    }

    #[test]
    fn blank_line_splits_comment_blocks() {
        // A blank line breaks a comment block into two separate nodes.
        let t = tree("# a1\n# a2\n\n# b1\na = 1\n");
        assert_eq!(
            keys_of(&t),
            vec![
                "# a1\n# a2".to_string(),
                "# b1".to_string(),
                "a".to_string()
            ]
        );
    }

    #[test]
    fn comment_only_file() {
        let t = tree("# just\n# comments\n");
        assert_eq!(keys_of(&t), vec!["# just\n# comments".to_string()]);
    }

    #[test]
    fn comment_inside_table_before_key() {
        let t = tree("[server]\n# explain\nport = 8080\n");
        let server = &t.root.children[0];
        assert_eq!(
            server
                .children
                .iter()
                .map(|n| n.key.clone())
                .collect::<Vec<_>>(),
            vec!["# explain".to_string(), "port".to_string()]
        );
    }

    #[test]
    fn comment_before_table_header() {
        // §7.1 row 1, Table case: comment in `Table::decor().prefix()`, not in
        // the key's leaf_decor. Must surface as a sibling Comment before [server].
        let t = tree("# about\n[server]\nport = 8080\n");
        assert_eq!(
            keys_of(&t),
            vec!["# about".to_string(), "server".to_string()]
        );
        assert_eq!(t.root.children[0].kind, NodeKind::Comment("# about".into()));
    }

    #[test]
    fn comment_between_tables() {
        // §7.1 row 3: comment after last key of one table, before the next
        // table header. toml_edit stores it on the following table's decor.
        let t = tree("[s]\np = 1\n# mid\n[d]\nn = \"t\"\n");
        assert_eq!(
            keys_of(&t),
            vec!["s".to_string(), "# mid".to_string(), "d".to_string()]
        );
    }

    #[test]
    fn comment_before_dotted_key() {
        // §7.1 row 1, dotted-key case: toml_edit stores the comment on the
        // INNER key's leaf_decor inside the implicit table (not on the outer
        // key, and not on the implicit table's decor). flatten_dotted must
        // surface it as a sibling Comment before the dotted leaf.
        let t = tree("# lead\na.b = 1\n");
        assert_eq!(keys_of(&t), vec!["# lead".to_string(), "a.b".to_string()]);
        assert_eq!(t.root.children[0].kind, NodeKind::Comment("# lead".into()));
    }

    #[test]
    fn comment_between_aot_entries() {
        // §7.1 row 3 (AoT case): comment between successive [[s]] entries lives
        // on aot[1].decor().prefix(). Must surface as a Comment child of the AoT
        // branch, between [0] and [1].
        let t = tree("[[s]]\na = 1\n# mid\n[[s]]\nb = 2\n");
        let s = &t.root.children[0];
        assert_eq!(s.kind, NodeKind::ArrayOfTables);
        let child_keys: Vec<String> = s.children.iter().map(|n| n.key.clone()).collect();
        assert_eq!(
            child_keys,
            vec!["[0]".to_string(), "# mid".to_string(), "[1]".to_string()]
        );
    }

    #[test]
    fn trailing_eol_comment_is_not_a_node() {
        // §7: end-of-line comments (`port = 8080  # http`) are decoration on
        // their owning node, NOT standalone Comment nodes.
        let t = tree("port = 8080  # http\n");
        assert_eq!(keys_of(&t), vec!["port".to_string()]);
    }

    #[test]
    fn scalars_and_tables() {
        let t = tree("title = \"x\"\n[server]\nport = 8080\n");
        let root = &t.root;
        assert_eq!(root.kind, NodeKind::Root);
        assert_eq!(root.children.len(), 2);
        assert_eq!(root.children[0].key, "title");
        assert_eq!(root.children[0].kind, NodeKind::Scalar(ScalarType::String));
        let server = &root.children[1];
        assert_eq!(server.kind, NodeKind::Table);
        assert_eq!(server.children[0].key, "port");
        assert_eq!(
            server.children[0].kind,
            NodeKind::Scalar(ScalarType::Integer)
        );
        assert_eq!(
            server.children[0].path,
            vec![Seg::Key("server".into()), Seg::Key("port".into())]
        );
    }

    #[test]
    fn arrays_and_inline_tables_and_aot() {
        let t = tree("nums = [1, 2]\npt = { x = 1 }\n[[item]]\nn = 1\n[[item]]\nn = 2\n");
        let root = &t.root;
        let nums = root.children.iter().find(|n| n.key == "nums").unwrap();
        assert_eq!(nums.kind, NodeKind::Array);
        assert_eq!(nums.children.len(), 2);
        assert_eq!(nums.children[0].key, "[0]");
        assert_eq!(
            nums.children[0].path,
            vec![Seg::Key("nums".into()), Seg::Index(0)]
        );

        let pt = root.children.iter().find(|n| n.key == "pt").unwrap();
        assert_eq!(pt.kind, NodeKind::InlineTable);
        assert_eq!(pt.children[0].key, "x");

        let item = root.children.iter().find(|n| n.key == "item").unwrap();
        assert_eq!(item.kind, NodeKind::ArrayOfTables);
        assert_eq!(item.children.len(), 2);
        assert_eq!(
            item.children[0].path,
            vec![Seg::Key("item".into()), Seg::Index(0)]
        );
    }

    fn leaf_named<'a>(t: &'a crate::model::node::NodeTree, key: &str) -> &'a Node {
        t.root.children.iter().find(|n| n.key == key).unwrap()
    }

    #[test]
    fn integer_formats_detected() {
        let t = tree("dec = 255\nhx = 0xFF\noc = 0o377\nbn = 0b1111_1111\n");
        assert_eq!(leaf_named(&t, "dec").format, Format::Decimal);
        assert_eq!(leaf_named(&t, "hx").format, Format::Hex);
        assert_eq!(leaf_named(&t, "oc").format, Format::Octal);
        assert_eq!(leaf_named(&t, "bn").format, Format::Binary);
        // all classify as Integer regardless of base
        for k in ["dec", "hx", "oc", "bn"] {
            assert_eq!(
                leaf_named(&t, k).kind,
                NodeKind::Scalar(ScalarType::Integer)
            );
        }
    }

    #[test]
    fn string_formats_detected() {
        let t = tree("b = \"hi\"\nl = 'hi'\nmb = \"\"\"hi\"\"\"\nml = '''hi'''\n");
        assert_eq!(leaf_named(&t, "b").format, Format::BasicString);
        assert_eq!(leaf_named(&t, "l").format, Format::Literal);
        assert_eq!(leaf_named(&t, "mb").format, Format::MultilineBasic);
        assert_eq!(leaf_named(&t, "ml").format, Format::MultilineLiteral);
    }

    #[test]
    fn datetime_subtypes_split_into_four() {
        let t = tree(
            "odt = 2021-01-01T00:00:00Z\nldt = 2021-01-01T00:00:00\nld = 2021-01-01\nlt = 12:34:56\n",
        );
        assert_eq!(
            leaf_named(&t, "odt").kind,
            NodeKind::Scalar(ScalarType::OffsetDatetime)
        );
        assert_eq!(
            leaf_named(&t, "ldt").kind,
            NodeKind::Scalar(ScalarType::LocalDatetime)
        );
        assert_eq!(
            leaf_named(&t, "ld").kind,
            NodeKind::Scalar(ScalarType::LocalDate)
        );
        assert_eq!(
            leaf_named(&t, "lt").kind,
            NodeKind::Scalar(ScalarType::LocalTime)
        );
        // datetimes carry the single Plain writing style
        assert_eq!(leaf_named(&t, "odt").format, Format::Plain);
    }

    #[test]
    fn dotted_key_is_single_leaf() {
        let t = tree("a.b.c = 1\n");
        let root = &t.root;
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].key, "a.b.c");
        assert_eq!(root.children[0].kind, NodeKind::Scalar(ScalarType::Integer));
        // Display key is the dotted join, but the path keeps real segments so the
        // node stays navigable for mutation (doc["a"]["b"]["c"]).
        assert_eq!(
            root.children[0].path,
            vec![
                Seg::Key("a".into()),
                Seg::Key("b".into()),
                Seg::Key("c".into())
            ]
        );
    }

    #[test]
    fn dotted_table_header_nests_under_implicit_parent() {
        // `[x.a]` / `[x.b]` with no explicit `[x]`: toml_edit creates an
        // implicit (non-dotted) `x`. It must project as one `x` branch with
        // children `a` and `b`, not two flat `x.a` / `x.b` siblings.
        let t = tree("[x.a]\nname = \"A\"\n\n[x.b]\nname = \"B\"\n");
        let root = &t.root;
        assert_eq!(root.children.len(), 1);
        let x = &root.children[0];
        assert_eq!(x.key, "x");
        assert_eq!(x.kind, NodeKind::Table);
        assert_eq!(x.path, vec![Seg::Key("x".into())]);
        let child_keys: Vec<&str> = x.children.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(child_keys, vec!["a", "b"]);
        assert_eq!(
            x.children[0].path,
            vec![Seg::Key("x".into()), Seg::Key("a".into())]
        );
    }
}
