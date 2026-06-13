# JSON/JSONC Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a lossless JSON/JSONC backend to confy, mirroring the TOML CST trio (`cst_doc`/`cst_project`/`cst_edit`), wired through the Phase-1 `AnyDocument`/`ConfigDocument`/`kind_options` abstraction so JSON/JSONC files load, project, edit and round-trip byte-identically.

**Architecture:** A hand-rolled lossless lexer + recursive-descent parser builds a `rowan` green tree (rowan added as a direct dependency, pinned to taplo's version). Six files under `src/model/json/` mirror the `cst_*` idioms: `clone_for_update` atomic commit, `validate_semantics` backstop, golden projection tests. JSONC adds `//` line comments (Comment nodes / trailing comments), read-only `/* */` blocks (new `Node.read_only` flag), and accepted-but-never-emitted trailing commas. New model atoms: `ScalarType::Null`, `Format::Exponent`, `KindTarget::TableMultiline`, KIND tags `[S:null]`/`[T/M]`/`[F:exp ]`.

**Tech Stack:** Rust, `rowan` 0.15 (green/red trees), the existing `ConfigDocument` trait, ratatui TUI.

**Locked design decisions (from planning):**
- KIND tags use the **letter-prefix** convention already in `type_tag` (`[I:dec ]`, `[B:bool]`, `[F:flt ]`/`[F:exp ]`); `null` is `[S:null]`, multiline object is `[T/M]`.
- A `null` value edited to another type goes through the **existing inline type-change** prompt (`K` returns no options for null).
- JSONC upgrade saves **in place** — the `.json` extension is never rewritten.

---

## File structure

```
src/model/json/
  mod.rs        re-exports (pub use doc::JsonDocument; pub(crate) modules)
  syntax.rs     SyntaxKind enum + rowan Language impl + tree-builder type aliases
  parse.rs      lexer (tokens incl. // /* */, trailing comma) + recursive-descent parser → green tree
  doc.rs        JsonDocument: load/serialize/apply (atomic commit) + ConfigDocument facets + kind_options + validate_semantics
  project.rs    CST → NodeTree projection + golden tests
  edit.rs       splice helpers: one fn per Mutation variant + serialize_fragment + the path→element walk
```

**Files modified (shared model/TUI):**
- `Cargo.toml` — add `rowan = "=0.15.18"`.
- `src/model/mod.rs` — `pub mod json;`.
- `src/model/node.rs` — `ScalarType::Null`, `Format::Exponent`, `Node.read_only`.
- `src/model/document.rs` — `KindTarget::TableMultiline`.
- `src/model/any_doc.rs` — `AnyDocument::Json` variant, `load_as` wiring.
- `src/tui/app.rs` — `type_tag` (`[S:null]`/`[T/M]`/`[F:exp ]`), `node_type_label` (Null), JSONC-upgrade flow, read-only guards.
- `src/tui/type_filter.rs` — `TypeToken::Null`/`FloatExp`/`TableMultiline`, `classify` arms, `layout(format)`/`nav_rows(format)` facet filtering.
- `src/tui/state.rs` — `PromptKind::JsoncUpgrade`.
- `src/tui/keys.rs` — `JSON_HELP` constant + `help_text` arm.
- `src/tui/ui.rs` — pass `DocFormat` to `layout`.
- `tests/fixtures/*.json`, `*.jsonc` — roundtrip corpus.
- Docs: `CHANGELOG.md`, `CLAUDE.md`, `CONTEXT.md`, `README.md`.

---

## Conventions for every task

- After each task: `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test` must all be green before committing.
- Tests live in `#[cfg(test)] mod tests` at the bottom of each module (the TOML trio's pattern), except the integration roundtrip which lives in `tests/`.
- Never drive the TUI via a pty or a long-lived background process — manual real-binary verification is the user's job (noted where relevant).

---

## Task 1: Model atoms — `ScalarType::Null`, `Format::Exponent`, `Node.read_only`, `KindTarget::TableMultiline`

**Files:**
- Modify: `src/model/node.rs`
- Modify: `src/model/document.rs:120-138` (`KindTarget`)

- [ ] **Step 1: Add the enum variants**

In `src/model/node.rs`, add `Null` to `ScalarType`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarType {
    String,
    Integer,
    Float,
    Bool,
    Null,
    OffsetDatetime,
    LocalDatetime,
    LocalDate,
    LocalTime,
}
```

Add `Exponent` to `Format` (after the float `Inf`/`Nan` group):

```rust
    // Float (plain floats stay `Plain`)
    Inf,
    Nan,
    /// Float written in exponent notation (`1e5`, `1.2E-3`). New in the JSON
    /// backend; the TOML projection still detects exponent from value text.
    Exponent,
```

- [ ] **Step 2: Add `read_only` to `Node` and both constructors**

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub key: String,
    pub path: Path,
    pub kind: NodeKind,
    pub children: Vec<Node>,
    pub value: Option<String>,
    pub format: Format,
    pub key_sign: KeySign,
    pub trailing_comment: Option<String>,
    /// Read-only nodes (a JSONC `/* */` block comment, a Phase-3 opaque YAML
    /// node) display and copy but reject `e`/`d`/`x`/`r`/insert-into. Default false.
    pub read_only: bool,
}
```

In both `Node::branch` and `Node::leaf`, add `read_only: false,` to the struct literal.

- [ ] **Step 3: Add `KindTarget::TableMultiline`**

In `src/model/document.rs`, after `TableScope`:

```rust
    TableScope,
    /// A JSON object spread over multiple lines (`[T/M]`). TOML's scope table
    /// stays `[T/S]`; this is the JSON multiline-object form.
    TableMultiline,
```

- [ ] **Step 4: Build to verify exhaustive matches still compile**

Run: `cargo build 2>&1 | head -40`
Expected: errors only at the **non-exhaustive match** sites that switch on `ScalarType`/`Format`/`KindTarget` (e.g. `app.rs` `type_tag`/`node_type_label`, `type_filter.rs` `classify`). These are fixed in Task 2.

- [ ] **Step 5: Commit**

```bash
git add src/model/node.rs src/model/document.rs
git commit -m "feat(model): add ScalarType::Null, Format::Exponent, Node.read_only, KindTarget::TableMultiline"
```

---

## Task 2: KIND tags & type-filter facets for the new atoms (keep `classify`↔`type_tag` inverse)

**Files:**
- Modify: `src/tui/app.rs:2399-2459` (`node_type_label`, `type_tag`)
- Modify: `src/tui/type_filter.rs` (`TypeToken`, `classify`, `Group`, `token_label`, the inverse-invariant test)

- [ ] **Step 1: Extend the inverse-invariant test first (it must fail to compile / fail)**

In `src/tui/type_filter.rs` `mod tests`, the existing `classify_covers_every_kind_slot` test gets new asserts. Add after the float asserts:

```rust
        // New JSON atoms.
        assert_eq!(
            classify(&NodeKind::Scalar(ScalarType::Null), Format::Plain),
            TypeToken::Null
        );
        assert_eq!(
            classify(&NodeKind::Scalar(ScalarType::Float), Format::Exponent),
            TypeToken::FloatExp
        );
        assert_eq!(
            classify(&NodeKind::Table, Format::Multiline),
            TypeToken::TableMultiline
        );
```

- [ ] **Step 2: Add the new `TypeToken` variants**

In `src/tui/type_filter.rs`, add to `enum TypeToken`:

```rust
    TableMultiline,    // [T/M]  JSON multiline object
    Null,              // [S:null]
    FloatExp,          // [F:exp ]
```

- [ ] **Step 3: Extend `classify` arms (mirror `type_tag` exactly)**

```rust
        NodeKind::Table => match format {
            Format::Dotted => TypeToken::TableDotted,
            Format::Multiline => TypeToken::TableMultiline,
            _ => TypeToken::TableScope,
        },
        NodeKind::Scalar(st) => match (st, format) {
            // ... existing string/int arms ...
            (ScalarType::Float, Format::Inf) => TypeToken::FloatInf,
            (ScalarType::Float, Format::Nan) => TypeToken::FloatNan,
            (ScalarType::Float, Format::Exponent) => TypeToken::FloatExp,
            (ScalarType::Float, _) => TypeToken::FloatPlain,
            (ScalarType::Bool, _) => TypeToken::Bool,
            (ScalarType::Null, _) => TypeToken::Null,
            // ... existing datetime arms ...
        },
```

- [ ] **Step 4: Extend `type_tag` in `app.rs` arm-for-arm**

```rust
        NodeKind::Table => match format {
            Format::Dotted => "[T/D]",
            Format::Multiline => "[T/M]",
            _ => "[T/S]",
        },
        NodeKind::Scalar(st) => match (st, format) {
            // ... existing ...
            (ScalarType::Float, Format::Inf) => "[F:inf ]",
            (ScalarType::Float, Format::Nan) => "[F:nan ]",
            (ScalarType::Float, Format::Exponent) => "[F:exp ]",
            (ScalarType::Float, _) => "[F:flt ]",
            (ScalarType::Bool, _) => "[B:bool]",
            (ScalarType::Null, _) => "[S:null]",
            // ... existing datetime ...
        },
```

And `node_type_label` already does `format!("{st:?}").to_lowercase()` for `Scalar(st)`, so `Null` renders `"null"` automatically — no change needed there.

- [ ] **Step 5: Wire the new tokens into `Group::tokens` and `token_label`**

In `Group::tokens`, add `TableMultiline` to the Table group and `FloatExp` to the Float group:

```rust
            Group::Table => &[Aot, InlineTable, TableScope, TableDotted, TableMultiline],
            Group::Float => &[FloatPlain, FloatInf, FloatNan, FloatExp],
```

In `token_label`:

```rust
        TableMultiline => "[T/M] multiline",
        Null => "[S:null]",
        FloatExp => "[F:exp ]",
```

`Null` is a standalone cell (like `Bool`/`Root`/`Comment`), not in a group — handled by layout in Task 17.

- [ ] **Step 6: Run the invariant test**

Run: `cargo test -p confy type_filter 2>&1 | tail -20` (or `cargo test classify_covers`)
Expected: PASS. Also `cargo test --lib 2>&1 | tail -5` green.

- [ ] **Step 7: Commit**

```bash
git add src/tui/app.rs src/tui/type_filter.rs
git commit -m "feat(tui): KIND tags [S:null]/[T/M]/[F:exp ] + type-filter facets (classify↔type_tag invariant)"
```

---

## Task 3: rowan direct dependency + `json/syntax.rs`

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/model/mod.rs`
- Create: `src/model/json/mod.rs`
- Create: `src/model/json/syntax.rs`

- [ ] **Step 1: Pin rowan to taplo's version**

Add under `[dependencies]` in `Cargo.toml` (taplo 0.14 → rowan 0.15.18, confirmed via `cargo tree -i rowan`):

```toml
rowan = "=0.15.18"        # lossless hand-rolled JSON/JSONC CST (same version taplo uses)
```

Run: `cargo tree -i rowan 2>&1 | head` — expect a single `rowan v0.15.18` in the tree (no duplicate).

- [ ] **Step 2: Register the module**

In `src/model/mod.rs`, add:

```rust
pub mod json;
```

- [ ] **Step 3: Write `json/mod.rs`**

```rust
//! Lossless JSON/JSONC backend (mirrors the `cst_*` TOML trio). A hand-rolled
//! lexer/parser builds a `rowan` green tree as the single source of truth, so
//! `serialize()` is plain token concatenation and an untouched file round-trips
//! byte-identically. JSONC extensions: `//` line comments (Comment nodes /
//! trailing comments), read-only `/* */` blocks, and trailing commas accepted on
//! parse but never emitted by confy's own splices.

pub mod syntax;
pub mod parse;
pub mod doc;
pub mod project;
pub mod edit;

pub use doc::JsonDocument;
```

- [ ] **Step 4: Write `json/syntax.rs`**

```rust
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u16)]
#[allow(non_camel_case_types)]
pub enum SyntaxKind {
    // trivia
    WHITESPACE = 0,
    NEWLINE,
    LINE_COMMENT,   // // … (to end of line, newline NOT included)
    BLOCK_COMMENT,  // /* … */ (may span lines)
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
    KEY,     // wraps the STRING token used as an object key
    VALUE,   // wraps one value: a scalar token OR an OBJECT/ARRAY node
    MEMBER,  // KEY COLON VALUE (trivia interspersed)
    OBJECT,  // L_BRACE … R_BRACE
    ARRAY,   // L_BRACK … R_BRACK
    ROOT,    // whole document
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
        // ROOT is the highest discriminant; assert keeps the cast honest.
        assert!(raw.0 <= SyntaxKind::ROOT as u16);
        // SAFETY: SyntaxKind is repr(u16), contiguous 0..=ROOT.
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }
    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

pub type SyntaxNode = rowan::SyntaxNode<Json>;
pub type SyntaxToken = rowan::SyntaxToken<Json>;
pub type SyntaxElement = rowan::SyntaxElement<Json>;
```

- [ ] **Step 5: Stub `parse.rs`, `doc.rs`, `project.rs`, `edit.rs` so the crate compiles**

Create each with a `//!` doc line and a single placeholder so module registration compiles. They are filled by later tasks. Example `src/model/json/parse.rs`:

```rust
//! Lossless JSON/JSONC lexer + recursive-descent parser → rowan green tree.
```

`doc.rs`:

```rust
//! `JsonDocument` — the lossless JSON/JSONC backend (mirrors `cst_doc.rs`).
```

`project.rs`:

```rust
//! JSON CST → `NodeTree` projection (mirrors `cst_project.rs`; golden tests).
```

`edit.rs`:

```rust
//! JSON rowan splice helpers: one fn per `Mutation` variant (mirrors `cst_edit.rs`).
```

- [ ] **Step 6: Build**

Run: `cargo build 2>&1 | tail -10`
Expected: clean (unused-module warnings allowed; no errors).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src/model/mod.rs src/model/json/
git commit -m "feat(json): rowan dep + json module skeleton (syntax.rs grammar)"
```

---

## Task 4: Lexer (`json/parse.rs` — tokens)

The lexer produces a `Vec<(SyntaxKind, String)>` of lossless tokens covering every byte. The parser (Task 5) assembles them into a green tree.

**Files:**
- Modify: `src/model/json/parse.rs`
- Test: same file `mod tests`

- [ ] **Step 1: Write the failing lexer test**

```rust
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
            "{ \"a\": 1, }",            // trailing comma
            "{\"s\":\"a\\\"b\",\"e\":1.5e-3}",
        ] {
            assert_eq!(text(src), src, "lex not lossless for {src:?}");
        }
    }

    #[test]
    fn lex_token_kinds() {
        assert_eq!(
            kinds("{ \"a\": 1 }"),
            vec![K::L_BRACE, K::WHITESPACE, K::STRING, K::COLON,
                 K::WHITESPACE, K::NUMBER, K::WHITESPACE, K::R_BRACE]
        );
        assert_eq!(kinds("// hi\n"), vec![K::LINE_COMMENT, K::NEWLINE]);
        assert_eq!(kinds("/* x */"), vec![K::BLOCK_COMMENT]);
        assert_eq!(kinds("true false null"),
            vec![K::TRUE, K::WHITESPACE, K::FALSE, K::WHITESPACE, K::NULL]);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p confy json::parse 2>&1 | tail -20`
Expected: FAIL — `lex` not found.

- [ ] **Step 3: Implement the lexer**

Add to `parse.rs`:

```rust
use crate::model::json::syntax::SyntaxKind;

pub(crate) type Lexeme = (SyntaxKind, String);

/// Tokenize losslessly: every byte of `src` lands in exactly one lexeme, so
/// `lex(src).map(|(_, t)| t).concat() == src`. Malformed runs become `ERROR`
/// tokens (the parser turns the presence of any `ERROR` into a load failure).
pub(crate) fn lex(src: &str) -> Vec<Lexeme> {
    use SyntaxKind::*;
    let b = src.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i];
        let start = i;
        let kind = match c {
            b'\n' => {
                // Greedily group consecutive newline+inter-line whitespace runs is
                // NOT done here — one NEWLINE token per '\n' keeps positions simple;
                // a bare '\r' before '\n' is folded in.
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
            b'{' => { i += 1; L_BRACE }
            b'}' => { i += 1; R_BRACE }
            b'[' => { i += 1; L_BRACK }
            b']' => { i += 1; R_BRACK }
            b':' => { i += 1; COLON }
            b',' => { i += 1; COMMA }
            b'"' => {
                i += 1;
                while i < b.len() {
                    match b[i] {
                        b'\\' => i += 2,
                        b'"' => { i += 1; break; }
                        _ => i += 1,
                    }
                }
                STRING
            }
            b'-' | b'0'..=b'9' => {
                // number: optional -, int, optional frac, optional exp
                if b[i] == b'-' { i += 1; }
                while i < b.len() && matches!(b[i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-') {
                    i += 1;
                }
                NUMBER
            }
            b't' if src[i..].starts_with("true") => { i += 4; TRUE }
            b'f' if src[i..].starts_with("false") => { i += 5; FALSE }
            b'n' if src[i..].starts_with("null") => { i += 4; NULL }
            _ => {
                // unknown byte run → ERROR up to the next structural/trivia char
                i += 1;
                while i < b.len()
                    && !matches!(b[i], b'{'|b'}'|b'['|b']'|b':'|b','|b'"'|b' '|b'\t'|b'\r'|b'\n'|b'/')
                {
                    i += 1;
                }
                ERROR
            }
        };
        out.push((kind, src[start..i].to_string()));
    }
    out
}
```

> Note: the `NUMBER` lexer is intentionally permissive (it accepts `1.2.3`); the parser's structural validation plus `validate_semantics` (and the projection's numeric classification) reject genuinely malformed numbers. The losslessness guarantee only requires byte coverage, which this satisfies.

- [ ] **Step 4: Run the lexer tests**

Run: `cargo test -p confy json::parse 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/model/json/parse.rs
git commit -m "feat(json): lossless JSON/JSONC lexer"
```

---

## Task 5: Parser (`json/parse.rs` — green tree) + byte-identical roundtrip

**Files:**
- Modify: `src/model/json/parse.rs`
- Create: `tests/fixtures/sample.json`, `tests/fixtures/sample.jsonc`

- [ ] **Step 1: Add fixtures**

`tests/fixtures/sample.json`:

```json
{
  "name": "confy",
  "version": 5,
  "ratio": 1.5,
  "enabled": true,
  "disabled": false,
  "nothing": null,
  "tags": ["a", "b", "c"],
  "nested": {
    "host": "localhost",
    "port": 8080,
    "exp": 1.2e-3
  },
  "matrix": [[1, 2], [3, 4]]
}
```

`tests/fixtures/sample.jsonc`:

```jsonc
// top-of-file comment
{
  // a line comment above a member
  "name": "confy", // trailing comment
  "list": [
    1,
    2, // trailing on element
  ],
  /* a read-only block comment */
  "ok": true,
}
```

- [ ] **Step 2: Write the failing roundtrip test (in `parse.rs`)**

```rust
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
            let node = super::super::syntax::SyntaxNode::new_root(green);
            assert_eq!(node.to_string(), src, "roundtrip mismatch for {src:?}");
        }
    }

    #[test]
    fn parse_rejects_invalid() {
        assert!(parse("{ \"a\": }").is_err());      // missing value
        assert!(parse("{ \"a\" 1 }").is_err());      // missing colon
        assert!(parse("[1 2]").is_err());            // missing comma
        assert!(parse("@nonsense").is_err());        // ERROR token
    }
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p confy parse_roundtrips 2>&1 | tail -20`
Expected: FAIL — `parse` not found.

- [ ] **Step 4: Implement the parser**

Add to `parse.rs`:

```rust
use rowan::{GreenNode, GreenNodeBuilder};
use crate::model::json::syntax::Json;

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
    // Anything left over (a second value, stray punctuation) is an error.
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

    /// Emit the current token into the tree and advance.
    fn bump(&mut self) {
        if let Some((k, t)) = self.tokens.get(self.pos) {
            self.builder.token((*k).into(), t);
            self.pos += 1;
        }
    }

    /// Emit trivia (whitespace, newlines, comments, and any ERROR — recorded) at
    /// the current position into the *current* node.
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

    /// Parse one VALUE node (scalar token, OBJECT, or ARRAY), wrapping it.
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
            // optional comma (trailing comma accepted)
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
```

- [ ] **Step 5: Run the roundtrip + reject tests**

Run: `cargo test -p confy json::parse 2>&1 | tail -20`
Expected: PASS (lexer + roundtrip + reject).

- [ ] **Step 6: Commit**

```bash
git add src/model/json/parse.rs tests/fixtures/sample.json tests/fixtures/sample.jsonc
git commit -m "feat(json): recursive-descent parser → lossless green tree + roundtrip fixtures"
```

---

## Task 6: `JsonDocument` load/serialize + ConfigDocument facets

**Files:**
- Modify: `src/model/json/doc.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::document::{ConfigDocument, DocFormat};
    use std::io::Write;

    fn json_from_str(name: &str, s: &str) -> JsonDocument {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, s).unwrap();
        let doc = JsonDocument::load(&path).unwrap();
        std::mem::forget(dir); // keep temp alive for the doc's path
        doc
    }

    #[test]
    fn roundtrip_and_facets() {
        let src = "{\n  \"a\": 1\n}\n";
        let doc = json_from_str("c.json", src);
        assert_eq!(doc.serialize(), src);
        assert!(!doc.is_dirty());
        assert_eq!(doc.format(), DocFormat::Json);
        assert_eq!(doc.comment_prefix(), "//");
    }

    #[test]
    fn pure_json_starts_without_comment_support() {
        let doc = json_from_str("c.json", "{}\n");
        assert!(!doc.supports_comments()); // pure .json, no comments yet
    }

    #[test]
    fn jsonc_extension_supports_comments() {
        let doc = json_from_str("c.jsonc", "{}\n");
        assert!(doc.supports_comments());
    }

    #[test]
    fn existing_comment_enables_support() {
        let doc = json_from_str("c.json", "// hi\n{}\n");
        assert!(doc.supports_comments()); // a .json that already has // is JSONC
    }

    #[test]
    fn load_rejects_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bad.json");
        std::fs::write(&p, "{ \"a\": }").unwrap();
        assert!(JsonDocument::load(&p).is_err());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p confy json::doc 2>&1 | tail -20`
Expected: FAIL — `JsonDocument` not found.

- [ ] **Step 3: Implement `JsonDocument` core**

```rust
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::model::document::{ConfigDocument, DocFormat, KindTarget, MutateError, Mutation};
use crate::model::json::syntax::{Json, SyntaxKind, SyntaxNode};
use crate::model::node::{NodeTree, Seg};
use rowan::ast::AstNode; // not used directly; keep parity imports minimal

pub struct JsonDocument {
    pub(crate) syntax: SyntaxNode,
    pub(crate) path: PathBuf,
    pub(crate) original: String,
    pub(crate) filename: String,
    /// True once authored comments are legal: the file already contained a `//`
    /// or `/* */` at load, OR the extension is `.jsonc`, OR the user accepted the
    /// JSONC upgrade this session. A pure `.json` with no comments starts false.
    pub(crate) comments_enabled: bool,
}

impl ConfigDocument for JsonDocument {
    fn load(path: &Path) -> anyhow::Result<Self> {
        let original =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let green = crate::model::json::parse::parse(&original)
            .map_err(|e| anyhow::anyhow!("parsing {} as JSON: {}", path.display(), e))?;
        let syntax = SyntaxNode::new_root(green);
        let filename = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let is_jsonc_ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("jsonc"))
            .unwrap_or(false);
        let has_comment = original.contains("//") || original.contains("/*");
        let comments_enabled = is_jsonc_ext || has_comment;
        Ok(JsonDocument {
            syntax,
            path: path.to_path_buf(),
            original,
            filename,
            comments_enabled,
        })
    }

    fn project(&self) -> NodeTree {
        crate::model::json::project::project(&self.syntax, &self.filename)
    }

    fn serialize(&self) -> String {
        self.syntax.to_string()
    }

    fn is_dirty(&self) -> bool {
        self.serialize() != self.original
    }

    fn serialize_fragment(&self, path: &[Seg]) -> String {
        if path.is_empty() {
            return self.serialize();
        }
        crate::model::json::edit::serialize_fragment(&self.syntax, path)
    }

    fn serialize_fragment_relative(&self, path: &[Seg]) -> String {
        // JSON has no dotted scope tables, so relative == absolute fragment.
        self.serialize_fragment(path)
    }

    fn apply(&mut self, m: Mutation) -> Result<(), MutateError> {
        let new = crate::model::json::edit::apply(&self.syntax, m)?;
        // Re-parse the spliced text so the next apply works on a fresh tree
        // (byte-identical, like the TOML backend).
        let text = new.to_string();
        let green = crate::model::json::parse::parse(&text)
            .map_err(MutateError::Fragment)?;
        self.syntax = SyntaxNode::new_root(green);
        Ok(())
    }

    fn format(&self) -> DocFormat {
        DocFormat::Json
    }
    fn comment_prefix(&self) -> &'static str {
        "//"
    }
    fn supports_comments(&self) -> bool {
        self.comments_enabled
    }

    fn kind_options(&self, path: &[Seg]) -> Vec<(String, KindTarget)> {
        crate::model::json::doc::kind_options(&self.project(), path)
    }
}

impl JsonDocument {
    pub fn save(&self) -> std::io::Result<()> {
        std::fs::write(&self.path, self.serialize())
    }
    pub fn mark_saved(&mut self) {
        self.original = self.serialize();
    }
    pub fn replace_from_str(&mut self, s: &str) -> Result<(), MutateError> {
        let green = crate::model::json::parse::parse(s).map_err(MutateError::Fragment)?;
        self.syntax = SyntaxNode::new_root(green);
        Ok(())
    }
    /// Accept the JSONC upgrade: authored comments become legal for this session.
    pub fn enable_comments(&mut self) {
        self.comments_enabled = true;
    }
}
```

> Remove the unused `rowan::ast::AstNode` import if clippy flags it; it is listed only as a reminder and is not required.

`kind_options` is implemented in Task 16 — for now add a stub returning `Vec::new()`:

```rust
pub(crate) fn kind_options(
    _tree: &NodeTree,
    _path: &[Seg],
) -> Vec<(String, KindTarget)> {
    Vec::new()
}
```

Also stub `edit::apply`, `edit::serialize_fragment` and `project::project` minimally so this compiles (filled in later tasks). In `project.rs`:

```rust
use crate::model::json::syntax::SyntaxNode;
use crate::model::node::{Format, NodeKind, NodeTree, Node, KeySign};

pub fn project(_syntax: &SyntaxNode, filename: &str) -> NodeTree {
    NodeTree { root: Node::branch(filename, NodeKind::Root) }
}
```

In `edit.rs`:

```rust
use crate::model::document::{MutateError, Mutation};
use crate::model::json::syntax::SyntaxNode;
use crate::model::node::Seg;

pub fn apply(_syntax: &SyntaxNode, _m: Mutation) -> Result<SyntaxNode, MutateError> {
    Err(MutateError::Unsupported)
}
pub fn serialize_fragment(_syntax: &SyntaxNode, _path: &[Seg]) -> String {
    String::new()
}
```

- [ ] **Step 4: Run the doc tests**

Run: `cargo test -p confy json::doc 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/model/json/
git commit -m "feat(json): JsonDocument load/serialize/facets (apply/project/kind_options stubbed)"
```

---

## Task 7: Projection — scalars, objects, arrays (golden tests)

This is the crux for display. Mirror `cst_project.rs`: a single `walk` builds the `NodeTree` and a `path → SyntaxElement` resolver index together.

**Files:**
- Modify: `src/model/json/project.rs`

Projection rules (spec §2.2):

| construct | NodeKind | Format | KeySign | value |
|---|---|---|---|---|
| object | `Table` | `Inline` (one-line) / `Multiline` | `Quoted` (its key) / `None` (root/element) | `None` |
| array | `Array` | `Inline` / `Multiline` | as above | one-line repr if `Inline`, else `None` |
| string | `Scalar(String)` | `Plain` | `Quoted`/`None` | decoded-for-display? **No — raw token incl. quotes**, matching how TOML keeps source repr |
| int (no `.`/`eE`) | `Scalar(Integer)` | `Decimal` | … | raw number text |
| float | `Scalar(Float)` | `Plain` / `Exponent` | … | raw number text |
| `true`/`false` | `Scalar(Bool)` | `Plain` | … | `"true"`/`"false"` |
| `null` | `Scalar(Null)` | `Plain` | … | `"null"` |
| standalone `//` line(s) | `Comment` | `Plain` | `None` | the `//…` text |
| end-of-line `//` | owner `trailing_comment` | — | — | — |
| `/* */` block | `Comment`, `read_only:true` | `Plain` | `None` | raw block text |

Addressing: object members by `Seg::Key(name)` (the key STRING with quotes stripped for the segment, like TOML bare/quoted keys map to `Seg::Key`); array elements and comments by `Seg::Index(i)` over the parent's **full child sequence** (comments share the slot space) — identical to TOML.

Root: if the top value is an object, the root node's children are its members; if an array, its children are the elements; if a scalar, the root has one keyless child (the scalar). The Root node has the empty path.

- [ ] **Step 1: Write the failing golden test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::json::syntax::SyntaxNode;
    use crate::model::node::{Format, KeySign, NodeKind, ScalarType, Seg};

    fn tree(src: &str) -> NodeTree {
        let green = crate::model::json::parse::parse(src).unwrap();
        project(&SyntaxNode::new_root(green), "c.json")
    }

    #[test]
    fn scalars_and_object() {
        let t = tree("{\n  \"s\": \"x\",\n  \"i\": 8080,\n  \"f\": 1.5,\n  \"e\": 1e3,\n  \"b\": true,\n  \"n\": null\n}\n");
        let root = &t.root;
        assert_eq!(root.kind, NodeKind::Root);
        let by_key = |k: &str| root.children.iter().find(|c| c.key == k).unwrap();
        assert_eq!(by_key("s").kind, NodeKind::Scalar(ScalarType::String));
        assert_eq!(by_key("s").key_sign, KeySign::Quoted);
        assert_eq!(by_key("i").kind, NodeKind::Scalar(ScalarType::Integer));
        assert_eq!(by_key("i").format, Format::Decimal);
        assert_eq!(by_key("f").kind, NodeKind::Scalar(ScalarType::Float));
        assert_eq!(by_key("f").format, Format::Plain);
        assert_eq!(by_key("e").format, Format::Exponent);
        assert_eq!(by_key("b").kind, NodeKind::Scalar(ScalarType::Bool));
        assert_eq!(by_key("n").kind, NodeKind::Scalar(ScalarType::Null));
        // the object itself is the multiline root container's children; paths:
        assert_eq!(by_key("i").path, vec![Seg::Key("i".into())]);
    }

    #[test]
    fn nested_object_and_array() {
        let t = tree("{\n  \"o\": { \"a\": 1 },\n  \"arr\": [1, 2]\n}\n");
        let o = t.root.children.iter().find(|c| c.key == "o").unwrap();
        assert_eq!(o.kind, NodeKind::Table);
        assert_eq!(o.format, Format::Inline);
        assert_eq!(o.children[0].key, "a");
        assert_eq!(o.children[0].path, vec![Seg::Key("o".into()), Seg::Key("a".into())]);
        let arr = t.root.children.iter().find(|c| c.key == "arr").unwrap();
        assert_eq!(arr.kind, NodeKind::Array);
        assert_eq!(arr.format, Format::Inline);
        assert_eq!(arr.children.len(), 2);
        assert_eq!(arr.children[0].path, vec![Seg::Key("arr".into()), Seg::Index(0)]);
    }

    #[test]
    fn root_array_document() {
        let t = tree("[1, 2, 3]\n");
        assert_eq!(t.root.kind, NodeKind::Root);
        assert_eq!(t.root.children.len(), 3);
        assert_eq!(t.root.children[1].path, vec![Seg::Index(1)]);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p confy json::project 2>&1 | tail -20`
Expected: FAIL (stub returns an empty root).

- [ ] **Step 3: Implement the projection walk**

Replace the stub `project` with a recursive walk. Build a resolver index in parallel (used by `edit.rs`):

```rust
use crate::model::json::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use crate::model::node::{Format, KeySign, Node, NodeKind, NodeTree, ScalarType, Seg};
use rowan::NodeOrToken;

/// The source element a projected node maps to (mutation resolution).
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) enum Target {
    /// A `MEMBER` node (`"k": v`).
    Member(SyntaxNode),
    /// A `VALUE` node that is an array element (no key).
    Element(SyntaxNode),
    /// The first comment token of a standalone (possibly multi-line) `//` block.
    Comment(SyntaxToken),
    /// A `BLOCK_COMMENT` token (read-only).
    Block(SyntaxToken),
}

pub(crate) type JsonIndex = Vec<(Vec<Seg>, Target)>;

pub fn project(syntax: &SyntaxNode, filename: &str) -> NodeTree {
    walk(syntax, filename).0
}

pub(crate) fn walk(syntax: &SyntaxNode, filename: &str) -> (NodeTree, JsonIndex) {
    let mut root = Node::branch(filename, NodeKind::Root);
    let mut idx: JsonIndex = Vec::new();
    // The ROOT wraps trivia + one VALUE. Project that VALUE's *contents* directly
    // as the root's children (an object's members / an array's elements / a lone
    // scalar), plus any standalone comments around it.
    if let Some(value) = syntax
        .children()
        .find(|n| n.kind() == SyntaxKind::VALUE)
    {
        project_container_body(&value, &mut root, &[], &mut idx);
    }
    // Top-level standalone comments (before/after the value, inside ROOT) attach
    // to the root scope.
    project_scope_comments(syntax, &mut root, &[], &mut idx);
    (NodeTree { root }, idx)
}
```

Then the helpers. `project_container_body(value_node, parent, parent_path, idx)`:
- inspect the inner node of `value_node` (its single non-trivia child):
  - `OBJECT` → set `parent.kind`/`format` if `parent` is itself a value (handled by caller for nested), iterate `MEMBER` children → `project_member`; iterate standalone comments → Comment nodes; assign trailing comments.
  - `ARRAY` → iterate element `VALUE` children → `project_element`; comments likewise.
  - scalar token → push one keyless leaf child (root-scalar / element case).

Provide the full helper code:

```rust
/// Classify a scalar token into (kind, format, value-repr).
fn classify_scalar(tok: &SyntaxToken) -> (NodeKind, Format, String) {
    let text = tok.text().to_string();
    match tok.kind() {
        SyntaxKind::STRING => (NodeKind::Scalar(ScalarType::String), Format::Plain, text),
        SyntaxKind::TRUE | SyntaxKind::FALSE => {
            (NodeKind::Scalar(ScalarType::Bool), Format::Plain, text)
        }
        SyntaxKind::NULL => (NodeKind::Scalar(ScalarType::Null), Format::Plain, text),
        SyntaxKind::NUMBER => {
            let is_float = text.contains('.') || text.contains(['e', 'E']);
            if is_float {
                let fmt = if text.contains(['e', 'E']) {
                    Format::Exponent
                } else {
                    Format::Plain
                };
                (NodeKind::Scalar(ScalarType::Float), fmt, text)
            } else {
                (NodeKind::Scalar(ScalarType::Integer), Format::Decimal, text)
            }
        }
        _ => (NodeKind::Scalar(ScalarType::String), Format::Plain, text),
    }
}

/// The inner structural node/token of a VALUE wrapper (skipping trivia).
fn value_inner(value: &SyntaxNode) -> Option<rowan::NodeOrToken<SyntaxNode, SyntaxToken>> {
    value.children_with_tokens().find(|e| {
        !matches!(
            e.kind(),
            SyntaxKind::WHITESPACE
                | SyntaxKind::NEWLINE
                | SyntaxKind::LINE_COMMENT
                | SyntaxKind::BLOCK_COMMENT
        )
    })
}

/// Strip the surrounding quotes of a STRING key token for the `Seg::Key`/`key`.
fn key_name(key_node: &SyntaxNode) -> String {
    let raw = key_node.text().to_string();
    raw.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .map(|s| s.to_string())
        .unwrap_or(raw)
}
```

The container body, member, element and comment routines (the heart of the projection). Implement `project_container_body`, `project_member`, `project_element`, `project_scope_comments` and a `build_value_node` that, given a `VALUE` node, a key (or None) and a path, returns a fully-built `Node` (recursing into nested objects/arrays). Key behaviors:

- **Inline vs Multiline**: a container is `Multiline` if its source text contains a `\n` between its braces/brackets, else `Inline`. For an `Inline` array/object, set `value` to its one-line source repr (the node's `to_string()` trimmed); `Multiline` leaves `value = None` — mirrors TOML.
- **Standalone comment**: a `LINE_COMMENT` token that is the first non-whitespace token on its line (i.e. preceded only by `WHITESPACE`/`NEWLINE`/start) becomes a `Comment` node at the current `Seg::Index` slot; consecutive `//` lines merge into one Comment node (a blank line or non-comment splits) — copy the `blocks`/`lines`/`first_tok` accumulator pattern from `cst_project.rs:63-93`.
- **Trailing comment**: a `LINE_COMMENT` after a value on the same line (no `NEWLINE` between the value and the comment) becomes the owning node's `trailing_comment` (text incl. `//`).
- **Block comment**: a `BLOCK_COMMENT` token always projects as a `Comment` node with `read_only: true`, value = raw block text. (Even an end-of-line `/* */` is a standalone read-only node in v1 — simplest correct fence.)

Because this is the largest helper, here is the concrete `build_value_node`:

```rust
/// Build the Node for a VALUE wrapper. `key`/`key_sign` describe how it was
/// addressed (a member key, or None for an element/root scalar). `path` is the
/// node's own path. Recurses into nested objects/arrays, threading `idx`.
fn build_value_node(
    value: &SyntaxNode,
    key: &str,
    key_sign: KeySign,
    path: Vec<Seg>,
    src_element: Target,
    idx: &mut JsonIndex,
) -> Node {
    idx.push((path.clone(), src_element));
    let inner = value_inner(value);
    match inner {
        Some(NodeOrToken::Node(obj)) if obj.kind() == SyntaxKind::OBJECT => {
            let mut n = Node::branch(key, NodeKind::Table);
            n.path = path.clone();
            n.key_sign = key_sign;
            n.format = container_format(&obj);
            if n.format == Format::Inline {
                n.value = Some(obj.text().to_string());
            }
            project_object_members(&obj, &mut n, &path, idx);
            n
        }
        Some(NodeOrToken::Node(arr)) if arr.kind() == SyntaxKind::ARRAY => {
            let mut n = Node::branch(key, NodeKind::Array);
            n.path = path.clone();
            n.key_sign = key_sign;
            n.format = container_format(&arr);
            if n.format == Format::Inline {
                n.value = Some(arr.text().to_string());
            }
            project_array_elements(&arr, &mut n, &path, idx);
            n
        }
        Some(NodeOrToken::Token(tok)) => {
            let (kind, format, repr) = classify_scalar(&tok);
            let mut n = Node::leaf(key, kind);
            n.path = path;
            n.key_sign = key_sign;
            n.format = format;
            n.value = Some(repr);
            n.trailing_comment = trailing_comment_of(value);
            n
        }
        _ => {
            // empty/garbage VALUE — render as an empty string leaf (defensive).
            let mut n = Node::leaf(key, NodeKind::Scalar(ScalarType::String));
            n.path = path;
            n.key_sign = key_sign;
            n
        }
    }
}

fn container_format(container: &SyntaxNode) -> Format {
    if container.text().to_string().contains('\n') {
        Format::Multiline
    } else {
        Format::Inline
    }
}
```

Implement `project_object_members`, `project_array_elements`, `project_scope_comments`, `trailing_comment_of` following the `cst_project.rs` comment-accumulator idiom. `project_object_members` walks the OBJECT's `children_with_tokens`, tracks the running `Seg::Index` for comments + the member ordinal for keyed members. **Important addressing note:** like TOML, comments and elements share the full-child-sequence index, but object *members* are addressed by `Seg::Key`. Follow the TOML rule precisely: a member's path is `parent_path + Seg::Key(name)`; a standalone comment inside the object gets `parent_path + Seg::Index(full_seq_pos)`.

`build_value_node` for the **root** case: in `walk`, instead of calling `build_value_node` (which would create a single child), the root *adopts the container's children as its own*. So `project_container_body` (called from `walk`) special-cases: if the root value is an object, iterate its members into `root.children`; if an array, into `root.children`; if a scalar, push one keyless leaf. Use the same member/element helpers with `parent = &mut root` and `parent_path = []`.

- [ ] **Step 4: Run the golden tests**

Run: `cargo test -p confy json::project 2>&1 | tail -30`
Expected: PASS (scalars_and_object, nested_object_and_array, root_array_document).

- [ ] **Step 5: Commit**

```bash
git add src/model/json/project.rs
git commit -m "feat(json): CST→NodeTree projection (scalars/objects/arrays, golden tests)"
```

---

## Task 8: Projection — comments (standalone `//`, trailing `//`, read-only `/* */`)

**Files:**
- Modify: `src/model/json/project.rs`

- [ ] **Step 1: Write the failing comment tests**

```rust
    #[test]
    fn standalone_and_trailing_comments() {
        let t = tree("{\n  // above\n  \"a\": 1, // trailing\n  \"b\": 2\n}\n");
        // standalone comment is a child node before "a"
        let kinds: Vec<_> = t.root.children.iter().map(|c| (&c.kind, c.key.clone())).collect();
        assert!(matches!(kinds[0].0, NodeKind::Comment(_)));
        let a = t.root.children.iter().find(|c| c.key == "a").unwrap();
        assert_eq!(a.trailing_comment.as_deref(), Some("// trailing"));
    }

    #[test]
    fn merged_multiline_comment() {
        let t = tree("{\n  // l1\n  // l2\n  \"a\": 1\n}\n");
        let c = &t.root.children[0];
        assert!(matches!(c.kind, NodeKind::Comment(_)));
        assert_eq!(c.value.as_deref(), Some("// l1\n// l2"));
    }

    #[test]
    fn block_comment_is_read_only() {
        let t = tree("{\n  /* note */\n  \"a\": 1\n}\n");
        let c = &t.root.children[0];
        assert!(matches!(c.kind, NodeKind::Comment(_)));
        assert!(c.read_only);
        assert_eq!(c.value.as_deref(), Some("/* note */"));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p confy json::project 2>&1 | tail -20`
Expected: FAIL.

- [ ] **Step 3: Implement comment projection**

Fill the comment-accumulator in `project_object_members`/`project_array_elements`/`project_scope_comments`:

- A `LINE_COMMENT` token preceded (on its line) only by whitespace/newline → standalone; accumulate consecutive lines into one `Comment` node (`value = lines.join("\n")`, `NodeKind::Comment(text)`, `key = text`, `key_sign = None`, `read_only = false`). A `NEWLINE` containing ≥2 `\n` (blank line) flushes the block.
- A `LINE_COMMENT` token *after* a value on the same line → handled by `trailing_comment_of(value)`: walk the VALUE's following siblings inside its MEMBER/array slot until the next `NEWLINE`; if a `LINE_COMMENT` appears first, that is the trailing comment. (Mirror how TOML reads the trailing comment from inside the entry's VALUE.)
- A `BLOCK_COMMENT` token → always a standalone `Comment` node with `read_only = true`, `value = raw text`. It flushes any pending `//` block first (a `/* */` never merges with `//`).
- Index each standalone comment with `Target::Comment(first_tok)` (or `Target::Block(tok)` for blocks).

`trailing_comment_of(value: &SyntaxNode) -> Option<String>`:

```rust
fn trailing_comment_of(value: &SyntaxNode) -> Option<String> {
    let mut sib = value.next_sibling_or_token();
    while let Some(el) = sib {
        match el.kind() {
            SyntaxKind::WHITESPACE => {}
            SyntaxKind::LINE_COMMENT => {
                return el.as_token().map(|t| t.text().trim_end().to_string());
            }
            _ => break, // a COMMA, NEWLINE, or another node ends the line
        }
        sib = el.next_sibling_or_token();
    }
    None
}
```

> Note the COMMA case: in `"a": 1, // trailing`, the comma sits between the VALUE and the comment. Handle by also skipping a single `COMMA` (and trailing `WHITESPACE`) before the `LINE_COMMENT` — extend the `match` to `SyntaxKind::COMMA => {}`. The trailing comment is conceptually the member's; attach it to the value node either way.

- [ ] **Step 4: Run the comment tests**

Run: `cargo test -p confy json::project 2>&1 | tail -30`
Expected: PASS.

- [ ] **Step 5: Golden snapshot test for the fixtures**

Add a golden test that projects `sample.jsonc` and asserts the overall shape (children count, the block comment's `read_only`, the trailing comment on `"name"`). Keep it concrete:

```rust
    #[test]
    fn jsonc_fixture_shape() {
        let src = include_str!("../../../tests/fixtures/sample.jsonc");
        let t = project(&SyntaxNode::new_root(crate::model::json::parse::parse(src).unwrap()), "sample.jsonc");
        // top-of-file comment is the first child
        assert!(matches!(t.root.children[0].kind, NodeKind::Comment(_)));
        let name = t.root.children.iter().find(|c| c.key == "name").unwrap();
        assert_eq!(name.trailing_comment.as_deref(), Some("// trailing comment"));
        // the /* */ block projects read-only
        assert!(t.root.children.iter().any(|c| matches!(c.kind, NodeKind::Comment(_)) && c.read_only));
    }
```

Run: `cargo test -p confy json::project 2>&1 | tail -20` → PASS.

- [ ] **Step 6: Commit**

```bash
git add src/model/json/project.rs
git commit -m "feat(json): comment projection (standalone // merge, trailing //, read-only /* */)"
```

---

## Task 9: Edit infrastructure — `walk`-based resolver + `serialize_fragment` + atomic `apply`

**Files:**
- Modify: `src/model/json/edit.rs`

The edit layer reuses the projection's `walk` to get the `path → Target` index, resolves the syntax element, and splices a `clone_for_update` copy of the tree, committing on success. This mirrors `cst_edit.rs`.

- [ ] **Step 1: Write the failing `serialize_fragment` test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::json::syntax::SyntaxNode;
    use crate::model::node::Seg;

    fn parse(src: &str) -> SyntaxNode {
        SyntaxNode::new_root(crate::model::json::parse::parse(src).unwrap())
    }

    #[test]
    fn fragment_of_member() {
        let t = parse("{\n  \"a\": 1,\n  \"b\": { \"x\": 2 }\n}\n");
        assert_eq!(serialize_fragment(&t, &[Seg::Key("a".into())]), "\"a\": 1");
        assert_eq!(
            serialize_fragment(&t, &[Seg::Key("b".into())]),
            "\"b\": { \"x\": 2 }"
        );
    }

    #[test]
    fn fragment_of_element() {
        let t = parse("[10, 20, 30]\n");
        assert_eq!(serialize_fragment(&t, &[Seg::Index(1)]), "20");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p confy json::edit 2>&1 | tail -20`
Expected: FAIL (stub returns empty string).

- [ ] **Step 3: Implement the resolver + `serialize_fragment`**

```rust
use crate::model::document::{MutateError, Mutation};
use crate::model::json::project::{walk, Target};
use crate::model::json::syntax::{SyntaxKind, SyntaxNode};
use crate::model::node::Seg;

/// Resolve `path` to its source element using the projection's index.
pub(crate) fn resolve(syntax: &SyntaxNode, path: &[Seg]) -> Option<Target> {
    let (_, idx) = walk(syntax, "");
    idx.into_iter()
        .find(|(p, _)| p == path)
        .map(|(_, t)| t)
}

/// Serialize the node at `path` as a standalone fragment (member `"k": v` /
/// element value / comment text), trimming surrounding trivia.
pub fn serialize_fragment(syntax: &SyntaxNode, path: &[Seg]) -> String {
    match resolve(syntax, path) {
        Some(Target::Member(m)) => m.text().to_string().trim().to_string(),
        Some(Target::Element(v)) => v.text().to_string().trim().to_string(),
        Some(Target::Comment(tok)) => {
            // re-collect the merged // block from the token's standalone run
            comment_block_text(&tok)
        }
        Some(Target::Block(tok)) => tok.text().to_string(),
        None => String::new(),
    }
}
```

Add `comment_block_text(first: &SyntaxToken) -> String` that walks forward over consecutive `LINE_COMMENT` tokens (separated only by single-`\n` `NEWLINE`s and `WHITESPACE`) joining their texts with `\n` — the inverse of the projection's merge.

- [ ] **Step 4: Run the fragment tests**

Run: `cargo test -p confy json::edit 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Add the `apply` dispatcher (variants stubbed)**

```rust
pub fn apply(syntax: &SyntaxNode, m: Mutation) -> Result<SyntaxNode, MutateError> {
    let tree = syntax.clone_for_update();
    match m {
        Mutation::Replace { path, fragment } => replace(&tree, &path, &fragment)?,
        Mutation::Delete { path } => delete(&tree, &path)?,
        Mutation::Insert { target, fragment, on_collision } => {
            insert(&tree, &target, &fragment, on_collision)?
        }
        Mutation::Rename { path, new_key } => rename(&tree, &path, &new_key)?,
        Mutation::Remark { path } => remark(&tree, &path)?,
        Mutation::EditComment { path, text } => edit_comment(&tree, &path, &text)?,
        Mutation::InsertComment { target, text } => insert_comment(&tree, &target, &text)?,
        Mutation::Move { sources, target, on_collision } => {
            move_nodes(&tree, &sources, &target, on_collision)?
        }
        Mutation::ConvertKind { path, target } => convert_kind(&tree, &path, target)?,
    }
    validate_semantics(&tree)?;
    Ok(tree)
}
```

Add stub fns for each (returning `Err(MutateError::Unsupported)` except where implemented in later tasks) and a `validate_semantics`:

```rust
/// Backstop: re-parse the spliced text and reject duplicate object keys
/// (`Collision`) / structural breakage (`Illegal`). Mirrors `cst_edit`'s DOM check.
fn validate_semantics(tree: &SyntaxNode) -> Result<(), MutateError> {
    let text = tree.to_string();
    let green = crate::model::json::parse::parse(&text)
        .map_err(|e| MutateError::Illegal(e))?;
    let reparsed = SyntaxNode::new_root(green);
    // duplicate-key check over every OBJECT
    for obj in reparsed.descendants().filter(|n| n.kind() == SyntaxKind::OBJECT) {
        let mut seen = std::collections::HashSet::new();
        for member in obj.children().filter(|n| n.kind() == SyntaxKind::MEMBER) {
            if let Some(key) = member.children().find(|n| n.kind() == SyntaxKind::KEY) {
                let name = key.text().to_string();
                if !seen.insert(name.clone()) {
                    return Err(MutateError::Collision(
                        name.trim_matches('"').to_string(),
                    ));
                }
            }
        }
    }
    Ok(())
}
```

Run: `cargo build 2>&1 | tail` → clean.

- [ ] **Step 6: Commit**

```bash
git add src/model/json/edit.rs
git commit -m "feat(json): edit resolver + serialize_fragment + atomic apply dispatcher (variants stubbed)"
```

---

## Task 10: `Replace` (inline value edit + whole-document + container)

**Files:**
- Modify: `src/model/json/edit.rs`

`Replace` semantics (mirror TOML):
- empty path → whole-document replace: re-parse `fragment` as a full JSON doc; reject invalid as `Fragment`.
- member path → replace the member's VALUE with the parsed value of `fragment`. `fragment` may be a bare value (`42`, `"x"`) **or** a `"key": value` pair (inline editor sends the latter when the name is unchanged? No — Replace fragment for a leaf is `"key": value`; see app's edit_commit). Support both: if `fragment` parses as a member, replace the whole MEMBER; if it parses as a bare value, replace only the VALUE.
- element path (`Seg::Index` under an array) → replace that element VALUE with the parsed bare value.

- [ ] **Step 1: Write the failing tests**

```rust
    fn apply_str(src: &str, m: Mutation) -> String {
        let t = parse(src);
        super::apply(&t, m).unwrap().to_string()
    }

    #[test]
    fn replace_member_value() {
        let out = apply_str(
            "{\n  \"a\": 1\n}\n",
            Mutation::Replace { path: vec![Seg::Key("a".into())], fragment: "\"a\": 2".into() },
        );
        assert_eq!(out, "{\n  \"a\": 2\n}\n");
    }

    #[test]
    fn replace_element() {
        let out = apply_str(
            "[1, 2, 3]\n",
            Mutation::Replace { path: vec![Seg::Index(1)], fragment: "20".into() },
        );
        assert_eq!(out, "[1, 20, 3]\n");
    }

    #[test]
    fn replace_whole_document() {
        let out = apply_str(
            "{ \"a\": 1 }\n",
            Mutation::Replace { path: vec![], fragment: "{ \"b\": 2 }\n".into() },
        );
        assert_eq!(out, "{ \"b\": 2 }\n");
    }
```

- [ ] **Step 2: Run → FAIL** (`Unsupported`).

Run: `cargo test -p confy json::edit replace 2>&1 | tail -20`

- [ ] **Step 3: Implement `replace`**

```rust
fn replace(tree: &SyntaxNode, path: &[Seg], fragment: &str) -> Result<(), MutateError> {
    if path.is_empty() {
        let green = crate::model::json::parse::parse(fragment)
            .map_err(MutateError::Fragment)?;
        let new_root = SyntaxNode::new_root(green).clone_for_update();
        // Splice the new ROOT's children in place of the old ROOT's children.
        let n = tree.children_with_tokens().count();
        let new_children: Vec<_> = new_root.children_with_tokens().collect();
        tree.splice_children(0..n, new_children);
        return Ok(());
    }
    match resolve(tree, path).ok_or(MutateError::NotFound)? {
        Target::Member(member) => {
            // fragment may be `"k": v` (replace member) or bare value (replace VALUE)
            if let Some(new_member) = parse_member_fragment(fragment) {
                replace_node(&member, new_member);
            } else {
                let value = member
                    .children()
                    .find(|n| n.kind() == SyntaxKind::VALUE)
                    .ok_or(MutateError::NotFound)?;
                let new_value = parse_value_fragment(fragment)?;
                replace_node(&value, new_value);
            }
            Ok(())
        }
        Target::Element(value) => {
            let new_value = parse_value_fragment(fragment)?;
            replace_node(&value, new_value);
            Ok(())
        }
        Target::Comment(_) | Target::Block(_) => Err(MutateError::Illegal(
            "use EditComment to edit a comment".into(),
        )),
    }
}
```

Helpers:
- `parse_value_fragment(frag) -> Result<SyntaxNode, MutateError>`: parse `frag` as a document, extract the single `VALUE` node (`clone_for_update`), error `Fragment` if it isn't exactly one value.
- `parse_member_fragment(frag) -> Option<SyntaxNode>`: wrap `frag` as `{ <frag> }`, parse, extract the single `MEMBER`. Returns `None` if `frag` is not a `key: value` pair.
- `replace_node(old: &SyntaxNode, new: SyntaxNode)`: `old.replace_with(new.green().into())` — but on a mutable tree use `splice_children` on the parent over the old node's index. Concretely:

```rust
fn replace_node(old: &SyntaxNode, new: SyntaxNode) {
    let parent = old.parent().expect("node has parent");
    let idx = old.index();
    parent.splice_children(idx..idx + 1, vec![new.into()]);
}
```

- [ ] **Step 4: Run → PASS.** Then run the full suite: `cargo test -p confy json:: 2>&1 | tail`.

- [ ] **Step 5: Commit**

```bash
git add src/model/json/edit.rs
git commit -m "feat(json): Replace mutation (inline value, member, whole-document)"
```

---

## Task 11: `Delete`

**Files:** Modify `src/model/json/edit.rs`

Delete removes the member/element/comment at `path` plus its structural punctuation: a member takes its trailing (or leading) `COMMA` and the surrounding indent/newline so the remaining text stays well-formed and re-indented. Mirror `cst_edit`'s deletion normalization.

- [ ] **Step 1: Failing tests**

```rust
    #[test]
    fn delete_middle_member() {
        let out = apply_str(
            "{\n  \"a\": 1,\n  \"b\": 2,\n  \"c\": 3\n}\n",
            Mutation::Delete { path: vec![Seg::Key("b".into())] },
        );
        assert_eq!(out, "{\n  \"a\": 1,\n  \"c\": 3\n}\n");
    }

    #[test]
    fn delete_last_member_fixes_comma() {
        let out = apply_str(
            "{\n  \"a\": 1,\n  \"b\": 2\n}\n",
            Mutation::Delete { path: vec![Seg::Key("b".into())] },
        );
        assert_eq!(out, "{\n  \"a\": 1\n}\n");
    }

    #[test]
    fn delete_element() {
        let out = apply_str(
            "[1, 2, 3]\n",
            Mutation::Delete { path: vec![Seg::Index(1)] },
        );
        assert_eq!(out, "[1, 3]\n");
    }

    #[test]
    fn delete_comment() {
        let out = apply_str(
            "{\n  // gone\n  \"a\": 1\n}\n",
            Mutation::Delete { path: vec![Seg::Index(0)] },
        );
        assert_eq!(out, "{\n  \"a\": 1\n}\n");
    }
```

- [ ] **Step 2: Run → FAIL.**

- [ ] **Step 3: Implement `delete`**

The algorithm: resolve to the source element; compute the **deletion span** (the element + adjacent comma + the run of whitespace/newline tokens that would otherwise orphan). Concretely, find the element's index range in its parent, extend left to swallow a leading `NEWLINE`+indent `WHITESPACE`, and remove a `COMMA` (prefer the following comma; if none, the preceding comma — the last-item case). Splice that range out:

```rust
fn delete(tree: &SyntaxNode, path: &[Seg]) -> Result<(), MutateError> {
    let elem: SyntaxNode = match resolve(tree, path).ok_or(MutateError::NotFound)? {
        Target::Member(m) => m,
        Target::Element(v) => v,
        Target::Comment(tok) | Target::Block(tok) => {
            // a comment lives as bare tokens; delete the block's token span
            return delete_comment_tokens(tree, &tok);
        }
    };
    let parent = elem.parent().expect("parent");
    let mut start = elem.index();
    let mut end = start + 1;
    // swallow a following comma (+ inter-token whitespace before it)
    extend_over_following_comma(&parent, &mut end);
    // if no following comma was found and a preceding one exists (last item), drop it
    if !range_contains_comma(&parent, start..end) {
        extend_over_preceding_comma(&parent, &mut start);
    }
    // swallow one leading newline+indent so the line vanishes cleanly
    swallow_leading_line(&parent, &mut start);
    parent.splice_children(start..end, vec![]);
    Ok(())
}
```

Implement the four helpers as small index walks over `parent.children_with_tokens()` checking `SyntaxKind`. Keep them local and concrete (no placeholder). `delete_comment_tokens` removes the contiguous `LINE_COMMENT`(+`NEWLINE`/`WHITESPACE`) run, or the single `BLOCK_COMMENT` token + its trailing newline.

> Spacing edge: the spec accepts that "multiline-array element insert/delete spacing is not yet byte-perfect" for TOML; hold JSON to the inline + simple-multiline cases the tests cover, and document any residual edge in CONTEXT.md (Task 19).

- [ ] **Step 4: Run → PASS.**

- [ ] **Step 5: Commit**

```bash
git add src/model/json/edit.rs
git commit -m "feat(json): Delete mutation (member/element/comment with comma+indent normalization)"
```

---

## Task 12: `Insert` (member into object, element into array, keyed↔element pack/unpack)

**Files:** Modify `src/model/json/edit.rs`

Insert adapts the fragment to the destination (mirror `parse_fragment_adapted`):
- keyed fragment (`"k": v`) into an **object** → inserted as a member at `target.index`; collision per `on_collision`.
- bare value into an **array** → inserted as an element.
- keyed fragment into an **array** → wrapped as a single-member object element `{ "k": v }` (`wrap_keyed_as_inline_element`).
- bare value into an **object** → synthesized `"placeholder"` key (auto-renamed on collision).
- object element pasted into an object → **unpacks** its members (`unpack_inline_table` inverse).

- [ ] **Step 1: Failing tests**

```rust
    #[test]
    fn insert_member_into_object() {
        let out = apply_str(
            "{\n  \"a\": 1\n}\n",
            Mutation::Insert {
                target: Target { parent: vec![], index: 1 },
                fragment: "\"b\": 2".into(),
                on_collision: OnCollision::Cancel,
            },
        );
        assert_eq!(out, "{\n  \"a\": 1,\n  \"b\": 2\n}\n");
    }

    #[test]
    fn insert_element_into_array() {
        let out = apply_str(
            "[1, 2]\n",
            Mutation::Insert {
                target: Target { parent: vec![], index: 2 },
                fragment: "3".into(),
                on_collision: OnCollision::Cancel,
            },
        );
        assert_eq!(out, "[1, 2, 3]\n");
    }

    #[test]
    fn insert_keyed_into_array_wraps() {
        let out = apply_str(
            "[1]\n",
            Mutation::Insert {
                target: Target { parent: vec![], index: 1 },
                fragment: "\"k\": 2".into(),
                on_collision: OnCollision::Cancel,
            },
        );
        assert_eq!(out, "[1, { \"k\": 2 }]\n");
    }

    #[test]
    fn insert_collision_cancels() {
        let t = parse("{ \"a\": 1 }\n");
        let r = super::apply(&t, Mutation::Insert {
            target: Target { parent: vec![], index: 1 },
            fragment: "\"a\": 2".into(),
            on_collision: OnCollision::Cancel,
        });
        assert!(matches!(r, Err(MutateError::Collision(_))));
    }
```

(Import `OnCollision`, `Target` in the test module.)

- [ ] **Step 2: Run → FAIL.**

- [ ] **Step 3: Implement `insert`**

Resolve `target.parent` to the destination container (object or array; empty path → the root value's container). Determine container kind. Build the new child node(s) from the fragment via an `adapt_fragment(container_kind, fragment)` that returns either a `MEMBER` node or a `VALUE` (element) node, applying the wrap/unpack/placeholder rules above. Then splice at the projected `target.index`, translating the projected index (which counts comments + elements) to the raw child position, and inserting the correct surrounding `COMMA`/`NEWLINE`/indent so the result re-indents to the container's style:
- Inline container: separator `, `.
- Multiline container: a `NEWLINE` + the container's per-member indent (detected from an existing member's leading whitespace, default 2 spaces deeper than the container's own indent).

Collision: for an object member insert, scan existing keys; `Cancel`→`Err(Collision)`, `Rename`→append `_2`/`_3`, `Overwrite`→replace the colliding member.

Provide `wrap_keyed_as_inline_element(member_text) -> "{ <member_text> }"` and `unpack_object_element(value_node) -> Vec<member_node>` concretely.

- [ ] **Step 4: Run → PASS** (all four insert tests).

- [ ] **Step 5: Commit**

```bash
git add src/model/json/edit.rs
git commit -m "feat(json): Insert mutation (member/element, keyed↔element pack/unpack, collision)"
```

---

## Task 13: `Rename`, `Remark`, `EditComment`, `InsertComment`

**Files:** Modify `src/model/json/edit.rs`

- **Rename**: swap the KEY token's STRING in place (`"old"` → `"new"`, re-quoted), position-preserving, collision-checked against sibling members. No dotted semantics — a `.` in the key is literal.
- **Remark**: toggle a live member ↔ comment. Live→comment writes `// "key": value,` (one `//` per line of the member's extent), removing the member node and inserting a `LINE_COMMENT` token run in its slot. Comment→live: strip the `//` prefix from each line, parse the remainder as a member, splice it back. A pure-`.json` first remark requires comments enabled — but that gate is enforced in the **TUI** (Task 18); the model splice itself just writes `//`.
- **EditComment**: replace the standalone comment block's tokens with the new `//`-prefixed text (validate every line starts with `//`).
- **InsertComment**: splice a `//`-prefixed block into `target.parent` at the projected child index (validate prefixes; never collides). A comment into a **single-line array** upgrades it to multiline first (mirror TOML's `ArrayUpgrade`) — but per spec this is gated by the TUI prompt; the model performs the upgrade when asked.

- [ ] **Step 1: Failing tests**

```rust
    #[test]
    fn rename_member_key() {
        let out = apply_str(
            "{ \"a\": 1 }\n",
            Mutation::Rename { path: vec![Seg::Key("a".into())], new_key: "b".into() },
        );
        assert_eq!(out, "{ \"b\": 1 }\n");
    }

    #[test]
    fn rename_collision() {
        let t = parse("{ \"a\": 1, \"b\": 2 }\n");
        let r = super::apply(&t, Mutation::Rename {
            path: vec![Seg::Key("a".into())], new_key: "b".into(),
        });
        assert!(matches!(r, Err(MutateError::Collision(_))));
    }

    #[test]
    fn remark_member_to_comment() {
        let out = apply_str(
            "{\n  \"a\": 1\n}\n",
            Mutation::Remark { path: vec![Seg::Key("a".into())] },
        );
        assert_eq!(out, "{\n  // \"a\": 1\n}\n");
    }

    #[test]
    fn remark_comment_to_member() {
        let out = apply_str(
            "{\n  // \"a\": 1\n}\n",
            Mutation::Remark { path: vec![Seg::Index(0)] },
        );
        assert_eq!(out, "{\n  \"a\": 1\n}\n");
    }

    #[test]
    fn edit_comment_text() {
        let out = apply_str(
            "{\n  // old\n  \"a\": 1\n}\n",
            Mutation::EditComment { path: vec![Seg::Index(0)], text: "// new".into() },
        );
        assert_eq!(out, "{\n  // new\n  \"a\": 1\n}\n");
    }

    #[test]
    fn insert_comment_block() {
        let out = apply_str(
            "{\n  \"a\": 1\n}\n",
            Mutation::InsertComment {
                target: Target { parent: vec![], index: 0 },
                text: "// note".into(),
            },
        );
        assert_eq!(out, "{\n  // note\n  \"a\": 1\n}\n");
    }
```

- [ ] **Step 2: Run → FAIL.**

- [ ] **Step 3: Implement the four splices.** Each is a localized token surgery mirroring `cst_edit`'s same-named function. Key details:
  - Remark live→comment: capture the member's full text (incl. nested newlines), prefix each line with `// `, append a trailing `,` if the member had one, replace the MEMBER node with the `LINE_COMMENT` token run.
  - Remark comment→live: strip `// ?` from each line, join, parse as `{ <text> }` member, splice in; the trailing `,` is normalized by the surrounding object.
  - EditComment / InsertComment: validate `text.lines().all(|l| l.trim_start().starts_with("//"))`, else `Fragment`.

- [ ] **Step 4: Run → PASS** (all six).

- [ ] **Step 5: Commit**

```bash
git add src/model/json/edit.rs
git commit -m "feat(json): Rename/Remark/EditComment/InsertComment mutations"
```

---

## Task 14: `Move` (atomic cut, keyed↔element pack/unpack across containers)

**Files:** Modify `src/model/json/edit.rs`

Move deletes the sources and reinserts at the target on a scratch tree, committed only on success (mirror TOML `move_nodes`). Reuses `insert`'s adaptation: a member moved into an array wraps; an object element moved into an object unpacks; multiple keyed nodes into one array/element join into one `{ ... }`.

- [ ] **Step 1: Failing tests**

```rust
    #[test]
    fn move_member_within_object() {
        // move "a" to the end
        let out = apply_str(
            "{\n  \"a\": 1,\n  \"b\": 2\n}\n",
            Mutation::Move {
                sources: vec![vec![Seg::Key("a".into())]],
                target: Target { parent: vec![], index: 2 },
                on_collision: OnCollision::Cancel,
            },
        );
        assert_eq!(out, "{\n  \"b\": 2,\n  \"a\": 1\n}\n");
    }

    #[test]
    fn move_member_into_nested_array_wraps() {
        let out = apply_str(
            "{\n  \"a\": 1,\n  \"arr\": []\n}\n",
            Mutation::Move {
                sources: vec![vec![Seg::Key("a".into())]],
                target: Target { parent: vec![Seg::Key("arr".into())], index: 0 },
                on_collision: OnCollision::Cancel,
            },
        );
        assert_eq!(out, "{\n  \"arr\": [{ \"a\": 1 }]\n}\n");
    }
```

- [ ] **Step 2: Run → FAIL.**

- [ ] **Step 3: Implement `move_nodes`**: on a `clone_for_update` scratch, capture each source's fragment text first (before deletion shifts indices), delete sources back-to-front, then run the insert-adaptation at the resolved target. Restore-on-failure is provided by `apply`'s atomic commit (the whole `apply` returns `Err`, leaving `self.syntax` untouched in `doc.rs`).

- [ ] **Step 4: Run → PASS.**

- [ ] **Step 5: Commit**

```bash
git add src/model/json/edit.rs
git commit -m "feat(json): Move mutation (atomic cut, cross-container pack/unpack)"
```

---

## Task 15: `ConvertKind` (object Inline↔Multiline, array Inline↔Multiline, float Plain↔Exponent)

**Files:** Modify `src/model/json/edit.rs`

JSON `K` options (spec §2.3/§2.4):
- object: `TableInline` ↔ `TableMultiline`. Inline→Multiline: spread members one per line with indent. Multiline→Inline: collapse to `{ a, b }`, **rejecting** held comments (`Illegal`). 
- array: `ArrayInline` ↔ `ArrayMultiline`. Same comment/multi-line-element rejection on collapse (mirror TOML).
- float: `FloatPlain` ↔ `FloatExponent`. Re-render the parsed `f64` (plain `1500` ↔ `1.5e3`); `Format::Exponent` drives detection.
- string/int/bool/null: no options (kind_options returns empty; `convert_kind` returns `Unsupported` if somehow called).

- [ ] **Step 1: Failing tests**

```rust
    use crate::model::document::KindTarget;

    #[test]
    fn object_inline_to_multiline() {
        let out = apply_str(
            "{ \"o\": { \"a\": 1, \"b\": 2 } }\n",
            Mutation::ConvertKind {
                path: vec![Seg::Key("o".into())],
                target: KindTarget::TableMultiline,
            },
        );
        assert_eq!(out, "{ \"o\": {\n  \"a\": 1,\n  \"b\": 2\n} }\n");
    }

    #[test]
    fn array_multiline_to_inline() {
        let out = apply_str(
            "{\n  \"a\": [\n    1,\n    2\n  ]\n}\n",
            Mutation::ConvertKind {
                path: vec![Seg::Key("a".into())],
                target: KindTarget::ArrayInline,
            },
        );
        assert_eq!(out, "{\n  \"a\": [1, 2]\n}\n");
    }

    #[test]
    fn float_plain_to_exponent() {
        let out = apply_str(
            "{ \"f\": 1500.0 }\n",
            Mutation::ConvertKind {
                path: vec![Seg::Key("f".into())],
                target: KindTarget::FloatExponent,
            },
        );
        assert_eq!(out, "{ \"f\": 1.5e3 }\n");
    }

    #[test]
    fn inline_collapse_rejects_comment() {
        let t = parse("{\n  \"a\": [\n    1, // c\n    2\n  ]\n}\n");
        let r = super::apply(&t, Mutation::ConvertKind {
            path: vec![Seg::Key("a".into())],
            target: KindTarget::ArrayInline,
        });
        assert!(matches!(r, Err(MutateError::Illegal(_))));
    }
```

> The exact multiline indent in `object_inline_to_multiline` is an implementation choice; adjust the expected string to whatever the indent engine produces, but keep it deterministic and re-collapsible. Prefer 2-space indent at the container's own depth.

- [ ] **Step 2: Run → FAIL.**

- [ ] **Step 3: Implement `convert_kind`** with one arm per `KindTarget` JSON supports; reject the rest as `Unsupported`. The collapse/spread share an indent helper with `insert` (extract `reindent_block`).

- [ ] **Step 4: Run → PASS.**

- [ ] **Step 5: Commit**

```bash
git add src/model/json/edit.rs
git commit -m "feat(json): ConvertKind (object/array inline↔multiline, float plain↔exponent)"
```

---

## Task 16: `kind_options` for JSON

**Files:** Modify `src/model/json/doc.rs`

- [ ] **Step 1: Failing test (in `doc.rs`)**

```rust
    #[test]
    fn kind_options_per_node() {
        use crate::model::document::KindTarget as KT;
        let doc = json_from_str("c.json", "{\n  \"o\": {\n    \"a\": 1\n  },\n  \"arr\": [1],\n  \"f\": 1.5,\n  \"s\": \"x\"\n}\n");
        let opts = |p: &[Seg]| -> Vec<KT> {
            doc.kind_options(p).into_iter().map(|(_, t)| t).collect()
        };
        // multiline object → can go inline
        assert_eq!(opts(&[Seg::Key("o".into())]), vec![KT::TableInline]);
        // inline array → can go multiline
        assert_eq!(opts(&[Seg::Key("arr".into())]), vec![KT::ArrayMultiline]);
        // plain float → exponent
        assert_eq!(opts(&[Seg::Key("f".into())]), vec![KT::FloatExponent]);
        // string → no options
        assert!(opts(&[Seg::Key("s".into())]).is_empty());
    }
```

- [ ] **Step 2: Run → FAIL** (stub returns empty).

- [ ] **Step 3: Implement `kind_options`**

```rust
pub(crate) fn kind_options(tree: &NodeTree, path: &[Seg]) -> Vec<(String, KindTarget)> {
    use crate::model::document::KindTarget as KT;
    use crate::model::node::{Format, NodeKind};
    let Some(node) = tree.node_at(path) else {
        return Vec::new();
    };
    match &node.kind {
        NodeKind::Table => {
            if node.format == Format::Multiline {
                vec![("inline object  [T/I]".into(), KT::TableInline)]
            } else {
                vec![("multiline object  [T/M]".into(), KT::TableMultiline)]
            }
        }
        NodeKind::Array => {
            if node.format == Format::Multiline {
                vec![("inline array  [A/I]".into(), KT::ArrayInline)]
            } else {
                vec![("multiline array  [A/M]".into(), KT::ArrayMultiline)]
            }
        }
        NodeKind::Scalar(crate::model::node::ScalarType::Float) => {
            if node.format == Format::Exponent {
                vec![("plain float  1.5".into(), KT::FloatPlain)]
            } else {
                vec![("exponent float  1e5".into(), KT::FloatExponent)]
            }
        }
        _ => Vec::new(), // string/int/bool/null: no notation switch
    }
}
```

- [ ] **Step 4: Run → PASS.**

- [ ] **Step 5: Commit**

```bash
git add src/model/json/doc.rs
git commit -m "feat(json): kind_options capability query (object/array/float notations)"
```

---

## Task 17: Wire `AnyDocument::Json` + type-filter facet set by format

**Files:**
- Modify: `src/model/any_doc.rs`
- Modify: `src/tui/type_filter.rs` (`layout(format)`, `nav_rows(format)`)
- Modify: `src/tui/ui.rs` (pass format to `layout`)
- Modify: `src/tui/app.rs` (callers of `layout`/`nav_rows`)

- [ ] **Step 1: Add the `Json` variant + delegation**

In `any_doc.rs`:

```rust
use crate::model::json::JsonDocument;

pub enum AnyDocument {
    Toml(CstDocument),
    Json(JsonDocument),
    // Yaml(YamlDocument)  — Phase 3
}
```

Update the `delegate!` macro to include the new arm:

```rust
macro_rules! delegate {
    ($self:ident, $d:ident => $body:expr) => {
        match $self {
            AnyDocument::Toml($d) => $body,
            AnyDocument::Json($d) => $body,
        }
    };
}
```

In `load_as`:

```rust
        DocFormat::Json => Ok(Self::Json(JsonDocument::load(path)?)),
```

Add a passthrough so the TUI can accept the JSONC upgrade:

```rust
    /// Accept the JSONC upgrade (enables authored comments). No-op for TOML.
    pub fn enable_comments(&mut self) {
        if let AnyDocument::Json(d) = self {
            d.enable_comments();
        }
    }
```

Update the existing `any_document_delegates_to_toml`-style tests with a JSON case:

```rust
    #[test]
    fn any_document_loads_json() {
        let f = tempfile::NamedTempFile::with_suffix(".json").unwrap();
        std::fs::write(f.path(), "{ \"a\": 1 }\n").unwrap();
        let doc = AnyDocument::load(f.path()).unwrap();
        assert_eq!(doc.format(), DocFormat::Json);
        assert_eq!(doc.serialize(), "{ \"a\": 1 }\n");
    }
```

- [ ] **Step 2: Format-aware facet layout**

In `type_filter.rs`, change `layout()` → `layout(format: DocFormat)` and `nav_rows()` → `nav_rows(format: DocFormat)`. For `DocFormat::Json`, emit only the JSON-reachable cells: key signs `(Q)`/`(-)` only (no `(B)`/`(D)`); type cells root, comment, array (`[A/I]`/`[A/M]`), table (`[T/I]`/`[T/M]` only — no `[A/T]`/`[T/D]`/`[T/S]`), string (`[S:str ]` only), integer (`[I:dec ]` only), float (`[F:flt ]`/`[F:exp ]`), bool, null. For `DocFormat::Toml`, the existing full set (now including the Yaml-absent atoms is fine — TOML simply never adds Null/FloatExp/TableMultiline rows). Drive the difference with a small `match format` inside `layout`.

Test:

```rust
    #[test]
    fn json_layout_hides_toml_only_facets() {
        let labels: Vec<&str> = layout(DocFormat::Json)
            .iter()
            .flat_map(|r| match r {
                LayoutRow::Cells(cs) => cs.iter().map(|c| c.label()).collect::<Vec<_>>(),
                LayoutRow::Header(_) => vec![],
            })
            .collect();
        assert!(labels.contains(&"[S:null]"));
        assert!(!labels.iter().any(|l| l.contains("[T/D]")));
        assert!(!labels.iter().any(|l| l.contains("(B) bare")));
    }
```

- [ ] **Step 3: Thread `format` through callers**

`ui.rs:646` `for row in layout()` → `layout(app.doc.as_ref().map(|d| d.format()).unwrap_or(DocFormat::Toml))`. In `app.rs`, every `nav_rows()` call gets the doc's format (the App has `doc: Option<AnyDocument>`; add a small `self.doc_format()` helper returning `DocFormat::Toml` when `None`).

- [ ] **Step 4: Run the suite + build the binary**

Run: `cargo test 2>&1 | tail -15` and `cargo clippy -- -D warnings 2>&1 | tail`.
Expected: green.

- [ ] **Step 5: Commit**

```bash
git add src/model/any_doc.rs src/tui/type_filter.rs src/tui/ui.rs src/tui/app.rs
git commit -m "feat(tui): wire AnyDocument::Json + per-format type-filter facet set"
```

---

## Task 18: JSONC-upgrade prompt + comment-prefix-aware validation in the TUI

**Files:**
- Modify: `src/tui/state.rs` (`PromptKind::JsoncUpgrade`)
- Modify: `src/tui/app.rs` (gate comment-introducing ops; prompt flow; use `comment_prefix()`)
- Modify: `src/tui/mod.rs` (event-loop prompt handling, if `ArrayUpgrade` is handled there)

The comment-introducing ops are: `r` remark (live→comment), comment paste (`v` of a comment fragment), and `InsertComment`. When `!doc.supports_comments()` and the op would author a comment, enter `Mode::Prompt(JsoncUpgrade { pending })` instead of applying. On `y`: `doc.enable_comments()` then re-issue the pending op. On `n`: cancel.

- [ ] **Step 1: Add the prompt variant**

```rust
    /// Introducing the first comment into a pure `.json` makes it JSONC.
    /// `pending` is the op to re-issue after the user accepts.
    JsoncUpgrade {
        pending: Box<PendingComment>,
    },
```

Define `PendingComment` (an enum of the three comment-introducing ops with their args) in `state.rs`, or reuse the existing pending-op machinery if one exists. Keep it minimal:

```rust
pub enum PendingComment {
    Remark { path: Path },
    PasteComment {
        target: crate::model::document::Target,
        on_collision: crate::model::document::OnCollision,
    },
}
```

- [ ] **Step 2: Gate the ops**

At each comment-introducing call site in `app.rs`, before dispatching, check:

```rust
let authoring_comment = /* this op creates a // comment */;
if authoring_comment && !doc.supports_comments() {
    self.mode = Mode::Prompt(PromptKind::JsoncUpgrade { pending: Box::new(/* ... */) });
    return;
}
```

For `r` remark: it authors a comment only when toggling a **live** node → comment (not comment → live). Detect via the cursor node's kind.

- [ ] **Step 3: Handle the prompt keys**

In the prompt key handler (alongside `ArrayUpgrade` at `app.rs:2256`):

```rust
Mode::Prompt(PromptKind::JsoncUpgrade { .. }) => match key {
    'y' | 'Y' => {
        if let Mode::Prompt(PromptKind::JsoncUpgrade { pending }) =
            std::mem::replace(&mut self.mode, Mode::Normal)
        {
            self.doc_mut().enable_comments();
            self.run_pending_comment(*pending);
        }
    }
    'n' | 'N' | Esc => self.mode = self.resting_mode(),
    _ => {}
},
```

Implement `run_pending_comment` to re-dispatch the stored op. Render the prompt text in `ui.rs` ("Introduce a `//` comment? This makes the file JSONC. [y/n]").

- [ ] **Step 4: Comment-prefix-aware validation**

The inline comment editor's `#`-prefix validation and `app.rs:2951` `r.key.starts_with('#')` (InsertComment placement) must use `doc.comment_prefix()`. Replace the hard-coded `'#'` checks: in `edit_commit`'s comment branch, validate `text` lines against `comment_prefix()`; in the InsertComment placement scan, match on `comment_prefix()` instead of `'#'`.

- [ ] **Step 5: Tests (unit, no TUI driving)**

Add an `app.rs` test that constructs an `App` over a pure `.json` doc, sets the cursor to a member, invokes the remark handler, and asserts `self.mode` is `Mode::Prompt(PromptKind::JsoncUpgrade { .. })` and the doc is unchanged. Then simulate `y` and assert the doc now contains `//` and `supports_comments()` is true.

```rust
    #[test]
    fn pure_json_remark_prompts_then_upgrades() {
        let mut app = App::with_doc_from_str("c.json", "{\n  \"a\": 1\n}\n");
        app.cursor_to_key("a");
        app.remark(); // authoring a comment on a pure .json
        assert!(matches!(app.mode, Mode::Prompt(PromptKind::JsoncUpgrade { .. })));
        assert!(!app.doc().is_dirty());
        app.prompt_key('y');
        assert!(app.doc().serialize().contains("//"));
        assert!(app.doc().supports_comments());
    }
```

(Provide `App::with_doc_from_str` / `cursor_to_key` / `prompt_key` test helpers if not present, mirroring existing app-test helpers.)

- [ ] **Step 6: Run → PASS;** `cargo clippy -- -D warnings` green.

- [ ] **Step 7: Commit**

```bash
git add src/tui/state.rs src/tui/app.rs src/tui/mod.rs src/tui/ui.rs
git commit -m "feat(tui): JSONC-upgrade prompt + comment-prefix-aware validation"
```

---

## Task 19: Read-only node guards (block comments) in the TUI

**Files:** Modify `src/tui/app.rs`

A `read_only` node rejects `e`/`E`/`d`/`x`/`r`/`a`-into and insert-into with a status line "read-only node (block comment)"; copy (`c`) is allowed (fragment = raw text).

- [ ] **Step 1: Failing unit test**

```rust
    #[test]
    fn block_comment_rejects_mutation() {
        let mut app = App::with_doc_from_str("c.jsonc", "{\n  /* ro */\n  \"a\": 1\n}\n");
        app.cursor_to_index(0); // the block comment
        app.delete();
        assert!(app.status.contains("read-only"));
        assert!(app.doc().serialize().contains("/* ro */"));
    }

    #[test]
    fn block_comment_allows_copy() {
        let mut app = App::with_doc_from_str("c.jsonc", "{\n  /* ro */\n  \"a\": 1\n}\n");
        app.cursor_to_index(0);
        app.copy_selected();
        assert!(app.clipboard.is_some());
    }
```

- [ ] **Step 2: Run → FAIL.**

- [ ] **Step 3: Implement the guard**

Add a helper `App::cursor_is_read_only()` (look up the cursor node, check `read_only`). At the top of `delete`/`edit`/`force_editor`/`remark`/`add`/paste-into handlers, if the **target** node is read-only, set `self.status = "read-only node (block comment)".into()` and return. Leave `copy_selected`/`cut_selected`… actually cut would delete → also guard cut. Copy stays allowed.

- [ ] **Step 4: Run → PASS.**

- [ ] **Step 5: Commit**

```bash
git add src/tui/app.rs
git commit -m "feat(tui): read-only guards for /* */ block-comment nodes"
```

---

## Task 20: JSON help text

**Files:** Modify `src/tui/keys.rs`

- [ ] **Step 1: Add `JSON_HELP` and the match arm**

```rust
    match format {
        DocFormat::Toml => TOML_HELP,
        DocFormat::Json => JSON_HELP,
        DocFormat::Yaml => TOML_HELP, // Phase 3
    }
```

Write `JSON_HELP` as a `const &str` cloned from `TOML_HELP` with the JSON differences: drop dotted/AoT lines; `r` line reads "Remark toggle (comments out as `//`)"; the `K` line lists "object/array inline↔multiline, float plain↔exponent"; the KIND legend lists `(Q)/(−)` signs and the JSON type slots incl. `[S:null]`/`[T/M]`/`[F:exp ]`; drop `[T/D]`/`[T/S]`/`[A/T]`/`[I:hex ]`/`[S:lit ]` etc.

- [ ] **Step 2: Test**

```rust
    #[test]
    fn json_help_differs_from_toml() {
        let j = help_text(crate::model::document::DocFormat::Json);
        assert!(j.contains("//"));
        assert!(j.contains("[S:null]"));
        assert!(!j.contains("dotted"));
    }
```

Run → PASS.

- [ ] **Step 3: Commit**

```bash
git add src/tui/keys.rs
git commit -m "feat(tui): JSON-mode help text"
```

---

## Task 21: Integration roundtrip test for JSON/JSONC fixtures

**Files:**
- Modify: `tests/roundtrip.rs` (or add `tests/roundtrip_json.rs`)
- Add fixtures: `tests/fixtures/edgecases.json`, `tests/fixtures/comments.jsonc`

- [ ] **Step 1: Add fixtures covering** nested objects/arrays, root-array doc, all scalar types incl. `null` and exponent, trailing commas, `//` standalone + trailing, `/* */` block, empty object `{}`, empty array `[]`.

- [ ] **Step 2: Write the byte-identical roundtrip test**

```rust
#[test]
fn json_fixtures_roundtrip_byte_identical() {
    let fx = std::path::Path::new("tests/fixtures");
    for entry in std::fs::read_dir(fx).unwrap() {
        let p = entry.unwrap().path();
        let ext = p.extension().and_then(|e| e.to_str());
        if !matches!(ext, Some("json") | Some("jsonc")) {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap();
        let doc = confy::model::json::JsonDocument::load(&p).unwrap();
        use confy::model::document::ConfigDocument;
        assert_eq!(doc.serialize(), text, "roundtrip mismatch for {p:?}");
    }
}
```

(Confirm `JsonDocument` and `ConfigDocument` are re-exported from `lib.rs`; add re-exports if missing.)

- [ ] **Step 3: Run → PASS.** Also add a mutation-then-roundtrip check (load → apply a Replace → serialize → re-load → serialize equals) to prove `apply`'s re-parse stays lossless.

- [ ] **Step 4: Commit**

```bash
git add tests/ 
git commit -m "test(json): byte-identical roundtrip + mutation integration over json/jsonc fixtures"
```

---

## Task 22: Documentation — CHANGELOG, CLAUDE.md, CONTEXT.md, README

**Files:** `CHANGELOG.md`, `CLAUDE.md`, `CONTEXT.md`, `README.md`, `Cargo.toml`

- [ ] **Step 1: CHANGELOG** — add an `Unreleased Update` entry dated 2026-06-13 describing the JSON/JSONC backend (matching the squash/commit message).

- [ ] **Step 2: CLAUDE.md** — extend the Architecture section: a paragraph on the JSON backend (hand-rolled rowan CST, `//`/`/* */` handling, JSONC upgrade, read-only nodes, no dotted/AoT/datetime/radix/multiline-string), and add the `src/model/json/` six-file block to the Module map. Note the new model atoms (`ScalarType::Null`, `Format::Exponent`, `KindTarget::TableMultiline`, `Node.read_only`) and KIND tags.

- [ ] **Step 3: CONTEXT.md** — add glossary entries: **Opaque node** (forward-ref to Phase 3) / **Read-only node**, **JSONC upgrade**, **DocFormat**, and the `[S:null]`/`[T/M]`/`[F:exp ]` tags.

- [ ] **Step 4: README** — add/extend a format-support table (TOML: full; JSON/JSONC: supported; YAML: planned) and update `Cargo.toml`'s `description` line if it names only TOML.

- [ ] **Step 5: Verify build/lint/test once more**

Run: `cargo fmt --check && cargo clippy -- -D warnings && cargo test 2>&1 | tail -15`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add CHANGELOG.md CLAUDE.md CONTEXT.md README.md Cargo.toml
git commit -m "docs: JSON/JSONC backend (changelog, architecture, glossary, README)"
```

---

## Final manual verification (user-run; do NOT automate via pty)

Hand off to the user with this checklist (the real-binary gate per the project's testing rule):

1. `cargo run -- tests/fixtures/sample.jsonc` — tree renders; KIND column shows `[S:null]`, `[T/M]`, `[F:exp ]`, `(Q)` signs; block comment row marked read-only.
2. Edit a string/int inline (`e`), toggle a bool (`←/→`), `K` on an object (inline↔multiline) and a float (plain↔exponent).
3. On a pure `.json`: `r` on a member → JSONC-upgrade prompt → `y` writes `//`.
4. `d`/`x`/`c`/`v` a member and an element; paste a member into an array (wraps `{ }`); `w` save → reopen → byte-identical aside from intended edits.
5. `f` type filter shows only JSON facets; `?` help shows JSON text.

---

## Self-review against the spec (§Phase 2)

- **2.1 parser six-file family** → Tasks 3–9 (syntax/parse/doc/project/edit, mod). rowan direct dep + version pin → Task 3.
- **2.1 trailing commas accepted, never emitted** → Task 5 (parser accepts), Tasks 11–15 splices emit none (covered by roundtrip asserting no stray comma after delete/last-member).
- **2.1 atomicity / validate_semantics / dup-key Collision** → Task 9.
- **2.2 projection mapping table** → Tasks 7–8 (every row: object/array Inline/Multiline, string/int/float/bool/null, standalone//, trailing//, /* */ read-only). `ScalarType::Null`, `[S:null]`, `Format::Exponent` → Tasks 1–2 & 7. Root array document → Task 7. `Node.read_only` → Tasks 1, 8, 19.
- **2.3 behavior matrix** → e/E (existing dispatch + JSON fragments) Tasks 10/13; a add — covered by Insert seeding `""` (note: the `a` seed path lives in app.rs and already seeds `""`; JSON fragments `"key": ""` flow through Insert — verify in Task 12 follow-up); value null type-change → existing inline path (decision locked, no new code); Rename no-dotted → Task 13; d/x/c/v → Tasks 11/14 + app paste; element↔object moves → Tasks 12/14; r remark `//` → Task 13; comments `//`/InsertComment/`/* */` → Tasks 8/13/19; JSONC upgrade prompt → Task 18; K options → Tasks 15–16; absent features simply never offered → kind_options/help.
- **2.4 KIND column** → Tasks 1–2 (`[T/M]`, `[S:null]`, `[F:exp ]`, classify↔type_tag invariant), Task 17 (facet set by DocFormat).
- **2.5 help** → Task 20.
- **Testing** (golden projection, byte-identical roundtrip fixtures, mutation units mirroring cst_edit, classify↔type_tag invariant) → Tasks 5/7/8/9–16/21 & Task 2.
- **Docs** → Task 22.

**Open follow-up flagged for execution:** the `a` (add) seed lives in `app.rs`; confirm during Task 12 that it routes a `"key": ""` / `""` fragment through `Insert` for JSON (add a small app-level test if the seed text is hard-coded TOML-style).

**Out of scope (Phase 3/4, not in this plan):** YAML, document-level conversion, `to_value`/`render_value`, cross-format clipboard.
