//! Type-filter state for the `f` checkbox popup.
//!
//! Filters the tree by a node's **type facets** — the same facets the KIND column
//! shows (`KeySign`, `NodeKind`, `Format`). Two independent dimensions:
//!
//! * **Key sign** (`key_signs`): `(B)/(Q)/(D)/(-)` — a node matches if its key
//!   sign is in the set.
//! * **Type** (`types`): one [`TypeToken`] per KIND-column slot — a node matches
//!   if `classify(kind, format, doc, read_only)` is in the set.
//!
//! Within each half the selections **union**; across halves they **intersect**
//! (see [`TypeFilter::matches`]). An empty half imposes no constraint. The popup
//! is laid out as [`layout`] rows; the cursor (`row`/`col`) walks the selectable
//! cells only (headers are skipped during navigation).

use crate::model::document::DocFormat;
use crate::model::node::{Format, KeySign, NodeKind, ScalarType};
use std::collections::HashSet;

/// A leaf type atom — one per KIND-column slot. Mirrors `type_tag` in `app.rs`
/// so the popup and the KIND column can never drift apart (see [`classify`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TypeToken {
    Root,
    Comment,
    ArrayInline,
    ArrayMultiline,
    Aot,
    InlineTable,
    TableScope,
    TableDotted,
    TableMultiline, // [T/M]  JSON multiline object
    Null,           // [S:null]
    FloatExp,       // [F:exp ]
    StrBasic,
    StrMBasic,
    StrLit,
    StrMLit,
    IntDec,
    IntHex,
    IntOct,
    IntBin,
    FloatPlain,
    FloatInf,
    FloatNan,
    Bool,
    Odt,
    Ldt,
    LDate,
    LTime,
    // YAML atoms.
    SeqBlock,        // [A/B] block sequence
    SeqFlow,         // [A/F] flow sequence
    MapBlock,        // [T/B] block mapping
    MapFlow,         // [T/F] flow mapping (also YAML inline table)
    StrSingle,       // [S:sq  ]
    StrDouble,       // [S:dq  ]
    StrLiteralBlock, // [S:lit ] (YAML literal block, shares tag with TOML Literal)
    StrFolded,       // [S:fold]
    Opaque,          // [opaq ] YAML out-of-subset read-only node
}

/// Map a node's `(kind, format, doc, read_only)` to its [`TypeToken`] — the
/// inverse of `type_tag`'s slot match (kept arm-for-arm identical). `doc` and
/// `read_only` thread the YAML opaque gate and block/flow split.
pub fn classify(kind: &NodeKind, format: Format, doc: DocFormat, read_only: bool) -> TypeToken {
    // Mirror `type_tag`'s opaque gate: a YAML out-of-subset read-only node is
    // `[opaq ]` regardless of its underlying kind.
    if read_only && doc == DocFormat::Yaml {
        return TypeToken::Opaque;
    }
    match kind {
        NodeKind::Root => TypeToken::Root,
        NodeKind::Comment(_) => TypeToken::Comment,
        NodeKind::Array => match (doc, format) {
            (DocFormat::Yaml, Format::Block) => TypeToken::SeqBlock,
            (DocFormat::Yaml, _) => TypeToken::SeqFlow,
            (_, Format::Multiline) => TypeToken::ArrayMultiline,
            _ => TypeToken::ArrayInline,
        },
        NodeKind::ArrayOfTables => TypeToken::Aot,
        NodeKind::InlineTable => match doc {
            DocFormat::Yaml => TypeToken::MapFlow,
            _ => TypeToken::InlineTable,
        },
        NodeKind::Table => match (doc, format) {
            (DocFormat::Yaml, Format::Block) => TypeToken::MapBlock,
            (DocFormat::Yaml, _) => TypeToken::MapFlow,
            (_, Format::Dotted) => TypeToken::TableDotted,
            (_, Format::Multiline) => TypeToken::TableMultiline,
            _ => TypeToken::TableScope,
        },
        NodeKind::Scalar(st) => match (st, format) {
            (ScalarType::String, Format::MultilineBasic) => TypeToken::StrMBasic,
            (ScalarType::String, Format::Literal) => TypeToken::StrLit,
            (ScalarType::String, Format::MultilineLiteral) => TypeToken::StrMLit,
            (ScalarType::String, Format::SingleQuoted) => TypeToken::StrSingle,
            (ScalarType::String, Format::DoubleQuoted) => TypeToken::StrDouble,
            (ScalarType::String, Format::LiteralBlock) => TypeToken::StrLiteralBlock,
            (ScalarType::String, Format::Folded) => TypeToken::StrFolded,
            (ScalarType::String, _) => TypeToken::StrBasic,
            (ScalarType::Integer, Format::Hex) => TypeToken::IntHex,
            (ScalarType::Integer, Format::Octal) => TypeToken::IntOct,
            (ScalarType::Integer, Format::Binary) => TypeToken::IntBin,
            (ScalarType::Integer, _) => TypeToken::IntDec,
            (ScalarType::Float, Format::Inf) => TypeToken::FloatInf,
            (ScalarType::Float, Format::Nan) => TypeToken::FloatNan,
            (ScalarType::Float, Format::Exponent) => TypeToken::FloatExp,
            (ScalarType::Float, _) => TypeToken::FloatPlain,
            (ScalarType::Bool, _) => TypeToken::Bool,
            (ScalarType::Null, _) => TypeToken::Null,
            (ScalarType::OffsetDatetime, _) => TypeToken::Odt,
            (ScalarType::LocalDatetime, _) => TypeToken::Ldt,
            (ScalarType::LocalDate, _) => TypeToken::LDate,
            (ScalarType::LocalTime, _) => TypeToken::LTime,
        },
    }
}

/// A multi-format category whose `all` row quick-toggles every member token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Array,
    Table,
    String,
    Integer,
    Float,
    Date,
    // YAML-specific groups (membership differs from the TOML/JSON groups).
    Seq,         // block + flow sequences
    Map,         // block + flow mappings
    StringYaml,  // plain + sq + dq + literal-block + folded (no TOML mstr/mlit)
    IntegerYaml, // dec + hex + oct (no binary)
}

impl Group {
    /// The leaf tokens this group's `all` row toggles. `[A/T]` is grouped under
    /// Table per the spec.
    pub fn tokens(self) -> &'static [TypeToken] {
        use TypeToken::*;
        match self {
            Group::Array => &[ArrayInline, ArrayMultiline],
            Group::Table => &[Aot, InlineTable, TableScope, TableDotted, TableMultiline],
            Group::String => &[StrBasic, StrMBasic, StrLit, StrMLit],
            Group::Integer => &[IntDec, IntHex, IntOct, IntBin],
            Group::Float => &[FloatPlain, FloatInf, FloatNan, FloatExp],
            Group::Date => &[Odt, Ldt, LDate, LTime],
            Group::Seq => &[SeqBlock, SeqFlow],
            Group::Map => &[MapBlock, MapFlow],
            Group::StringYaml => &[StrBasic, StrSingle, StrDouble, StrLiteralBlock, StrFolded],
            Group::IntegerYaml => &[IntDec, IntHex, IntOct],
        }
    }
}

/// One selectable checkbox in the popup grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Sign(KeySign),
    All(Group),
    Token(TypeToken),
}

impl Cell {
    /// Display label (the checkbox prefix is added by the renderer).
    pub fn label(self) -> &'static str {
        match self {
            Cell::All(_) => "all",
            Cell::Sign(s) => match s {
                KeySign::Bare => "(B) bare",
                KeySign::Quoted => "(Q) quoted",
                KeySign::Dotted => "(D) dotted",
                KeySign::None => "(-) no key",
            },
            Cell::Token(t) => token_label(t),
        }
    }
}

fn token_label(t: TypeToken) -> &'static str {
    use TypeToken::*;
    match t {
        Root => "[G] root",
        Comment => "[C] comment",
        ArrayInline => "[A/I] inline",
        ArrayMultiline => "[A/M] multiline",
        Aot => "[A/T] aot",
        InlineTable => "[T/I] inline-tbl",
        TableScope => "[T/S] scope",
        TableDotted => "[T/D] dotted",
        StrBasic => "[S:str ]",
        StrMBasic => "[S:mstr]",
        StrLit => "[S:lit ]",
        StrMLit => "[S:mlit]",
        IntDec => "[I:dec ]",
        IntHex => "[I:hex ]",
        IntOct => "[I:oct ]",
        IntBin => "[I:bin ]",
        FloatPlain => "[F:flt ]",
        FloatInf => "[F:inf ]",
        FloatNan => "[F:nan ]",
        FloatExp => "[F:exp ]",
        Bool => "[B:bool]",
        Null => "[S:null]",
        TableMultiline => "[T/M] multiline",
        Odt => "[D:odt ]",
        Ldt => "[D:ldt ]",
        LDate => "[D:ldat]",
        LTime => "[D:ltim]",
        SeqBlock => "[A/B] block",
        SeqFlow => "[A/F] flow",
        MapBlock => "[T/B] block",
        MapFlow => "[T/F] flow",
        StrSingle => "[S:sq  ]",
        StrDouble => "[S:dq  ]",
        StrLiteralBlock => "[S:lit ]",
        StrFolded => "[S:fold]",
        Opaque => "[opaq ] read-only",
    }
}

/// A row in the popup layout: a non-selectable section header or a row of cells.
pub enum LayoutRow {
    Header(&'static str),
    Cells(Vec<Cell>),
}

/// The full popup layout (headers + cell rows), the single source of truth for
/// both rendering and navigation. [`nav_rows`] derives the navigable grid from it.
/// Pass the loaded document's [`DocFormat`] so JSON omits TOML-only facets.
pub fn layout(format: DocFormat) -> Vec<LayoutRow> {
    use Cell::*;
    use Group as G;
    use KeySign as K;
    use TypeToken as T;
    match format {
        DocFormat::Json => vec![
            LayoutRow::Header("Key sign"),
            LayoutRow::Cells(vec![Sign(K::Quoted), Sign(K::None)]),
            LayoutRow::Header("Type"),
            LayoutRow::Cells(vec![Token(T::Root), Token(T::Comment)]),
            LayoutRow::Header("Arrays"),
            LayoutRow::Cells(vec![All(G::Array)]),
            LayoutRow::Cells(vec![Token(T::ArrayInline), Token(T::ArrayMultiline)]),
            LayoutRow::Header("Tables"),
            LayoutRow::Cells(vec![Token(T::InlineTable), Token(T::TableMultiline)]),
            LayoutRow::Header("String"),
            LayoutRow::Cells(vec![Token(T::StrBasic)]),
            LayoutRow::Header("Integer"),
            LayoutRow::Cells(vec![Token(T::IntDec)]),
            LayoutRow::Header("Float"),
            LayoutRow::Cells(vec![Token(T::FloatPlain), Token(T::FloatExp)]),
            LayoutRow::Header("Bool"),
            LayoutRow::Cells(vec![Token(T::Bool)]),
            LayoutRow::Header("Null"),
            LayoutRow::Cells(vec![Token(T::Null)]),
        ],
        DocFormat::Yaml => vec![
            LayoutRow::Header("Key sign"),
            LayoutRow::Cells(vec![Sign(K::Bare), Sign(K::Quoted)]),
            LayoutRow::Cells(vec![Sign(K::None)]),
            LayoutRow::Header("Type"),
            LayoutRow::Cells(vec![Token(T::Root), Token(T::Comment)]),
            LayoutRow::Header("Sequences"),
            LayoutRow::Cells(vec![All(G::Seq)]),
            LayoutRow::Cells(vec![Token(T::SeqBlock), Token(T::SeqFlow)]),
            LayoutRow::Header("Mappings"),
            LayoutRow::Cells(vec![All(G::Map)]),
            LayoutRow::Cells(vec![Token(T::MapBlock), Token(T::MapFlow)]),
            LayoutRow::Header("String"),
            LayoutRow::Cells(vec![All(G::StringYaml)]),
            LayoutRow::Cells(vec![Token(T::StrBasic), Token(T::StrSingle)]),
            LayoutRow::Cells(vec![Token(T::StrDouble), Token(T::StrLiteralBlock)]),
            LayoutRow::Cells(vec![Token(T::StrFolded)]),
            LayoutRow::Header("Integer"),
            LayoutRow::Cells(vec![All(G::IntegerYaml)]),
            LayoutRow::Cells(vec![Token(T::IntDec), Token(T::IntHex)]),
            LayoutRow::Cells(vec![Token(T::IntOct)]),
            LayoutRow::Header("Float"),
            LayoutRow::Cells(vec![All(G::Float)]),
            LayoutRow::Cells(vec![
                Token(T::FloatPlain),
                Token(T::FloatExp),
                Token(T::FloatInf),
                Token(T::FloatNan),
            ]),
            LayoutRow::Header("Bool"),
            LayoutRow::Cells(vec![Token(T::Bool)]),
            LayoutRow::Header("Null"),
            LayoutRow::Cells(vec![Token(T::Null)]),
            LayoutRow::Header("Opaque"),
            LayoutRow::Cells(vec![Token(T::Opaque)]),
        ],
        // TOML: full facet set, unchanged.
        _ => vec![
            LayoutRow::Header("Key sign"),
            LayoutRow::Cells(vec![Sign(K::Bare), Sign(K::Quoted)]),
            LayoutRow::Cells(vec![Sign(K::Dotted), Sign(K::None)]),
            LayoutRow::Header("Type"),
            LayoutRow::Cells(vec![Token(T::Root), Token(T::Comment)]),
            LayoutRow::Header("Arrays"),
            LayoutRow::Cells(vec![All(G::Array)]),
            LayoutRow::Cells(vec![Token(T::ArrayInline), Token(T::ArrayMultiline)]),
            LayoutRow::Header("Tables"),
            LayoutRow::Cells(vec![All(G::Table)]),
            LayoutRow::Cells(vec![Token(T::Aot), Token(T::InlineTable)]),
            LayoutRow::Cells(vec![Token(T::TableScope), Token(T::TableDotted)]),
            LayoutRow::Header("String"),
            LayoutRow::Cells(vec![All(G::String)]),
            LayoutRow::Cells(vec![Token(T::StrBasic), Token(T::StrMBasic)]),
            LayoutRow::Cells(vec![Token(T::StrLit), Token(T::StrMLit)]),
            LayoutRow::Header("Integer"),
            LayoutRow::Cells(vec![All(G::Integer)]),
            LayoutRow::Cells(vec![Token(T::IntDec), Token(T::IntHex)]),
            LayoutRow::Cells(vec![Token(T::IntOct), Token(T::IntBin)]),
            LayoutRow::Header("Float"),
            LayoutRow::Cells(vec![All(G::Float)]),
            LayoutRow::Cells(vec![
                Token(T::FloatPlain),
                Token(T::FloatInf),
                Token(T::FloatNan),
            ]),
            LayoutRow::Header("Bool"),
            LayoutRow::Cells(vec![Token(T::Bool)]),
            LayoutRow::Header("Date"),
            LayoutRow::Cells(vec![All(G::Date)]),
            LayoutRow::Cells(vec![Token(T::Odt), Token(T::Ldt)]),
            LayoutRow::Cells(vec![Token(T::LDate), Token(T::LTime)]),
        ],
    }
}

/// The navigable grid (cell rows only, headers dropped) in cursor order.
pub fn nav_rows(format: DocFormat) -> Vec<Vec<Cell>> {
    layout(format)
        .into_iter()
        .filter_map(|r| match r {
            LayoutRow::Cells(cells) => Some(cells),
            LayoutRow::Header(_) => None,
        })
        .collect()
}

/// Tristate display for a checkbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckState {
    On,
    Partial,
    Off,
}

/// Selected facets plus the popup cursor. Selections persist across popup opens
/// (the type-filter analogue of `last_filter`).
#[derive(Default)]
pub struct TypeFilter {
    pub key_signs: HashSet<KeySign>,
    pub types: HashSet<TypeToken>,
    /// Cursor row index into [`nav_rows`].
    pub row: usize,
    /// Cursor column index within the current row.
    pub col: usize,
}

impl TypeFilter {
    /// Any selection in either half — when false, the type filter is off.
    pub fn is_active(&self) -> bool {
        !self.key_signs.is_empty() || !self.types.is_empty()
    }

    /// Clear all selections (Esc-peel of the type layer). Leaves the cursor.
    pub fn clear(&mut self) {
        self.key_signs.clear();
        self.types.clear();
    }

    /// Does a node pass the type filter? Empty half = no constraint; the two
    /// halves intersect (AND), selections within a half union (OR).
    pub fn matches(
        &self,
        key_sign: KeySign,
        kind: &NodeKind,
        format: Format,
        doc: DocFormat,
        read_only: bool,
    ) -> bool {
        let sign_ok = self.key_signs.is_empty() || self.key_signs.contains(&key_sign);
        let type_ok =
            self.types.is_empty() || self.types.contains(&classify(kind, format, doc, read_only));
        sign_ok && type_ok
    }

    /// Move the cursor by `(dr, dc)`, clamping at the grid edges; the column is
    /// clamped to the destination row's width.
    pub fn move_cursor(&mut self, dr: i32, dc: i32, format: DocFormat) {
        let rows = nav_rows(format);
        if rows.is_empty() {
            return;
        }
        if dr != 0 {
            let r = (self.row as i32 + dr).clamp(0, rows.len() as i32 - 1) as usize;
            self.row = r;
            let w = rows[r].len();
            if self.col >= w {
                self.col = w - 1;
            }
        }
        if dc != 0 {
            let w = rows[self.row].len();
            self.col = (self.col as i32 + dc).clamp(0, w as i32 - 1) as usize;
        }
    }

    /// The cell under the cursor, if any.
    pub fn current_cell(&self, format: DocFormat) -> Option<Cell> {
        nav_rows(format)
            .get(self.row)
            .and_then(|r| r.get(self.col))
            .copied()
    }

    /// Toggle the cell under the cursor (Space).
    pub fn toggle_current(&mut self, format: DocFormat) {
        if let Some(cell) = self.current_cell(format) {
            self.toggle(cell);
        }
    }

    fn toggle(&mut self, cell: Cell) {
        match cell {
            Cell::Sign(s) => {
                if !self.key_signs.remove(&s) {
                    self.key_signs.insert(s);
                }
            }
            Cell::Token(t) => {
                if !self.types.remove(&t) {
                    self.types.insert(t);
                }
            }
            Cell::All(g) => {
                // Not all selected -> select whole group; all selected -> clear it.
                if self.group_state(g) == CheckState::On {
                    for t in g.tokens() {
                        self.types.remove(t);
                    }
                } else {
                    for t in g.tokens() {
                        self.types.insert(*t);
                    }
                }
            }
        }
    }

    /// Tristate of a group's `all` row from how many member tokens are selected.
    pub fn group_state(&self, g: Group) -> CheckState {
        let n = g.tokens().iter().filter(|t| self.types.contains(t)).count();
        if n == 0 {
            CheckState::Off
        } else if n == g.tokens().len() {
            CheckState::On
        } else {
            CheckState::Partial
        }
    }

    /// Display state of any cell.
    pub fn cell_state(&self, cell: Cell) -> CheckState {
        match cell {
            Cell::Sign(s) => bool_state(self.key_signs.contains(&s)),
            Cell::Token(t) => bool_state(self.types.contains(&t)),
            Cell::All(g) => self.group_state(g),
        }
    }
}

fn bool_state(on: bool) -> CheckState {
    if on {
        CheckState::On
    } else {
        CheckState::Off
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_layout_hides_toml_only_facets() {
        use crate::model::document::DocFormat;
        let labels: Vec<&str> = layout(DocFormat::Json)
            .iter()
            .flat_map(|r| match r {
                LayoutRow::Cells(cs) => cs.iter().map(|c| c.label()).collect::<Vec<_>>(),
                LayoutRow::Header(_) => vec![],
            })
            .collect();
        assert!(labels.iter().any(|l| l.contains("[S:null]")));
        assert!(!labels.iter().any(|l| l.contains("[T/D]")));
        assert!(!labels.iter().any(|l| l.contains("[A/T]")));
        assert!(!labels.iter().any(|l| l.contains("(B) bare")));
    }

    #[test]
    fn classify_covers_every_kind_slot() {
        let c = |k: &NodeKind, f| classify(k, f, DocFormat::Toml, false);
        assert_eq!(c(&NodeKind::Root, Format::Plain), TypeToken::Root);
        assert_eq!(
            c(&NodeKind::Comment("# x".into()), Format::Plain),
            TypeToken::Comment
        );
        assert_eq!(c(&NodeKind::Array, Format::Inline), TypeToken::ArrayInline);
        assert_eq!(
            c(&NodeKind::Array, Format::Multiline),
            TypeToken::ArrayMultiline
        );
        assert_eq!(c(&NodeKind::ArrayOfTables, Format::Plain), TypeToken::Aot);
        assert_eq!(
            c(&NodeKind::InlineTable, Format::Inline),
            TypeToken::InlineTable
        );
        assert_eq!(c(&NodeKind::Table, Format::Scope), TypeToken::TableScope);
        assert_eq!(c(&NodeKind::Table, Format::Dotted), TypeToken::TableDotted);
        let s = |f| c(&NodeKind::Scalar(ScalarType::String), f);
        assert_eq!(s(Format::BasicString), TypeToken::StrBasic);
        assert_eq!(s(Format::MultilineBasic), TypeToken::StrMBasic);
        assert_eq!(s(Format::Literal), TypeToken::StrLit);
        assert_eq!(s(Format::MultilineLiteral), TypeToken::StrMLit);
        let i = |f| c(&NodeKind::Scalar(ScalarType::Integer), f);
        assert_eq!(i(Format::Decimal), TypeToken::IntDec);
        assert_eq!(i(Format::Hex), TypeToken::IntHex);
        assert_eq!(i(Format::Octal), TypeToken::IntOct);
        assert_eq!(i(Format::Binary), TypeToken::IntBin);
        let fl = |f| c(&NodeKind::Scalar(ScalarType::Float), f);
        assert_eq!(fl(Format::Plain), TypeToken::FloatPlain);
        assert_eq!(fl(Format::Inf), TypeToken::FloatInf);
        assert_eq!(fl(Format::Nan), TypeToken::FloatNan);
        assert_eq!(
            c(&NodeKind::Scalar(ScalarType::Bool), Format::Plain),
            TypeToken::Bool
        );
        assert_eq!(
            c(&NodeKind::Scalar(ScalarType::OffsetDatetime), Format::Plain),
            TypeToken::Odt
        );
        assert_eq!(
            c(&NodeKind::Scalar(ScalarType::LocalDatetime), Format::Plain),
            TypeToken::Ldt
        );
        assert_eq!(
            c(&NodeKind::Scalar(ScalarType::LocalDate), Format::Plain),
            TypeToken::LDate
        );
        assert_eq!(
            c(&NodeKind::Scalar(ScalarType::LocalTime), Format::Plain),
            TypeToken::LTime
        );
        // New JSON atoms.
        assert_eq!(
            c(&NodeKind::Scalar(ScalarType::Null), Format::Plain),
            TypeToken::Null
        );
        assert_eq!(
            c(&NodeKind::Scalar(ScalarType::Float), Format::Exponent),
            TypeToken::FloatExp
        );
        assert_eq!(
            c(&NodeKind::Table, Format::Multiline),
            TypeToken::TableMultiline
        );
    }

    #[test]
    fn classify_covers_every_yaml_slot() {
        let c = |k: &NodeKind, f| classify(k, f, DocFormat::Yaml, false);
        // Sequences split block/flow.
        assert_eq!(c(&NodeKind::Array, Format::Block), TypeToken::SeqBlock);
        assert_eq!(c(&NodeKind::Array, Format::Inline), TypeToken::SeqFlow);
        // Mappings split block/flow; an InlineTable is also a flow map.
        assert_eq!(c(&NodeKind::Table, Format::Block), TypeToken::MapBlock);
        assert_eq!(c(&NodeKind::Table, Format::Inline), TypeToken::MapFlow);
        assert_eq!(
            c(&NodeKind::InlineTable, Format::Inline),
            TypeToken::MapFlow
        );
        // YAML string styles.
        let s = |f| c(&NodeKind::Scalar(ScalarType::String), f);
        assert_eq!(s(Format::SingleQuoted), TypeToken::StrSingle);
        assert_eq!(s(Format::DoubleQuoted), TypeToken::StrDouble);
        assert_eq!(s(Format::LiteralBlock), TypeToken::StrLiteralBlock);
        assert_eq!(s(Format::Folded), TypeToken::StrFolded);
        assert_eq!(s(Format::Plain), TypeToken::StrBasic);
        // Opaque gate: any kind, read_only, YAML -> Opaque.
        assert_eq!(
            classify(&NodeKind::Table, Format::Block, DocFormat::Yaml, true),
            TypeToken::Opaque
        );
        assert_eq!(
            classify(
                &NodeKind::Scalar(ScalarType::String),
                Format::Plain,
                DocFormat::Yaml,
                true
            ),
            TypeToken::Opaque
        );
        // The gate is YAML-only: a read-only JSONC block comment is not Opaque.
        assert_ne!(
            classify(
                &NodeKind::Comment("/* x */".into()),
                Format::Plain,
                DocFormat::Json,
                true
            ),
            TypeToken::Opaque
        );
    }

    #[test]
    fn yaml_layout_hides_toml_only_facets() {
        let labels: Vec<&str> = layout(DocFormat::Yaml)
            .iter()
            .flat_map(|r| match r {
                LayoutRow::Cells(cs) => cs.iter().map(|c| c.label()).collect::<Vec<_>>(),
                LayoutRow::Header(_) => vec![],
            })
            .collect();
        // YAML-reachable facets present.
        assert!(labels.iter().any(|l| l.contains("[A/B]")));
        assert!(labels.iter().any(|l| l.contains("[T/B]")));
        assert!(labels.iter().any(|l| l.contains("[S:fold]")));
        assert!(labels.iter().any(|l| l.contains("[opaq ]")));
        // TOML/JSON-only facets absent.
        assert!(!labels.iter().any(|l| l.contains("(D) dotted")));
        assert!(!labels.iter().any(|l| l.contains("[A/T]")));
        assert!(!labels.iter().any(|l| l.contains("[A/M]")));
        assert!(!labels.iter().any(|l| l.contains("[I:bin ]")));
        assert!(!labels.iter().any(|l| l.contains("[D:odt ]")));
    }

    #[test]
    fn empty_filter_matches_everything() {
        let f = TypeFilter::default();
        assert!(!f.is_active());
        assert!(f.matches(
            KeySign::Bare,
            &NodeKind::Scalar(ScalarType::Integer),
            Format::Hex,
            DocFormat::Toml,
            false
        ));
    }

    #[test]
    fn halves_intersect_atoms_union() {
        let mut f = TypeFilter::default();
        // Type half: hex OR decimal integers.
        f.types.insert(TypeToken::IntHex);
        f.types.insert(TypeToken::IntDec);
        // Sign half: bare only.
        f.key_signs.insert(KeySign::Bare);
        let int = NodeKind::Scalar(ScalarType::Integer);
        let m = |ks, f2| f.matches(ks, &int, f2, DocFormat::Toml, false);
        // bare hex -> both halves pass.
        assert!(m(KeySign::Bare, Format::Hex));
        // bare decimal -> union within type half passes.
        assert!(m(KeySign::Bare, Format::Decimal));
        // quoted hex -> sign half fails (intersection).
        assert!(!m(KeySign::Quoted, Format::Hex));
        // bare octal -> type half fails.
        assert!(!m(KeySign::Bare, Format::Octal));
    }

    #[test]
    fn all_row_is_tristate() {
        let mut f = TypeFilter::default();
        assert_eq!(f.group_state(Group::Integer), CheckState::Off);
        f.toggle(Cell::All(Group::Integer)); // select whole group
        assert_eq!(f.group_state(Group::Integer), CheckState::On);
        f.toggle(Cell::Token(TypeToken::IntHex)); // clear one child
        assert_eq!(f.group_state(Group::Integer), CheckState::Partial);
        f.toggle(Cell::All(Group::Integer)); // partial -> select all again
        assert_eq!(f.group_state(Group::Integer), CheckState::On);
        f.toggle(Cell::All(Group::Integer)); // all -> clear
        assert_eq!(f.group_state(Group::Integer), CheckState::Off);
    }

    #[test]
    fn navigation_clamps_at_edges() {
        use crate::model::document::DocFormat;
        let fmt = DocFormat::Toml;
        let mut f = TypeFilter::default();
        let rows = nav_rows(fmt);
        f.move_cursor(-1, 0, fmt); // already at top
        assert_eq!(f.row, 0);
        f.move_cursor(0, -1, fmt); // already at left
        assert_eq!(f.col, 0);
        f.move_cursor(0, 1, fmt); // into second column of row 0
        assert_eq!(f.col, 1);
        f.move_cursor(1000, 0, fmt); // clamp to last row
        assert_eq!(f.row, rows.len() - 1);
        // Column clamps to the destination row width.
        assert!(f.col < rows[f.row].len());
    }
}
