✅ **Shipped — historical reference.** See `CHANGELOG.md` for current behavior; this plan is kept for context, not as a live task list.

# VS Code `DocumentSymbolProvider` (Outline / Breadcrumbs) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Populate `Node.text_range`/`Node.key_text_range` in `confy-core`'s three
format backends, expose a read-only `ConfySession::outline()` in `confy-ffi`,
and register a `vscode.DocumentSymbolProvider` in `editors/vscode` so TOML and
YAML files opened in VS Code's native text editor get Outline / breadcrumbs /
`Cmd+Shift+O`.

**Architecture:** Three modules, bottom-up, each additive/read-only. `Node`
gains two byte-range fields populated for free at the same rowan
`SyntaxNode`/`SyntaxToken` visits the three existing `walk()` projections
already perform. `confy-ffi` adds one read-only method + one new wire type
(`OutlineNode`). `editors/vscode` adds a new provider module that loads the
already-built wasm into the extension host (Node.js) process — a second,
independent runtime instance of the same compiled `confy_ffi_bg.wasm` the
webview also loads (see spec's "Two independent parser instances" section).

**Tech Stack:** Rust (rowan-based lossless CST, `taplo` for TOML), wasm-bindgen
+ serde-wasm-bindgen, TypeScript + esbuild, VS Code Extension API 1.85+.

**Spec:** `docs/superpowers/specs/2026-08-20-vscode-outline-provider-design.md`
(read both — this plan argues from that spec; it is not restated in full here).
Related decision record: `docs/adr/0006-outline-symbol-representative-span-anchoring.md`.

## Global Constraints

- Zero behavior change to any existing `confy-core`/`confy-ffi`/`editors/vscode`
  functionality — every change here is purely additive.
- `Node.text_range: Range<usize>` (not `span`) / `Node.key_text_range: Option<Range<usize>>`
  (not `key_span`) — deliberately distinct from `CONTEXT.md`'s existing "Member
  spans" term.
- Scattered/synthetic-node representative-range policy (ADR 0006): a
  `Format::Dotted` synthetic Table's `text_range` is its **first member's**
  `text_range()`, never a min-max envelope; a normal Table's `text_range`
  never widens to enclose non-adjacent descendant sections either.
- `confy-core` and `confy-ffi` callers of `Node::branch`/`Node::leaf` are
  confined to `node.rs`'s own unit tests (verified — grep found no other
  callers); those get `0..0`/`None` since they build synthetic trees never
  fed through `outline()`.
- After every task: `cargo fmt --check` / `cargo clippy --workspace --all-targets`
  0 warnings / `cargo test --workspace` for core tasks; `tsc --noEmit` for the
  vscode task. Append a `CHANGELOG.md` `Unreleased Update` entry per task,
  matching the commit message, before committing (repo rule).
- No project-wide GUI smoke test can run headless — final manual VS Code
  Extension Development Host verification is the user's job (repo convention).

---

## Task 1: `confy-core` — add `text_range`/`key_text_range` to `Node`

**Files:**
- Modify: `crates/confy-core/src/model/node.rs:112-172` (`Node` struct + `Node::branch`/`Node::leaf`)

**Interfaces:**
- Produces: `Node.text_range: std::ops::Range<usize>`, `Node.key_text_range: Option<std::ops::Range<usize>>` — every later task's `Node { .. }` literal must set both.

- [ ] **Step 1: Add the two fields to the struct**

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub key: String,
    pub path: Path,
    pub kind: NodeKind,
    pub children: Vec<Node>,
    pub value: Option<String>,
    pub format: Format,
    pub key_sign: KeySign,
    pub trailing_comment: Option<String>,
    pub read_only: bool,
    /// Byte range (half-open, UTF-8 byte offsets into the source text) of the
    /// whole node, including its key and value/children. Distinct from
    /// `CONTEXT.md`'s "Member spans" (the discrete, possibly-scattered source
    /// pieces that *constitute* a table) — this is a single contiguous
    /// representative range for editor symbol-tree purposes (VS Code Outline
    /// / breadcrumbs). See ADR 0006 for the anchoring policy on synthetic /
    /// scattered-definition nodes.
    pub text_range: std::ops::Range<usize>,
    /// Byte range of just the key token; `None` for keyless nodes (array
    /// elements, AoT entries, Root, comments) — the same nodes where
    /// `key_sign` is already `KeySign::None`.
    pub key_text_range: Option<std::ops::Range<usize>>,
}
```

- [ ] **Step 2: Update `Node::branch`/`Node::leaf`**

Both are only called from this file's own `#[cfg(test)]` module (verified by
repo-wide grep — no other caller exists), building synthetic in-memory trees
never passed through `outline()`. Give both a trivial default:

```rust
    pub fn branch(key: impl Into<String>, kind: NodeKind) -> Self {
        // ...unchanged debug_assert...
        Node {
            key: key.into(),
            path: Vec::new(),
            kind,
            children: Vec::new(),
            value: None,
            format: Format::Plain,
            key_sign: KeySign::None,
            trailing_comment: None,
            read_only: false,
            text_range: 0..0,
            key_text_range: None,
        }
    }

    pub fn leaf(key: impl Into<String>, kind: NodeKind) -> Self {
        // ...unchanged debug_assert...
        Node {
            key: key.into(),
            path: Vec::new(),
            kind,
            children: Vec::new(),
            value: None,
            format: Format::Plain,
            key_sign: KeySign::None,
            trailing_comment: None,
            read_only: false,
            text_range: 0..0,
            key_text_range: None,
        }
    }
```

- [ ] **Step 3: Confirm the crate does not yet compile — expected, on purpose**

Run: `cargo build -p confy-core 2>&1 | grep "missing structure fields" | wc -l`
Expected: a non-zero count of compile errors, one per `Node { .. }` literal in
`cst_project.rs` / `json/project.rs` / `yaml/project.rs` that Tasks 2-4 fix.
This is the intended compiler-driven completeness check — every literal that
still needs its `text_range`/`key_text_range` set will fail to compile until
fixed, so none can be silently skipped.

- [ ] **Step 4: Commit**

```bash
cd /Volumes/Home/Users/wen/repos/confy
git add crates/confy-core/src/model/node.rs
git commit -m "feat(core): add Node.text_range/key_text_range fields (compile-broken until Tasks 2-4 land)"
```

(This commit is intentionally red — `cargo build` fails until Task 2 lands.
If your workflow requires green commits, squash Tasks 1-4 into one commit
instead; the step-by-step split above is for reviewability, not for landing
each step independently.)

---

## Task 2: `confy-core` — populate spans in the TOML backend (`cst_project.rs`)

**Files:**
- Modify: `crates/confy-core/src/model/cst_project.rs` (every `Node { .. }` literal; `~9` sites)

**Interfaces:**
- Consumes: Task 1's `Node.text_range`/`Node.key_text_range` fields.
- Produces: nothing new — internal to this file.

Every `Node { .. }` literal in this file is fed by a `taplo::rowan::SyntaxNode`
or `SyntaxToken` already in scope at that literal (that's what's being
projected). Rowan's `SyntaxNode`/`SyntaxToken::text_range()` returns a
`rowan::TextRange` (UTF-8 byte offsets); convert with a small local helper:

```rust
fn to_range(r: rowan::TextRange) -> std::ops::Range<usize> {
    let start: usize = r.start().into();
    let end: usize = r.end().into();
    start..end
}
```//place this above `pub fn project` alongside the other module-level helpers.

- [ ] **Step 1: Root node (`walk`, line ~44)**

`root`'s `text_range` is the whole document — `syntax.text_range()` (the
`syntax: &SyntaxNode` parameter already passed to `walk`). `key_text_range`
is `None` (Root is keyless, matching its existing `key_sign: KeySign::None`).

```rust
let mut root = Node {
    key: filename.to_string(),
    path: Vec::new(),
    kind: NodeKind::Root,
    children: Vec::new(),
    value: None,
    format: Format::Plain,
    key_sign: KeySign::None,
    trailing_comment: None,
    read_only: false,
    text_range: to_range(syntax.text_range()),
    key_text_range: None,
};
```

- [ ] **Step 2: AoT group node (line ~159) and AoT entry node (line ~181)**

The AoT group node (`kind: NodeKind::ArrayOfTables`) is itself a synthetic
container (no single backing `SyntaxNode`, like a Dotted table) — apply the
same ADR 0006 policy: anchor at the entry that created it, `n` (the
`TABLE_ARRAY_HEADER` node already in scope as `n: SyntaxNode` in this match
arm). Its `key_text_range` comes from the header's `KEY` child, via a small
helper mirroring `header_path`/`header_key_signs`:

```rust
fn header_key_range(header: &SyntaxNode) -> Option<std::ops::Range<usize>> {
    header
        .children()
        .find(|c| c.kind() == SyntaxKind::KEY)
        .map(|k| to_range(k.text_range()))
}
```

```rust
let aot = Node {
    key: aot_key,
    path: path.clone(),
    kind: NodeKind::ArrayOfTables,
    children: Vec::new(),
    value: None,
    format: Format::Plain,
    key_sign: signs.last().copied().unwrap_or(KeySign::None),
    trailing_comment: None,
    read_only: false,
    text_range: to_range(n.text_range()),
    key_text_range: header_key_range(&n),
};
```

The per-entry `Table` node (`format!("[{ordinal}]")`, keyless — matches its
existing `key_sign: KeySign::None`) also anchors at `n` (the same
`TABLE_ARRAY_HEADER`), with no `key_text_range`:

```rust
aot.children.push(Node {
    key: format!("[{ordinal}]"),
    path: entry_path.clone(),
    kind: NodeKind::Table,
    children: Vec::new(),
    value: None,
    format: Format::Plain,
    key_sign: KeySign::None,
    trailing_comment: entry_trailing_comment(&n),
    read_only: false,
    text_range: to_range(n.text_range()),
    key_text_range: None,
});
```

- [ ] **Step 3: Standalone Comment nodes (`flush_comments`, line ~220)**

The loop already holds `tok: SyntaxToken` (the first `COMMENT` token of the
block). Comments are keyless:

```rust
container.children.push(Node {
    key: text.clone(),
    path: path.clone(),
    kind: NodeKind::Comment(text.clone()),
    children: Vec::new(),
    value: Some(text),
    format: Format::Plain,
    key_sign: KeySign::None,
    trailing_comment: None,
    read_only: false,
    text_range: to_range(tok.text_range()),
    key_text_range: None,
});
```

- [ ] **Step 4: `ensure_table_path`'s synthetic intermediate Table (line ~271)**

This is the **multi-segment dotted key chain** case from ADR 0006/spec Q6 —
`path[..=i]` is one segment of a `[x.a.b]` header (or of a dotted entry key).
`ensure_table_path` is called with `signs` aligned to `path`, but has no
per-segment token range parameter yet — thread one through, mirroring `signs`:
add a `key_ranges: &[Range<usize>]` parameter (same length/alignment as
`signs`), sourced by both call sites from a new `header_key_segment_ranges`
(for `[x.a]` headers) built the same way `key_segments`/`key_signs` already
walk a `KEY` node's child tokens — reuse that walk, this time reading each
token's `.text_range()` instead of its sign:

```rust
fn key_segment_ranges(key: &SyntaxNode) -> Vec<std::ops::Range<usize>> {
    key.children_with_tokens()
        .filter_map(|c| match c {
            NodeOrToken::Token(t) => match t.kind() {
                SyntaxKind::IDENT | SyntaxKind::IDENT_WITH_GLOB | SyntaxKind::STRING
                | SyntaxKind::STRING_LITERAL => Some(to_range(t.text_range())),
                _ => None,
            },
            NodeOrToken::Node(_) => None,
        })
        .collect()
}

fn header_key_ranges(header: &SyntaxNode) -> Vec<std::ops::Range<usize>> {
    header
        .children()
        .find(|c| c.kind() == SyntaxKind::KEY)
        .map(|k| key_segment_ranges(&k))
        .unwrap_or_default()
}
```

For the node's own `text_range` (the *whole* synthetic table, not just its
key segment), use the full header/entry `SyntaxNode`'s range at that
recursion depth — `ensure_table_path` doesn't currently carry that node, so
thread a `source: &SyntaxNode` parameter alongside `signs`/`key_ranges` (the
`TABLE_HEADER` or dotted `ENTRY` node the caller already has: `&n` at the
`TABLE_HEADER`/`TABLE_ARRAY_HEADER` call site in `walk`, and the dotted
entry's `SyntaxNode` at its call site — check both call sites of
`ensure_table_path` in this file and pass their local `SyntaxNode` through):

```rust
fn ensure_table_path(
    root: &mut Node,
    path: &[Seg],
    signs: &[KeySign],
    key_ranges: &[std::ops::Range<usize>],
    source: &SyntaxNode,
) {
    for i in 0..path.len() {
        if node_at(root, &path[..=i]).is_some() {
            continue;
        }
        let key = match &path[i] {
            Seg::Key(k) => k.clone(),
            Seg::Index(_) => return,
        };
        let node = Node {
            key,
            path: path[..=i].to_vec(),
            kind: NodeKind::Table,
            children: Vec::new(),
            value: None,
            format: Format::Scope,
            key_sign: signs.get(i).copied().unwrap_or(KeySign::None),
            trailing_comment: None,
            read_only: false,
            text_range: to_range(source.text_range()),
            key_text_range: key_ranges.get(i).cloned(),
        };
        append_child(root, &path[..i], node);
    }
}
```

Update both call sites accordingly (pass `&header_key_ranges(&n)` / the
dotted-entry equivalent, and `&n`/the entry node).

- [ ] **Step 5: `project_entry`'s synthetic dotted-chain Table (line ~488) — the Dotted-table ADR 0006 anchor**

This is the `Format::Dotted` synthetic-table case ADR 0006 governs directly.
`project_entry` walks a dotted entry's `KEY` segments to build the chain of
synthetic Tables; per ADR 0006, each synthetic Table anchors at its **first
member's** own range — read the existing `key_segments(k)`/`key_signs(k)`
call site at line ~397 and reuse the same `k: SyntaxNode` (the `KEY` node) to
build `key_segment_ranges(k)` (from Step 4) for per-segment `key_text_range`;
the *whole-node* `text_range` for each synthetic segment is `entry.text_range()`
(the whole `ENTRY` node, which — for a truly first-definition segment — is
already "this entry" per the ADR; consolidating rewrites are handled
elsewhere and don't change projection). Apply the same pattern as Step 4's
`node` literal — `text_range: to_range(entry.text_range())`, `key_text_range:
ranges.get(i).cloned()`.

- [ ] **Step 6: `project_value_node`, `project_array`, `project_inline`, and the two remaining literals — apply the identical pattern**

For every remaining `Node { .. }` literal the compiler still flags in this
file (`project_value_node`'s scalar/array/inline-table arms, `project_array`'s
element Comment node, `project_inline`'s member handling, the shared `leaf`/
`branch` private helpers at lines ~758/~772): each literal already has the
originating `SyntaxNode`/`SyntaxToken` in scope (that's what's being
projected into that `Node`) — set `text_range: to_range(<that node/token>.text_range())`
and `key_text_range` from the corresponding key node's range (via
`key_segment_ranges`, taking the sole entry when there's exactly one segment,
or `None` for keyless nodes — array elements, comments). Run `cargo build -p
confy-core` after each literal to confirm the compiler error at that site is
resolved; do not move to Task 3 until this file compiles clean.

- [ ] **Step 7: Add span assertions to existing tests**

`cst_project.rs`'s `#[cfg(test)] mod tests` (or a new `mod span_tests` in the
same file) — add cases slicing `text_range`/`key_text_range` out of the
source string and asserting the substring:

```rust
#[test]
fn text_range_slices_expected_substring() {
    let src = "[server]\nhost = \"localhost\"\nport = 8080\n";
    let tree = project(&parse(src).unwrap(), "f.toml");
    let server = &tree.root.children[0];
    assert_eq!(&src[server.text_range.clone()], "[server]");
    let port = &server.children[1];
    assert_eq!(&src[port.text_range.clone()], "port = 8080");
    assert_eq!(
        port.key_text_range.clone().map(|r| &src[r]),
        Some("port")
    );
}

#[test]
fn dotted_table_text_range_anchors_first_member() {
    let src = "a.b = 1\nx = 0\na.c = 2\n";
    let tree = project(&parse(src).unwrap(), "f.toml");
    let a = &tree.root.children[0];
    // ADR 0006: anchors at the FIRST member (`a.b = 1`), not an envelope.
    assert_eq!(&src[a.text_range.clone()], "a.b = 1");
}
```

(Adjust the exact `parse`/`project` call signature to match this file's
existing test helpers — check the `mod tests` block's existing setup before
writing these.)

- [ ] **Step 8: Run core tests**

Run: `cargo test -p confy-core cst_project`
Expected: PASS, including the two new tests above.

- [ ] **Step 9: Commit**

```bash
cd /Volumes/Home/Users/wen/repos/confy
git add crates/confy-core/src/model/cst_project.rs
git commit -m "feat(core): populate Node.text_range/key_text_range in the TOML projection"
```

---

## Task 3: `confy-core` — populate spans in the JSON backend (`json/project.rs`)

**Files:**
- Modify: `crates/confy-core/src/model/json/project.rs` (every `Node { .. }` literal; `~7` sites)

**Interfaces:**
- Consumes: Task 1's `Node` fields; the same `to_range(rowan::TextRange) -> Range<usize>` helper pattern as Task 2 (JSON's rowan types are `crate::model::json::syntax::{SyntaxNode, SyntaxToken}` — add a local copy of `to_range`, or hoist it into a shared `crate::model::text_range` module and import it from all three backends — prefer the shared module since JSON/YAML/TOML all need the identical byte-offset extraction and this avoids `TextRange`-type-inference duplication across three near-identical private fns).

JSON has no scattered/dotted-table complication — every `Node` here has a
single backing `SyntaxNode`/`SyntaxToken` (`value`/`container`/`tok` already
in scope at each literal in `build_value_node`, `walk_container_tokens`'s
Comment-node pushes, and `root`'s own construction in `walk`).

- [ ] **Step 1: Root node (`walk`, line ~28)** — `text_range: to_range(syntax.text_range())`, `key_text_range: None`.

- [ ] **Step 2: `build_value_node`'s four arms (lines ~202, ~223, ~239, ~254, ~266)**

Each arm already has the value's originating node/token in scope
(`container`, `tok`, or falls through to `value`/`value.text_range()` for the
`None`/unexpected-node arms). For `key_text_range`: `build_value_node` already
receives `key: &str` but not the key's own `SyntaxNode` — check its callers
(`walk_container_tokens`) for whether the `KEY` node is available there; if
so, thread a `key_range: Option<std::ops::Range<usize>>` parameter through
(mirroring how `key: &str` is already threaded) sourced from that `KEY`
node's `.text_range()`; array elements (no `KEY` node) pass `None`.

- [ ] **Step 3: Standalone/block Comment node literals (lines ~321, ~375)** — `text_range` from the `tok`/`tok.clone()` already in scope at each site; `key_text_range: None`.

- [ ] **Step 4: Implicit-null literal (line ~410)** — same pattern, sourced from whatever `SyntaxNode`/token that arm already holds for the member/element being defaulted to null.

- [ ] **Step 5: Run `cargo build -p confy-core` until this file compiles clean**, same discipline as Task 2 Step 6.

- [ ] **Step 6: Mirror Task 2 Step 7's span-assertion tests for JSON**

```rust
#[test]
fn text_range_slices_expected_substring() {
    let src = r#"{"server": {"host": "localhost", "port": 8080}}"#;
    let tree = project(&parse(src).unwrap(), "f.json");
    let server = &tree.root.children[0];
    let port = &server.children[1];
    assert_eq!(&src[port.text_range.clone()], "8080");
    assert_eq!(port.key_text_range.clone().map(|r| &src[r]), Some(r#""port""#));
}
```

(Match this file's actual existing `parse`/`project` test-helper signatures —
read `mod tests` at the bottom of this file before writing.)

- [ ] **Step 7: Run tests**

Run: `cargo test -p confy-core json::project`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
cd /Volumes/Home/Users/wen/repos/confy
git add crates/confy-core/src/model/json/project.rs crates/confy-core/src/model/ # include shared text_range helper if hoisted
git commit -m "feat(core): populate Node.text_range/key_text_range in the JSON projection"
```

---

## Task 4: `confy-core` — populate spans in the YAML backend (`yaml/project.rs`)

**Files:**
- Modify: `crates/confy-core/src/model/yaml/project.rs` (every `Node { .. }` literal; `~14` sites)

**Interfaces:**
- Consumes: Task 1's `Node` fields; the shared `to_range` helper (or YAML-local copy, matching Task 3's decision).

Same mechanical pattern as Tasks 2-3: every literal already has its
originating `SyntaxNode`/`SyntaxToken` in scope (`node`, `tok`, `child`,
`inner_node`, `value`, `entry`). No dotted-table complication (YAML has no
`[T/D]` equivalent — `CONTEXT.md`/the spec's Non-goals already exclude this
concern for YAML). The one YAML-specific case worth flagging explicitly:

- [ ] **Step 1: `root` (`walk`, line ~42)** — `to_range(syntax.text_range())`, `None`.

- [ ] **Step 2: Comment-node literals (lines ~80, ~172, ~191, ~243)** — from the local `tok`/`node` already in scope; `key_text_range: None`. Note line ~191/~243 are `Target::Opaque` (read-only) nodes — per spec Q4, opaque nodes get no special outline treatment, just their real `text_range` like any other node.

- [ ] **Step 3: `build_value_node_from_child`'s five arms (lines ~452, ~467, ~483, ~499, ~519, ~531) and the implicit-null literals (lines ~377, ~419)**

Each already has `child`/`key_label` (or an implicit-null fallback) in scope;
apply `text_range: to_range(child.text_range())` (or the appropriate
already-in-scope node for the null-fallback arms). `key_text_range`: check
whether the calling `project_map_entry`/`project_seq_entry` has the `KEY`
token in scope (it does — see `key_name_and_sign(entry)` at line ~760) and
thread a `key_range: Option<Range<usize>>` through the same way, sourced from
that KEY token's `.text_range()` (`None` for `SEQ_ENTRY`, which is keyless).

- [ ] **Step 4: `build_value_node`'s arms (lines ~561, ~577, ~606, ~620, ~652, ~666, ~678, ~692)** — same pattern, sourced from `inner_node`/`tok`/`value` already in scope at each arm.

- [ ] **Step 5: Run `cargo build -p confy-core` until this file compiles clean.**

- [ ] **Step 6: Mirror Task 2 Step 7's span-assertion tests for YAML**

```rust
#[test]
fn text_range_slices_expected_substring() {
    let src = "server:\n  host: localhost\n  port: 8080\n";
    let tree = project(&parse(src).unwrap(), "f.yaml");
    let server = &tree.root.children[0];
    let port = &server.children[1];
    assert_eq!(&src[port.text_range.clone()], "8080");
    assert_eq!(port.key_text_range.clone().map(|r| &src[r]), Some("port"));
}
```

(Match this file's actual existing test-helper signatures — read `mod tests`
before writing.)

- [ ] **Step 7: Run every core test (all three backends must still pass together)**

Run: `cargo test -p confy-core`
Expected: PASS, no regressions in any of the three backends' existing test suites.

- [ ] **Step 8: Run full workspace gate**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean.

- [ ] **Step 9: Commit**

```bash
cd /Volumes/Home/Users/wen/repos/confy
git add crates/confy-core/src/model/yaml/project.rs
git commit -m "feat(core): populate Node.text_range/key_text_range in the YAML projection"
```

---

## Task 5: `confy-ffi` — `OutlineNode` wire type + `ConfySession::outline()`

**Files:**
- Modify: `crates/confy-core/src/session/view.rs` (or a new small module — follow this file's existing `ChildView` placement convention) to add `OutlineNode`
- Modify: `crates/confy-core/src/session/session.rs` (near `children_of`, line ~284) to add `Session::outline`
- Modify: `crates/confy-ffi/src/lib.rs` (near `children`, line ~119) to add `ConfySession::outline`
- Modify: `crates/confy-ffi/functional_smoke.mjs` (append before the final `console.log(failures === 0 …)` line)

**Interfaces:**
- Consumes: Task 4's fully-populated `Node.text_range`/`key_text_range`; `crate::session::status_fmt::node_type_label` (existing helper, same vocabulary as `ChildView::type_label`).
- Produces: `OutlineNode { key, path, type_label, value, text_range: (u32, u32), key_text_range: Option<(u32, u32)>, children }`; `Session::outline(&self) -> Vec<OutlineNode>`; FFI `ConfySession.outline() -> OutlineNode[]` (JS).

- [ ] **Step 1: Add `OutlineNode` to `crates/confy-core/src/session/view.rs`**

```rust
/// Read-only outline transport — deliberately separate from the internal
/// `Node`/`NodeKind` wire shape, matching the existing `ChildView`/
/// `KindOptionView` convention of small dedicated FFI-boundary types.
/// Consumed by editor Outline/breadcrumb integrations (VS Code
/// `DocumentSymbolProvider`, spec `docs/superpowers/specs/2026-08-20-vscode-outline-provider-design.md`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlineNode {
    pub key: String,
    pub path: Path,
    /// Same vocabulary as `ViewRow::type_label`/`ChildView::type_label`.
    pub type_label: String,
    /// Scalar leaves only — carried through for the editor's `detail` field.
    pub value: Option<String>,
    pub text_range: (u32, u32),
    pub key_text_range: Option<(u32, u32)>,
    pub children: Vec<OutlineNode>,
}
```

- [ ] **Step 2: Add `Session::outline` in `session.rs`**

Place it next to `children_of` (line ~284). It walks the *whole* tree from
Root (unlike `children_of`, which returns one node's immediate children,
independent of expansion state), skipping the Root wrapper itself (spec: "mirrors
VS Code's own JSON outline, which does not synthesize a whole-document
symbol") and Comment nodes:

```rust
/// Read-only outline tree for editor Outline/breadcrumb integrations —
/// the whole document, independent of `Session`'s own cursor/expansion
/// state. Root itself is not included (its children are returned
/// directly); `Comment` nodes are omitted.
pub fn outline(&self) -> Vec<OutlineNode> {
    fn convert(n: &Node) -> Option<OutlineNode> {
        if matches!(n.kind, NodeKind::Comment(_)) {
            return None;
        }
        Some(OutlineNode {
            key: n.key.clone(),
            path: n.path.clone(),
            type_label: node_type_label(&n.kind),
            value: if n.is_leaf() { n.value.clone() } else { None },
            text_range: (n.text_range.start as u32, n.text_range.end as u32),
            key_text_range: n
                .key_text_range
                .as_ref()
                .map(|r| (r.start as u32, r.end as u32)),
            children: n.children.iter().filter_map(convert).collect(),
        })
    }
    self.tree.root.children.iter().filter_map(convert).collect()
}
```

Check this file's existing imports at the top for `Node`/`NodeKind` and
`node_type_label` (already imported for `children_of`'s use at line ~290) —
reuse them, don't re-import.

- [ ] **Step 3: Add `ConfySession::outline` in `crates/confy-ffi/src/lib.rs`**

Place it next to `children` (line ~119):

```rust
/// Read-only symbol tree for editor Outline/breadcrumb integrations
/// (`OutlineNode[]`), independent of cursor/expansion state.
#[wasm_bindgen]
pub fn outline(&self) -> Result<JsValue, JsValue> {
    to_value(&self.session.outline()).map_err(js_serde_error)
}
```

(Match this file's existing `#[wasm_bindgen] impl ConfySession { .. }` block
placement and the exact `js_serde_error`/`to_value` helper names already used
by `children`.)

- [ ] **Step 4: Rebuild the wasm package**

```bash
cd /Volumes/Home/Users/wen/repos/confy/crates/confy-ffi
wasm-pack build --target web
```
Expected: builds clean, `pkg/confy_ffi.js`/`pkg/confy_ffi_bg.wasm` updated.

- [ ] **Step 5: Add `functional_smoke.mjs` checks**

Insert before the final `console.log(failures === 0 …)` line, following this
file's existing numbered-section convention (see the existing `// ---- N. …
----` comments):

```js
// ---- N. Outline: whole-tree read-only symbol export ----
const outlineSrc = `[server]\nhost = "localhost"\nport = 8080\n`;
const outlineSession = new ConfySession(outlineSrc, "toml");
const outline = outlineSession.outline();
check("outline: top-level has one entry (server)", outline.length === 1, JSON.stringify(outline.map(o => o.key)));
const serverNode = outline[0];
check("outline: server type_label=table", serverNode.type_label === "table", serverNode.type_label);
check("outline: server has 2 children", serverNode.children.length === 2, serverNode.children.length);
const portNode = serverNode.children.find(c => c.key === "port");
check("outline: port value carried for detail", portNode.value === "8080", portNode.value);
check(
  "outline: port text_range slices the right substring",
  outlineSrc.slice(portNode.text_range[0], portNode.text_range[1]) === "port = 8080",
  JSON.stringify(portNode.text_range),
);
check(
  "outline: port key_text_range slices \"port\"",
  outlineSrc.slice(portNode.key_text_range[0], portNode.key_text_range[1]) === "port",
  JSON.stringify(portNode.key_text_range),
);
```

- [ ] **Step 6: Run the smoke suite**

```bash
cd /Volumes/Home/Users/wen/repos/confy/crates/confy-ffi
node functional_smoke.mjs
```
Expected: `ALL FUNCTIONAL CHECKS PASSED`, including the new outline checks (92 + 6 = 98).

- [ ] **Step 7: Commit**

```bash
cd /Volumes/Home/Users/wen/repos/confy
git add crates/confy-core/src/session/view.rs crates/confy-core/src/session/session.rs crates/confy-ffi/src/lib.rs crates/confy-ffi/functional_smoke.mjs crates/confy-ffi/pkg
git commit -m "feat(ffi): expose ConfySession::outline() read-only symbol tree"
```
(If `crates/confy-ffi/pkg` is gitignored, drop it from the add — check `git status` first, per repo convention noted in prior plans.)

---

## Task 6: `editors/vscode` — `outlineProvider.ts` + registration

**Files:**
- Create: `editors/vscode/src/outlineProvider.ts`
- Create: `editors/vscode/src/formatFromName.ts` (shared helper extracted from `editorProvider.ts:7-11`)
- Create: `editors/vscode/src/byteToPosition.ts` (pure UTF-8-byte → `vscode.Position` helper + its unit test)
- Create: `editors/vscode/src/byteToPosition.test.ts` (or `.spec.ts` — match whatever `node:test` convention the repo's other `.spec.mjs` files use; this is TS so a small `tsx`/compiled runner may be needed — check `package.json` scripts for precedent before choosing the runner)
- Modify: `editors/vscode/src/editorProvider.ts:7-11` (replace local `formatFromName` with the shared import)
- Modify: `editors/vscode/src/extension.ts` (register the provider in `activate`)
- Modify: `editors/vscode/package.json` (add `activationEvents`; verified absent today — VS Code cannot auto-infer activation for a runtime-only `registerDocumentSymbolProvider` call, unlike the declarative `contributes.customEditors` entries)

**Interfaces:**
- Consumes: Task 5's `ConfySession.outline() -> OutlineNode[]` (fields: `key, path, type_label, value, text_range: [number, number], key_text_range: [number, number] | undefined, children`).
- Produces: `formatFromName(name: string): ConfigFormat` (moved, same signature as today's private copy); `byteOffsetsToUtf16Range(text: string, startByte: number, endByte: number): vscode.Range` (pure, testable).

- [ ] **Step 1: Extract `formatFromName` into its own module**

`editors/vscode/src/formatFromName.ts`:
```ts
import type { ConfigFormat } from "../../../web/vscode-protocol.js";

// Mirrors web/host-io.ts's formatFromName (same folding: .jsonc→json,
// .yml→yaml); duplicated because the extension host must not import web
// internals, but the return type is the one shared ConfigFormat.
export function formatFromName(name: string): ConfigFormat {
  if (name.endsWith(".json") || name.endsWith(".jsonc")) return "json";
  if (name.endsWith(".yaml") || name.endsWith(".yml")) return "yaml";
  return "toml";
}
```

Update `editorProvider.ts:7-11`: delete the local function, add
`import { formatFromName } from "./formatFromName.js";` at the top.

- [ ] **Step 2: Write the byte-offset → `vscode.Position` helper + its test**

`editors/vscode/src/byteToPosition.ts`:
```ts
import * as vscode from "vscode";

// rowan's TextRange (and therefore OutlineNode.text_range/key_text_range) is
// UTF-8 byte offsets; vscode.TextDocument.positionAt expects UTF-16 code-unit
// offsets. This walks `text` once, converting a UTF-8 byte offset target into
// a UTF-16 code-unit offset — a single linear pass shared by every symbol in
// one document's outline() call (call once per byte offset needed; callers
// batch-sort offsets ascending for one shared forward pass if profiling ever
// shows this matters — not needed at config-file scale today).
export function utf8ByteOffsetToUtf16Offset(text: string, byteOffset: number): number {
  let bytes = 0;
  for (let i = 0; i < text.length; i++) {
    if (bytes >= byteOffset) return i;
    const code = text.codePointAt(i)!;
    bytes += utf8ByteLength(code);
    if (code > 0xffff) i++; // surrogate pair consumes two UTF-16 units
  }
  return text.length;
}

function utf8ByteLength(codePoint: number): number {
  if (codePoint <= 0x7f) return 1;
  if (codePoint <= 0x7ff) return 2;
  if (codePoint <= 0xffff) return 3;
  return 4;
}

export function byteOffsetsToRange(
  document: vscode.TextDocument,
  startByte: number,
  endByte: number,
): vscode.Range {
  const text = document.getText();
  const start = document.positionAt(utf8ByteOffsetToUtf16Offset(text, startByte));
  const end = document.positionAt(utf8ByteOffsetToUtf16Offset(text, endByte));
  return new vscode.Range(start, end);
}
```

`editors/vscode/src/byteToPosition.test.ts` — check `package.json`'s
`scripts` and any existing `.spec.mjs`/test runner in this directory first
(there is none yet per the spec's Testing section); if none exists, add a
minimal `node:test` runner consistent with the repo's plain-`node:assert`
convention used by `crates/confy-ffi/functional_smoke.mjs` and
`web/toolbar-fold.spec.mjs` — no new test framework dependency:

```ts
import { test } from "node:test";
import assert from "node:assert";
import { utf8ByteOffsetToUtf16Offset } from "./byteToPosition.js";

test("ASCII: byte offset equals UTF-16 offset", () => {
  assert.strictEqual(utf8ByteOffsetToUtf16Offset("port = 8080", 7), 7);
});

test("CJK: multi-byte UTF-8 char counted as 1 UTF-16 unit", () => {
  // "鍵" is 3 UTF-8 bytes, 1 UTF-16 code unit.
  const text = "鍵 = 1";
  assert.strictEqual(utf8ByteOffsetToUtf16Offset(text, 3), 1); // right after 鍵
  assert.strictEqual(utf8ByteOffsetToUtf16Offset(text, 4), 2); // right after the space
});

test("emoji: astral char counted as 2 UTF-16 units (surrogate pair)", () => {
  // "😀" is 4 UTF-8 bytes, 2 UTF-16 code units (surrogate pair).
  const text = "😀x";
  assert.strictEqual(utf8ByteOffsetToUtf16Offset(text, 4), 2); // right after 😀
});
```

- [ ] **Step 3: Run the new unit test**

```bash
cd /Volumes/Home/Users/wen/repos/confy/editors/vscode
npx tsc --noEmit && node --experimental-strip-types src/byteToPosition.test.ts
```
Expected: all 3 assertions pass (adjust the run command to whatever
`tsc`/`node` version combination this repo's `engines`/`devDependencies`
actually support — Node 18/20 needs `ts-node`/`tsx` instead of
`--experimental-strip-types`; check `editors/vscode/package.json`
`devDependencies` and add the minimal runner dependency if genuinely needed).

- [ ] **Step 4: Write `outlineProvider.ts`**

```ts
import * as vscode from "vscode";
import { readFileSync } from "node:fs";
import { formatFromName } from "./formatFromName.js";
import { byteOffsetsToRange } from "./byteToPosition.js";

interface OutlineNode {
  key: string;
  path: unknown;
  type_label: string;
  value: string | null;
  text_range: [number, number];
  key_text_range: [number, number] | undefined;
  children: OutlineNode[];
}

interface ConfySessionCtor {
  new (text: string, format: string): { outline(): OutlineNode[] };
}

let ffiInit: Promise<ConfySessionCtor> | undefined;

// Loading the wasm in the extension host (Node.js), not the webview: the
// generated `--target web` glue only calls `fetch()` when `init()` receives a
// string/URL/Request; passing raw bytes makes it call
// `WebAssembly.instantiate(bytes, imports)` directly — identical API in Node
// and the browser (confirmed against the generated confy_ffi.js). Module-level
// singleton: first call wins, no per-request re-init.
async function loadConfySession(context: vscode.ExtensionContext): Promise<ConfySessionCtor> {
  if (!ffiInit) {
    ffiInit = (async () => {
      const bytes = readFileSync(
        vscode.Uri.joinPath(context.extensionUri, "media/pkg/confy_ffi_bg.wasm").fsPath,
      );
      const ffi = await import("../media/pkg/confy_ffi.js");
      await ffi.default(bytes);
      return ffi.ConfySession as unknown as ConfySessionCtor;
    })();
  }
  return ffiInit;
}

const KIND_MAP: Record<string, vscode.SymbolKind> = {
  table: vscode.SymbolKind.Object,
  "inline table": vscode.SymbolKind.Object,
  array: vscode.SymbolKind.Array,
  "array of tables": vscode.SymbolKind.Array,
  string: vscode.SymbolKind.String,
  integer: vscode.SymbolKind.Number,
  float: vscode.SymbolKind.Number,
  bool: vscode.SymbolKind.Boolean,
  null: vscode.SymbolKind.Null,
};

function symbolKindFor(typeLabel: string): vscode.SymbolKind {
  return KIND_MAP[typeLabel] ?? vscode.SymbolKind.Constant; // datetime variants etc.
}

function toDocumentSymbol(node: OutlineNode, document: vscode.TextDocument): vscode.DocumentSymbol {
  const range = byteOffsetsToRange(document, node.text_range[0], node.text_range[1]);
  const selectionRange = node.key_text_range
    ? byteOffsetsToRange(document, node.key_text_range[0], node.key_text_range[1])
    : range;
  const detail = node.value ?? ""; // scalar leaves only (spec Q3); containers stay empty.
  const symbol = new vscode.DocumentSymbol(
    node.key,
    detail,
    symbolKindFor(node.type_label),
    range,
    selectionRange,
  );
  symbol.children = node.children.map((c) => toDocumentSymbol(c, document));
  return symbol;
}

export class ConfyOutlineProvider implements vscode.DocumentSymbolProvider {
  constructor(private readonly context: vscode.ExtensionContext) {}

  async provideDocumentSymbols(
    document: vscode.TextDocument,
    token: vscode.CancellationToken,
  ): Promise<vscode.DocumentSymbol[]> {
    try {
      const ConfySession = await loadConfySession(this.context);
      if (token.isCancellationRequested) return [];
      const format = formatFromName(document.fileName);
      const session = new ConfySession(document.getText(), format);
      const outline = session.outline();
      if (token.isCancellationRequested) return [];
      return outline.map((n) => toDocumentSymbol(n, document));
    } catch {
      // Never throw into VS Code's UI — an empty Outline is an acceptable
      // degraded state for a read-only convenience feature (e.g. mid-edit
      // invalid document, or wasm init failure).
      return [];
    }
  }
}
```

- [ ] **Step 5: Register the provider in `extension.ts`**

```ts
import { ConfyOutlineProvider } from "./outlineProvider.js";
```

Inside `activate`, alongside the existing `context.subscriptions.push(...)`:
```ts
  context.subscriptions.push(
    vscode.languages.registerDocumentSymbolProvider(
      [{ pattern: "**/*.toml" }, { pattern: "**/*.yaml" }, { pattern: "**/*.yml" }],
      new ConfyOutlineProvider(context),
    ),
  );
```
(Add this as its own `context.subscriptions.push(...)` call, or fold into the
existing one — match this file's existing style of one `push` call per
logical group.)

- [ ] **Step 6: Add `activationEvents` to `package.json`**

Verified absent today. `registerDocumentSymbolProvider` is a pure runtime
call with no declarative `contributes` equivalent, so VS Code cannot
auto-infer when to activate the extension for it. Add, right after
`"engines"`:
```json
  "activationEvents": ["onStartupFinished"],
```

- [ ] **Step 7: Rebuild + typecheck**

```bash
cd /Volumes/Home/Users/wen/repos/confy/editors/vscode
npx tsc --noEmit
```
Expected: 0 errors.

```bash
node build.mjs
```
Expected: `built: dist/extension.js + media/` (per the plan's Global
Constraints / prior plans' memory: esbuild deadlocks under `/Volumes/Home` —
run this from a scratchpad copy of the repo if it hangs, then copy
`dist/`/`media/` back).

- [ ] **Step 8: Manual verification (user, not automated)**

Ask the user to open the Extension Development Host (`F5` in
`editors/vscode`), open a real `.toml` file and a real `.yaml` file in VS
Code's **native** text editor (not confy's own custom editor tab —
`confy.reopenAsText` / `confy.openTextBeside`), and confirm:
- The Outline panel (Explorer sidebar) populates with the file's structure.
- `Cmd+Shift+O` / `Ctrl+Shift+O` shows the same symbols, jump-to-line works.
- Breadcrumbs (top of the editor, if enabled) show the nesting path.
- A malformed/mid-edit document does not error — Outline just goes empty.
- Confirm confy's own custom editor tab (`confy.editor`) is unaffected (no
  Outline/breadcrumbs there — expected, per the spec's Platform constraint).

- [ ] **Step 9: Update `CHANGELOG.md` + `VSCODE.md`**

Append an `Unreleased Update` entry to `CHANGELOG.md` (timestamp + description
matching the commit message). Add a short note to `VSCODE.md`'s feature list
describing the new Outline/breadcrumb support and its platform-constraint
scope (native text editor only, not confy's own custom editor tab).

- [ ] **Step 10: Commit**

```bash
cd /Volumes/Home/Users/wen/repos/confy
git add editors/vscode/src/outlineProvider.ts editors/vscode/src/formatFromName.ts \
  editors/vscode/src/byteToPosition.ts editors/vscode/src/byteToPosition.test.ts \
  editors/vscode/src/editorProvider.ts editors/vscode/src/extension.ts \
  editors/vscode/package.json editors/vscode/dist editors/vscode/media \
  CHANGELOG.md VSCODE.md
git commit -m "feat(vscode): register DocumentSymbolProvider for Outline/breadcrumbs on native TOML/YAML editors"
```
(Check `git status` first — if `dist/`/`media/` are gitignored build output,
drop them from the add, matching this repo's established pattern for
`crates/confy-ffi/pkg`.)

---

## Final Verification (whole plan)

- [ ] `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` — clean.
- [ ] `cd crates/confy-ffi && node functional_smoke.mjs` — `ALL FUNCTIONAL CHECKS PASSED` (98/98).
- [ ] `cd editors/vscode && npx tsc --noEmit` — 0 errors.
- [ ] `cd web && npx tsc --noEmit` — 0 errors (unaffected; confirms no accidental cross-module breakage).
- [ ] User completes the Task 6 Step 8 manual Extension Development Host pass and confirms Outline/breadcrumbs work on real TOML and YAML files.
