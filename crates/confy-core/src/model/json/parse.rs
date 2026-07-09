//! Lossless JSON/JSONC lexer + recursive-descent parser → rowan green tree.

use crate::model::json::syntax::SyntaxKind;
use rowan::{GreenNode, GreenNodeBuilder};

pub(crate) type Lexeme = (SyntaxKind, String);

/// Parse `src` into a lossless green tree. Returns `Err(message)` on a structural
/// error or any `ERROR` token. Every byte is preserved, so
/// `SyntaxNode::new_root(parse(src)?).to_string() == src`.
pub(crate) fn parse(src: &str) -> Result<GreenNode, String> {
    let tokens = lex(src);
    let mut p = Parser {
        tokens,
        pos: 0,
        builder: GreenNodeBuilder::new(),
        error: None,
    };
    p.builder.start_node(SyntaxKind::ROOT.into());
    p.skip_trivia();
    if p.error.is_none() && p.peek().is_some() {
        p.value();
    }
    p.skip_trivia();
    if p.error.is_none() {
        if let Some((k, t)) = p.peek() {
            p.error = Some(format!("unexpected `{}` ({k:?}) after document", t));
        }
    }
    p.builder.finish_node(); // ROOT
    match p.error {
        Some(e) => Err(e),
        None => Ok(p.builder.finish()),
    }
}

struct Parser {
    tokens: Vec<Lexeme>,
    pos: usize,
    builder: GreenNodeBuilder<'static>,
    error: Option<String>,
}

impl Parser {
    fn peek(&self) -> Option<(SyntaxKind, &str)> {
        self.tokens.get(self.pos).map(|(k, t)| (*k, t.as_str()))
    }
    fn bump(&mut self) {
        if let Some((k, t)) = self.tokens.get(self.pos) {
            self.builder.token((*k).into(), t);
            self.pos += 1;
        }
    }
    fn skip_trivia(&mut self) {
        use SyntaxKind::*;
        while let Some((k, t)) = self.peek() {
            match k {
                WHITESPACE | NEWLINE | LINE_COMMENT | BLOCK_COMMENT => self.bump(),
                ERROR => {
                    if self.error.is_none() {
                        self.error = Some(format!("unexpected token: {t:?}"));
                    }
                    self.bump();
                }
                _ => break,
            }
        }
    }
    fn expect(&mut self, kind: SyntaxKind) -> bool {
        if self.peek().map(|(k, _)| k) == Some(kind) {
            self.bump();
            true
        } else {
            if self.error.is_none() {
                self.error = Some(format!(
                    "expected {kind:?}, found {:?}",
                    self.peek().map(|(k, _)| k)
                ));
            }
            false
        }
    }
    fn value(&mut self) {
        use SyntaxKind::*;
        self.builder.start_node(VALUE.into());
        match self.peek().map(|(k, _)| k) {
            Some(L_BRACE) => self.object(),
            Some(L_BRACK) => self.array(),
            Some(STRING | NUMBER | TRUE | FALSE | NULL) => self.bump(),
            other => {
                if self.error.is_none() {
                    self.error = Some(format!("expected a value, found {other:?}"));
                }
            }
        }
        self.builder.finish_node(); // VALUE
    }
    fn object(&mut self) {
        use SyntaxKind::*;
        self.builder.start_node(OBJECT.into());
        self.bump(); // {
        loop {
            self.skip_trivia();
            match self.peek().map(|(k, _)| k) {
                Some(R_BRACE) | None => break,
                Some(STRING) => self.member(),
                other => {
                    if self.error.is_none() {
                        self.error = Some(format!("expected string key, found {other:?}"));
                    }
                    break;
                }
            }
            self.skip_trivia();
            if self.peek().map(|(k, _)| k) == Some(COMMA) {
                self.bump();
            } else {
                break;
            }
        }
        self.skip_trivia();
        self.expect(R_BRACE);
        self.builder.finish_node(); // OBJECT
    }
    fn member(&mut self) {
        use SyntaxKind::*;
        self.builder.start_node(MEMBER.into());
        self.builder.start_node(KEY.into());
        self.bump(); // STRING key
        self.builder.finish_node(); // KEY
        self.skip_trivia();
        self.expect(COLON);
        self.skip_trivia();
        self.value();
        self.builder.finish_node(); // MEMBER
    }
    fn array(&mut self) {
        use SyntaxKind::*;
        self.builder.start_node(ARRAY.into());
        self.bump(); // [
        loop {
            self.skip_trivia();
            match self.peek().map(|(k, _)| k) {
                Some(R_BRACK) | None => break,
                Some(STRING | NUMBER | TRUE | FALSE | NULL | L_BRACE | L_BRACK) => self.value(),
                other => {
                    if self.error.is_none() {
                        self.error = Some(format!("expected a value, found {other:?}"));
                    }
                    break;
                }
            }
            self.skip_trivia();
            if self.peek().map(|(k, _)| k) == Some(COMMA) {
                self.bump();
            } else {
                break;
            }
        }
        self.skip_trivia();
        self.expect(R_BRACK);
        self.builder.finish_node(); // ARRAY
    }
}

/// Tokenize losslessly: every byte of `src` lands in exactly one lexeme, so
/// `lex(src).map(|(_, t)| t).concat() == src`. Malformed runs become `ERROR`
/// tokens (the parser turns the presence of any `ERROR` into a load failure).
pub(crate) fn lex(src: &str) -> Vec<Lexeme> {
    use SyntaxKind::*;
    let b = src.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let start = i;
        let c = b[i];
        let kind = match c {
            b'\n' => {
                i += 1;
                NEWLINE
            }
            b'\r' if b.get(i + 1) == Some(&b'\n') => {
                i += 2;
                NEWLINE
            }
            b' ' | b'\t' | b'\r' => {
                while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\r') {
                    i += 1;
                }
                WHITESPACE
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                i += 2;
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                LINE_COMMENT
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                    i += 1;
                }
                if i < b.len() {
                    i += 2; // consume */
                }
                BLOCK_COMMENT
            }
            b'{' => {
                i += 1;
                L_BRACE
            }
            b'}' => {
                i += 1;
                R_BRACE
            }
            b'[' => {
                i += 1;
                L_BRACK
            }
            b']' => {
                i += 1;
                R_BRACK
            }
            b':' => {
                i += 1;
                COLON
            }
            b',' => {
                i += 1;
                COMMA
            }
            b'"' => {
                i += 1;
                while i < b.len() {
                    match b[i] {
                        b'\\' => {
                            // guard: only advance past the escaped byte if it exists
                            i += 1;
                            if i < b.len() {
                                i += 1;
                            }
                        }
                        b'"' => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                STRING
            }
            b'-' | b'0'..=b'9' => {
                if b[i] == b'-' {
                    i += 1;
                }
                while i < b.len() && matches!(b[i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
                {
                    i += 1;
                }
                NUMBER
            }
            b't' if src[i..].starts_with("true") => {
                i += 4;
                TRUE
            }
            b'f' if src[i..].starts_with("false") => {
                i += 5;
                FALSE
            }
            b'n' if src[i..].starts_with("null") => {
                i += 4;
                NULL
            }
            _ => {
                i += 1;
                while i < b.len()
                    && !matches!(
                        b[i],
                        b'{' | b'}'
                            | b'['
                            | b']'
                            | b':'
                            | b','
                            | b'"'
                            | b' '
                            | b'\t'
                            | b'\r'
                            | b'\n'
                            | b'/'
                    )
                {
                    i += 1;
                }
                ERROR
            }
        };
        // Clamp: guard against any arithmetic overshoot before slicing.
        let i = i.min(b.len());
        out.push((kind, src[start..i].to_string()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::json::syntax::{SyntaxKind as K, SyntaxNode};

    #[test]
    fn parse_roundtrips_byte_identical() {
        for src in [
            "{}",
            "{ \"a\": 1 }\n",
            "[1, 2, 3]\n",
            "// c\n{\n  \"x\": true,\n}\n",
            "/* b */ null\n",
            "{\n  \"a\": 1,\n}\n",
            include_str!("../../../tests/fixtures/sample.json"),
            include_str!("../../../tests/fixtures/sample.jsonc"),
        ] {
            let green = parse(src).expect("parse ok");
            let node = SyntaxNode::new_root(green);
            assert_eq!(node.to_string(), src, "roundtrip mismatch for {src:?}");
        }
    }

    #[test]
    fn parse_rejects_invalid() {
        assert!(parse("{ \"a\": }").is_err()); // missing value
        assert!(parse("{ \"a\" 1 }").is_err()); // missing colon
        assert!(parse("[1 2]").is_err()); // missing comma
        assert!(parse("@nonsense").is_err()); // ERROR token
    }

    fn kinds(src: &str) -> Vec<K> {
        lex(src).into_iter().map(|(k, _)| k).collect()
    }
    fn text(src: &str) -> String {
        lex(src).into_iter().map(|(_, t)| t).collect()
    }

    #[test]
    fn lex_is_lossless() {
        for src in [
            "{}",
            "{ \"a\": 1 }\n",
            "[1, 2, 3]",
            "// c\n{\n  \"x\": true, // trailing\n}\n",
            "/* block */ null",
            "{ \"a\": 1, }",
            "{\"s\":\"a\\\"b\",\"e\":1.5e-3}",
        ] {
            assert_eq!(text(src), src, "lex not lossless for {src:?}");
        }
    }

    #[test]
    fn lex_token_kinds() {
        assert_eq!(
            kinds("{ \"a\": 1 }"),
            vec![
                K::L_BRACE,
                K::WHITESPACE,
                K::STRING,
                K::COLON,
                K::WHITESPACE,
                K::NUMBER,
                K::WHITESPACE,
                K::R_BRACE
            ]
        );
        assert_eq!(kinds("// hi\n"), vec![K::LINE_COMMENT, K::NEWLINE]);
        assert_eq!(kinds("/* x */"), vec![K::BLOCK_COMMENT]);
        assert_eq!(
            kinds("true false null"),
            vec![K::TRUE, K::WHITESPACE, K::FALSE, K::WHITESPACE, K::NULL]
        );
    }

    #[test]
    fn lex_handles_unterminated() {
        // must not panic; lossless coverage still holds
        assert_eq!(
            lex("\"\\")
                .iter()
                .map(|(_, t)| t.clone())
                .collect::<String>(),
            "\"\\"
        );
        assert_eq!(
            lex("/*").iter().map(|(_, t)| t.clone()).collect::<String>(),
            "/*"
        );
        assert_eq!(
            lex("tru")
                .iter()
                .map(|(_, t)| t.clone())
                .collect::<String>(),
            "tru"
        );
    }
}
