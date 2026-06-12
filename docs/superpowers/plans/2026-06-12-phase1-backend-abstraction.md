# Phase 1: Backend Abstraction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the three TOML leaks between the TUI and the document layer so JSON/YAML backends (Phases 2–3) can plug in — no new format ships, TOML behavior is byte-for-byte unchanged.

**Architecture:** Introduce an `AnyDocument` enum wrapper implementing `ConfigDocument` by match-delegation; rename the `toml:` fragment fields format-neutral; move the `K` kind-switch option list behind a `kind_options` trait method; add `DocFormat`/`comment_prefix` facets to the trait; extend CLI detection to recognize (and politely bail on) `.json`/`.yaml`.

**Tech Stack:** Rust, existing crates only (no new dependencies).

**Spec:** `docs/superpowers/specs/2026-06-12-multiformat-backends-design.md` §Phase 1.

**Acceptance gate:** the entire existing test suite passes with no behavioral change — only mechanical renames (`toml:` → `fragment:`) and call-site moves are allowed to touch tests. `cargo test && cargo clippy -- -D warnings && cargo fmt --check` clean after every task.

**Deviations from spec (simplifications, same intent):**
- `kind_options` returns `Vec<(String, KindTarget)>` — label + target — not
  `Vec<KindTarget>`. The labels are format-specific notation names (`"hex  0x…"`), so
  they belong to the backend; keeping them in the TUI would re-create the leak this
  phase removes.
- Spec 1.5's "`cli::Format` grows variants" is implemented by **deleting** `cli::Format`
  and using `DocFormat` directly (one enum, DRY); detection lives in `model/any_doc.rs`.
- Spec 1.6's "per-help-row format mask" is implemented as `help_text(DocFormat)`
  returning a per-format string — whole-text switch instead of row masks; Phases 2–3
  just add their constants.

---

### Task 1: Rename `toml:` mutation fields to `fragment:`

**Files:**
- Modify: `src/model/document.rs:43-51` (Mutation::Insert/Replace fields)
- Modify: every construction/destructuring site (find with `rg -n 'toml:' src tests`)

- [ ] **Step 1: Rename the enum fields**

In `src/model/document.rs`:

```rust
    Insert {
        target: Target,
        fragment: String,
        on_collision: OnCollision,
    },
    Replace {
        path: Path,
        fragment: String,
    },
```

- [ ] **Step 2: Fix every use site**

Run: `rg -ln 'toml:' src tests` and rename the field at each `Mutation::Insert`/`Mutation::Replace` construction and pattern-match. Do NOT touch unrelated identifiers (e.g. file names, `--format toml`). Local variable names feeding the field may stay as-is.

- [ ] **Step 3: Also de-TOML the `Fragment` error display**

In `src/model/document.rs`, change:

```rust
    #[error("invalid fragment: {0}")]
    Fragment(String),
```

(The TOML backend already embeds context in the payload string; check with `rg -n 'Fragment\(' src` that no message becomes ambiguous — if a payload was relying on the prefix saying "TOML", prepend "TOML: " to that payload at the construction site in `cst_edit.rs`.)

Check the TUI: `rg -n '"invalid TOML fragment' src/tui` — if any test or status-line string asserts the old prefix, update it to the new one.

- [ ] **Step 4: Verify**

Run: `cargo test && cargo clippy -- -D warnings && cargo fmt`
Expected: PASS, no warnings.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "refactor(model): rename Mutation fragment fields toml: -> fragment:"
```

---

### Task 2: `DocFormat` + format facets on the trait

**Files:**
- Modify: `src/model/document.rs` (new enum + 3 trait methods)
- Modify: `src/model/cst_doc.rs` (impl)

- [ ] **Step 1: Write the failing test**

In `src/model/cst_doc.rs` tests module:

```rust
#[test]
fn toml_format_facets() {
    let f = tempfile::NamedTempFile::with_suffix(".toml").unwrap();
    std::fs::write(f.path(), "a = 1\n").unwrap();
    let doc = CstDocument::load(f.path()).unwrap();
    assert_eq!(doc.format(), DocFormat::Toml);
    assert_eq!(doc.comment_prefix(), "#");
    assert!(doc.supports_comments());
}
```

(Match the existing test-module import style in `cst_doc.rs`; if its tests build docs differently, follow that pattern.)

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test toml_format_facets`
Expected: compile FAIL — `DocFormat`/`format()` not defined.

- [ ] **Step 3: Implement**

In `src/model/document.rs`:

```rust
/// Which config syntax a document speaks. Backends report it via
/// [`ConfigDocument::format`]; the TUI uses it for the title bar, help text
/// and comment validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocFormat {
    Toml,
    Json,
    Yaml,
}
```

Add to the `ConfigDocument` trait:

```rust
    fn format(&self) -> DocFormat;
    /// Line-comment leader for this format ("#" / "//").
    fn comment_prefix(&self) -> &'static str;
    /// Whether authored comments are currently legal in this document
    /// (false only for a pure `.json` before the JSONC upgrade, Phase 2).
    fn supports_comments(&self) -> bool;
```

In `src/model/cst_doc.rs`'s `impl ConfigDocument for CstDocument`:

```rust
    fn format(&self) -> DocFormat {
        DocFormat::Toml
    }
    fn comment_prefix(&self) -> &'static str {
        "#"
    }
    fn supports_comments(&self) -> bool {
        true
    }
```

(import `DocFormat` in the existing `use` line). Re-export `DocFormat` wherever the
other document types are re-exported — check `src/model/mod.rs` / `src/lib.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test && cargo clippy -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(model): DocFormat + comment_prefix/supports_comments trait facets"
```

Note: this task only *introduces* the facets; the TUI's hard-coded `#` checks are
rewired in Task 6.

---

### Task 3: Move `node_at` lookup onto `NodeTree`

The kind-switch backend impl (Task 4) needs path→node lookup in the model layer;
today the helper lives in the TUI.

**Files:**
- Modify: `src/model/node.rs` (new method)
- Modify: `src/tui/app.rs` (the free fn `node_at` — find with `rg -n 'fn node_at' src/tui/app.rs` — becomes a thin call or is deleted with callers updated)

- [ ] **Step 1: Write the failing test**

In `src/model/node.rs` tests:

```rust
#[test]
fn node_at_resolves_paths() {
    let mut port = Node::leaf("port", NodeKind::Scalar(ScalarType::Integer));
    port.path = vec![Seg::Key("server".into()), Seg::Key("port".into())];
    let mut server = Node::branch("server", NodeKind::Table);
    server.path = vec![Seg::Key("server".into())];
    server.children = vec![port];
    let mut root = Node::branch("f.toml", NodeKind::Root);
    root.children = vec![server];
    let tree = NodeTree { root };

    assert!(tree.node_at(&[]).is_some_and(|n| n.key == "f.toml"));
    let p = vec![Seg::Key("server".into()), Seg::Key("port".into())];
    assert!(tree.node_at(&p).is_some_and(|n| n.key == "port"));
    assert!(tree.node_at(&[Seg::Key("nope".into())]).is_none());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test node_at_resolves_paths`
Expected: compile FAIL — no method `node_at`.

- [ ] **Step 3: Implement by moving the TUI helper**

Read the existing `fn node_at` in `src/tui/app.rs` and port its exact matching logic
to a method (it matches `n.path == path` by walking children — keep whatever it does,
do not redesign):

```rust
impl NodeTree {
    /// Resolve a projected path to its node; empty path is the Root.
    pub fn node_at(&self, path: &[Seg]) -> Option<&Node> {
        fn walk<'a>(n: &'a Node, path: &[Seg]) -> Option<&'a Node> {
            if n.path == path {
                return Some(n);
            }
            n.children.iter().find_map(|c| walk(c, path))
        }
        walk(&self.root, path)
    }
}
```

**If the TUI version's logic differs from the sketch above, the TUI version wins** —
move it verbatim. Then update `app.rs`: either delete the free fn and switch callers
(`rg -n 'node_at\(' src/tui`) to `self.tree.node_at(...)`, or keep a one-line wrapper
if call sites are many — choose the smaller diff.

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: PASS (all existing app tests too).

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "refactor(model): move node_at path lookup onto NodeTree"
```

---

### Task 4: Kind-switch options become a backend capability (`kind_options`)

**Files:**
- Modify: `src/model/document.rs` (trait method)
- Modify: `src/model/cst_doc.rs` (impl — receives the moved match)
- Modify: `src/tui/app.rs:551-660ish` (`open_kind_switch` shrinks to a query)

- [ ] **Step 1: Add the trait method**

In `src/model/document.rs`:

```rust
    /// The kinds/notations the node at `path` can convert to via
    /// [`Mutation::ConvertKind`], as `(label, target)` pairs — the current
    /// notation excluded. Empty when the node's kind cannot be switched.
    /// Labels are format-specific notation names rendered verbatim in the
    /// `K` popup. Positional legality (capture rules…) is still checked by
    /// `apply`; this lists only what is legal *by kind*.
    fn kind_options(&self, path: &[crate::model::node::Seg]) -> Vec<(String, KindTarget)>;
```

- [ ] **Step 2: Implement on `CstDocument` by moving the match**

In `src/model/cst_doc.rs`:

```rust
    fn kind_options(&self, path: &[Seg]) -> Vec<(String, KindTarget)> {
        let tree = self.project();
        let Some(node) = tree.node_at(path) else {
            return Vec::new();
        };
        // <— the entire `let options: Vec<(String, KT)> = match &node.kind { … }`
        //    block moves here VERBATIM from app.rs:564-649, with two edits:
        //    1. the final `_ => { self.error = …; return; }` arm becomes
        //       `_ => Vec::new(),`
        //    2. `path.last()` (used by the Table arm) reads the `path` argument.
        options
    }
```

Move the needed imports (`KindTarget as KT`, `ScalarType as ST`, `Format`, `NodeKind`,
`Seg`) into `cst_doc.rs`. The `ST::Float` arm reads `node.value` — it comes along
unchanged.

- [ ] **Step 3: Shrink `open_kind_switch`**

In `src/tui/app.rs`, `open_kind_switch` becomes:

```rust
    pub fn open_kind_switch(&mut self) {
        let Some(row) = self.rows.get(self.cursor) else {
            return;
        };
        let path = row.path.clone();
        let Some(doc) = &self.doc else {
            return;
        };
        let options = doc.kind_options(&path);
        if options.is_empty() {
            self.error = Some("this node's kind cannot be switched".into());
            return;
        }
        // …everything after the old match (popup-state setup, Mode::KindSwitch
        //  entry) stays exactly as it was.
    }
```

Read the current tail of the function first (after app.rs:649): if it had a separate
empty-options message (e.g. for bool scalars) distinct from the non-convertible arm,
fold both into the single `options.is_empty()` message above — the user-visible
string must keep covering both cases; pick the existing wording.

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: PASS — `open_kind_switch` tests at app.rs:2994/3010/3023 confirm the popup
contents are unchanged.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "refactor(model): K kind-switch options served by the backend (kind_options)"
```

---

### Task 5: `AnyDocument` enum wrapper; `App.doc` switches to it

**Files:**
- Create: `src/model/any_doc.rs`
- Modify: `src/model/mod.rs` (declare + re-export)
- Modify: `src/tui/app.rs:47,116-117` (+ the two test constructors at 2986/3604)
- Modify: `src/tui/mod.rs:19-20` (`run` loads `AnyDocument`)
- Modify: `src/tui/ui.rs` test constructors (7 sites, `rg -n 'CstDocument::load' src/tui/ui.rs`)

- [ ] **Step 1: Write the failing test**

In the new `src/model/any_doc.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::document::{ConfigDocument, DocFormat};

    #[test]
    fn any_document_delegates_to_toml() {
        let f = tempfile::NamedTempFile::with_suffix(".toml").unwrap();
        std::fs::write(f.path(), "a = 1\n").unwrap();
        let doc = AnyDocument::load(f.path()).unwrap();
        assert_eq!(doc.format(), DocFormat::Toml);
        assert_eq!(doc.serialize(), "a = 1\n");
        assert!(!doc.is_dirty());
    }

    #[test]
    fn load_rejects_unknown_extension() {
        let f = tempfile::NamedTempFile::with_suffix(".ini").unwrap();
        std::fs::write(f.path(), "a = 1\n").unwrap();
        assert!(AnyDocument::load(f.path()).is_err());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test any_document`
Expected: compile FAIL — `AnyDocument` not defined.

- [ ] **Step 3: Implement**

`src/model/any_doc.rs`:

```rust
//! Format dispatch: one enum wrapping every backend, so the TUI holds a single
//! concrete type and a new format is one more variant (spec §Phase 1.1).

use crate::model::cst_doc::CstDocument;
use crate::model::document::{ConfigDocument, DocFormat, KindTarget, MutateError, Mutation};
use crate::model::node::{NodeTree, Seg};
use std::path::Path as FsPath;

pub enum AnyDocument {
    Toml(CstDocument),
    // Json(JsonDocument)  — Phase 2
    // Yaml(YamlDocument)  — Phase 3
}

/// Format from the file extension. `None` = unrecognized.
pub fn detect_format(path: &FsPath) -> Option<DocFormat> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("toml") => Some(DocFormat::Toml),
        Some("json") | Some("jsonc") => Some(DocFormat::Json),
        Some("yaml") | Some("yml") => Some(DocFormat::Yaml),
        _ => None,
    }
}

impl AnyDocument {
    /// Load `path` as `format` (caller resolved detection/override).
    pub fn load_as(path: &FsPath, format: DocFormat) -> anyhow::Result<Self> {
        match format {
            DocFormat::Toml => Ok(Self::Toml(CstDocument::load(path)?)),
            DocFormat::Json => anyhow::bail!("JSON support is coming in a later release"),
            DocFormat::Yaml => anyhow::bail!("YAML support is coming in a later release"),
        }
    }
}

macro_rules! delegate {
    ($self:ident, $d:ident => $body:expr) => {
        match $self {
            AnyDocument::Toml($d) => $body,
        }
    };
}

impl ConfigDocument for AnyDocument {
    fn load(path: &FsPath) -> anyhow::Result<Self> {
        let fmt = detect_format(path)
            .ok_or_else(|| anyhow::anyhow!("unrecognized config format: {}", path.display()))?;
        Self::load_as(path, fmt)
    }
    fn project(&self) -> NodeTree {
        delegate!(self, d => d.project())
    }
    fn serialize(&self) -> String {
        delegate!(self, d => d.serialize())
    }
    fn is_dirty(&self) -> bool {
        delegate!(self, d => d.is_dirty())
    }
    fn apply(&mut self, m: Mutation) -> Result<(), MutateError> {
        delegate!(self, d => d.apply(m))
    }
    fn serialize_fragment(&self, path: &[Seg]) -> String {
        delegate!(self, d => d.serialize_fragment(path))
    }
    fn serialize_fragment_relative(&self, path: &[Seg]) -> String {
        delegate!(self, d => d.serialize_fragment_relative(path))
    }
    fn format(&self) -> DocFormat {
        delegate!(self, d => d.format())
    }
    fn comment_prefix(&self) -> &'static str {
        delegate!(self, d => d.comment_prefix())
    }
    fn supports_comments(&self) -> bool {
        delegate!(self, d => d.supports_comments())
    }
    fn kind_options(&self, path: &[Seg]) -> Vec<(String, KindTarget)> {
        delegate!(self, d => d.kind_options(path))
    }
}
```

Declare in `src/model/mod.rs` (`pub mod any_doc;`) following its existing style.

- [ ] **Step 4: Switch the TUI to `AnyDocument`**

- `src/tui/app.rs:47`: `pub doc: Option<crate::model::any_doc::AnyDocument>,`
- `src/tui/app.rs:116-117`: `App::new(doc: crate::model::any_doc::AnyDocument)`.
- `src/tui/mod.rs:20`: `let doc = crate::model::any_doc::AnyDocument::load(path)?;`
- Every test constructor (`app.rs:2986,3604`, the 7 in `ui.rs`):
  `AnyDocument::Toml(CstDocument::load(...).unwrap())` — or, where the test loads a
  `.toml` tempfile anyway, simply `AnyDocument::load(f.path()).unwrap()`. Pick one
  style and use it at all 9 sites.

- [ ] **Step 5: Run tests**

Run: `cargo test && cargo clippy -- -D warnings`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(model): AnyDocument enum dispatch; App holds the wrapper"
```

---

### Task 6: TUI uses `comment_prefix`/`format` facets instead of hard-coded `#`

**Files:**
- Modify: `src/tui/app.rs` (comment validation sites)
- Modify: `src/tui/ui.rs` (title bar, if it names the format)

- [ ] **Step 1: Inventory the hard-coded `#` checks**

Run: `rg -n '"#"|starts_with\(.#.\)|# ' src/tui/app.rs | rg -i 'comment'` and read each
hit. Expected sites (from CLAUDE.md): inline comment-editor validation ("stays in the
editor on a non-`#` validation error"), `InsertComment` paste validation ("validates
every line starts with `#`"), and the `r` remark prefix builder if it lives TUI-side.

- [ ] **Step 2: Rewire each through the doc facet**

At each site, replace the literal with the doc's facet, e.g.:

```rust
let prefix = self.doc.as_ref().map_or("#", |d| d.comment_prefix());
if !line.trim_start().starts_with(prefix) { /* same error path as today */ }
```

Error-message strings that embed `#` get the same treatment (format with `{prefix}`).
**Do not** change validation *logic* — same checks, parameterized leader.
If a site lives in `model/cst_edit.rs` rather than the TUI, leave it: the TOML backend
hard-coding `#` internally is correct.

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: PASS — all comment-editing tests unchanged.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "refactor(tui): comment leader comes from the backend (comment_prefix)"
```

---

### Task 7: CLI recognizes json/yaml extensions (and bails politely)

**Files:**
- Modify: `src/cli.rs` (delete local `Format` + `detect_format`; use the model's)

- [ ] **Step 1: Update the tests first**

Replace `cli.rs`'s test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::document::DocFormat;
    #[test]
    fn detects_known_formats() {
        use crate::model::any_doc::detect_format;
        let p = |s: &str| detect_format(std::path::Path::new(s));
        assert_eq!(p("a.toml"), Some(DocFormat::Toml));
        assert_eq!(p("a.json"), Some(DocFormat::Json));
        assert_eq!(p("a.jsonc"), Some(DocFormat::Json));
        assert_eq!(p("a.yaml"), Some(DocFormat::Yaml));
        assert_eq!(p("a.yml"), Some(DocFormat::Yaml));
        assert_eq!(p("a.ini"), None);
    }
}
```

Run: `cargo test detects_known_formats` — FAILS (old `detect_format` returns `Result<cli::Format>`).

- [ ] **Step 2: Rewrite `cli.rs` run flow**

```rust
use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

use crate::model::any_doc::detect_format;
use crate::model::document::DocFormat;

#[derive(Parser)]
#[command(name = "confy", about = "TUI editor for structured config files")]
struct Args {
    /// Path to the config file to edit
    file: PathBuf,
    /// Override format detection (toml; json/yaml planned)
    #[arg(long)]
    format: Option<String>,
}

pub fn run() -> Result<()> {
    let args = Args::parse();
    let fmt = match args.format.as_deref() {
        Some("toml") => DocFormat::Toml,
        Some("json") | Some("jsonc") => DocFormat::Json,
        Some("yaml") | Some("yml") => DocFormat::Yaml,
        Some(other) => anyhow::bail!("unknown format: {other}"),
        None => detect_format(&args.file).ok_or_else(|| {
            anyhow::anyhow!("unrecognized config format: {}", args.file.display())
        })?,
    };
    crate::tui::run(&args.file, fmt)
}
```

`src/tui/mod.rs` `run` gains the parameter:

```rust
pub fn run(path: &std::path::Path, format: DocFormat) -> anyhow::Result<()> {
    // …
    let doc = crate::model::any_doc::AnyDocument::load_as(path, format)?;
    // …
}
```

(The json/yaml bail happens inside `load_as` — one message, one place.) Update any
`tui::run` call sites/tests (`rg -n 'tui::run' src tests`). Check `tests/roundtrip.rs`
for CLI-level assertions on the old "MVP supports .toml only" message and update them
to the new messages.

- [ ] **Step 3: Run tests**

Run: `cargo test && cargo clippy -- -D warnings`
Expected: PASS. Also smoke-test by hand: `cargo run -- /tmp/x.json` (after
`echo '{}' > /tmp/x.json`) prints "JSON support is coming in a later release".

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat(cli): detect json/jsonc/yaml extensions (load bails until their phase)"
```

---

### Task 8: Help text takes the format

**Files:**
- Modify: `src/tui/keys.rs:87` (`help_text` signature)
- Modify: its caller in `src/tui/ui.rs` (find with `rg -n 'help_text' src/tui`)

- [ ] **Step 1: Parameterize**

```rust
/// Keybinding help text, displayed in the `?` overlay. Format-specific: the
/// op list and KIND legend differ per backend (Phases 2–3 add their texts).
pub fn help_text(format: DocFormat) -> &'static str {
    match format {
        DocFormat::Toml => TOML_HELP,
        // Until their backends land (load_as bails), these are unreachable;
        // wire real texts in Phases 2–3.
        DocFormat::Json | DocFormat::Yaml => TOML_HELP,
    }
}

const TOML_HELP: &str = "\
…(the existing string, moved verbatim)…
";
```

Caller passes `app.doc.as_ref().map_or(DocFormat::Toml, |d| d.format())`.

- [ ] **Step 2: Run tests + gate**

Run: `cargo test && cargo clippy -- -D warnings && cargo fmt --check`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "refactor(tui): help text keyed by DocFormat"
```

---

### Task 9: Docs

**Files:**
- Modify: `CHANGELOG.md` (Unreleased entry: backend abstraction, json/yaml detection stubs)
- Modify: `CLAUDE.md` (architecture: AnyDocument dispatch, kind_options capability, DocFormat facets; module map: `model/any_doc.rs`)
- Modify: `CONTEXT.md` glossary if it defines `Mutation` field names

- [ ] **Step 1: Write the entries** (describe what shipped, mirroring commit messages)
- [ ] **Step 2: Final gate**

Run: `cargo test && cargo clippy -- -D warnings && cargo fmt --check`
Expected: all clean.

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "docs: phase 1 backend abstraction (changelog, CLAUDE.md, module map)"
```
