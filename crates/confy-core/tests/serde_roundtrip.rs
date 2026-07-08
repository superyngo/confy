//! PORTING.md §7 exit gate #3: the types that cross the future WASM boundary
//! (`Intent`, `ViewRow`, `Mutation`) survive a `serde_json` round-trip.
//! This rehearses the JS-interop contract before any WASM target exists.
use confy_core::model::document::{KindTarget, Mutation, OnCollision, Target};
use confy_core::model::node::{Format, Path, ScalarType, Seg};
use confy_core::session::{Intent, ViewRow};

fn roundtrip<T>(v: &T) -> T
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let json = serde_json::to_string(v).expect("serialize");
    serde_json::from_str(&json).expect("deserialize")
}

/// Assert `v` round-trips losslessly: serialize → deserialize → serialize
/// yields the same JSON (compared as `serde_json::Value` so the domain types
/// need not derive `PartialEq`).
fn assert_roundtrip<T>(v: &T)
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let before = serde_json::to_value(v).expect("serialize-before");
    let after = serde_json::to_value(roundtrip(v)).expect("serialize-after");
    assert_eq!(before, after);
}

fn sample_path() -> Path {
    vec![
        Seg::Key("plugins".into()),
        Seg::Index(1),
        Seg::Key("name".into()),
    ]
}

#[test]
fn intent_roundtrips() {
    let variants: Vec<Intent> = vec![
        Intent::CursorDown,
        Intent::SetCursor(sample_path()),
        Intent::PageDown(12),
        Intent::FilterChar('x'),
        Intent::TypeFilterMove(1, -1),
        Intent::KindSwitchMove(-2),
        Intent::ConvertMove(1),
        Intent::DetailScrollBy(-3, 80),
        Intent::EditChar('a'),
        Intent::Nudge(-5),
        Intent::ApplyReplace {
            path: sample_path(),
            text: "name = \"x\"\n".into(),
        },
        Intent::ApplyEditComment {
            path: vec![Seg::Index(0)],
            text: "# hi\n".into(),
        },
        Intent::PromptKey('y'),
        Intent::CommitEdit {
            value: Some("42".into()),
            name: None,
        },
        Intent::CommitEdit {
            value: None,
            name: Some("renamed".into()),
        },
        Intent::CommitKind {
            path: sample_path(),
            target: confy_core::model::document::KindTarget::IntHex,
        },
        Intent::SetSelection {
            paths: vec![sample_path(), vec![Seg::Index(0)]],
        },
        Intent::MoveSelectionTo {
            sources: vec![sample_path()],
            target: vec![Seg::Key("dest".into())],
            index: 2,
        },
        Intent::SetFilter("needle".into()),
        Intent::SetConvertFormat(confy_core::model::document::DocFormat::Json),
        Intent::SetConvertPath("out.json".into()),
    ];
    for v in &variants {
        assert_roundtrip(v);
    }
}

#[test]
fn view_row_roundtrips() {
    let row = ViewRow {
        path: sample_path(),
        depth: 2,
        is_branch: false,
        key: "name".into(),
        value: Some("\"x\"".into()),
        scalar_type: Some(ScalarType::String),
        format: Format::BasicString,
        type_label: "string".into(),
        child_count: 0,
        trailing_comment: Some("# bind".into()),
        key_sign: "bare".into(),
        read_only: false,
        selected: true,
        is_cursor: false,
    };
    assert_roundtrip(&row);

    let branch = ViewRow {
        path: vec![Seg::Key("plugins".into())],
        depth: 0,
        is_branch: true,
        key: "plugins".into(),
        value: None,
        scalar_type: None,
        format: Format::Multiline,
        type_label: "array".into(),
        child_count: 2,
        trailing_comment: None,
        key_sign: "none".into(),
        read_only: true,
        selected: false,
        is_cursor: true,
    };
    assert_roundtrip(&branch);
}

#[test]
fn mutation_roundtrips() {
    let variants: Vec<Mutation> = vec![
        Mutation::Delete {
            path: sample_path(),
        },
        Mutation::Insert {
            target: Target {
                parent: vec![Seg::Key("a".into())],
                index: 0,
            },
            fragment: "x = 1\n".into(),
            on_collision: OnCollision::Rename,
        },
        Mutation::Replace {
            path: vec![Seg::Key("a".into())],
            fragment: "a = 2\n".into(),
        },
        Mutation::Rename {
            path: vec![Seg::Key("a".into())],
            new_key: "b".into(),
        },
        Mutation::Remark {
            path: vec![Seg::Index(0)],
        },
        Mutation::EditComment {
            path: vec![Seg::Index(0)],
            text: "# note\n".into(),
        },
        Mutation::Move {
            sources: vec![vec![Seg::Key("a".into())]],
            target: Target {
                parent: vec![],
                index: 1,
            },
            on_collision: OnCollision::Overwrite,
        },
        Mutation::InsertComment {
            target: Target {
                parent: vec![],
                index: 0,
            },
            text: "# header\n".into(),
        },
        Mutation::ConvertKind {
            path: vec![Seg::Key("a".into())],
            target: KindTarget::IntHex,
        },
        Mutation::SetTrailingComment {
            path: vec![Seg::Key("a".into())],
            comment: Some("# x".into()),
        },
    ];
    for v in &variants {
        assert_roundtrip(v);
    }
}

#[test]
fn leaf_enums_roundtrip() {
    let segs = vec![Seg::Key("a".into()), Seg::Index(3)];
    assert_roundtrip(&segs);

    let scalars = vec![
        ScalarType::String,
        ScalarType::Integer,
        ScalarType::Null,
        ScalarType::OffsetDatetime,
    ];
    assert_roundtrip(&scalars);

    let formats = vec![
        Format::Plain,
        Format::BasicString,
        Format::MultilineLiteral,
        Format::Exponent,
        Format::Block,
        Format::Folded,
    ];
    assert_roundtrip(&formats);

    let targets = vec![
        KindTarget::TableDotted,
        KindTarget::Flow,
        KindTarget::StringLiteralBlock,
    ];
    assert_roundtrip(&targets);

    let cols = vec![
        OnCollision::Overwrite,
        OnCollision::Rename,
        OnCollision::Cancel,
    ];
    assert_roundtrip(&cols);
}
