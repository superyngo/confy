//! `SyntaxKind` for the YAML-subset grammar + the rowan `Language` impl.
//!
//! SPIKE SCOPE (spec §3.3 gate): only the token/node kinds the lossless
//! lexer/parser needs to (a) round-trip byte-identically and (b) fence
//! out-of-subset constructs as `OPAQUE`. Projection/edit kinds come in the
//! full Phase 3 plan, not here.
//!
//! Indentation is part of the token stream (`INDENT` — the leading whitespace
//! of a line), so `serialize()` stays plain token concatenation. Trivia
//! (`INDENT`/`WHITESPACE`/`NEWLINE`/`COMMENT`) float as direct children of the
//! container they sit in, exactly like the JSON backend's flat trivia.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
#[allow(non_camel_case_types)]
pub enum SyntaxKind {
    // trivia
    WHITESPACE = 0,
    NEWLINE,
    INDENT,  // leading whitespace of a line
    COMMENT, // # … to end of line (newline NOT included)
    // structural punctuation
    DASH,    // `- ` block-sequence indicator
    COLON,   // `: ` mapping indicator
    COMMA,   // flow separator
    L_BRACE, // {
    R_BRACE, // }
    L_BRACK, // [
    R_BRACK, // ]
    // scalar tokens
    PLAIN,        // plain scalar run
    SINGLE,       // 'single quoted'
    DOUBLE,       // "double quoted"
    BLOCK_HEADER, // | or > with optional chomping/indent indicators
    // markers / out-of-subset triggers
    DOC_MARKER, // --- or ...
    ANCHOR,     // &name
    ALIAS,      // *name
    TAG,        // !tag / !!tag
    MERGE,      // << (merge key)
    ERROR,
    // nodes
    KEY,
    VALUE,
    SCALAR,
    BLOCK_SCALAR,
    MAP_ENTRY,
    MAPPING,
    SEQ_ENTRY,
    SEQUENCE,
    FLOW_MAP,
    FLOW_SEQ,
    FLOW_ENTRY, // a `key: value` member inside a FLOW_MAP
    OPAQUE,     // out-of-subset span; projects read-only
    ROOT,
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(k: SyntaxKind) -> Self {
        rowan::SyntaxKind(k as u16)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Yaml {}

impl rowan::Language for Yaml {
    type Kind = SyntaxKind;
    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        use SyntaxKind::*;
        match raw.0 {
            0 => WHITESPACE,
            1 => NEWLINE,
            2 => INDENT,
            3 => COMMENT,
            4 => DASH,
            5 => COLON,
            6 => COMMA,
            7 => L_BRACE,
            8 => R_BRACE,
            9 => L_BRACK,
            10 => R_BRACK,
            11 => PLAIN,
            12 => SINGLE,
            13 => DOUBLE,
            14 => BLOCK_HEADER,
            15 => DOC_MARKER,
            16 => ANCHOR,
            17 => ALIAS,
            18 => TAG,
            19 => MERGE,
            20 => ERROR,
            21 => KEY,
            22 => VALUE,
            23 => SCALAR,
            24 => BLOCK_SCALAR,
            25 => MAP_ENTRY,
            26 => MAPPING,
            27 => SEQ_ENTRY,
            28 => SEQUENCE,
            29 => FLOW_MAP,
            30 => FLOW_SEQ,
            31 => FLOW_ENTRY,
            32 => OPAQUE,
            33 => ROOT,
            n => panic!("unknown SyntaxKind discriminant: {n}"),
        }
    }
    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

pub type SyntaxNode = rowan::SyntaxNode<Yaml>;
#[allow(dead_code)]
pub type SyntaxToken = rowan::SyntaxToken<Yaml>;
