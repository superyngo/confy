//! SPIKE: lossless YAML-subset lexer + indentation-driven parser → rowan green
//! tree (spec §3.3 gate task).
//!
//! Goal of this file is to *prove the gate*, not ship production code:
//!   1. `lex` is lossless — `lex(src).concat() == src`.
//!   2. `parse` builds a structural tree (mappings / sequences / scalars / flow)
//!      AND fences out-of-subset constructs (anchors, aliases, merge keys, tags,
//!      multi-line flow) as `OPAQUE` nodes, so `serialize() == src` byte-for-byte.
//!   3. multi-document files (`---` more than once) are rejected at load.
//!
//! Round-trip is structurally guaranteed because every token is bumped exactly
//! once and never reordered; the parser falls back to whole-line absorption for
//! anything it does not model, so losslessness never depends on perfect parsing.

use crate::model::yaml::syntax::SyntaxKind;
use rowan::{GreenNode, GreenNodeBuilder};

pub(crate) type Lexeme = (SyntaxKind, String);

/// Parse `src` into a lossless green tree, or `Err(message)`.
#[allow(dead_code)]
pub(crate) fn parse(src: &str) -> Result<GreenNode, String> {
    let tokens = lex(src);
    // Multi-document files are out of subset (v1): reject at load.
    let doc_markers = tokens
        .iter()
        .filter(|(k, t)| *k == SyntaxKind::DOC_MARKER && t == "---")
        .count();
    if doc_markers > 1 {
        return Err("multi-document YAML is not supported (found multiple `---`)".into());
    }
    let mut p = Parser {
        tokens,
        pos: 0,
        builder: GreenNodeBuilder::new(),
        error: None,
    };
    p.builder.start_node(SyntaxKind::ROOT.into());
    p.skip_trivia_lines();
    // optional leading document marker
    if p.line_head() == Some(SyntaxKind::DOC_MARKER) {
        p.bump_line();
        p.skip_trivia_lines();
    }
    if p.line_head().is_some() {
        match p.line_head() {
            Some(SyntaxKind::DASH) => p.parse_sequence(0, false),
            _ => p.parse_mapping(0, false),
        }
    }
    p.skip_trivia_lines();
    if p.error.is_none() && p.pos < p.tokens.len() {
        let (k, t) = &p.tokens[p.pos];
        p.error = Some(format!("unconsumed `{t}` ({k:?}) at top level"));
    }
    // Drain anything left so the tree is still lossless even on error.
    while p.pos < p.tokens.len() {
        p.bump();
    }
    p.builder.finish_node(); // ROOT
    match p.error {
        Some(e) => Err(e),
        None => Ok(p.builder.finish()),
    }
}

struct LineInfo {
    indent: usize,
    head: Option<SyntaxKind>,
    blank: bool,
    comment: bool,
}

struct Parser {
    tokens: Vec<Lexeme>,
    pos: usize,
    builder: GreenNodeBuilder<'static>,
    error: Option<String>,
}

impl Parser {
    fn get(&self, j: usize) -> Option<SyntaxKind> {
        self.tokens.get(j).map(|(k, _)| *k)
    }
    fn at(&self) -> Option<SyntaxKind> {
        self.get(self.pos)
    }
    fn bump(&mut self) {
        if let Some((k, t)) = self.tokens.get(self.pos) {
            self.builder.token((*k).into(), t);
            self.pos += 1;
        }
    }
    /// Bump tokens through the next `NEWLINE` (inclusive) or to EOF.
    fn bump_line(&mut self) {
        loop {
            match self.at() {
                None => break,
                Some(SyntaxKind::NEWLINE) => {
                    self.bump();
                    break;
                }
                _ => self.bump(),
            }
        }
    }
    /// Bump trailing in-line trivia: any `WHITESPACE`/`COMMENT`, then a `NEWLINE`.
    fn bump_trailing(&mut self) {
        while matches!(
            self.at(),
            Some(SyntaxKind::WHITESPACE | SyntaxKind::COMMENT)
        ) {
            self.bump();
        }
        if self.at() == Some(SyntaxKind::NEWLINE) {
            self.bump();
        }
    }

    /// Inspect the current line (does not consume): its indent width, the kind
    /// of its first content token, and whether it is blank / comment-only.
    fn peek_line(&self) -> LineInfo {
        let mut j = self.pos;
        let mut indent = 0;
        if self.get(j) == Some(SyntaxKind::INDENT) {
            indent = self.tokens[j].1.chars().count();
            j += 1;
        }
        match self.get(j) {
            None | Some(SyntaxKind::NEWLINE) => LineInfo {
                indent,
                head: None,
                blank: true,
                comment: false,
            },
            Some(SyntaxKind::COMMENT) => LineInfo {
                indent,
                head: Some(SyntaxKind::COMMENT),
                blank: false,
                comment: true,
            },
            Some(k) => LineInfo {
                indent,
                head: Some(k),
                blank: false,
                comment: false,
            },
        }
    }
    fn line_head(&self) -> Option<SyntaxKind> {
        self.peek_line().head
    }

    /// Float leading blank / comment-only lines into the current container.
    fn skip_trivia_lines(&mut self) {
        loop {
            let li = self.peek_line();
            if self.at().is_none() {
                break;
            }
            if li.blank || li.comment {
                self.bump_line();
            } else {
                break;
            }
        }
    }

    /// Does this line contain a top-level `COLON` before its `NEWLINE`?
    /// (Used to tell a compact mapping `- k: v` from a plain element `- v`.)
    fn line_has_colon(&self) -> bool {
        let mut j = self.pos;
        let mut depth = 0i32;
        while let Some(k) = self.get(j) {
            match k {
                SyntaxKind::NEWLINE => break,
                SyntaxKind::L_BRACE | SyntaxKind::L_BRACK => depth += 1,
                SyntaxKind::R_BRACE | SyntaxKind::R_BRACK => depth -= 1,
                SyntaxKind::COLON if depth == 0 => return true,
                _ => {}
            }
            j += 1;
        }
        false
    }

    // ---- block mapping --------------------------------------------------

    fn parse_mapping(&mut self, indent: usize, first_inline: bool) {
        self.builder.start_node(SyntaxKind::MAPPING.into());
        let mut first = first_inline;
        loop {
            if !first {
                self.skip_trivia_lines();
                let li = self.peek_line();
                if li.head.is_none() || li.indent != indent || li.head == Some(SyntaxKind::DASH) {
                    break;
                }
            }
            if self.line_head() == Some(SyntaxKind::MERGE) {
                self.parse_opaque_entry(indent, first);
            } else {
                self.parse_map_entry(indent, first);
            }
            first = false;
        }
        self.builder.finish_node();
    }

    fn parse_map_entry(&mut self, indent: usize, inline: bool) {
        self.builder.start_node(SyntaxKind::MAP_ENTRY.into());
        if !inline && self.at() == Some(SyntaxKind::INDENT) {
            self.bump();
        }
        self.builder.start_node(SyntaxKind::KEY.into());
        // key scalar token
        if matches!(
            self.at(),
            Some(SyntaxKind::PLAIN | SyntaxKind::SINGLE | SyntaxKind::DOUBLE)
        ) {
            self.bump();
        } else if self.error.is_none() {
            self.error = Some(format!("expected a mapping key, found {:?}", self.at()));
        }
        self.builder.finish_node(); // KEY
        while self.at() == Some(SyntaxKind::WHITESPACE) {
            self.bump();
        }
        if self.at() == Some(SyntaxKind::COLON) {
            self.bump();
        } else if self.error.is_none() {
            self.error = Some(format!("expected `:`, found {:?}", self.at()));
        }
        self.parse_value(indent);
        self.builder.finish_node(); // MAP_ENTRY
    }

    /// A merge-key entry (`<<: …`) — out of subset → opaque.
    fn parse_opaque_entry(&mut self, indent: usize, inline: bool) {
        self.builder.start_node(SyntaxKind::OPAQUE.into());
        if !inline && self.at() == Some(SyntaxKind::INDENT) {
            self.bump();
        }
        self.bump_line();
        self.absorb_deeper(indent);
        self.builder.finish_node();
    }

    // ---- block sequence -------------------------------------------------

    fn parse_sequence(&mut self, indent: usize, first_inline: bool) {
        self.builder.start_node(SyntaxKind::SEQUENCE.into());
        let mut first = first_inline;
        loop {
            if !first {
                self.skip_trivia_lines();
                let li = self.peek_line();
                if li.head != Some(SyntaxKind::DASH) || li.indent != indent {
                    break;
                }
            }
            self.parse_seq_entry(indent, first);
            first = false;
        }
        self.builder.finish_node();
    }

    fn parse_seq_entry(&mut self, indent: usize, inline: bool) {
        self.builder.start_node(SyntaxKind::SEQ_ENTRY.into());
        if !inline && self.at() == Some(SyntaxKind::INDENT) {
            self.bump();
        }
        self.bump(); // DASH
        let mut content_col = indent + 1;
        while self.at() == Some(SyntaxKind::WHITESPACE) {
            content_col += self.tokens[self.pos].1.chars().count();
            self.bump();
        }
        match self.at() {
            None | Some(SyntaxKind::NEWLINE) | Some(SyntaxKind::COMMENT) => {
                // value on following (deeper) lines, or empty
                self.bump_trailing();
                self.skip_trivia_lines();
                let li = self.peek_line();
                if li.head == Some(SyntaxKind::DASH) && li.indent > indent {
                    self.parse_sequence(li.indent, false);
                } else if li.head.is_some()
                    && !li.blank
                    && !li.comment
                    && li.indent > indent
                    && li.head != Some(SyntaxKind::DASH)
                {
                    self.parse_mapping(li.indent, false);
                }
            }
            Some(SyntaxKind::DASH) => self.parse_sequence(content_col, true),
            Some(SyntaxKind::BLOCK_HEADER) => self.parse_block_scalar(indent),
            Some(SyntaxKind::ANCHOR | SyntaxKind::ALIAS | SyntaxKind::TAG) => {
                self.parse_opaque_value(indent)
            }
            Some(SyntaxKind::L_BRACE) => self.parse_flow_or_opaque(SyntaxKind::FLOW_MAP),
            Some(SyntaxKind::L_BRACK) => self.parse_flow_or_opaque(SyntaxKind::FLOW_SEQ),
            _ => {
                if self.line_has_colon() {
                    self.parse_mapping(content_col, true);
                } else {
                    self.parse_scalar_value();
                }
            }
        }
        self.builder.finish_node(); // SEQ_ENTRY
    }

    // ---- values ---------------------------------------------------------

    fn parse_value(&mut self, parent_indent: usize) {
        while self.at() == Some(SyntaxKind::WHITESPACE) {
            self.bump();
        }
        match self.at() {
            None | Some(SyntaxKind::NEWLINE) | Some(SyntaxKind::COMMENT) => {
                // empty inline → child block on following lines (or implicit null)
                self.bump_trailing();
                self.skip_trivia_lines();
                let li = self.peek_line();
                if li.head == Some(SyntaxKind::DASH) && li.indent >= parent_indent {
                    self.parse_sequence(li.indent, false);
                } else if li.head.is_some()
                    && !li.blank
                    && !li.comment
                    && li.indent > parent_indent
                    && li.head != Some(SyntaxKind::DASH)
                {
                    self.parse_mapping(li.indent, false);
                }
            }
            Some(SyntaxKind::BLOCK_HEADER) => self.parse_block_scalar(parent_indent),
            Some(SyntaxKind::ANCHOR | SyntaxKind::ALIAS | SyntaxKind::TAG) => {
                self.parse_opaque_value(parent_indent)
            }
            Some(SyntaxKind::L_BRACE) => self.parse_flow_or_opaque(SyntaxKind::FLOW_MAP),
            Some(SyntaxKind::L_BRACK) => self.parse_flow_or_opaque(SyntaxKind::FLOW_SEQ),
            _ => self.parse_scalar_value(),
        }
    }

    fn parse_scalar_value(&mut self) {
        self.builder.start_node(SyntaxKind::VALUE.into());
        self.builder.start_node(SyntaxKind::SCALAR.into());
        while !matches!(
            self.at(),
            None | Some(SyntaxKind::NEWLINE) | Some(SyntaxKind::COMMENT)
        ) {
            self.bump();
        }
        self.builder.finish_node(); // SCALAR
        self.builder.finish_node(); // VALUE
        self.bump_trailing();
    }

    fn parse_block_scalar(&mut self, parent_indent: usize) {
        self.builder.start_node(SyntaxKind::VALUE.into());
        self.builder.start_node(SyntaxKind::BLOCK_SCALAR.into());
        self.bump(); // BLOCK_HEADER
        self.bump_trailing();
        loop {
            let li = self.peek_line();
            if self.at().is_none() {
                break;
            }
            if li.blank || li.indent > parent_indent {
                self.bump_line();
            } else {
                break;
            }
        }
        self.builder.finish_node(); // BLOCK_SCALAR
        self.builder.finish_node(); // VALUE
    }

    fn parse_opaque_value(&mut self, parent_indent: usize) {
        self.builder.start_node(SyntaxKind::VALUE.into());
        self.builder.start_node(SyntaxKind::OPAQUE.into());
        self.bump_line();
        self.absorb_deeper(parent_indent);
        self.builder.finish_node(); // OPAQUE
        self.builder.finish_node(); // VALUE
    }

    /// Absorb following blank / more-indented lines (the value's block extent).
    fn absorb_deeper(&mut self, parent_indent: usize) {
        loop {
            let li = self.peek_line();
            if self.at().is_none() {
                break;
            }
            if li.blank || li.indent > parent_indent {
                self.bump_line();
            } else {
                break;
            }
        }
    }

    fn parse_flow_or_opaque(&mut self, single_line_kind: SyntaxKind) {
        // single-line if the matching close comes before any NEWLINE
        let mut j = self.pos;
        let mut depth = 0i32;
        let mut saw_newline = false;
        let single_line = loop {
            match self.get(j) {
                None => break false,
                Some(SyntaxKind::NEWLINE) => {
                    saw_newline = true;
                    j += 1;
                }
                Some(SyntaxKind::L_BRACE | SyntaxKind::L_BRACK) => {
                    depth += 1;
                    j += 1;
                }
                Some(SyntaxKind::R_BRACE | SyntaxKind::R_BRACK) => {
                    depth -= 1;
                    j += 1;
                    if depth == 0 {
                        break !saw_newline;
                    }
                }
                _ => j += 1,
            }
        };
        self.builder.start_node(SyntaxKind::VALUE.into());
        if single_line {
            // Recursively build nested FLOW_MAP/FLOW_SEQ + FLOW_ENTRY nodes so a
            // nested `{…}` value is a real child (not a flat token run) and each
            // map member is individually addressable.
            self.parse_flow_collection(single_line_kind);
        } else {
            // Multi-line flow is out of subset → opaque flat span.
            self.builder.start_node(SyntaxKind::OPAQUE.into());
            let mut depth = 0i32;
            loop {
                match self.at() {
                    None => break,
                    Some(SyntaxKind::L_BRACE | SyntaxKind::L_BRACK) => {
                        depth += 1;
                        self.bump();
                    }
                    Some(SyntaxKind::R_BRACE | SyntaxKind::R_BRACK) => {
                        depth -= 1;
                        self.bump();
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => self.bump(),
                }
            }
            self.builder.finish_node(); // OPAQUE
        }
        self.builder.finish_node(); // VALUE
        self.bump_trailing();
    }

    /// Parse a single-line flow collection (`{…}` or `[…]`) into a structured
    /// node. The opening `{`/`[` is the current token. Separators (`,`) and
    /// inter-member whitespace float as direct children of the collection node.
    fn parse_flow_collection(&mut self, kind: SyntaxKind) {
        self.builder.start_node(kind.into());
        self.bump(); // L_BRACE / L_BRACK
        if kind == SyntaxKind::FLOW_MAP {
            self.parse_flow_map_body();
        } else {
            self.parse_flow_seq_body();
        }
        // Closing brace/bracket.
        if matches!(self.at(), Some(SyntaxKind::R_BRACE | SyntaxKind::R_BRACK)) {
            self.bump();
        }
        self.builder.finish_node(); // kind
    }

    /// Flow map body: a run of FLOW_ENTRY members, with `,`/whitespace floating
    /// between them. Stops at the closing brace (or EOF).
    fn parse_flow_map_body(&mut self) {
        loop {
            match self.at() {
                None | Some(SyntaxKind::R_BRACE | SyntaxKind::R_BRACK) => break,
                Some(SyntaxKind::WHITESPACE | SyntaxKind::COMMA) => self.bump(),
                Some(SyntaxKind::PLAIN | SyntaxKind::SINGLE | SyntaxKind::DOUBLE) => {
                    self.parse_flow_entry()
                }
                // Defensive: anything unexpected floats (keeps the tree lossless).
                _ => self.bump(),
            }
        }
    }

    /// A flow map member: `KEY : value`. Whitespace around the colon and the
    /// value itself live inside the FLOW_ENTRY; the trailing `,` floats outside.
    fn parse_flow_entry(&mut self) {
        self.builder.start_node(SyntaxKind::FLOW_ENTRY.into());
        self.builder.start_node(SyntaxKind::KEY.into());
        self.bump(); // key scalar token
        self.builder.finish_node(); // KEY
        while self.at() == Some(SyntaxKind::WHITESPACE) {
            self.bump();
        }
        if self.at() == Some(SyntaxKind::COLON) {
            self.bump();
        }
        while self.at() == Some(SyntaxKind::WHITESPACE) {
            self.bump();
        }
        self.parse_flow_value();
        self.builder.finish_node(); // FLOW_ENTRY
    }

    /// The value of a flow map member: a nested flow collection (VALUE-wrapped,
    /// like a block value) or a scalar token, or nothing (implicit null `{x:}`).
    fn parse_flow_value(&mut self) {
        match self.at() {
            Some(SyntaxKind::L_BRACE) => {
                self.builder.start_node(SyntaxKind::VALUE.into());
                self.parse_flow_collection(SyntaxKind::FLOW_MAP);
                self.builder.finish_node();
            }
            Some(SyntaxKind::L_BRACK) => {
                self.builder.start_node(SyntaxKind::VALUE.into());
                self.parse_flow_collection(SyntaxKind::FLOW_SEQ);
                self.builder.finish_node();
            }
            Some(SyntaxKind::PLAIN | SyntaxKind::SINGLE | SyntaxKind::DOUBLE) => {
                self.builder.start_node(SyntaxKind::VALUE.into());
                self.builder.start_node(SyntaxKind::SCALAR.into());
                self.bump();
                self.builder.finish_node(); // SCALAR
                self.builder.finish_node(); // VALUE
            }
            _ => {} // implicit null
        }
    }

    /// Flow seq body: scalar elements as bare tokens, nested collections as real
    /// child nodes, with `,`/whitespace floating between them.
    fn parse_flow_seq_body(&mut self) {
        loop {
            match self.at() {
                None | Some(SyntaxKind::R_BRACE | SyntaxKind::R_BRACK) => break,
                Some(SyntaxKind::WHITESPACE | SyntaxKind::COMMA) => self.bump(),
                Some(SyntaxKind::L_BRACE) => self.parse_flow_collection(SyntaxKind::FLOW_MAP),
                Some(SyntaxKind::L_BRACK) => self.parse_flow_collection(SyntaxKind::FLOW_SEQ),
                Some(SyntaxKind::PLAIN | SyntaxKind::SINGLE | SyntaxKind::DOUBLE) => self.bump(),
                _ => self.bump(),
            }
        }
    }
}

/// Tokenize losslessly: every byte of `src` lands in exactly one lexeme, so
/// `lex(src).map(|(_, t)| t).concat() == src`.
pub(crate) fn lex(src: &str) -> Vec<Lexeme> {
    use SyntaxKind::*;
    let b = src.as_bytes();
    let n = b.len();
    let mut i = 0;
    let mut out: Vec<Lexeme> = Vec::new();
    // `node_start`: we are at a position where a fresh node value may begin
    // (line content start, or just after `- ` / `: `). Controls whether `-`,
    // `|`/`>`, `{`/`[`, `&*!`, `<<`, `---` are structural vs plain text.
    let mut node_start = true;
    let mut line_start = true;
    let mut flow_depth = 0i32;

    let push = |out: &mut Vec<Lexeme>, k: SyntaxKind, s: &str| out.push((k, s.to_string()));

    while i < n {
        let start = i;
        let c = b[i];

        // newline
        if c == b'\n' {
            i += 1;
            push(&mut out, NEWLINE, &src[start..i]);
            line_start = true;
            node_start = true;
            continue;
        }
        if c == b'\r' {
            i += if b.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
            push(&mut out, NEWLINE, &src[start..i]);
            line_start = true;
            node_start = true;
            continue;
        }

        // leading indent of a line
        if line_start && (c == b' ' || c == b'\t') {
            while i < n && (b[i] == b' ' || b[i] == b'\t') {
                i += 1;
            }
            push(&mut out, INDENT, &src[start..i]);
            line_start = false;
            continue;
        }
        line_start = false;

        // inter-token whitespace
        if c == b' ' || c == b'\t' {
            while i < n && (b[i] == b' ' || b[i] == b'\t') {
                i += 1;
            }
            push(&mut out, WHITESPACE, &src[start..i]);
            // whitespace does not by itself change node_start
            continue;
        }

        // comment: `#` at content start of line, or preceded by whitespace
        if c == b'#' {
            let prev = out.last().map(|(k, _)| *k);
            let comment_ok = matches!(prev, None | Some(NEWLINE) | Some(INDENT) | Some(WHITESPACE));
            if comment_ok {
                while i < n && b[i] != b'\n' && b[i] != b'\r' {
                    i += 1;
                }
                push(&mut out, COMMENT, &src[start..i]);
                continue;
            }
        }

        if node_start {
            // document markers
            if (c == b'-' && src[i..].starts_with("---"))
                || (c == b'.' && src[i..].starts_with("..."))
            {
                let rest = &src[i + 3..];
                if rest.is_empty() || rest.starts_with([' ', '\t', '\n', '\r']) {
                    i += 3;
                    push(&mut out, DOC_MARKER, &src[start..i]);
                    node_start = false;
                    continue;
                }
            }
            // block sequence indicator `- `
            if c == b'-' {
                let next = b.get(i + 1).copied();
                if matches!(
                    next,
                    None | Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r')
                ) {
                    i += 1;
                    push(&mut out, DASH, &src[start..i]);
                    // node_start stays true: `- - x`, `- key: v`
                    continue;
                }
            }
            // block scalar header
            if c == b'|' || c == b'>' {
                let mut j = i + 1;
                while j < n && matches!(b[j], b'+' | b'-' | b'0'..=b'9') {
                    j += 1;
                }
                let mut k = j;
                while k < n && (b[k] == b' ' || b[k] == b'\t') {
                    k += 1;
                }
                if k >= n || b[k] == b'\n' || b[k] == b'\r' || b[k] == b'#' {
                    i = j;
                    push(&mut out, BLOCK_HEADER, &src[start..i]);
                    node_start = false;
                    continue;
                }
            }
            // out-of-subset triggers
            if c == b'&' || c == b'*' || c == b'!' {
                i += 1;
                while i < n && !matches!(b[i], b' ' | b'\t' | b'\n' | b'\r') {
                    i += 1;
                }
                let kind = match c {
                    b'&' => ANCHOR,
                    b'*' => ALIAS,
                    _ => TAG,
                };
                push(&mut out, kind, &src[start..i]);
                node_start = false;
                continue;
            }
            if c == b'<' && src[i..].starts_with("<<") {
                i += 2;
                push(&mut out, MERGE, &src[start..i]);
                node_start = false;
                continue;
            }
        }

        // flow punctuation (open only meaningful at node_start; close/comma any time in flow)
        match c {
            b'{' if node_start => {
                i += 1;
                flow_depth += 1;
                push(&mut out, L_BRACE, &src[start..i]);
                node_start = true;
                continue;
            }
            b'[' if node_start => {
                i += 1;
                flow_depth += 1;
                push(&mut out, L_BRACK, &src[start..i]);
                node_start = true;
                continue;
            }
            b'}' if flow_depth > 0 => {
                i += 1;
                flow_depth -= 1;
                push(&mut out, R_BRACE, &src[start..i]);
                node_start = false;
                continue;
            }
            b']' if flow_depth > 0 => {
                i += 1;
                flow_depth -= 1;
                push(&mut out, R_BRACK, &src[start..i]);
                node_start = false;
                continue;
            }
            b',' if flow_depth > 0 => {
                i += 1;
                push(&mut out, COMMA, &src[start..i]);
                node_start = true;
                continue;
            }
            _ => {}
        }

        // mapping indicator `:` (followed by space / EOL, or a flow indicator in flow)
        if c == b':' {
            let next = b.get(i + 1).copied();
            let is_colon = matches!(
                next,
                None | Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r')
            ) || (flow_depth > 0
                && matches!(next, Some(b',') | Some(b'}') | Some(b']')));
            if is_colon {
                i += 1;
                push(&mut out, COLON, &src[start..i]);
                node_start = true;
                continue;
            }
        }

        // single / double quoted
        if c == b'\'' {
            i += 1;
            while i < n {
                if b[i] == b'\'' {
                    if b.get(i + 1) == Some(&b'\'') {
                        i += 2; // escaped ''
                    } else {
                        i += 1;
                        break;
                    }
                } else if b[i] == b'\n' || b[i] == b'\r' {
                    break; // unterminated on this line
                } else {
                    i += 1;
                }
            }
            push(&mut out, SINGLE, &src[start..i]);
            node_start = false;
            continue;
        }
        if c == b'"' {
            i += 1;
            while i < n {
                match b[i] {
                    b'\\' => {
                        i += 1;
                        if i < n {
                            i += 1;
                        }
                    }
                    b'"' => {
                        i += 1;
                        break;
                    }
                    b'\n' | b'\r' => break,
                    _ => i += 1,
                }
            }
            push(&mut out, DOUBLE, &src[start..i]);
            node_start = false;
            continue;
        }

        // plain scalar run
        {
            let scalar_start = i;
            while i < n {
                let ch = b[i];
                if ch == b'\n' || ch == b'\r' {
                    break;
                }
                // ` #` ends a plain scalar (trailing comment)
                if ch == b' ' || ch == b'\t' {
                    // peek the next non-space char
                    let mut j = i;
                    while j < n && (b[j] == b' ' || b[j] == b'\t') {
                        j += 1;
                    }
                    if j >= n || b[j] == b'\n' || b[j] == b'\r' {
                        break; // trailing whitespace → not part of scalar
                    }
                    if b[j] == b'#' {
                        break; // space then comment
                    }
                    i = j;
                    continue;
                }
                // `: ` (or `:` at EOL) ends the key/plain scalar
                if ch == b':' {
                    let next = b.get(i + 1).copied();
                    let stops = matches!(
                        next,
                        None | Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r')
                    ) || (flow_depth > 0
                        && matches!(next, Some(b',') | Some(b'}') | Some(b']')));
                    if stops {
                        break;
                    }
                }
                if flow_depth > 0 && matches!(ch, b',' | b'}' | b']') {
                    break;
                }
                i += 1;
            }
            if i > scalar_start {
                push(&mut out, PLAIN, &src[scalar_start..i]);
                node_start = false;
                continue;
            }
        }

        // anything else: a single ERROR byte (kept lossless)
        i += 1;
        push(&mut out, ERROR, &src[start..i]);
        node_start = false;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::yaml::syntax::SyntaxNode;

    const CORPUS: &[(&str, &str)] = &[
        (
            "docker-compose",
            include_str!("../../../tests/fixtures/yaml/docker-compose.yaml"),
        ),
        (
            "github-actions",
            include_str!("../../../tests/fixtures/yaml/github-actions.yaml"),
        ),
        (
            "deployment",
            include_str!("../../../tests/fixtures/yaml/deployment.yaml"),
        ),
        (
            "helm-values",
            include_str!("../../../tests/fixtures/yaml/helm-values.yaml"),
        ),
        (
            "prometheus",
            include_str!("../../../tests/fixtures/yaml/prometheus.yaml"),
        ),
        (
            "simple-config",
            include_str!("../../../tests/fixtures/yaml/simple-config.yaml"),
        ),
        (
            "flow-style",
            include_str!("../../../tests/fixtures/yaml/flow-style.yaml"),
        ),
        (
            "scalars",
            include_str!("../../../tests/fixtures/yaml/scalars.yaml"),
        ),
        (
            "comments",
            include_str!("../../../tests/fixtures/yaml/comments.yaml"),
        ),
        (
            "tags-and-anchors",
            include_str!("../../../tests/fixtures/yaml/tags-and-anchors.yaml"),
        ),
    ];

    fn opaque_count(green: GreenNode) -> usize {
        SyntaxNode::new_root(green)
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::OPAQUE)
            .count()
    }

    #[test]
    fn lex_is_lossless() {
        for (name, src) in CORPUS {
            let joined: String = lex(src).into_iter().map(|(_, t)| t).collect();
            assert_eq!(&joined, src, "lex not lossless for {name}");
        }
    }

    #[test]
    fn parse_roundtrips_byte_identical() {
        for (name, src) in CORPUS {
            let green = parse(src).unwrap_or_else(|e| panic!("parse {name} failed: {e}"));
            let got = SyntaxNode::new_root(green).to_string();
            assert_eq!(&got, src, "roundtrip mismatch for {name}");
        }
    }

    #[test]
    fn out_of_subset_is_fenced_opaque() {
        // docker-compose: x-common anchor block + two <<: merge entries  → 3 opaque
        let dc = parse(CORPUS[0].1).unwrap();
        assert!(
            opaque_count(dc) >= 3,
            "docker-compose anchors/merges not fenced"
        );
        // tags-and-anchors: anchor blocks, merge, tags, aliases → several opaque
        let ta = parse(CORPUS[9].1).unwrap();
        assert!(opaque_count(ta) >= 5, "tags/anchors/aliases not fenced");
        // a subset-only file has zero opaque nodes
        let simple = parse(CORPUS[5].1).unwrap();
        assert_eq!(
            opaque_count(simple),
            0,
            "simple-config should be fully in subset"
        );
    }

    #[test]
    fn multi_document_is_rejected() {
        let src = include_str!("../../../tests/fixtures/yaml/multi-doc.yaml");
        assert!(parse(src).is_err(), "multi-doc should be rejected at load");
    }

    #[test]
    fn flow_collections_parse_inline() {
        let green = parse(CORPUS[6].1).unwrap();
        let root = SyntaxNode::new_root(green);
        let has_flow = root
            .descendants()
            .any(|n| matches!(n.kind(), SyntaxKind::FLOW_MAP | SyntaxKind::FLOW_SEQ));
        assert!(has_flow, "flow-style.yaml should yield flow nodes");
        assert_eq!(opaque_count(parse(CORPUS[6].1).unwrap()), 0);
    }

    #[test]
    fn roundtrips_edge_cases() {
        for src in [
            "a: b: c\n",                   // colon in plain value
            "url: http://example.com\n",   // colon-not-indicator stays plain
            "tpl: ${{ matrix.os }}\n",     // braces mid-plain-scalar are literal
            "empty:\n",                    // implicit null value
            "nested:\n  - - 1\n  - - 2\n", // compact nested sequence
            "k: 'a''b'\n",                 // single-quote escape
            "k: \"a\\\"b\"\n",             // double-quote escape
            "- - a\n- - b\n",              // sequence of sequences
            "a:\n- x\n- y\n",              // seq at key indent (block, same column)
            "x: |-\n  no trailing nl\n",   // chomping indicator
            "  # leading-indented comment\nk: 1\n",
        ] {
            let green = parse(src).unwrap_or_else(|e| panic!("parse {src:?}: {e}"));
            assert_eq!(
                SyntaxNode::new_root(green).to_string(),
                src,
                "roundtrip {src:?}"
            );
        }
    }

    #[test]
    fn nested_flow_roundtrips_and_nests() {
        for src in [
            "server: {host: a, inner: {x: 1, y: 2}}\n",
            "items: [{a: 1}, {b: 2}]\n",
            "deep: {a: {b: {c: 1}}}\n",
            "mix: {list: [1, 2], name: x}\n",
            "trailing: {a: 1,}\n",
            "seq: [[1, 2], [3, 4]]\n",
            "empty: {}\n",
        ] {
            let green = parse(src).unwrap_or_else(|e| panic!("parse {src:?}: {e}"));
            assert_eq!(
                SyntaxNode::new_root(green).to_string(),
                src,
                "roundtrip {src:?}"
            );
        }
        // A nested flow map is a real child node, not a flat token run.
        let green = parse("server: {host: a, inner: {x: 1}}\n").unwrap();
        let root = SyntaxNode::new_root(green);
        let flow_maps = root
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::FLOW_MAP)
            .count();
        assert_eq!(flow_maps, 2, "outer + nested FLOW_MAP expected");
        let flow_entries = root
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::FLOW_ENTRY)
            .count();
        assert_eq!(flow_entries, 3, "host, inner, x");
    }

    #[test]
    fn smoke_small_inputs() {
        for src in [
            "key: value\n",
            "- a\n- b\n",
            "a:\n  b: 1\n  c: 2\n",
            "list:\n  - x\n  - y\n",
            "flow: {a: 1, b: 2}\n",
            "seq: [1, 2, 3]\n",
            "block: |\n  line one\n  line two\n",
            "folded: >\n  a b c\n",
            "ref: *anchor\n",
            "tagged: !!str 7\n",
            "",
            "# just a comment\n",
        ] {
            let green = parse(src).unwrap_or_else(|e| panic!("parse {src:?} failed: {e}"));
            assert_eq!(
                SyntaxNode::new_root(green).to_string(),
                src,
                "roundtrip {src:?}"
            );
        }
    }
}
