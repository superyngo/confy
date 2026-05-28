use crate::model::node::{Node, NodeKind, NodeTree, ScalarType, Seg};
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value};

pub fn project(doc: &DocumentMut, filename: &str) -> NodeTree {
    let mut root = Node::branch(filename.to_string(), NodeKind::Root);
    root.children = project_table(doc.as_table(), &[]);
    NodeTree { root }
}

fn scalar_type(v: &Value) -> ScalarType {
    match v {
        Value::String(_) => ScalarType::String,
        Value::Integer(_) => ScalarType::Integer,
        Value::Float(_) => ScalarType::Float,
        Value::Boolean(_) => ScalarType::Bool,
        Value::Datetime(_) => ScalarType::Datetime,
        Value::Array(_) | Value::InlineTable(_) => unreachable!("handled by item dispatch"),
    }
}

fn project_table(table: &Table, base: &[Seg]) -> Vec<Node> {
    let mut out = Vec::new();
    for (key, item) in table.iter() {
        let mut path = base.to_vec();
        path.push(Seg::Key(key.to_string()));
        match item {
            Item::Table(t) if t.is_implicit() => {
                flatten_dotted(t, key, &path, &mut out);
            }
            _ => {
                out.push(project_item(key, item, path));
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
        let dotted_key = format!("{prefix}.{key}");
        let mut path = seg_path.to_vec();
        path.push(Seg::Key(key.to_string()));
        match item {
            Item::Table(t) if t.is_implicit() => {
                flatten_dotted(t, &dotted_key, &path, out);
            }
            _ => {
                out.push(project_item(&dotted_key, item, path));
            }
        }
    }
}

fn project_item(key: &str, item: &Item, path: Vec<Seg>) -> Node {
    match item {
        Item::Value(Value::Array(arr)) => project_array(key, arr, path),
        Item::Value(Value::InlineTable(it)) => project_inline(key, it, path),
        Item::Value(v) => {
            let mut n = Node::leaf(key.to_string(), NodeKind::Scalar(scalar_type(v)));
            n.path = path;
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
            let mut n = Node::leaf(key.to_string(), NodeKind::Scalar(scalar_type(other)));
            n.path = path;
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
        assert_eq!(server.children[0].kind, NodeKind::Scalar(ScalarType::Integer));
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
}
