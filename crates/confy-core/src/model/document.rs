use crate::model::node::{NodeKind, NodeTree, Path};
use std::path::Path as FsPath;

pub trait ConfigDocument: Sized {
    fn load(path: &FsPath) -> anyhow::Result<Self>;
    fn project(&self) -> NodeTree;
    fn serialize(&self) -> String;
    fn is_dirty(&self) -> bool;
    fn apply(&mut self, m: Mutation) -> Result<(), MutateError>;

    /// Serialize the node at `path` as a standalone fragment (for the clipboard and
    /// `$EDITOR`), starting at the node's own header/value line — an adjacent
    /// standalone comment is an independent node and is never part of the fragment.
    /// The empty path returns the whole document.
    fn serialize_fragment(&self, path: &[crate::model::node::Seg]) -> String;

    /// Like [`serialize_fragment`](Self::serialize_fragment) but **scope-relative**:
    /// a node copied out of a `[T/D]` dotted table has its leading dotted-ancestor
    /// key segments dropped (`dotted.test.bool_true` → `bool_true`). Used by
    /// copy/cut so a paste re-prefixes only for the new destination.
    fn serialize_fragment_relative(&self, path: &[crate::model::node::Seg]) -> String;

    /// The config syntax this document speaks (title bar, help text, comment
    /// validation).
    fn format(&self) -> DocFormat;
    /// Line-comment leader for this format ("#" / "//").
    fn comment_prefix(&self) -> &'static str;
    /// Whether authored comments are currently legal in this document
    /// (false only for a pure `.json` before the JSONC upgrade, Phase 2).
    fn supports_comments(&self) -> bool;

    /// The kinds/notations the node at `path` can convert to via
    /// [`Mutation::ConvertKind`], as `(label, target)` pairs — the current
    /// notation excluded. Empty when the node's kind cannot be switched.
    /// Labels are format-specific notation names rendered verbatim in the
    /// `K` popup. Positional legality (capture rules…) is still checked by
    /// `apply`; this lists only what is legal *by kind*.
    fn kind_options(&self, path: &[crate::model::node::Seg]) -> Vec<(String, KindTarget)>;

    /// Wrap a value repr (and optional key) into a one-node fragment in this
    /// format, suitable for `Replace`/`Insert` from the inline editor and
    /// `nudge`: `key = value` (TOML) / `"key": value` (JSON). With `key: None`
    /// (an array element) it returns the bare value fragment the backend's
    /// element `Replace` expects. So the TUI never hard-codes a notation.
    fn scalar_fragment(&self, key: Option<&str>, value: &str) -> String;

    /// A fragment that, inserted into an array/sequence, yields a single
    /// **keyless** element holding `value`. Unlike `scalar_fragment(None, …)`
    /// — which produces a *value-Replace* fragment a backend may wrap with a
    /// synthetic key (TOML's `__elem__ = value`) — this is the bare element form
    /// the array `Insert` adapter expects, so all backends seed array elements
    /// uniformly. Default suits flat value syntaxes (TOML/JSON, where a bare
    /// value re-wraps and is spliced keyless); YAML overrides with its `- `
    /// element form.
    fn array_element_fragment(&self, value: &str) -> String {
        format!("{value}\n")
    }

    /// An empty-container seed for `a` (add): the backend's notation for an empty
    /// Array / inline table / scope table / array-of-tables named `key` (keyless
    /// when `key` is `None` → the bare element form). Keeps the TUI from
    /// hard-coding a notation. Default suits flat map/array syntaxes (JSON/YAML):
    /// `[]` for an array, `{}` for any map. TOML overrides the header forms
    /// (`Table → [key]`, `ArrayOfTables → [[key]]`).
    fn empty_container_fragment(&self, kind: &NodeKind, key: Option<&str>) -> String {
        let v = if matches!(kind, NodeKind::Array) {
            "[]"
        } else {
            "{}"
        };
        match key {
            None => self.array_element_fragment(v),
            Some(k) => self.scalar_fragment(Some(k), v),
        }
    }

    /// Whether a value nested *within* an array/sequence — an element itself, or a
    /// scalar reached through a sequence index — is individually `Replace`-addressable.
    /// YAML's `resolve` descends `Index`→`Key` so every block/flow element is
    /// addressable (`true`); TOML/JSON array elements are not addressable as bare
    /// fragments (`false`), so the editor either truncates to the whole array or wraps
    /// the element repr. Drives the inline-vs-`$EDITOR` routing and the external-edit
    /// element wrap.
    fn array_elements_addressable(&self) -> bool {
        false
    }

    /// Whether a key **rename** can change the node's *type* — TOML only, where a
    /// dotted key (`foo` → `foo.x`) turns a scalar into a `[T/D]` table, so the
    /// inline editor must run a type-change check on rename. JSON/YAML keys never
    /// carry structure, so a rename never changes type (`false`).
    fn rename_can_change_type(&self) -> bool {
        false
    }

    /// The [`NodeKind`] a bare `value` repr projects to in this format — used by
    /// the inline editor's type-change detection. `Err` (with the parse message)
    /// when the value doesn't parse, so the editor can stay open on a bad edit.
    fn value_kind(&self, value: &str) -> Result<NodeKind, String>;

    /// Split an inline-editor buffer of the form `value  # comment` into its
    /// `(value, Option<comment>)` parts using the backend's own lexer, so a
    /// `#`/`//` *inside* a string value (`"a # b"  # note`) is not mistaken for
    /// the trailing comment. The returned comment keeps its `#`/`//` prefix; the
    /// value is the exact source text. A buffer with no trailing comment returns
    /// `(buffer, None)`.
    fn split_value_comment(&self, buffer: &str) -> (String, Option<String>) {
        // Backend-agnostic fallback: no split (overridden by comment-capable
        // backends). Keeps a hypothetical comment-less backend correct.
        (buffer.to_string(), None)
    }

    /// Whether a value-scoped `Replace` keeps an existing trailing inline comment.
    /// TOML/JSON `Replace` rewrites only the value token and leaves the comment in
    /// place (`true`); YAML `Replace` swaps the whole `key: value` entry and drops
    /// it (`false`), so the inline editor must re-assert the comment via
    /// `SetTrailingComment` even when the comment text is unchanged.
    fn replace_preserves_trailing_comment(&self) -> bool {
        true
    }

    /// Lower the whole document to the format-neutral [`Value`](crate::model::value::Value)
    /// tree for document-level conversion (spec §Phase 4), decoding every scalar
    /// to typed data and carrying standalone + trailing comments in order.
    /// Returns `(value, warnings)` where `warnings` are normalization notes
    /// gathered during the walk (notation that the default-style render will
    /// drop). `Err(ConvertAbort)` when the document holds a construct that cannot
    /// be represented at all (a YAML opaque node). The source is never modified.
    fn to_value(&self) -> Result<(crate::model::value::Value, Vec<String>), ConvertAbort>;
}

/// A document-level conversion aborted before any output: the source holds a
/// construct that cannot be represented in the neutral [`Value`] tree, or (added
/// by the target-format loss check in `convert.rs`) a value the target format
/// has no notation for. No file is written.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ConvertAbort(pub String);

/// Which config syntax a document speaks. Backends report it via
/// [`ConfigDocument::format`]; the TUI uses it for the title bar, help text
/// and comment validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocFormat {
    Toml,
    Json,
    Yaml,
}

impl DocFormat {
    /// Human-readable name for status/error messages ("TOML", "JSON", "YAML").
    pub fn name(self) -> &'static str {
        match self {
            DocFormat::Toml => "TOML",
            DocFormat::Json => "JSON",
            DocFormat::Yaml => "YAML",
        }
    }
}

/// Where an insert/move lands: insert as a child of `parent` at `index`.
#[derive(Clone, Debug)]
pub struct Target {
    pub parent: Path,
    pub index: usize,
}

#[derive(Clone, Copy, Debug)]
pub enum OnCollision {
    Overwrite,
    Rename, // append _2, _3, ...
    Cancel,
}

#[derive(Clone, Debug)]
pub enum Mutation {
    Delete {
        path: Path,
    },
    Insert {
        target: Target,
        fragment: String,
        on_collision: OnCollision,
    },
    Replace {
        path: Path,
        fragment: String,
    },
    /// Rename the key at `path` to `new_key`, preserving its position and decor.
    Rename {
        path: Path,
        new_key: String,
    },
    Remark {
        path: Path,
    },
    /// Replace the text of the (multi-line) comment node at `path` with `text`,
    /// rewriting it in place within the owning decor slot.
    EditComment {
        path: Path,
        text: String,
    },
    Move {
        sources: Vec<Path>,
        target: Target,
        on_collision: OnCollision,
    },
    /// Insert a standalone comment block (`# …` lines) into `target.parent`'s
    /// decor at the projected `target.index`. Comments live in decor — no key,
    /// no collision.
    InsertComment {
        target: Target,
        text: String,
    },
    /// Convert the node at `path` to another notation of the same kind in place
    /// (the `K` kind-switch): a scalar to another notation of its type (string
    /// basic/literal/multiline forms, integer radix, float plain ↔ exponent), an
    /// array between inline, multiline and `[[…]]` group form, a table between
    /// `[T/I]`/`[T/D]`/`[T/S]` writing styles. Illegal conversions (a value the
    /// target notation can't represent, a position that would break the
    /// table-capture rule, comments that can't survive the target form) reject
    /// with the document untouched.
    ConvertKind {
        path: Path,
        target: KindTarget,
    },
    /// Set, change, or clear the **trailing inline comment** of the keyed scalar
    /// at `path` (`host: x  # bind`). `Some(text)` sets/changes it (the text
    /// carries its own `#`/`//` prefix); `None` clears it. Independent of the
    /// value `Replace` so a plain value edit / `nudge` / paste still *preserve*
    /// the comment (they never issue this), while the inline editor issues it
    /// only when the comment portion of the buffer changed.
    SetTrailingComment {
        path: Path,
        comment: Option<String>,
    },
}

/// The target of a [`Mutation::ConvertKind`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KindTarget {
    StringBasic,
    StringLiteral,
    StringMultiline,
    StringMultilineLiteral,
    IntDecimal,
    IntHex,
    IntOctal,
    IntBinary,
    FloatPlain,
    FloatExponent,
    ArrayInline,
    ArrayMultiline,
    ArrayOfTables,
    TableInline,
    TableDotted,
    TableScope,
    /// A JSON object spread over multiple lines (`[T/M]`). TOML's scope table
    /// stays `[T/S]`; this is the JSON multiline-object form.
    TableMultiline,
    /// YAML flow collection (single-line `{ }` / `[ ]`).
    Flow,
    /// YAML block collection (`key:\n  …` / `- …`).
    Block,
    /// YAML plain (unquoted) scalar.
    StringPlain,
    /// YAML 'single quoted' scalar.
    StringSingle,
    /// YAML "double quoted" scalar.
    StringDouble,
    /// YAML literal block scalar `|`.
    StringLiteralBlock,
    /// YAML folded block scalar `>`.
    StringFolded,
}

#[derive(Debug, thiserror::Error)]
pub enum MutateError {
    #[error("path not found")]
    NotFound,
    #[error("key collision: {0}")]
    Collision(String),
    #[error("invalid fragment: {0}")]
    Fragment(String),
    /// The node type is incompatible with the destination container, or the
    /// position would break TOML's table-capture rule (a scalar after a `[table]`
    /// header, or a `[table]` before the keys it would capture). Source-order /
    /// semantic legality — see the cross-layer-ops plan (D1/D5).
    #[error("{0}")]
    Illegal(String),
    #[error("operation not supported by this format")]
    Unsupported,
}
