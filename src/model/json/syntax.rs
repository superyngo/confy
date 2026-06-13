//! `SyntaxKind` for the JSON/JSONC grammar + the rowan `Language` impl.
//!
//! Token kinds are trivia (`WHITESPACE`, `NEWLINE`, `LINE_COMMENT`,
//! `BLOCK_COMMENT`), punctuation (`L_BRACE` … `COMMA`) and value tokens
//! (`STRING`, `NUMBER`, `TRUE`, `FALSE`, `NULL`). Node kinds reconstruct the
//! nesting: `ROOT` wraps the whole document, an `OBJECT`/`ARRAY` wraps its
//! braces/brackets, a `MEMBER` is one `KEY : VALUE` pair, and `VALUE` wraps the
//! actual value (a scalar token or a nested `OBJECT`/`ARRAY`). Trivia tokens
//! float as direct children of the container they sit in (same as taplo's flat
//! comment/newline tokens), so projection decides standalone-vs-trailing.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
#[allow(non_camel_case_types)]
pub enum SyntaxKind {
    // trivia
    WHITESPACE = 0,
    NEWLINE,
    LINE_COMMENT,  // // … (to end of line, newline NOT included)
    BLOCK_COMMENT, // /* … */ (may span lines)
    // punctuation
    L_BRACE,
    R_BRACE,
    L_BRACK,
    R_BRACK,
    COLON,
    COMMA,
    // value tokens
    STRING,
    NUMBER,
    TRUE,
    FALSE,
    NULL,
    ERROR,
    // nodes
    KEY,    // wraps the STRING token used as an object key
    VALUE,  // wraps one value: a scalar token OR an OBJECT/ARRAY node
    MEMBER, // KEY COLON VALUE (trivia interspersed)
    OBJECT, // L_BRACE … R_BRACE
    ARRAY,  // L_BRACK … R_BRACK
    ROOT,   // whole document
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(k: SyntaxKind) -> Self {
        rowan::SyntaxKind(k as u16)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Json {}

impl rowan::Language for Json {
    type Kind = SyntaxKind;
    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        match raw.0 {
            0 => SyntaxKind::WHITESPACE,
            1 => SyntaxKind::NEWLINE,
            2 => SyntaxKind::LINE_COMMENT,
            3 => SyntaxKind::BLOCK_COMMENT,
            4 => SyntaxKind::L_BRACE,
            5 => SyntaxKind::R_BRACE,
            6 => SyntaxKind::L_BRACK,
            7 => SyntaxKind::R_BRACK,
            8 => SyntaxKind::COLON,
            9 => SyntaxKind::COMMA,
            10 => SyntaxKind::STRING,
            11 => SyntaxKind::NUMBER,
            12 => SyntaxKind::TRUE,
            13 => SyntaxKind::FALSE,
            14 => SyntaxKind::NULL,
            15 => SyntaxKind::ERROR,
            16 => SyntaxKind::KEY,
            17 => SyntaxKind::VALUE,
            18 => SyntaxKind::MEMBER,
            19 => SyntaxKind::OBJECT,
            20 => SyntaxKind::ARRAY,
            21 => SyntaxKind::ROOT,
            n => panic!("unknown SyntaxKind discriminant: {n}"),
        }
    }
    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

#[allow(dead_code)]
pub type SyntaxNode = rowan::SyntaxNode<Json>;
#[allow(dead_code)]
pub type SyntaxToken = rowan::SyntaxToken<Json>;
#[allow(dead_code)]
pub type SyntaxElement = rowan::SyntaxElement<Json>;
