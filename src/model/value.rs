//! Format-neutral intermediate tree for document-level conversion (spec §Phase 4).
//!
//! A backend lowers its lossless CST to a [`Value`] via `ConfigDocument::to_value`
//! (decoding scalars to typed data, dropping notation), and a renderer in
//! `convert.rs` serializes the `Value` back out in a *target* format's default
//! style. The map/seq item lists are **ordered** and carry confy's first-class
//! comments (standalone blocks + trailing comments) so they survive the round-trip.

/// A decoded value, independent of any source notation.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// A date/time, kept as its raw text (TOML-only origin). Renders verbatim
    /// into TOML and as a quoted string into JSON/YAML.
    Datetime(String),
    /// Ordered sequence: elements (`Item::Node` with `key == None`) interleaved
    /// with standalone comments.
    Seq(Vec<Item>),
    /// Ordered mapping: entries (`Item::Node` with `key == Some`) interleaved
    /// with standalone comments.
    Map(Vec<Item>),
}

/// One ordered member of a `Seq`/`Map`: either a standalone comment node or a
/// keyed/positional value (with an optional trailing comment).
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// A standalone comment node's text, markers stripped, may be multi-line
    /// (one `String`, lines joined by `\n`).
    Comment(String),
    Node {
        /// `Some` inside a `Map`, `None` inside a `Seq`.
        key: Option<String>,
        value: Value,
        /// End-of-line comment travelling with the value (marker stripped).
        trailing: Option<String>,
    },
}

impl Value {
    /// Construct a keyed map entry with no trailing comment.
    pub fn entry(key: impl Into<String>, value: Value) -> Item {
        Item::Node {
            key: Some(key.into()),
            value,
            trailing: None,
        }
    }

    /// Construct a sequence element with no trailing comment.
    pub fn element(value: Value) -> Item {
        Item::Node {
            key: None,
            value,
            trailing: None,
        }
    }

    /// True if this subtree contains a `Null` anywhere (used by the loss check
    /// for `→ TOML`, which has no null).
    pub fn has_null(&self) -> bool {
        match self {
            Value::Null => true,
            Value::Seq(items) | Value::Map(items) => items.iter().any(|it| match it {
                Item::Node { value, .. } => value.has_null(),
                Item::Comment(_) => false,
            }),
            _ => false,
        }
    }

    /// True if this subtree contains a `Datetime` anywhere (used by the loss
    /// check for `→ JSON/YAML`, which stringify datetimes).
    pub fn has_datetime(&self) -> bool {
        match self {
            Value::Datetime(_) => true,
            Value::Seq(items) | Value::Map(items) => items.iter().any(|it| match it {
                Item::Node { value, .. } => value.has_datetime(),
                Item::Comment(_) => false,
            }),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_null_walks_nested() {
        let v = Value::Map(vec![
            Value::entry("a", Value::Int(1)),
            Value::entry(
                "b",
                Value::Seq(vec![
                    Value::element(Value::Null),
                    Value::element(Value::Bool(true)),
                ]),
            ),
        ]);
        assert!(v.has_null());
        assert!(!v.has_datetime());
    }

    #[test]
    fn has_datetime_walks_nested() {
        let v = Value::Map(vec![Value::entry(
            "when",
            Value::Datetime("2020-01-01".into()),
        )]);
        assert!(v.has_datetime());
        assert!(!v.has_null());
    }

    #[test]
    fn no_null_or_datetime_on_plain_tree() {
        let v = Value::Map(vec![
            Value::entry("s", Value::Str("hi".into())),
            Value::entry("n", Value::Float(1.5)),
        ]);
        assert!(!v.has_null());
        assert!(!v.has_datetime());
    }
}
