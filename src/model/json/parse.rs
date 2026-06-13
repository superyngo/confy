//! Lossless JSON/JSONC lexer + recursive-descent parser → rowan green tree.

use crate::model::json::syntax::SyntaxKind;

#[allow(dead_code)]
pub(crate) type Lexeme = (SyntaxKind, String);

/// Tokenize losslessly: every byte of `src` lands in exactly one lexeme, so
/// `lex(src).map(|(_, t)| t).concat() == src`. Malformed runs become `ERROR`
/// tokens (the parser turns the presence of any `ERROR` into a load failure).
#[allow(dead_code)]
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
                while i < b.len()
                    && matches!(b[i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
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
    use crate::model::json::syntax::SyntaxKind as K;

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
            vec![
                K::TRUE,
                K::WHITESPACE,
                K::FALSE,
                K::WHITESPACE,
                K::NULL
            ]
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
            lex("/*")
                .iter()
                .map(|(_, t)| t.clone())
                .collect::<String>(),
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
