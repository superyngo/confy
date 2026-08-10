# JSON Schema Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Detect, validate against, and offer constrained editing for JSON Schema across TOML/JSON/YAML documents, uniformly on TUI, desktop web, touch/mobile web, and Tauri desktop/mobile.

**Architecture:** A new `confy-core/src/schema/` module (detection, JSON-projection bridging, `jsonschema`-crate-backed validation, edit-hint resolution) plumbed through `Session`/`SessionSnapshot`/`Intent` exactly like the existing `external_edit`/`convert_write` host-signal pattern. Each host (confy-tui, web, touch) resolves schema text (local file or URL) and feeds it back; each host's existing single-select popup (TUI kind-switch, web `<select>`, touch bottom sheet) is reused for enum/const constrained editing. Violations are always soft — a new `schema_warn` field per row, never a blocking `MutateError`.

**Tech Stack:** Rust (confy-core, confy-tui), the `jsonschema` crate (draft 2020-12 validator), TypeScript (web/, web/touch/), Tauri v2 (confy-tauri).

**Spec:** `docs/superpowers/specs/2026-08-10-json-schema-support-design.md` (grilled/finalized). Every task below traces to a section of that spec.

## Global Constraints

- Validation is **always soft** — never blocks `Intent::Save`, `EditCommit`, `CommitEdit`, or any `Mutation::apply`. (Spec: Non-goals, §4)
- Detection is **in-file hints + explicit override only** — no filename-based sibling-file guessing. (Spec §1)
- Schema association is **session-only** — never persisted to `~/.config/confy/config.toml`. (Spec: Decisions locked)
- confy-core stays fully headless: no `std::fs`, no `reqwest`/network. All schema-text I/O is host-owned via the `schema_fetch_request` → `Intent::SchemaLoaded` handshake. (Spec: Host↔core async handshake)
- A local hint/override's relative path resolves against **the directory of the open config file** (host-side concern; confy-core never sees a filesystem path). (Spec §1)
- New dependency versions follow the workspace's existing pinning convention: one line in root `Cargo.toml`'s `[workspace.dependencies]`, referenced as `<crate>.workspace = true` in member `Cargo.toml`s.
- New CONTEXT.md vocabulary (**JSON projection**, **Violation**, **Soft constraint**) is already recorded — code should use these exact terms in doc comments.

---

## File Structure

**New files (confy-core):**
- `crates/confy-core/src/schema/mod.rs` — module root, re-exports.
- `crates/confy-core/src/schema/types.rs` — `SchemaSource`, `Category`, `Violation`, `EditHint`, `SchemaState`, `SchemaStatus`.
- `crates/confy-core/src/schema/value_bridge.rs` — `bridge()` (Node+Value → JSON projection + pointer map), `PointerMap`.
- `crates/confy-core/src/schema/hints.rs` — `detect_hint()` per `DocFormat`.
- `crates/confy-core/src/schema/validate.rs` — `validate()`.
- `crates/confy-core/src/schema/hints_edit.rs` — `resolve_edit_hint()`.
- `crates/confy-core/tests/schema_headless.rs` — integration tests.

**Modified files (confy-core):**
- `Cargo.toml` (workspace root) — add `jsonschema` to `[workspace.dependencies]`.
- `crates/confy-core/Cargo.toml` — add `jsonschema.workspace = true`.
- `crates/confy-core/src/lib.rs` — add `pub mod schema;`.
- `crates/confy-core/src/session/session.rs` — `Session.schema: Option<SchemaState>`, `apply_schema_text`/`revalidate_schema`/`schema_enum_move`/`schema_enum_commit`/`schema_allows_nudge` methods, `begin_inline_edit()` extended, existing Nudge handling clamped.
- `crates/confy-core/src/session/state.rs` — `Mode::SchemaEnum(SchemaEnumState)` variant, `SchemaEnumState` struct.
- `crates/confy-core/src/session/view.rs` — `SessionSnapshot.schema_status`/`schema_fetch_request`, `ViewRow.schema_warn`, `ModeView::SchemaEnum`.
- `crates/confy-core/src/session/intent.rs` — `Intent::SetSchema`, `Intent::SchemaLoaded`, `Intent::SchemaEnumMove`, `Intent::SchemaEnumCommit`.
- `crates/confy-core/src/session/dispatch.rs` — new match arms for the above; existing `Intent::Nudge` arm gains a `schema_allows_nudge` guard.

**New files (confy-tui):**
- `crates/confy-tui/src/tui/schema_io.rs` — `resolve_schema_source()` (local read + `ureq` URL fetch).

**Modified files (confy-tui):**
- `Cargo.toml` (workspace root) — add `ureq`.
- `crates/confy-tui/Cargo.toml` — add `ureq.workspace = true`.
- `crates/confy-tui/src/cli.rs` — `--schema` flag, threaded into `tui::run`.
- `crates/confy-tui/src/tui/mod.rs` — accept schema arg, resolve at startup, dispatch `SchemaLoaded`, key routing for `Mode::SchemaEnum`.
- `crates/confy-tui/src/tui/app.rs` — `rebuild_rows()` carries `schema_warn`, `type_tag` gains `!` suffix.
- `crates/confy-tui/src/tui/ui.rs` — `draw_tree` warning style arm, new `draw_schema_enum_overlay`, status-line summary.
- `crates/confy-tui/src/tui/state.rs` — re-export `SchemaEnumState`.

**Modified files (web):**
- `web/types.ts` — `SchemaStatus`, `ViewRow.schema_warn`, `EditView.constraint`, `ModeView` schema-enum variant.
- `web/fs.ts` — `readSiblingFile()`.
- `web/host-io.ts` — schema resolution wiring.
- `web/ui.ts` — attach-schema action, `focusInlineEdit()` `<select>` branch, status summary.
- `web/render.ts` — `renderValue()` `<select>` branch, `.schema-warn` class.
- `web/style.css` — `.schema-warn`, `--warn` variable.

**Modified files (touch):**
- `web/touch/app.ts` — attach-schema sheet action, enum sheet (parallels `openKindSheet`).
- `web/touch/render.ts` — `.schema-warn` class in `rowHTML`.
- `web/touch/style.css` — `.schema-warn`.

**Modified files (docs):**
- `TAURI.md` — Android relative-path limitation.
- `CHANGELOG.md` — Unreleased entry (repo convention).

---

## Phase 1 — confy-core schema engine (headless foundation)

### Task 1: Schema module skeleton + core types

**Files:**
- Create: `crates/confy-core/src/schema/mod.rs`
- Create: `crates/confy-core/src/schema/types.rs`
- Modify: `crates/confy-core/src/lib.rs`
- Modify: `Cargo.toml` (workspace root)
- Modify: `crates/confy-core/Cargo.toml`
- Test: `crates/confy-core/tests/schema_headless.rs` (new file, first test)

**Interfaces:**
- Produces: `schema::types::{SchemaSource, Category, Violation, EditHint, SchemaState, SchemaStatus}` — every later task in this plan imports from here.

- [ ] **Step 1: Add the `jsonschema` dependency**

In `Cargo.toml` (workspace root), inside `[workspace.dependencies]`, add (alphabetical order, matching the existing list's convention):

```toml
jsonschema = "0.30"
```

In `crates/confy-core/Cargo.toml`, inside `[dependencies]`, add:

```toml
jsonschema.workspace = true
```

- [ ] **Step 2: Run `cargo check -p confy-core` to confirm the dependency resolves**

Run: `cargo check -p confy-core`
Expected: compiles clean (no code uses `jsonschema` yet, so this only proves the dependency graph resolves).

- [ ] **Step 3: Write the core schema types**

Create `crates/confy-core/src/schema/types.rs`:

```rust
//! Core types for JSON Schema support. See `CONTEXT.md` § Schema for the
//! canonical vocabulary (JSON projection, Violation, Soft constraint).

use crate::model::node::Path;
use serde::{Deserialize, Serialize};

/// Where a schema came from — a relative/absolute local path, or a URL.
/// Never resolved to bytes by confy-core itself; hosts do the I/O (see
/// `Session::apply_schema_text`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchemaSource {
    Local(String),
    Url(String),
}

/// Whether a Violation is an ordinary value mismatch, or a case where the
/// document's *source format* cannot represent what the schema requires
/// (e.g. `type: null` against a TOML-sourced node, which has no null
/// literal). Both are soft — see `CONTEXT.md` § Schema "Soft constraint".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Category {
    Value,
    Representation,
}

/// A single JSON Schema constraint failure. Purely informational: never
/// blocks a Mutation, never appears in a `MutateError`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Violation {
    /// The Node this violation is reported against — the failing node itself,
    /// or (for a `required` failure, whose JSON Pointer targets the parent
    /// object that's missing a child) the parent's Path.
    pub path: Path,
    /// The raw JSON Pointer `jsonschema` reported (RFC 6901).
    pub pointer: String,
    /// The failing schema keyword (`"type"`, `"enum"`, `"required"`, …).
    pub keyword: String,
    /// Human-readable message, as `jsonschema` renders it.
    pub message: String,
    pub category: Category,
}

/// A resolved editing constraint for one node, used to swap the inline
/// editor's plain text input for a constrained widget (enum/const picker,
/// numeric bounds). Deliberately does not attempt to resolve `allOf`/
/// `oneOf`/`anyOf`/`not`/`if-then-else` (beyond the narrow oneOf/anyOf-of-const
/// carve-out) or remote `$ref` — those fall through to `None`. `validate()`
/// still fully enforces them regardless; only the *widget* stays plain text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EditHint {
    /// `(display_label, value)` pairs — from a schema `enum`, `const`, or a
    /// `oneOf`/`anyOf` where every branch is a bare `{const, title?,
    /// description?}`.
    Enum(Vec<(String, serde_json::Value)>),
    Bounded {
        minimum: Option<f64>,
        maximum: Option<f64>,
        multiple_of: Option<f64>,
    },
    None,
}

/// Per-session schema state. Lives on `Session`, not `Node`/`NodeTree` — the
/// projected tree is rebuilt from the document on every mutation, so
/// per-document state belongs one level up (mirrors `Session.clipboard`,
/// `Session.filter`, etc.).
#[derive(Clone, Debug)]
pub struct SchemaState {
    pub source: SchemaSource,
    /// `None` while `load_error` is set (load/compile failed) or before the
    /// host has resolved `schema_fetch_request`.
    pub compiled: Option<jsonschema::Validator>,
    /// The raw (uncompiled) schema JSON — `hints_edit::resolve_edit_hint`
    /// walks this directly (it needs keyword introspection the compiled
    /// `Validator` doesn't expose).
    pub raw: Option<serde_json::Value>,
    pub violations: Vec<Violation>,
    pub load_error: Option<String>,
}

/// Document-level summary surfaced to hosts (status line / toolbar).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SchemaStatus {
    pub source_label: String,
    pub violation_count: usize,
    pub load_error: Option<String>,
}

impl SchemaState {
    pub fn status(&self) -> SchemaStatus {
        let source_label = match &self.source {
            SchemaSource::Local(p) => p.clone(),
            SchemaSource::Url(u) => u.clone(),
        };
        SchemaStatus {
            source_label,
            violation_count: self.violations.len(),
            load_error: self.load_error.clone(),
        }
    }
}
```

- [ ] **Step 4: Wire the module and write a trivial construction test**

Create `crates/confy-core/src/schema/mod.rs`:

```rust
//! JSON Schema detection, validation, and constrained-editing support.
//! See `docs/superpowers/specs/2026-08-10-json-schema-support-design.md`.

pub mod hints;
pub mod hints_edit;
pub mod types;
pub mod validate;
pub mod value_bridge;

pub use types::{Category, EditHint, SchemaSource, SchemaState, SchemaStatus, Violation};
pub use value_bridge::PointerMap;
```

(`hints`/`hints_edit`/`validate`/`value_bridge` modules referenced above don't exist yet — Steps 5-6 add empty stubs so this compiles; Tasks 2-5 fill them in.)

Create empty stub files so `mod.rs` compiles:

`crates/confy-core/src/schema/hints.rs`:
```rust
//! Per-format schema-hint detection. Filled in by Task 3.
```

`crates/confy-core/src/schema/value_bridge.rs`:
```rust
//! Node+Value → JSON projection bridging. Filled in by Task 2.

pub struct PointerMap;
```

`crates/confy-core/src/schema/validate.rs`:
```rust
//! `jsonschema`-backed validation. Filled in by Task 4.
```

`crates/confy-core/src/schema/hints_edit.rs`:
```rust
//! Edit-hint resolution for constrained inline editing. Filled in by Task 5.
```

In `crates/confy-core/src/lib.rs`, add (alongside the existing `pub mod model;` / `pub mod session;` lines — match their exact style):

```rust
pub mod schema;
```

Create `crates/confy-core/tests/schema_headless.rs`:

```rust
//! Headless schema-engine tests — no TUI/host dependency, matches the
//! `session_headless.rs` convention (crate-root `#[test]` fns, tiny local
//! helpers, no test framework macros).
use confy_core::schema::types::{Category, SchemaSource};

#[test]
fn schema_source_variants_are_distinguishable() {
    let local = SchemaSource::Local("./schema.json".into());
    let url = SchemaSource::Url("https://example.com/s.json".into());
    assert_ne!(local, url);
    assert_eq!(Category::Value, Category::Value);
    assert_ne!(Category::Value, Category::Representation);
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p confy-core --test schema_headless`
Expected: PASS (1 test) — this only proves the types compile and derive `PartialEq`/`Eq` correctly; real behavior starts in Task 2.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/confy-core/Cargo.toml crates/confy-core/src/lib.rs crates/confy-core/src/schema crates/confy-core/tests/schema_headless.rs
git commit -m "feat(schema): add jsonschema dependency and core schema types"
```

---

### Task 2: `value_bridge.rs` — JSON projection + pointer↔path mapping

**Files:**
- Modify: `crates/confy-core/src/schema/value_bridge.rs`
- Test: `crates/confy-core/tests/schema_headless.rs`

**Interfaces:**
- Consumes: `crate::model::node::{Node, NodeKind, Path}` (Task-independent, existing), `crate::model::value::{Item, Value}` (existing).
- Produces: `pub fn bridge(root: &Node, root_value: &Value) -> (serde_json::Value, PointerMap)`, `PointerMap::resolve(&self, pointer: &str) -> Option<&Path>` — Task 4 (`validate.rs`) and Task 6 (`Session`) consume both.

- [ ] **Step 1: Write the failing tests**

Append to `crates/confy-core/tests/schema_headless.rs`:

```rust
use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::{ConfigDocument, DocFormat};
use confy_core::schema::value_bridge::bridge;
use serde_json::json;

fn toml_doc(src: &str) -> AnyDocument {
    AnyDocument::from_str_as(src, DocFormat::Toml).unwrap()
}

#[test]
fn bridge_projects_scalars_and_nesting() {
    let doc = toml_doc("name = \"svc\"\nport = 8080\n[db]\nhost = \"local\"\n");
    let tree = doc.project();
    let (value, _warnings) = doc.to_value().unwrap();
    let (json, _map) = bridge(&tree.root, &value);
    assert_eq!(
        json,
        json!({ "name": "svc", "port": 8080, "db": { "host": "local" } })
    );
}

#[test]
fn bridge_maps_pointers_to_paths_including_nested_and_required_parent() {
    use confy_core::model::node::Seg;
    let doc = toml_doc("[server]\nport = 8080\n");
    let tree = doc.project();
    let (value, _warnings) = doc.to_value().unwrap();
    let (_json, map) = bridge(&tree.root, &value);
    // Nested leaf resolves exactly.
    let leaf_path = map.resolve("/server/port").expect("leaf pointer mapped");
    assert_eq!(
        leaf_path,
        &vec![Seg::Key("server".into()), Seg::Key("port".into())]
    );
    // The parent object (a `required` failure's pointer) resolves too.
    let parent_path = map.resolve("/server").expect("parent pointer mapped");
    assert_eq!(parent_path, &vec![Seg::Key("server".into())]);
    // The document root resolves to the empty path.
    let root_path = map.resolve("").expect("root pointer mapped");
    assert_eq!(root_path, &Vec::<Seg>::new());
}

#[test]
fn bridge_skips_comments_and_keeps_array_order() {
    let doc = toml_doc("# a comment\nvals = [1, 2, 3]\n");
    let tree = doc.project();
    let (value, _warnings) = doc.to_value().unwrap();
    let (json, map) = bridge(&tree.root, &value);
    assert_eq!(json, json!({ "vals": [1, 2, 3] }));
    assert!(map.resolve("/vals/1").is_some());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p confy-core --test schema_headless`
Expected: FAIL to compile — `confy_core::schema::value_bridge::bridge` doesn't exist yet (only the `PointerMap` stub does).

- [ ] **Step 3: Implement `value_bridge.rs`**

Replace `crates/confy-core/src/schema/value_bridge.rs`:

```rust
//! Node+Value → **JSON projection** bridging (`CONTEXT.md` § Schema).
//!
//! Both the `Node` tree (paths, no decoded scalars) and the `Value` tree from
//! `ConfigDocument::to_value()` (decoded scalars, no paths) are order-preserving
//! 1:1 walks of the *same* backing document at every nesting level — every
//! child, including Comment nodes/`Item::Comment`, in document order (see
//! `CONTEXT.md` § Projection: "the backing document — not the Node tree — is
//! the single source of truth"). `bridge()` walks them together by position,
//! skipping Comment/`Item::Comment` pairs, to attach a `Path` to every JSON
//! projection node without reimplementing per-format scalar decoding (already
//! correctly done by `to_value()`).

use crate::model::node::{Node, NodeKind, Path};
use crate::model::value::{Item, Value};
use serde_json::{Map, Number, Value as Json};
use std::collections::HashMap;

/// JSON Pointer (RFC 6901 string, e.g. `/server/port`; `""` = document root)
/// → the Node `Path` it came from.
#[derive(Default)]
pub struct PointerMap(HashMap<String, Path>);

impl PointerMap {
    fn insert(&mut self, pointer: String, path: Path) {
        self.0.insert(pointer, path);
    }

    /// Resolve a violation's JSON Pointer to a Node Path. Falls back to the
    /// nearest ancestor pointer (strips one trailing `/segment` at a time)
    /// for any pointer the walk didn't visit directly — a defensive default,
    /// not the primary path: a `required` failure's pointer *is* the parent
    /// object, which the walk always visits and maps.
    pub fn resolve(&self, pointer: &str) -> Option<&Path> {
        let mut p = pointer;
        loop {
            if let Some(path) = self.0.get(p) {
                return Some(path);
            }
            match p.rfind('/') {
                Some(i) => p = &p[..i],
                None => return self.0.get(""),
            }
        }
    }
}

/// Lower `root`/`root_value` into a JSON projection, building the pointer map
/// as it goes.
pub fn bridge(root: &Node, root_value: &Value) -> (Json, PointerMap) {
    let mut map = PointerMap::default();
    let json = walk(root, root_value, "", &mut map);
    (json, map)
}

fn walk(node: &Node, value: &Value, pointer: &str, map: &mut PointerMap) -> Json {
    map.insert(pointer.to_string(), node.path.clone());
    match value {
        Value::Null => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Int(i) => Json::Number(Number::from(*i)),
        Value::Float(f) => Number::from_f64(*f).map(Json::Number).unwrap_or(Json::Null),
        // TOML datetimes have no JSON Schema-native type: bridged as a string
        // (RFC3339-shaped source text passes `format: date-time`/`date`/`time`
        // checks as-is; a schema requiring `type: null` against a TOML node
        // and other representation gaps are flagged by `validate.rs`, not here).
        Value::Str(s) | Value::Datetime(s) => Json::String(s.clone()),
        Value::Seq(items) => {
            let mut arr = Vec::new();
            let mut idx = 0usize;
            let mut child_nodes = node
                .children
                .iter()
                .filter(|c| !matches!(c.kind, NodeKind::Comment(_)));
            for it in items {
                let Item::Node { value, .. } = it else { continue };
                if let Some(child) = child_nodes.next() {
                    let child_pointer = format!("{pointer}/{idx}");
                    arr.push(walk(child, value, &child_pointer, map));
                    idx += 1;
                }
            }
            Json::Array(arr)
        }
        Value::Map(items) => {
            let mut obj = Map::new();
            let mut child_nodes = node
                .children
                .iter()
                .filter(|c| !matches!(c.kind, NodeKind::Comment(_)));
            for it in items {
                let Item::Node { key: Some(k), value, .. } = it else { continue };
                if let Some(child) = child_nodes.next() {
                    let child_pointer = format!("{pointer}/{}", escape_pointer_segment(k));
                    obj.insert(k.clone(), walk(child, value, &child_pointer, map));
                }
            }
            Json::Object(obj)
        }
    }
}

/// RFC 6901 pointer-segment escaping (`~` → `~0`, `/` → `~1`).
fn escape_pointer_segment(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}
```

Also update `crates/confy-core/src/schema/mod.rs`'s re-export line (it already says `pub use value_bridge::PointerMap;`, no change needed) — but confirm `bridge` is reachable at `crate::schema::value_bridge::bridge` (used qualified in the tests above, no `mod.rs` change required since the test imports the submodule path directly).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p confy-core --test schema_headless`
Expected: PASS (4 tests: the Task 1 test plus the 3 new ones).

- [ ] **Step 5: Commit**

```bash
git add crates/confy-core/src/schema/value_bridge.rs crates/confy-core/tests/schema_headless.rs
git commit -m "feat(schema): bridge Node+Value to a JSON projection with pointer↔path map"
```

---

### Task 3: `hints.rs` — per-format schema-hint detection

**Files:**
- Modify: `crates/confy-core/src/schema/hints.rs`
- Test: `crates/confy-core/tests/schema_headless.rs`

**Interfaces:**
- Consumes: `crate::model::document::DocFormat` (existing).
- Produces: `pub fn detect_hint(text: &str, format: DocFormat) -> Option<SchemaSource>` — Task 6 (`Session::detect_and_request_schema`) consumes this.

- [ ] **Step 1: Write the failing tests**

Append to `crates/confy-core/tests/schema_headless.rs`:

```rust
use confy_core::schema::hints::detect_hint;
use confy_core::schema::types::SchemaSource;

#[test]
fn detect_hint_json_root_schema_key() {
    let src = r#"{ "$schema": "./app.schema.json", "port": 1 }"#;
    assert_eq!(
        detect_hint(src, DocFormat::Json),
        Some(SchemaSource::Local("./app.schema.json".into()))
    );
}

#[test]
fn detect_hint_json_url_schema_key() {
    let src = r#"{ "$schema": "https://example.com/s.json" }"#;
    assert_eq!(
        detect_hint(src, DocFormat::Json),
        Some(SchemaSource::Url("https://example.com/s.json".into()))
    );
}

#[test]
fn detect_hint_json_none_when_absent() {
    let src = r#"{ "port": 1 }"#;
    assert_eq!(detect_hint(src, DocFormat::Json), None);
}

#[test]
fn detect_hint_yaml_modeline() {
    let src = "# yaml-language-server: $schema=./s.yaml\nport: 1\n";
    assert_eq!(
        detect_hint(src, DocFormat::Yaml),
        Some(SchemaSource::Local("./s.yaml".into()))
    );
}

#[test]
fn detect_hint_yaml_none_when_modeline_not_leading() {
    // The modeline must be a leading comment — not one that appears after
    // real content.
    let src = "port: 1\n# yaml-language-server: $schema=./s.yaml\n";
    assert_eq!(detect_hint(src, DocFormat::Yaml), None);
}

#[test]
fn detect_hint_toml_first_line_schema_comment() {
    let src = "#:schema ./app.schema.json\nport = 1\n";
    assert_eq!(
        detect_hint(src, DocFormat::Toml),
        Some(SchemaSource::Local("./app.schema.json".into()))
    );
}

#[test]
fn detect_hint_toml_none_when_not_first_line() {
    let src = "port = 1\n#:schema ./app.schema.json\n";
    assert_eq!(detect_hint(src, DocFormat::Toml), None);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p confy-core --test schema_headless`
Expected: FAIL to compile — `confy_core::schema::hints::detect_hint` doesn't exist yet.

- [ ] **Step 3: Implement `hints.rs`**

Replace `crates/confy-core/src/schema/hints.rs`:

```rust
//! Per-format schema-hint detection — pure, no I/O. Three ecosystem
//! conventions, one per format (spec §1):
//! - JSON/JSONC: a root-level `"$schema"` string member.
//! - YAML: a leading `# yaml-language-server: $schema=<path-or-url>` modeline.
//! - TOML: a first-line `#:schema <path-or-url>` comment (Taplo convention).

use super::types::SchemaSource;
use crate::model::document::DocFormat;

pub fn detect_hint(text: &str, format: DocFormat) -> Option<SchemaSource> {
    match format {
        DocFormat::Json => detect_json(text),
        DocFormat::Yaml => detect_yaml(text),
        DocFormat::Toml => detect_toml(text),
    }
}

fn to_source(raw: &str) -> SchemaSource {
    let raw = raw.trim();
    if raw.starts_with("http://") || raw.starts_with("https://") {
        SchemaSource::Url(raw.to_string())
    } else {
        SchemaSource::Local(raw.to_string())
    }
}

fn detect_json(text: &str) -> Option<SchemaSource> {
    // Parse-then-lookup rather than regex: `$schema` is a root member of a
    // JSON *value*, and a naive text scan would false-positive on a nested
    // `"$schema"` string value elsewhere in the document. JSONC `//`/`/* */`
    // comments would break `serde_json::from_str`, but a root-level
    // `"$schema"` key is legal even in strict JSON, so this degrades to
    // `None` (not a panic/error) on a JSONC file with comments before the
    // key — acceptable: JSONC's `//`/`/* */` upgrade is orthogonal to schema
    // detection, and a load failure here is never fatal (spec §1: "never a
    // hard-fail").
    let parsed: serde_json::Value = serde_json::from_str(text).ok()?;
    let schema = parsed.get("$schema")?.as_str()?;
    Some(to_source(schema))
}

fn detect_yaml(text: &str) -> Option<SchemaSource> {
    // "Leading" = the modeline must appear before any non-comment,
    // non-blank line (a real document line breaks the leading-comment run).
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('#') {
            if let Some(eq) = rest.trim_start().strip_prefix("yaml-language-server:") {
                if let Some(schema) = eq.trim_start().strip_prefix("$schema=") {
                    return Some(to_source(schema.trim()));
                }
            }
            continue; // some other leading comment — keep scanning
        }
        return None; // first non-comment, non-blank line — stop
    }
    None
}

fn detect_toml(text: &str) -> Option<SchemaSource> {
    let first_line = text.lines().next()?;
    let rest = first_line.strip_prefix("#:schema")?;
    let path = rest.trim();
    if path.is_empty() {
        return None;
    }
    Some(to_source(path))
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p confy-core --test schema_headless`
Expected: PASS (11 tests total).

- [ ] **Step 5: Commit**

```bash
git add crates/confy-core/src/schema/hints.rs crates/confy-core/tests/schema_headless.rs
git commit -m "feat(schema): detect in-file schema hints for JSON/YAML/TOML"
```

---

### Task 4: `validate.rs` — `jsonschema`-backed validation

**Files:**
- Modify: `crates/confy-core/src/schema/validate.rs`
- Test: `crates/confy-core/tests/schema_headless.rs`

**Interfaces:**
- Consumes: `jsonschema::Validator` (external crate, `Validator::new(schema: &serde_json::Value) -> Result<Validator, jsonschema::ValidationError<'static>>`, `validator.iter_errors(&instance) -> impl Iterator<Item = jsonschema::ValidationError>`; each error exposes `.instance_path() -> &Location` and `.schema_path() -> &Location`, both `Display`-able as JSON Pointer strings, and `Display`/`ToString` for the message), `schema::value_bridge::{bridge, PointerMap}` (Task 2), `schema::types::{Violation, Category}` (Task 1).
- Produces: `pub fn validate(projection: &serde_json::Value, compiled: &jsonschema::Validator, map: &PointerMap) -> Vec<Violation>` — Task 6 (`Session::revalidate_schema`) consumes this.

- [ ] **Step 1: Write the failing tests**

Append to `crates/confy-core/tests/schema_headless.rs`:

```rust
use confy_core::schema::validate::validate;
use confy_core::schema::value_bridge::bridge;
use jsonschema::Validator;

fn compiled(schema: serde_json::Value) -> Validator {
    Validator::new(&schema).expect("valid test schema")
}

#[test]
fn validate_reports_no_violations_for_a_conforming_document() {
    let doc = toml_doc("port = 8080\n");
    let tree = doc.project();
    let (value, _w) = doc.to_value().unwrap();
    let (json, map) = bridge(&tree.root, &value);
    let v = compiled(json!({
        "type": "object",
        "properties": { "port": { "type": "integer" } }
    }));
    assert!(validate(&json, &v, &map).is_empty());
}

#[test]
fn validate_reports_a_type_violation_with_the_leaf_path() {
    use confy_core::model::node::Seg;
    let doc = toml_doc("port = \"not-a-number\"\n");
    let tree = doc.project();
    let (value, _w) = doc.to_value().unwrap();
    let (json, map) = bridge(&tree.root, &value);
    let v = compiled(json!({
        "type": "object",
        "properties": { "port": { "type": "integer" } }
    }));
    let violations = validate(&json, &v, &map);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].path, vec![Seg::Key("port".into())]);
    assert_eq!(violations[0].keyword, "type");
    assert_eq!(violations[0].category, Category::Value);
}

#[test]
fn validate_reports_a_required_violation_against_the_parent_path() {
    use confy_core::model::node::Seg;
    let doc = toml_doc("[server]\nhost = \"local\"\n");
    let tree = doc.project();
    let (value, _w) = doc.to_value().unwrap();
    let (json, map) = bridge(&tree.root, &value);
    let v = compiled(json!({
        "type": "object",
        "properties": {
            "server": {
                "type": "object",
                "required": ["port"]
            }
        }
    }));
    let violations = validate(&json, &v, &map);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].keyword, "required");
    assert_eq!(violations[0].path, vec![Seg::Key("server".into())]);
    assert!(violations[0].message.contains("port"));
}

#[test]
fn validate_flags_null_type_against_toml_as_representation_category() {
    let doc = toml_doc("port = 8080\n");
    let tree = doc.project();
    let (value, _w) = doc.to_value().unwrap();
    let (json, map) = bridge(&tree.root, &value);
    let v = compiled(json!({
        "type": "object",
        "properties": { "port": { "type": "null" } }
    }));
    let violations = validate(&json, &v, &map);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].category, Category::Representation);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p confy-core --test schema_headless`
Expected: FAIL to compile — `confy_core::schema::validate::validate` doesn't exist yet.

- [ ] **Step 3: Implement `validate.rs`**

Replace `crates/confy-core/src/schema/validate.rs`:

```rust
//! `jsonschema`-backed validation over a JSON projection. Full draft 2020-12
//! semantics (composition, `$ref` to the schema's own `$defs`) apply
//! uniformly across TOML/JSON/YAML since this operates on the projection,
//! never on source syntax.

use super::types::{Category, Violation};
use super::value_bridge::PointerMap;
use jsonschema::Validator;
use serde_json::Value as Json;

/// Validate `projection` against `compiled`, returning every Violation.
/// Infallible: `Validator::iter_errors` only panics on malformed schemas,
/// which `Validator::new` already rejects at compile time (surfaced as
/// `SchemaState.load_error`, never reaching this function).
pub fn validate(projection: &Json, compiled: &Validator, map: &PointerMap) -> Vec<Violation> {
    compiled
        .iter_errors(projection)
        .map(|err| {
            let pointer = err.instance_path().to_string();
            let schema_path = err.schema_path().to_string();
            let keyword = schema_path
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string();
            let path = map.resolve(&pointer).cloned().unwrap_or_default();
            let message = err.to_string();
            // A `type: null` mismatch against a TOML-sourced document is a
            // structural representation gap (TOML has no null literal — the
            // bridge never emits `Json::Null` for a TOML scalar), not an
            // ordinary value error the user can fix by editing.
            let category = if keyword == "type" && message.contains("null") {
                Category::Representation
            } else {
                Category::Value
            };
            Violation {
                path,
                pointer,
                keyword,
                message,
                category,
            }
        })
        .collect()
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p confy-core --test schema_headless`
Expected: PASS (15 tests total).

- [ ] **Step 5: Commit**

```bash
git add crates/confy-core/src/schema/validate.rs crates/confy-core/tests/schema_headless.rs
git commit -m "feat(schema): validate a JSON projection with the jsonschema crate"
```

---

### Task 5: `hints_edit.rs` — constrained-editing hint resolution

**Files:**
- Modify: `crates/confy-core/src/schema/hints_edit.rs`
- Test: `crates/confy-core/tests/schema_headless.rs`

**Interfaces:**
- Consumes: `crate::model::node::{Path, Seg}` (existing), `schema::types::EditHint` (Task 1).
- Produces: `pub fn resolve_edit_hint(schema: &serde_json::Value, path: &Path) -> EditHint` — Task 6 (`Session::begin_inline_edit`) consumes this.

- [ ] **Step 1: Write the failing tests**

Append to `crates/confy-core/tests/schema_headless.rs`:

```rust
use confy_core::model::node::Seg;
use confy_core::schema::hints_edit::resolve_edit_hint;
use confy_core::schema::types::EditHint;

#[test]
fn resolve_edit_hint_finds_enum_via_properties() {
    let schema = json!({
        "type": "object",
        "properties": {
            "level": { "enum": ["debug", "info", "warn"] }
        }
    });
    let hint = resolve_edit_hint(&schema, &vec![Seg::Key("level".into())]);
    match hint {
        EditHint::Enum(opts) => {
            let labels: Vec<_> = opts.iter().map(|(l, _)| l.clone()).collect();
            assert_eq!(labels, vec!["debug", "info", "warn"]);
        }
        other => panic!("expected Enum, got {other:?}"),
    }
}

#[test]
fn resolve_edit_hint_finds_bounded_numeric() {
    let schema = json!({
        "type": "object",
        "properties": {
            "port": { "type": "integer", "minimum": 1, "maximum": 65535 }
        }
    });
    let hint = resolve_edit_hint(&schema, &vec![Seg::Key("port".into())]);
    assert_eq!(
        hint,
        EditHint::Bounded { minimum: Some(1.0), maximum: Some(65535.0), multiple_of: None }
    );
}

#[test]
fn resolve_edit_hint_carves_out_oneof_of_const() {
    let schema = json!({
        "type": "object",
        "properties": {
            "level": {
                "oneOf": [
                    { "const": "debug", "title": "Debug" },
                    { "const": "info", "title": "Info" }
                ]
            }
        }
    });
    let hint = resolve_edit_hint(&schema, &vec![Seg::Key("level".into())]);
    match hint {
        EditHint::Enum(opts) => {
            assert_eq!(
                opts,
                vec![
                    ("Debug".to_string(), json!("debug")),
                    ("Info".to_string(), json!("info")),
                ]
            );
        }
        other => panic!("expected Enum via oneOf carve-out, got {other:?}"),
    }
}

#[test]
fn resolve_edit_hint_declines_true_composition() {
    let schema = json!({
        "type": "object",
        "properties": {
            "value": {
                "oneOf": [
                    { "type": "string", "minLength": 1 },
                    { "type": "integer" }
                ]
            }
        }
    });
    let hint = resolve_edit_hint(&schema, &vec![Seg::Key("value".into())]);
    assert_eq!(hint, EditHint::None);
}

#[test]
fn resolve_edit_hint_resolves_array_items_and_local_ref() {
    let schema = json!({
        "type": "object",
        "properties": {
            "tags": { "type": "array", "items": { "$ref": "#/$defs/tag" } }
        },
        "$defs": { "tag": { "enum": ["a", "b"] } }
    });
    let hint = resolve_edit_hint(&schema, &vec![Seg::Key("tags".into()), Seg::Index(0)]);
    match hint {
        EditHint::Enum(opts) => {
            assert_eq!(opts.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
        }
        other => panic!("expected Enum via items+$ref, got {other:?}"),
    }
}

#[test]
fn resolve_edit_hint_none_for_unresolvable_path() {
    let schema = json!({ "type": "object", "properties": {} });
    let hint = resolve_edit_hint(&schema, &vec![Seg::Key("missing".into())]);
    assert_eq!(hint, EditHint::None);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p confy-core --test schema_headless`
Expected: FAIL to compile — `resolve_edit_hint` doesn't exist yet.

- [ ] **Step 3: Implement `hints_edit.rs`**

Replace `crates/confy-core/src/schema/hints_edit.rs`:

```rust
//! Best-effort resolution of the applicable sub-schema at one target `Path`,
//! for constrained inline editing (spec §3). Deliberately simpler than
//! `validate.rs`: resolves through `properties`/`items`/local `$defs` +
//! same-document `$ref`, plus a narrow `oneOf`/`anyOf`-of-`const` carve-out
//! (the single most common real-world enum-with-descriptions idiom). Any
//! other composition (`allOf`/`not`/`if-then-else`, a `oneOf`/`anyOf` branch
//! carrying more than `const`/`title`/`description`) or a remote `$ref`
//! declines to `EditHint::None` — `validate.rs` still enforces those fully,
//! only the editing *widget* stays plain text.

use super::types::EditHint;
use crate::model::node::{Path, Seg};
use serde_json::Value as Json;

pub fn resolve_edit_hint(schema: &Json, path: &Path) -> EditHint {
    let Some(sub) = resolve_subschema(schema, schema, path) else {
        return EditHint::None;
    };
    hint_from_subschema(schema, sub)
}

/// Walk `path` from the schema root, following `properties`/`items` and
/// resolving same-document `$ref`s along the way.
fn resolve_subschema<'a>(root: &'a Json, current: &'a Json, path: &[Seg]) -> Option<&'a Json> {
    let current = deref(root, current)?;
    match path.split_first() {
        None => Some(current),
        Some((Seg::Key(k), rest)) => {
            let next = current.get("properties")?.get(k)?;
            resolve_subschema(root, next, rest)
        }
        Some((Seg::Index(_), rest)) => {
            let next = current.get("items")?;
            resolve_subschema(root, next, rest)
        }
    }
}

/// Resolve a single `$ref` hop if present — same-document only (`#/...`).
/// A remote `$ref` (no leading `#`) returns `None` unresolved, which
/// `resolve_subschema` propagates as "no hint" (spec: "remote `$ref`
/// resolution" is out of scope for editing hints).
fn deref<'a>(root: &'a Json, schema: &'a Json) -> Option<&'a Json> {
    let Some(r) = schema.get("$ref").and_then(Json::as_str) else {
        return Some(schema);
    };
    let pointer = r.strip_prefix('#')?;
    root.pointer(pointer)
}

fn hint_from_subschema(root: &Json, sub: &Json) -> EditHint {
    let Some(sub) = deref(root, sub) else {
        return EditHint::None;
    };
    if let Some(values) = sub.get("enum").and_then(Json::as_array) {
        return EditHint::Enum(
            values
                .iter()
                .map(|v| (display_label(v), v.clone()))
                .collect(),
        );
    }
    if let Some(v) = sub.get("const") {
        return EditHint::Enum(vec![(display_label(v), v.clone())]);
    }
    if let Some(opts) = oneof_of_const(root, sub) {
        return EditHint::Enum(opts);
    }
    let minimum = sub.get("minimum").and_then(Json::as_f64);
    let maximum = sub.get("maximum").and_then(Json::as_f64);
    let multiple_of = sub.get("multipleOf").and_then(Json::as_f64);
    if minimum.is_some() || maximum.is_some() || multiple_of.is_some() {
        return EditHint::Bounded { minimum, maximum, multiple_of };
    }
    EditHint::None
}

/// The `oneOf`/`anyOf`-of-`const` carve-out: every branch must be a bare
/// `{const, title?, description?}` object (no other keywords) for this to
/// fire — any richer branch (e.g. carrying its own `type`/`properties`) is
/// true composition and declines to `None`.
fn oneof_of_const(root: &Json, sub: &Json) -> Option<Vec<(String, Json)>> {
    let branches = sub
        .get("oneOf")
        .or_else(|| sub.get("anyOf"))
        .and_then(Json::as_array)?;
    let allowed_keys = ["const", "title", "description"];
    let mut opts = Vec::with_capacity(branches.len());
    for branch in branches {
        let branch = deref(root, branch)?;
        let obj = branch.as_object()?;
        if obj.keys().any(|k| !allowed_keys.contains(&k.as_str())) {
            return None; // richer branch — true composition, decline
        }
        let value = obj.get("const")?;
        let label = obj
            .get("title")
            .and_then(Json::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| display_label(value));
        opts.push((label, value.clone()));
    }
    Some(opts)
}

fn display_label(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        other => other.to_string(),
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p confy-core --test schema_headless`
Expected: PASS (21 tests total).

- [ ] **Step 5: Commit**

```bash
git add crates/confy-core/src/schema/hints_edit.rs crates/confy-core/tests/schema_headless.rs
git commit -m "feat(schema): resolve constrained-editing hints incl. oneOf-of-const carve-out"
```

---

### Task 6: `Session` integration — schema state, loading handshake, revalidation

**Files:**
- Modify: `crates/confy-core/src/session/session.rs`
- Modify: `crates/confy-core/src/session/state.rs`
- Test: `crates/confy-core/tests/schema_headless.rs`

**Interfaces:**
- Consumes: `schema::{detect_hint, bridge, validate, resolve_edit_hint, types::*}` (Tasks 1-5); existing `Session` fields (`doc: Option<AnyDocument>`, `tree: NodeTree`, `mode: Mode`) and existing `Node`/`Path`/`Seg` types (exact signatures per Task grounding above).
- Produces: `Session.schema: Option<SchemaState>`; `Session::detect_and_request_schema(&mut self) -> Option<SchemaSource>` (called once after document load — Task 9/13/17 hosts call this then resolve the source and dispatch `SchemaLoaded`); `Session::apply_schema_text(&mut self, source: SchemaSource, text: Result<String, String>)`; `Session::revalidate_schema(&mut self)`; `Session::schema_enum_move(&mut self, delta: i32)`; `Session::schema_enum_commit(&mut self)`; `Session::schema_allows_nudge(&self, path: &Path, new_repr: &str) -> bool` (clamps the existing arrow-key nudge against a `Bounded` hint — spec §3). Task 7 (`Intent`/`dispatch.rs`) wires all of these to intents.

- [ ] **Step 1: Write the failing tests**

Append to `crates/confy-core/tests/schema_headless.rs`:

```rust
use confy_core::session::Session;

fn session_from(src: &str, format: DocFormat) -> Session {
    let doc = AnyDocument::from_str_as(src, format).unwrap();
    Session::new(doc)
}

#[test]
fn session_detects_toml_schema_hint_on_construction() {
    let s = session_from("#:schema ./s.json\nport = 1\n", DocFormat::Toml);
    // Detection itself doesn't load — schema stays None until the host
    // resolves the fetch request and dispatches the text back.
    assert!(s.schema.is_none());
}

#[test]
fn session_detect_and_request_schema_returns_the_hint() {
    let mut s = session_from("#:schema ./s.json\nport = 1\n", DocFormat::Toml);
    let source = s.detect_and_request_schema();
    assert_eq!(source, Some(SchemaSource::Local("./s.json".into())));
}

#[test]
fn session_detect_and_request_schema_none_without_a_hint() {
    let mut s = session_from("port = 1\n", DocFormat::Toml);
    assert_eq!(s.detect_and_request_schema(), None);
}

#[test]
fn session_apply_schema_text_compiles_and_revalidates() {
    let mut s = session_from("port = \"nope\"\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "port": { "type": "integer" } }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    let state = s.schema.as_ref().expect("schema loaded");
    assert!(state.load_error.is_none());
    assert_eq!(state.violations.len(), 1);
    assert_eq!(state.violations[0].keyword, "type");
}

#[test]
fn session_apply_schema_text_load_error_is_soft() {
    let mut s = session_from("port = 1\n", DocFormat::Toml);
    s.apply_schema_text(
        SchemaSource::Local("./missing.json".into()),
        Err("file not found".into()),
    );
    let state = s.schema.as_ref().expect("schema state present even on load error");
    assert!(state.load_error.is_some());
    assert!(state.compiled.is_none());
    // The document is still fully editable — no error on the session itself.
    assert!(s.error.is_none());
}

#[test]
fn session_revalidates_after_a_mutation_commit() {
    let mut s = session_from("port = 1\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "port": { "type": "string" } }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    assert_eq!(s.schema.as_ref().unwrap().violations.len(), 1);
    // Fix the value via the same Replace mutation path CommitEdit/Nudge use.
    let path = vec![Seg::Key("port".into())];
    let doc = s.doc.as_mut().unwrap();
    let fragment = doc.scalar_fragment(Some("port"), "\"eighty\"");
    doc.apply(confy_core::model::document::Mutation::Replace { path, fragment })
        .unwrap();
    s.tree = doc.project();
    s.revalidate_schema();
    assert!(s.schema.as_ref().unwrap().violations.is_empty());
}

#[test]
fn session_begin_inline_edit_sets_schema_enum_mode_for_an_enum_constrained_node() {
    use confy_core::session::state::Mode;
    let mut s = session_from("level = \"debug\"\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "level": { "enum": ["debug", "info"] } }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    s.cursor = vec![Seg::Key("level".into())];
    s.begin_inline_edit();
    assert!(matches!(s.mode, Mode::SchemaEnum(_)));
}

#[test]
fn session_schema_enum_commit_writes_the_chosen_value() {
    use confy_core::session::state::Mode;
    let mut s = session_from("level = \"debug\"\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "level": { "enum": ["debug", "info"] } }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    s.cursor = vec![Seg::Key("level".into())];
    s.begin_inline_edit();
    s.schema_enum_move(1); // move to "info"
    s.schema_enum_commit();
    assert!(matches!(s.mode, Mode::Normal));
    let node = s.tree.node_at(&[Seg::Key("level".into())]).unwrap();
    assert_eq!(node.value.as_deref(), Some("\"info\""));
}

#[test]
fn dispatch_nudge_clamps_to_schema_maximum() {
    let mut s = session_from("port = 65534\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "port": { "type": "integer", "minimum": 1, "maximum": 65535 } }
    })
    .to_string();
    s.apply_schema_text(SchemaSource::Local("./s.json".into()), Ok(schema_text));
    s.cursor = vec![Seg::Key("port".into())];
    // 65534 -> 65535 lands exactly at the maximum: allowed.
    let snap = s.dispatch(confy_core::session::Intent::Nudge(1));
    let row = snap.rows.iter().find(|r| r.key == "port").unwrap();
    assert_eq!(row.value.as_deref(), Some("65535"));
    // 65535 -> 65536 would exceed the maximum: clamped, silently a no-op.
    let snap = s.dispatch(confy_core::session::Intent::Nudge(1));
    let row = snap.rows.iter().find(|r| r.key == "port").unwrap();
    assert_eq!(row.value.as_deref(), Some("65535"));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p confy-core --test schema_headless`
Expected: FAIL to compile — `Session.schema`, `detect_and_request_schema`, `apply_schema_text`, `revalidate_schema`, `schema_enum_move`, `schema_enum_commit`, and `Mode::SchemaEnum` don't exist yet.

- [ ] **Step 3: Add `Mode::SchemaEnum` and `SchemaEnumState` to `state.rs`**

In `crates/confy-core/src/session/state.rs`, add (near `KindSwitchState`, matching its exact shape):

```rust
/// State for the schema-enum picker popup (spec §3: reuses the `K`
/// kind-switch popup's shape on every host). `options` are `(display_label,
/// value_repr)` pairs — `value_repr` is the document-format scalar text
/// `Session::schema_enum_commit` splices in directly via
/// `ConfigDocument::scalar_fragment`.
#[derive(Clone, Debug)]
pub struct SchemaEnumState {
    pub path: Path,
    pub key: String,
    pub is_element: bool,
    pub options: Vec<(String, String)>,
    pub cursor: usize,
}
```

Add a variant to the `Mode` enum (alongside the existing `KindSwitch(KindSwitchState)` arm):

```rust
    SchemaEnum(SchemaEnumState),
```

- [ ] **Step 4: Implement the `Session` methods**

In `crates/confy-core/src/session/session.rs`:

Add the field to `struct Session` (alongside `pub error: Option<String>,`):

```rust
    pub schema: Option<crate::schema::SchemaState>,
```

Add to `Session::new`'s field-initializer list (wherever `error: None,` is initialized — mirror it):

```rust
            schema: None,
```

Add these methods (near `begin_inline_edit`, in the same `impl Session` block):

```rust
    /// Detect an in-file schema hint on the current document. Does **not**
    /// load anything (confy-core is fs-free) — the host resolves the
    /// returned `SchemaSource` (local read or URL fetch) and calls
    /// `apply_schema_text` with the result. Returns `None` (leaving
    /// `self.schema` untouched) when no hint is found — editing proceeds
    /// exactly as before (spec §1).
    pub fn detect_and_request_schema(&mut self) -> Option<crate::schema::SchemaSource> {
        let doc = self.doc.as_ref()?;
        let text = doc.serialize();
        crate::schema::hints::detect_hint(&text, doc.format())
    }

    /// The host resolved `source`'s text (or failed to). `Ok` compiles and
    /// validates; `Err` sets a soft `load_error` — never touches
    /// `self.error`, and the document stays fully editable either way
    /// (spec §1: "never blocks opening, editing, or saving").
    pub fn apply_schema_text(&mut self, source: crate::schema::SchemaSource, text: Result<String, String>) {
        let state = match text {
            Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(raw) => match jsonschema::Validator::new(&raw) {
                    Ok(compiled) => crate::schema::SchemaState {
                        source,
                        compiled: Some(compiled),
                        raw: Some(raw),
                        violations: Vec::new(),
                        load_error: None,
                    },
                    Err(e) => crate::schema::SchemaState {
                        source,
                        compiled: None,
                        raw: None,
                        violations: Vec::new(),
                        load_error: Some(format!("invalid schema: {e}")),
                    },
                },
                Err(e) => crate::schema::SchemaState {
                    source,
                    compiled: None,
                    raw: None,
                    violations: Vec::new(),
                    load_error: Some(format!("schema is not valid JSON: {e}")),
                },
            },
            Err(msg) => crate::schema::SchemaState {
                source,
                compiled: None,
                raw: None,
                violations: Vec::new(),
                load_error: Some(msg),
            },
        };
        self.schema = Some(state);
        self.revalidate_schema();
    }

    /// Re-run validation against the current tree. Called after every
    /// successful mutation commit and once right after `apply_schema_text`.
    /// A no-op when no schema is loaded or it failed to compile.
    pub fn revalidate_schema(&mut self) {
        let Some(state) = self.schema.as_mut() else { return };
        let Some(compiled) = state.compiled.as_ref() else { return };
        let Some(doc) = self.doc.as_ref() else { return };
        let Ok((value, _warnings)) = doc.to_value() else {
            // A YAML opaque node or similar blocks `to_value()` — leave the
            // previous violation list rather than silently clearing it.
            return;
        };
        let (projection, map) = crate::schema::value_bridge::bridge(&self.tree.root, &value);
        state.violations = crate::schema::validate::validate(&projection, compiled, &map);
    }

    pub fn schema_enum_move(&mut self, delta: i32) {
        if let crate::session::state::Mode::SchemaEnum(st) = &mut self.mode {
            let len = st.options.len() as i32;
            if len == 0 {
                return;
            }
            st.cursor = ((st.cursor as i32 + delta).rem_euclid(len)) as usize;
        }
    }

    pub fn schema_enum_commit(&mut self) {
        let crate::session::state::Mode::SchemaEnum(st) =
            std::mem::replace(&mut self.mode, crate::session::state::Mode::Normal)
        else {
            return;
        };
        let Some(doc) = self.doc.as_mut() else { return };
        let Some((_, value_repr)) = st.options.get(st.cursor) else { return };
        let fragment = doc.scalar_fragment(
            if st.is_element { None } else { Some(&st.key) },
            value_repr,
        );
        match doc.apply(crate::model::document::Mutation::Replace {
            path: st.path.clone(),
            fragment,
        }) {
            Ok(()) => {
                self.tree = self.doc.as_ref().unwrap().project();
                self.status = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        self.revalidate_schema();
    }
```

Modify `begin_inline_edit()` (currently ends by setting `self.mode = Mode::Edit(EditState{...}); self.status = None;` — full body quoted in this plan's grounding research): insert a schema-hint check **before** the final `self.mode = Mode::Edit(...)` assignment, replacing it with a branch. The existing body computes `path`, `key`, `is_element`, `buffer`, etc. — keep all of that (it's still needed for the plain-text fallback), and change only the final assignment:

```rust
        // (existing body above builds `path`, `key`, `is_element`, `buffer`,
        // `orig_trailing`, etc. — unchanged)
        if let Some(hint) = self.schema.as_ref().and_then(|s| s.raw.as_ref()).map(|raw| {
            crate::schema::hints_edit::resolve_edit_hint(raw, &path)
        }) {
            if let crate::schema::EditHint::Enum(options) = hint {
                if !options.is_empty() {
                    let format = self.doc.as_ref().map(|d| d.format());
                    let opts: Vec<(String, String)> = options
                        .into_iter()
                        .filter_map(|(label, v)| scalar_repr_for(&v, format?).map(|r| (label, r)))
                        .collect();
                    if !opts.is_empty() {
                        self.mode = crate::session::state::Mode::SchemaEnum(
                            crate::session::state::SchemaEnumState {
                                path,
                                key,
                                is_element,
                                cursor: 0,
                                options: opts,
                            },
                        );
                        self.status = None;
                        return;
                    }
                }
            }
        }
        self.mode = crate::session::state::Mode::Edit(crate::session::state::EditState {
            path,
            key,
            field: crate::session::state::EditField::Value,
            is_element,
            is_comment,
            rename_only: false,
            buffer,
            cursor: buffer_cursor,
            scroll: 0,
            other_buffer: key_for_other,
            other_cursor: name_cursor,
            other_scroll: 0,
            orig_trailing,
            created_on_add: false,
        });
        self.status = None;
```

(The exact local variable names above — `buffer_cursor`, `key_for_other` — must match whatever `begin_inline_edit`'s existing body actually names them; when applying this task, re-read the live function body first and rename these two placeholders to match, since the plan's earlier grounding read paraphrased the body rather than quoting every local variable name verbatim.)

Add the scalar-repr helper as a free function in `session.rs` (near `nudge_scalar`):

```rust
/// A schema enum/const JSON value's text repr for `ConfigDocument::scalar_fragment`.
/// `format!("{:?}", s)` (Rust's Debug for `&str`) produces a `"…"`
/// backslash-escaped double-quoted form that is simultaneously valid TOML
/// basic-string, JSON string, and YAML double-quoted syntax — one repr
/// serves all three backends. `Json::Null` has no TOML representation (spec
/// §2: TOML never produces `Value::Null`) — filtered out for a TOML
/// document so the enum picker never offers an unwritable option.
fn scalar_repr_for(v: &serde_json::Value, format: crate::model::document::DocFormat) -> Option<String> {
    use crate::model::document::DocFormat;
    match v {
        serde_json::Value::String(s) => Some(format!("{s:?}")),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        serde_json::Value::Null if format == DocFormat::Toml => None,
        serde_json::Value::Null => Some("null".to_string()),
        _ => None, // arrays/objects are not valid scalar enum options
    }
}
```

- [ ] **Step 5: Wire the clamp into the arrow-key nudge path**

Add a new method to the same `impl Session` block:

```rust
    /// Whether nudging the node at `path` to `new_repr` stays within any
    /// schema `Bounded` constraint on it. `true` (allowed) whenever no
    /// schema is loaded, the node has no `Bounded` hint, or `new_repr`
    /// doesn't parse as a number — the arrow-key nudge only *clamps*
    /// against a schema, it never gains new rejection power beyond that
    /// (spec §3: "Free-text inline typing stays unclamped" — this guard is
    /// arrow-key-nudge-only, never applied to a typed/committed edit).
    fn schema_allows_nudge(&self, path: &crate::model::node::Path, new_repr: &str) -> bool {
        let Some(state) = self.schema.as_ref() else { return true };
        let Some(raw) = state.raw.as_ref() else { return true };
        let hint = crate::schema::hints_edit::resolve_edit_hint(raw, path);
        let crate::schema::EditHint::Bounded { minimum, maximum, .. } = hint else {
            return true;
        };
        let Ok(n) = new_repr.replace('_', "").parse::<f64>() else {
            return true;
        };
        if let Some(min) = minimum {
            if n < min {
                return false;
            }
        }
        if let Some(max) = maximum {
            if n > max {
                return false;
            }
        }
        true
    }
```

Then wire it into the existing Nudge handling. Search `crates/confy-core/src/session/session.rs` and `dispatch.rs` for `nudge_scalar(` — its one non-definition call site is `Intent::Nudge`'s handler (either a dedicated `pub fn nudge(&mut self, delta: i64)` method, or inlined directly in `dispatch.rs`'s `Intent::Nudge(delta) => { ... }` arm). That call site computes a new repr string via `nudge_scalar(st, fmt, repr, delta)` and, when `Some(new_repr)`, applies it via `doc.apply(Mutation::Replace { path, fragment })` (or an equivalent `scalar_fragment`-wrapped call — mirror `schema_enum_commit`'s `Mutation::Replace` usage above for the exact shape). Guard that existing apply call:

```rust
                    if self.schema_allows_nudge(&path, &new_repr) {
                        // ...the existing apply-and-reproject logic, unchanged...
                    }
                    // else: leave the document untouched — the nudge silently
                    // clamps at the schema boundary (spec §3).
```

Add `self.revalidate_schema();` at the end of the Nudge handler (alongside wherever it already calls `self.tree = doc.project();`), matching every other mutating path in this task.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p confy-core --test schema_headless`
Expected: PASS (31 tests total).

- [ ] **Step 7: Commit**

```bash
git add crates/confy-core/src/session/session.rs crates/confy-core/src/session/state.rs crates/confy-core/tests/schema_headless.rs
git commit -m "feat(schema): wire SchemaState into Session with revalidation and enum-picker edit mode"
```

---

### Task 7: `SessionSnapshot`/`ViewRow`/`Intent`/`dispatch` wiring

**Files:**
- Modify: `crates/confy-core/src/session/view.rs`
- Modify: `crates/confy-core/src/session/intent.rs`
- Modify: `crates/confy-core/src/session/dispatch.rs`
- Test: `crates/confy-core/tests/schema_headless.rs`

**Interfaces:**
- Consumes: Task 6's `Session` methods; existing `SessionSnapshot`/`ViewRow`/`Intent`/`ModeView` (exact field lists per grounding research above).
- Produces: `SessionSnapshot.schema_status: Option<SchemaStatus>`, `SessionSnapshot.schema_fetch_request: Option<SchemaSource>`, `ViewRow.schema_warn: Option<Vec<String>>`, `Intent::{SetSchema, SchemaLoaded, SchemaEnumMove, SchemaEnumCommit}`, `ModeView::SchemaEnum { options: Vec<String>, cursor: usize }` — every host task (9, 11, 13, 17) consumes these via `dispatch()`/`snapshot()`.

- [ ] **Step 1: Write the failing test**

Append to `crates/confy-core/tests/schema_headless.rs`:

```rust
use confy_core::session::Intent;

#[test]
fn dispatch_schema_loaded_populates_snapshot_status_and_row_warnings() {
    let mut s = session_from("port = \"nope\"\n", DocFormat::Toml);
    let schema_text = json!({
        "type": "object",
        "properties": { "port": { "type": "integer" } }
    })
    .to_string();
    let snap = s.dispatch(Intent::SchemaLoaded {
        source: SchemaSource::Local("./s.json".into()),
        text: Ok(schema_text),
    });
    let status = snap.schema_status.expect("schema_status set");
    assert_eq!(status.violation_count, 1);
    let port_row = snap.rows.iter().find(|r| r.key == "port").unwrap();
    assert!(port_row.schema_warn.is_some());
    assert!(port_row.schema_warn.as_ref().unwrap()[0].contains("type"));
}

#[test]
fn dispatch_set_schema_requests_a_fetch() {
    let mut s = session_from("port = 1\n", DocFormat::Toml);
    let snap = s.dispatch(Intent::SetSchema {
        source: SchemaSource::Local("./explicit.json".into()),
    });
    assert_eq!(
        snap.schema_fetch_request,
        Some(SchemaSource::Local("./explicit.json".into()))
    );
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p confy-core --test schema_headless`
Expected: FAIL to compile — `Intent::SchemaLoaded`/`SetSchema`, `SessionSnapshot.schema_status`/`schema_fetch_request`, `ViewRow.schema_warn` don't exist yet.

- [ ] **Step 3: Extend `view.rs`**

In `crates/confy-core/src/session/view.rs`, add a field to `ViewRow` (alongside `pub read_only: bool,`):

```rust
    /// Soft-constraint violation messages whose Path == this row's Path;
    /// `None` = clean. Never blocks anything (`CONTEXT.md` § Schema
    /// "Soft constraint").
    pub schema_warn: Option<Vec<String>>,
```

Add fields to `SessionSnapshot` (alongside `pub error: Option<String>,` / `pub convert_write: Option<(String, String)>,`):

```rust
    pub schema_status: Option<crate::schema::SchemaStatus>,
    /// Set when a detected/explicit schema source needs the host to resolve
    /// its text (local read or URL fetch) and dispatch `Intent::SchemaLoaded`
    /// back — mirrors `external_edit`/`convert_write`'s async-signal shape.
    pub schema_fetch_request: Option<crate::schema::SchemaSource>,
```

Add a variant to `ModeView` (the `pub enum ModeView` that `Mode::Edit(EditState)` maps to `ModeView::Edit(EditView)` for — mirror that mapping pattern) for the schema-enum picker:

```rust
    SchemaEnum { options: Vec<String>, cursor: usize },
```

Wherever `Session::snapshot()` builds `ModeView` from `self.mode` (a `match &self.mode { Mode::Edit(e) => ModeView::Edit(EditView{...}), ... }`), add an arm:

```rust
            crate::session::state::Mode::SchemaEnum(st) => ModeView::SchemaEnum {
                options: st.options.iter().map(|(label, _)| label.clone()).collect(),
                cursor: st.cursor,
            },
```

Wherever `Session::snapshot()` builds `SessionSnapshot { ... }` field-by-field, add:

```rust
            schema_status: self.schema.as_ref().map(|s| s.status()),
            schema_fetch_request: self.pending_schema_fetch.take(),
```

(`pending_schema_fetch` is a new one-shot field — add `pub pending_schema_fetch: Option<crate::schema::SchemaSource>,` to `struct Session` in `session.rs`, alongside `pub schema: Option<crate::schema::SchemaState>,`, initialized `None` in `Session::new`. `detect_and_request_schema`/the new `Intent::SetSchema` handler set it; `snapshot()`'s `.take()` clears it after one read, exactly mirroring how `pending_external_edit`'s `external_edit_view()` is read-then-cleared.)

Wherever `Session::snapshot()` builds each `ViewRow` from a `VisibleRow` (the `.map(|r| { ... ViewRow { ... } })` in `session.rs`'s `compute_rows`/`visible_rows`), add:

```rust
                    schema_warn: self.schema.as_ref().and_then(|s| {
                        let msgs: Vec<String> = s
                            .violations
                            .iter()
                            .filter(|v| v.path == r.node.path)
                            .map(|v| v.message.clone())
                            .collect();
                        (!msgs.is_empty()).then_some(msgs)
                    }),
```

- [ ] **Step 4: Extend `intent.rs`**

In `crates/confy-core/src/session/intent.rs`, add four variants to the `Intent` enum (grouped under a new `// Schema` comment header, following the file's existing per-section-comment convention):

```rust
    // Schema
    SetSchema { source: crate::schema::SchemaSource },
    SchemaLoaded { source: crate::schema::SchemaSource, text: Result<String, String> },
    SchemaEnumMove(i32),
    SchemaEnumCommit,
```

- [ ] **Step 5: Wire `dispatch.rs`**

In `crates/confy-core/src/session/dispatch.rs`'s `match intent { ... }`, add arms (following the existing style — most arms are a single delegating call):

```rust
            Intent::SetSchema { source } => self.pending_schema_fetch = Some(source),
            Intent::SchemaLoaded { source, text } => self.apply_schema_text(source, text),
            Intent::SchemaEnumMove(delta) => self.schema_enum_move(delta),
            Intent::SchemaEnumCommit => self.schema_enum_commit(),
```

Also: right after document construction inside `Session::new` (or wherever a fresh `Session` first becomes ready — the natural place is the very end of `Session::new`'s body, once `tree`/`doc` are set), call `self.detect_and_request_schema()` and stash the result into `self.pending_schema_fetch` so a freshly opened file's in-file hint surfaces on the very first `snapshot()`/`dispatch()` without requiring an explicit `SetSchema`:

```rust
        // At the end of `Session::new`, after all other fields are set:
        session.pending_schema_fetch = session.detect_and_request_schema();
```

(Adjust to match `Session::new`'s actual construction style — if it builds `Self { ... }` and returns it directly rather than a mutable `session` binding, change the last two lines to build the struct, bind it to `let mut session = Self { ... };`, run the detection call, then `session`.)

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p confy-core --test schema_headless`
Expected: PASS (33 tests total).

- [ ] **Step 7: Run the full confy-core test suite to confirm no regressions**

Run: `cargo test -p confy-core`
Expected: PASS — all pre-existing tests (including `session_headless.rs`) still pass; the `ViewRow`/`SessionSnapshot` field additions are additive and every existing test constructs these via `Session::snapshot()`, not struct literals, so no other test file needs updating.

- [ ] **Step 8: Commit**

```bash
git add crates/confy-core/src/session/view.rs crates/confy-core/src/session/intent.rs crates/confy-core/src/session/dispatch.rs crates/confy-core/tests/schema_headless.rs
git commit -m "feat(schema): thread SessionSnapshot/ViewRow/Intent schema fields through dispatch"
```

---

Phase 1 is now complete and independently verifiable: `cargo test -p confy-core` exercises the full schema engine headlessly, with zero host involvement.

---

## Phase 2 — confy-tui

### Task 8: `--schema` CLI flag + `schema_io.rs` (local read + URL fetch)

**Files:**
- Create: `crates/confy-tui/src/tui/schema_io.rs`
- Modify: `crates/confy-tui/src/cli.rs`
- Modify: `crates/confy-tui/src/tui/mod.rs`
- Modify: `crates/confy-tui/Cargo.toml`
- Modify: `Cargo.toml` (workspace root)
- Test: `crates/confy-tui/tests/` (new file `schema_io.rs`, unit-testing only the pure/local-file part; URL fetch is smoke-tested manually per Step 6, not unit tested, matching this crate's existing convention of not mocking network I/O)

**Interfaces:**
- Consumes: `Session::dispatch(Intent::SchemaLoaded{..})` (Task 7), `SessionSnapshot.schema_fetch_request` (Task 7).
- Produces: `pub fn resolve_schema_source(source: &SchemaSource, open_file_dir: &Path) -> Result<String, String>` — Task 9/App startup consumes this.

- [ ] **Step 1: Add the `ureq` dependency**

In `Cargo.toml` (workspace root) `[workspace.dependencies]`:

```toml
ureq = "2"
```

In `crates/confy-tui/Cargo.toml` `[dependencies]`:

```toml
ureq.workspace = true
```

- [ ] **Step 2: Write the failing test for local resolution**

Create `crates/confy-tui/tests/schema_io.rs`:

```rust
use confy_core::schema::SchemaSource;
use confy_tui::tui::schema_io::resolve_schema_source;
use std::fs;

#[test]
fn resolves_a_local_relative_path_against_the_open_files_directory() {
    let dir = std::env::temp_dir().join("confy_schema_io_test");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("s.json"), r#"{"type":"object"}"#).unwrap();
    let source = SchemaSource::Local("./s.json".into());
    let result = resolve_schema_source(&source, &dir);
    assert_eq!(result, Ok(r#"{"type":"object"}"#.to_string()));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_missing_local_file_is_a_soft_error_not_a_panic() {
    let dir = std::env::temp_dir().join("confy_schema_io_test_missing");
    let source = SchemaSource::Local("./nope.json".into());
    let result = resolve_schema_source(&source, &dir);
    assert!(result.is_err());
}
```

(`confy_tui::tui::schema_io` must be reachable as a library item for this integration test to import it — confirm `crates/confy-tui/src/lib.rs` exists and re-exports `pub mod tui;`; if `confy-tui` is currently bin-only with no `lib.rs`, add one that does `pub mod tui;` and have `main.rs` depend on the new lib crate, matching how `confy-tauri` already splits `confy_tauri_lib`/`confy-desktop` bin. Check `crates/confy-tui/src/main.rs`'s module declarations before assuming — if `mod tui;` is already declared there and no `lib.rs` exists, this integration test instead belongs as a `#[cfg(test)]` unit test inside `schema_io.rs` itself, which every other assertion in this task should follow for consistency.)

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p confy-tui --test schema_io` (or `cargo test -p confy-tui schema_io` if relocated to a unit test per Step 2's note)
Expected: FAIL to compile — `resolve_schema_source` doesn't exist yet.

- [ ] **Step 4: Implement `schema_io.rs`**

Create `crates/confy-tui/src/tui/schema_io.rs`:

```rust
//! Host-side schema-source resolution for the TUI: a local hint/override
//! resolves against the open file's directory (spec §1); a URL hint fetches
//! over HTTP with a blocking client (confy-tui has no other networking —
//! this is the one new capability the schema feature adds to this crate).

use confy_core::schema::SchemaSource;
use std::path::Path;

pub fn resolve_schema_source(source: &SchemaSource, open_file_dir: &Path) -> Result<String, String> {
    match source {
        SchemaSource::Local(rel) => {
            let path = open_file_dir.join(rel);
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
        }
        SchemaSource::Url(url) => ureq::get(url)
            .call()
            .map_err(|e| format!("{url}: {e}"))?
            .into_string()
            .map_err(|e| format!("{url}: {e}")),
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p confy-tui schema_io`
Expected: PASS (2 tests).

- [ ] **Step 6: Add the `--schema` CLI flag and thread it through startup**

In `crates/confy-tui/src/cli.rs`, add a field to `struct Args` (alongside `lang: Option<String>,`):

```rust
    /// Path or URL to a JSON Schema, overriding in-file hint detection.
    #[arg(long)]
    schema: Option<String>,
```

In `run()`, thread it into `crate::tui::run`:

```rust
            crate::tui::run(&file, fmt, lang, args.schema)
```

In `crate::tui::run`'s signature (in `crates/confy-tui/src/tui/mod.rs`), add a fifth parameter `schema_override: Option<String>`. At startup, right after the `Session` is constructed (before the first draw), resolve the schema source:

```rust
    let open_file_dir = file.parent().unwrap_or_else(|| std::path::Path::new("."));
    let source = schema_override
        .map(|s| {
            if s.starts_with("http://") || s.starts_with("https://") {
                confy_core::schema::SchemaSource::Url(s)
            } else {
                confy_core::schema::SchemaSource::Local(s)
            }
        })
        .or_else(|| app.session.detect_and_request_schema());
    if let Some(source) = source {
        let text = crate::tui::schema_io::resolve_schema_source(&source, open_file_dir);
        app.session.apply_schema_text(source, text);
        app.rebuild_rows();
    }
```

(Place this after `App::new(...)` construction and before the main event loop's `loop { ... }` begins — `app.session` is the field `App` wraps per this plan's grounding research; `app.rebuild_rows()` is the existing TUI-side row-rebuild method already called elsewhere in this file after every mutating operation.)

Add `pub mod schema_io;` to `crates/confy-tui/src/tui/mod.rs`'s module declarations (alongside the existing `pub mod app;` / `mod ui;` etc. — match whichever are `pub` vs private; `schema_io` needs `pub` since Task 8's Step 2 test imports it from outside the crate).

- [ ] **Step 7: Manual smoke test for the URL path**

Run: `echo '{"type":"object","properties":{"port":{"type":"string"}}}' > /tmp/s.json && cargo run -p confy-tui -- --schema /tmp/s.json <(echo 'port = 1')`
Expected: the TUI opens with the `port` row showing a soft-warning indicator (visual confirmation deferred to Task 10, which adds the render; for this task, confirm via `cargo run -p confy-tui -- --schema /tmp/s.json <path>` exits cleanly with no panic/error, proving `resolve_schema_source`+`apply_schema_text` didn't crash the startup path).

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml crates/confy-tui/Cargo.toml crates/confy-tui/src/cli.rs crates/confy-tui/src/tui/mod.rs crates/confy-tui/src/tui/schema_io.rs crates/confy-tui/tests/schema_io.rs
git commit -m "feat(tui): resolve schema sources (local/URL) and thread --schema through startup"
```

---

### Task 9: Row/KIND soft-warning rendering + status-line summary + Detail popup messages

**Files:**
- Modify: `crates/confy-tui/src/tui/app.rs`
- Modify: `crates/confy-tui/src/tui/ui.rs`

**Interfaces:**
- Consumes: `ViewRow.schema_warn` (Task 7), `SessionSnapshot.schema_status` (Task 7).
- Produces: nothing new consumed by later tasks — this is a leaf rendering task.

- [ ] **Step 1: Extend `RowSnapshot` and `rebuild_rows()`**

`rebuild_rows()` (app.rs) maps each `ViewRow` into the TUI's own `RowSnapshot`. Find `RowSnapshot`'s struct definition (in the same file/region as `rebuild_rows`) and add a field:

```rust
    pub schema_warn: Option<Vec<String>>,
```

In `rebuild_rows()`'s `.map(|vr| { ... RowSnapshot { ... } })` closure, add:

```rust
                schema_warn: vr.schema_warn.clone(),
```

Also, in the same closure, append a `!` to the already-built `type_tag` string when `vr.schema_warn.is_some()` (the KIND column's fixed-8-column budget: `type_tag` is `format!("{slot:<8}")`, so appending after truncation would misalign columns — instead replace the tag's own padding call by re-deriving it here):

```rust
                type_tag: {
                    let base = type_tag(&n.kind, vr.format, doc_fmt, n.read_only);
                    if vr.schema_warn.is_some() {
                        format!("{}!", base.trim_end())
                    } else {
                        base
                    }
                },
```

(This assumes `type_tag(...)` is called inline in the existing closure exactly as described in this plan's grounding research — `n.read_only` and `doc_fmt` are already in scope there. If the live closure instead stores the *result* of an earlier `type_tag` call in a local first, adapt this snippet to wrap that local instead of re-calling `type_tag`.)

- [ ] **Step 2: Add the soft-warning row style in `draw_tree`**

In `crates/confy-tui/src/tui/ui.rs`'s `draw_tree`, the `style` is computed via a `match () { _ if into_here => ..., _ if active_slot.is_some() => base, _ if is_cursor => ..., _ => base }` — `base` is itself computed from clipboard/selection state just above. Extend `base`'s computation to layer a schema-warning accent when nothing higher-priority (clipboard/selection) applies:

Locate:

```rust
            let base = if in_clipboard_source {
                let cut = app.session.clipboard.as_ref().is_some_and(|cb| cb.cut);
                let bg = if cut { Color::Green } else { Color::Blue };
                Style::default().bg(bg).fg(Color::White)
            } else if app.session.selection.contains(&row.path) {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
```

Replace the final `else` branch:

```rust
            let base = if in_clipboard_source {
                let cut = app.session.clipboard.as_ref().is_some_and(|cb| cb.cut);
                let bg = if cut { Color::Green } else { Color::Blue };
                Style::default().bg(bg).fg(Color::White)
            } else if app.session.selection.contains(&row.path) {
                Style::default().bg(Color::DarkGray)
            } else if row.schema_warn.is_some() {
                // Subdued, not alarming — a soft constraint, never a hard error.
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };
```

- [ ] **Step 3: Add the status-line summary**

Find the function that renders the status/error line (referenced by this crate's existing `snap.status`/`snap.error` rendering — search `ui.rs` for wherever `app.session.status`/`app.session.error` is drawn; it's a single-line `Paragraph` widget). Extend it: when `app.session.schema.is_some()` and has violations, append `" · N schema warnings"` to the status text (using `app.session.schema.as_ref().map(|s| s.violations.len()).unwrap_or(0)`), styled with the same subdued `Color::Yellow` as the row accent above, not the error color.

- [ ] **Step 4: Add violation messages to the Detail popup**

Find `draw_detail_overlay` (or equivalent — the popup shown by `i`/`Mode::Detail`, per README's keybinding table: "Toggle the detail/info popup"). It renders `app.session.detail_text`. Append, when the cursor row has a non-empty `schema_warn`, a new section below the existing detail text: a blank line, then `"Schema:"`, then one line per message in `Color::Yellow`.

- [ ] **Step 5: Manual smoke test**

Run: `echo '{"type":"object","properties":{"port":{"type":"string"}}}' > /tmp/s.json && printf 'port = 1\n' > /tmp/c.toml && cargo run -p confy-tui -- --schema /tmp/s.json /tmp/c.toml`
Expected: the `port` row's KIND column shows a trailing `!`, the row text renders in yellow, the status line shows "1 schema warnings", and pressing `i` on that row shows the violation message in the Detail popup.

- [ ] **Step 6: Commit**

```bash
git add crates/confy-tui/src/tui/app.rs crates/confy-tui/src/tui/ui.rs
git commit -m "feat(tui): render soft schema-violation indicators and Detail-popup messages"
```

---

### Task 10: Schema-enum picker overlay + key routing

**Files:**
- Modify: `crates/confy-tui/src/tui/ui.rs`
- Modify: `crates/confy-tui/src/tui/mod.rs`
- Modify: `crates/confy-tui/src/tui/state.rs`

**Interfaces:**
- Consumes: `Mode::SchemaEnum(SchemaEnumState)` (Task 6), `Session::schema_enum_move`/`schema_enum_commit` (Task 6), `centered_rect` (existing, exact signature per grounding research).

- [ ] **Step 1: Re-export `SchemaEnumState`**

In `crates/confy-tui/src/tui/state.rs`, add `SchemaEnumState` to the existing re-export list (alongside `KindSwitchState`):

```rust
pub use confy_core::session::state::{
    Clipboard, ConvertState, ConvertStep, EditField, EditState, FilterLayer, HelpTab, History,
    KindSwitchState, Mode, PasteSlot, PendingComment, PromptKind, SchemaEnumState, ABOUT_TEXT,
};
```

- [ ] **Step 2: Add `draw_schema_enum_overlay`**

In `crates/confy-tui/src/tui/ui.rs`, add a new function directly modeled on `draw_kind_switch_overlay` (same file, quoted verbatim in this plan's grounding research), swapping the data source and title:

```rust
/// The schema-constrained enum/const picker: reuses the `K` kind-switch
/// popup's exact shape (spec §3/§5).
fn draw_schema_enum_overlay(f: &mut Frame, app: &App) {
    let Mode::SchemaEnum(st) = &app.session.mode else {
        return;
    };
    let lines: Vec<Line> = st
        .options
        .iter()
        .enumerate()
        .map(|(i, (label, _))| {
            let marker = if i == st.cursor { "›" } else { " " };
            let mut style = Style::default();
            if i == st.cursor {
                style = style.add_modifier(Modifier::REVERSED);
            }
            Line::from(Span::styled(format!(" {marker} {label:<28}"), style))
        })
        .collect();
    let height = (lines.len() as u16 + 2).min(f.area().height);
    let area = centered_rect(40, height, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(" Schema value ")
        .title_bottom(" ↑↓ move · Enter apply · Esc cancel ")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black).fg(Color::White));
    f.render_widget(Paragraph::new(lines).block(block), area);
}
```

Find wherever `draw_kind_switch_overlay(f, app)` is called (the top-level `draw` function's overlay dispatch, likely a `match &app.session.mode { Mode::KindSwitch(_) => draw_kind_switch_overlay(f, app), ... }` or a sequence of `if let` guards) and add:

```rust
    if matches!(app.session.mode, Mode::SchemaEnum(_)) {
        draw_schema_enum_overlay(f, app);
    }
```

(matching whatever conditional style the existing call site uses — an `if let`/`match` arm alongside the `KindSwitch` one, not a new standalone block, so overlay z-ordering/mutual-exclusivity with other modal overlays is preserved.)

- [ ] **Step 3: Add key routing**

In `crates/confy-tui/src/tui/mod.rs`, alongside the existing `Mode::KindSwitch` modal key-interception block (quoted verbatim in this plan's grounding research, at the point checking `matches!(app.session.mode, ... Mode::KindSwitch(_))`), add a parallel block:

```rust
            if matches!(app.session.mode, crate::tui::state::Mode::SchemaEnum(_)) {
                use crossterm::event::KeyCode;
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => app.session.schema_enum_move(-1),
                    KeyCode::Down | KeyCode::Char('j') => app.session.schema_enum_move(1),
                    KeyCode::Enter => {
                        app.session.schema_enum_commit();
                        app.rebuild_rows();
                    }
                    KeyCode::Esc => app.session.mode = crate::tui::state::Mode::Normal,
                    _ => {}
                }
                continue; // modal — swallow all other key handling this iteration
            }
```

(Place this check at the same point in the event loop as the existing `Mode::KindSwitch` block — before the general `KeyAction` dispatch — and confirm whether that surrounding loop iteration uses `continue` or an early return after the `if` block; match the existing `KindSwitch` block's control-flow exactly, since this plan's grounding research showed the `if` block's body but not what follows it in the loop.)

- [ ] **Step 4: Manual smoke test**

Run: `echo '{"type":"object","properties":{"level":{"enum":["debug","info","warn"]}}}' > /tmp/s.json && printf 'level = "debug"\n' > /tmp/c.toml && cargo run -p confy-tui -- --schema /tmp/s.json /tmp/c.toml`
Expected: cursor on `level`, press `e` — a centered "Schema value" popup opens listing debug/info/warn instead of a plain-text input; `↓` `↓` `Enter` commits `warn`; the row now reads `level = "warn"`.

- [ ] **Step 5: Commit**

```bash
git add crates/confy-tui/src/tui/ui.rs crates/confy-tui/src/tui/mod.rs crates/confy-tui/src/tui/state.rs
git commit -m "feat(tui): add the schema-constrained enum/const picker popup"
```

---

## Phase 3 — Web desktop

### Task 11: `types.ts` additions

**Files:**
- Modify: `web/types.ts`

**Interfaces:**
- Produces: `SchemaStatus`, `ViewRow.schema_warn`, `EditView.constraint`, `ModeView`'s schema-enum variant — Tasks 12-14 consume these.

- [ ] **Step 1: Add the types**

In `web/types.ts`, add to `ViewRow` (alongside `read_only: boolean;`):

```ts
  schema_warn: string[] | undefined;
```

Add a new exported interface (near `ExternalEdit`):

```ts
export interface SchemaStatus {
  source_label: string;
  violation_count: number;
  load_error: string | undefined;
}
```

Add to `SessionSnapshot` (alongside `external_edit: ExternalEdit | undefined;`):

```ts
  schema_status: SchemaStatus | undefined;
  schema_fetch_request: { Local: string } | { Url: string } | undefined;
```

Add to `EditView` (alongside `rename_only: boolean;`):

```ts
  constraint: { Enum: [string, unknown][] } | { Bounded: { minimum: number | null; maximum: number | null; multiple_of: number | null } } | "None" | undefined;
```

(This mirrors serde's default externally-tagged enum representation for `EditHint`/`SchemaSource` crossing serde-wasm-bindgen — `EditHint::None` a unit variant serializes as the bare string `"None"`, `EditHint::Enum(v)`/`EditHint::Bounded{..}` as `{ VariantName: payload }`, matching the `{ CommitEdit: {...} }` shape already documented in `confy-ffi/functional_smoke.mjs`'s Intent-dispatch convention from this plan's grounding research. However `EditState.constraint` was defined in Task 6 as `Option<EditHint>` on the internal `EditState`, not surfaced onto the transport `EditView` yet — add that mapping now too:)

In `crates/confy-core/src/session/view.rs` (confy-core, not `web/types.ts` — noting the cross-reference since this TS type has no meaning without it), add to `EditView`:

```rust
    pub constraint: Option<crate::schema::EditHint>,
```

And in wherever `Mode::Edit(e) => ModeView::Edit(EditView { ... })` is built, add `constraint: None,` (the schema-enum picker uses `Mode::SchemaEnum`, a *different* Mode variant entirely — per Task 6's design, `EditState` never actually carries a live `EditHint::Enum`/`Bounded` at runtime today, since `begin_inline_edit` branches to `Mode::SchemaEnum` instead of populating this field on `Mode::Edit`. This `EditView.constraint` field is therefore always `None` for now — included for forward-compatibility with a future `Bounded` numeric-clamp surfacing on the plain `Edit` mode (Task 12 uses it for that), not populated by this task.) Run `cargo check -p confy-core` after this edit to confirm the additive field doesn't break existing `EditView` construction call sites (there should be exactly one, in `snapshot()`).

- [ ] **Step 2: Run the TypeScript build to verify no type errors**

Run: `cd web && npx tsc --noEmit`
Expected: PASS — these are additive optional/union fields; no existing code references them yet, so nothing breaks.

- [ ] **Step 3: Commit**

```bash
git add web/types.ts crates/confy-core/src/session/view.rs
git commit -m "feat(web): add SchemaStatus/schema_warn/constraint TypeScript types"
```

---

### Task 12: `fs.ts`/`host-io.ts` schema resolution wiring

**Files:**
- Modify: `web/fs.ts`
- Modify: `web/host-io.ts`
- Modify: `web/ui.ts`

**Interfaces:**
- Consumes: `SessionSnapshot.schema_fetch_request` (Task 11), `fetchUrlFile` (existing, verbatim per grounding research).
- Produces: schema resolution wired into the `main()`/`openText()` render loop — Task 16 ("Attach schema…") builds on this.

- [ ] **Step 1: Add a local sibling-file reader to `fs.ts`**

In `web/fs.ts`, add (near `fetchUrlFile`):

```ts
/**
 * Read a schema file by relative path, against the directory of the
 * currently open file. FS Access API: resolves via the open file handle's
 * parent directory handle. Tauri: reads `dirOf(currentPath) + '/' + rel`
 * directly (unrestricted `fs:scope` — spec: Tauri/Android section). No
 * File System Access API directory handle is retained today (`fileHandle`
 * only holds the file handle, not its parent) — Chromium exposes
 * `handle.getParent?.()` behind an experimental flag some browsers lack, so
 * this degrades to a soft failure (`Promise.reject`) there rather than
 * probing an unstable API, consistent with the "never a hard-fail" schema
 * convention (spec §1) — the caller (openText's schema wiring) always
 * treats rejection as a soft `load_error`, never a UI-blocking error.
 */
export async function readSiblingFile(
  relativePath: string,
  currentFilePath: string | null,
): Promise<string> {
  const g = tauriGlobal();
  if (g?.fs && currentFilePath) {
    const dir = currentFilePath.split(/[\\/]/).slice(0, -1).join("/");
    const resolved = relativePath.startsWith("./") || relativePath.startsWith("../")
      ? `${dir}/${relativePath}`
      : relativePath;
    return g.fs.readTextFile(resolved);
  }
  throw new Error("local schema file resolution is not available on this host");
}
```

- [ ] **Step 2: Wire schema resolution into `host-io.ts`**

In `web/host-io.ts`, add a new exported function:

```ts
/**
 * Resolve `snap.schema_fetch_request` (if any) and dispatch the result back
 * as `Intent::SchemaLoaded`. Mirrors `openFromUrl`'s try/catch-to-soft-error
 * shape — a failure here never surfaces as `io.err`'s blocking banner, only
 * as `SchemaStatus.load_error` on the next snapshot (spec §1).
 */
export async function resolveSchemaFetchRequest(
  session: Session,
  request: { Local: string } | { Url: string },
  currentFilePath: string | null,
): Promise<SessionSnapshot> {
  const source = "Local" in request
    ? { Local: request.Local }
    : { Url: request.Url };
  let text: { Ok: string } | { Err: string };
  try {
    const raw = "Local" in request
      ? await readSiblingFile(request.Local, currentFilePath)
      : (await fetchUrlFile(request.Url)).text;
    text = { Ok: raw };
  } catch (e) {
    text = { Err: String((e as Error).message ?? e) };
  }
  return session.dispatch({ SchemaLoaded: { source, text } });
}
```

(Import `readSiblingFile`/`fetchUrlFile` from `./fs.js` and `Session`/`SessionSnapshot` from `./confy.js`/`./types.js` at the top of `host-io.ts`, matching this file's existing import style.)

- [ ] **Step 3: Call it from `ui.ts`'s render loop**

In `web/ui.ts`'s `render()` function (the one that already checks `if (snap.external_edit) openExternalEdit(...)` and `if (snap.convert_write) void doConvertWrite(...)` — same function, same pattern), add:

```ts
  if (snap.schema_fetch_request) {
    void resolveSchemaFetchRequest(io, session!, snap.schema_fetch_request, fileHandle?.path ?? null).then(
      (next) => {
        snap = next;
        render();
      },
    );
  }
```

(Adjust the exact call signature to match Step 2's final parameter list once written — `fileHandle`'s shape/`path` field per this plan's grounding research on `OpenedFile`/`FsHandle`; if `fileHandle` doesn't carry a `path` string directly, use whatever field `pickOpenFile`'s returned `OpenedFile.path` documents.)

- [ ] **Step 4: Manual smoke test**

Run: `cd web && node build.mjs && node serve.mjs`, then in a browser open a `.toml` file (via the Tauri desktop build, since FS Access API sibling-directory access isn't wired — see Step 1's note) containing `#:schema ./s.json` with a real `s.json` beside it.
Expected: no console error; `session.snapshot().schema_status` (checked via devtools) shows `violation_count` matching the schema.

- [ ] **Step 5: Commit**

```bash
git add web/fs.ts web/host-io.ts web/ui.ts
git commit -m "feat(web): resolve schema_fetch_request via local sibling read or URL fetch"
```

---

### Task 13: `<select>` inline-edit branch for `Enum`/`Const` hints

**Files:**
- Modify: `web/render.ts`
- Modify: `web/ui.ts`

**Interfaces:**
- Consumes: `EditView.constraint` (Task 11) — **superseded by Task 6's design choice** that the enum picker uses a dedicated `Mode::SchemaEnum`, not `Mode::Edit`. Web therefore needs its **own** rendering of `ModeView::SchemaEnum` (a `<select>` inline, not a TUI-style popup — spec §3's table: "renderValue()'s edit branch emits `<select>`"), driven by `snap.mode` being the `SchemaEnum` variant rather than `Edit`.

- [ ] **Step 1: Extend `renderValue()`**

In `web/render.ts`, `renderValue(r, edit)` currently branches only on `edit && r.is_cursor && edit.field === "Value"`. Add a schema-enum branch, checked first (it takes priority over the plain inline `<input>` when active) — this requires `renderRow`/`renderTree` to also pass the current `ModeView` down (currently only `edit: EditView | null` is threaded). Change `renderRow`'s signature to accept an additional parameter:

```ts
function renderRow(
  r: ViewRow,
  idx: number,
  rows: ViewRow[],
  edit: EditView | null,
  schemaEnum: { options: string[]; cursor: number } | null,
  clip: "" | " clip-copy" | " clip-cut",
): string {
```

Thread `schemaEnum` through to `renderValue`:

```ts
function renderValue(r: ViewRow, edit: EditView | null, schemaEnum: { options: string[]; cursor: number } | null): string {
  if (schemaEnum && r.is_cursor) {
    const opts = schemaEnum.options
      .map((label, i) => `<option value="${i}"${i === schemaEnum.cursor ? " selected" : ""}>${escapeHtml(label)}</option>`)
      .join("");
    return `<select class="cell-input mono" data-editing="value" data-schema-enum="1">${opts}</select>`;
  }
  if (edit && r.is_cursor && edit.field === "Value") {
    const seed = valueEditSeed(r, edit.buffer);
    return `<input class="cell-input mono" data-editing="value" style="${editWidthStyle(seed)}" value="${escapeHtml(seed)}" />`;
  }
  return escapeHtml((r.value ?? "").replace(/\r?\n/g, " ↵ "));
}
```

Update `renderRow`'s call site for `renderValue` (inside the `else` branch handling non-comment rows) to pass `schemaEnum` through. Update `renderTree`'s call to `renderRow` to compute and pass `schemaEnum` from `snap.mode` — add near the top of `renderTree`:

```ts
  const schemaEnum =
    "SchemaEnum" in snap.mode
      ? { options: snap.mode.SchemaEnum.options, cursor: snap.mode.SchemaEnum.cursor }
      : null;
```

(`snap.mode` is a discriminated union per the existing `ModeView` TypeScript type generated alongside `EditView`/etc. — add the `SchemaEnum` arm to that union type in `web/types.ts` now if Task 11 didn't already cover `ModeView` specifically; Task 11's scope was `EditView`/`SessionSnapshot`/`ViewRow` only, so add here: `export type ModeView = ... | { SchemaEnum: { options: string[]; cursor: number } };` alongside wherever `ModeView`'s other variants — `{ Edit: EditView }` etc. — are declared.)

- [ ] **Step 2: Wire commit/navigation in `ui.ts`**

In `web/ui.ts`, `focusInlineEdit()` currently only handles `input[data-editing]`. Add a sibling function for the schema-enum `<select>`, called from the same place `focusInlineEdit()` is called (search `render()` for that call site):

```ts
function focusSchemaEnumSelect() {
  const select = tree.querySelector("select[data-schema-enum]") as HTMLSelectElement | null;
  if (!select) return;
  select.focus();
  select.onchange = () => {
    const idx = Number(select.value);
    const current = snap && "SchemaEnum" in snap.mode ? snap.mode.SchemaEnum.cursor : 0;
    send({ SchemaEnumMove: idx - current });
    send("SchemaEnumCommit");
  };
  select.onkeydown = (e) => {
    if (e.key === "Escape") {
      e.preventDefault();
      send("Escape");
    }
  };
}
```

At the call site (in `render()`, near the existing `focusInlineEdit()` call, likely guarded by `if (edit) focusInlineEdit();`), add a parallel guard:

```ts
  if (snap.mode && "SchemaEnum" in snap.mode) focusSchemaEnumSelect();
```

- [ ] **Step 3: Manual smoke test**

Run: `cd web && node build.mjs && node serve.mjs`, open a config with a schema hint whose current node's editor is triggered (double-click the value cell of an `enum`-constrained field).
Expected: a native `<select>` dropdown appears in place of the text input, pre-selected to the current value; choosing another option commits immediately.

- [ ] **Step 4: Commit**

```bash
git add web/render.ts web/ui.ts web/types.ts
git commit -m "feat(web): render a <select> for schema enum/const-constrained values"
```

---

### Task 14: `.schema-warn` CSS + row wiring + status-line summary

**Files:**
- Modify: `web/render.ts`
- Modify: `web/style.css`
- Modify: `web/ui.ts`

**Interfaces:**
- Consumes: `ViewRow.schema_warn` (Task 11), `SessionSnapshot.schema_status` (Task 11).

- [ ] **Step 1: Add the CSS**

In `web/style.css`, add (near the existing `.row.clip-copy`/`.row.clip-cut` rules):

```css
.row.schema-warn { outline: 1.5px dashed var(--warn); outline-offset: -3px; border-radius: 6px; }
.row.schema-warn::after { content: ""; position: absolute; right: 4px; top: 4px; width: 6px; height: 6px; border-radius: 50%; background: var(--warn); }
```

Add the CSS variable to the root theme block (wherever `--accent`/`--sel-edge` etc. are declared — the `:root { ... }` block near the top of the file):

```css
  --warn: #d9a441;
```

- [ ] **Step 2: Wire the class in `renderRow`**

In `web/render.ts`'s `renderRow`, the `cls` string is built from several booleans. Add:

```ts
    `${r.schema_warn ? " schema-warn" : ""}`;
```

appended to the existing `cls` template-literal concatenation (alongside `${r.read_only ? " readonly" : ""}`).

- [ ] **Step 3: Add the status-line summary**

In `web/ui.ts`'s `render()` (where `setStatus(snap.status, snap.error ?? "")` is called — per this plan's grounding research at ui.ts ~309), append a schema summary when present and no blocking error is showing:

```ts
  if (snap.schema_status && snap.schema_status.violation_count > 0 && !snap.error) {
    setStatus(`${snap.status ?? ""} · ${snap.schema_status.violation_count} schema warnings`.trim(), "");
  }
```

(Placed immediately after the existing `setStatus(snap.status, snap.error ?? "")` call so it doesn't fight with it — the second call overwrites the status text, keeping the error slot empty since schema warnings are soft, never rendered in the error styling.)

- [ ] **Step 4: Manual smoke test**

Run: `cd web && node build.mjs && node serve.mjs`, open a schema-violating document.
Expected: the violating row shows a dashed amber outline and a small corner dot; the status bar shows "N schema warnings".

- [ ] **Step 5: Commit**

```bash
git add web/render.ts web/style.css web/ui.ts
git commit -m "feat(web): soft schema-violation row indicator and status-line summary"
```

---

### Task 15: "Attach schema…" explicit-override action

**Files:**
- Modify: `web/ui.ts`
- Modify: `web/index.html` (desktop's page — confirmed by WEBUI.md — holds the toolbar markup)

**Interfaces:**
- Consumes: `Intent::SetSchema` (Task 7), `resolveSchemaFetchRequest` (Task 12).

- [ ] **Step 1: Add the toolbar action**

In `web/index.html`, add a button next to the existing Open action (matching its markup pattern — inspect the existing `<button id="openBtn">`-style element for exact classes/icon conventions and mirror them):

```html
<button id="attachSchemaBtn" title="Attach a JSON Schema…"><!-- icon matching existing toolbar buttons --></button>
```

- [ ] **Step 2: Wire the handler**

In `web/ui.ts`, add a handler (near `openUrlModal`'s definition, since "Attach schema" needs the same "local file or URL" choice `openUrlModal` already offers for opening a document):

```ts
async function attachSchema() {
  const choice = prompt("Path or URL to a JSON Schema file:");
  if (!choice) return;
  const source = choice.startsWith("http://") || choice.startsWith("https://")
    ? { Url: choice }
    : { Local: choice };
  snap = send({ SetSchema: { source } });
  render();
}
```

(A bare `prompt()` is a minimal, correct-but-unrefined placeholder for the URL/path *entry* UI — this plan's scope is the schema-attach *mechanism*; matching the existing `openUrlModal`'s dialog styling for a nicer input experience is a natural follow-up, not blocking this task's deliverable, which is that `SetSchema` → `schema_fetch_request` → `resolveSchemaFetchRequest` → `SchemaLoaded` round-trips correctly end to end.)

Wire the button in `main()` (alongside the other toolbar button wiring, e.g. `doOpen`/`doSave`'s event listener registration):

```ts
  document.getElementById("attachSchemaBtn")?.addEventListener("click", () => void attachSchema());
```

- [ ] **Step 3: Manual smoke test**

Run: `cd web && node build.mjs && node serve.mjs`, open any TOML file with no schema hint, click "Attach a JSON Schema…", enter a local schema path.
Expected: the document immediately shows schema-warn indicators for any violations, without a reload.

- [ ] **Step 4: Commit**

```bash
git add web/ui.ts web/index.html
git commit -m "feat(web): add an explicit Attach Schema action"
```

---

## Phase 4 — Touch/mobile web

### Task 16: Touch schema resolution + "Attach schema" sheet action

**Files:**
- Modify: `web/touch/app.ts`

**Interfaces:**
- Consumes: `resolveSchemaFetchRequest` (Task 12, shared with desktop — touch imports `host-io.ts`/`fs.ts` directly, per this plan's grounding research showing touch and desktop already share those host-I/O modules, only the DOM/render layer is separate).

- [ ] **Step 1: Wire schema resolution into touch's `render()`**

In `web/touch/app.ts`'s `render()` function (quoted verbatim in this plan's grounding research — the block handling `snap.external_edit`/`snap.convert_write` near the end), add the same pattern as Task 12's desktop wiring:

```ts
  if (snap.schema_fetch_request) {
    void resolveSchemaFetchRequest(io, session!, snap.schema_fetch_request, fileHandle?.path ?? null).then(
      (next) => {
        snap = next;
        render();
      },
    );
  }
```

(Import `resolveSchemaFetchRequest` from `../host-io.js` at the top of `web/touch/app.ts`, alongside its existing imports from that module.)

- [ ] **Step 2: Add an "Attach schema…" item to the touch ⋯ menu**

Find the touch overflow/⋯ menu's item list (the sheet built for `openSheet("menu")` or similar — search `app.ts` for where menu items like language/save are defined) and add an entry that prompts for a path/URL and dispatches `SetSchema`, mirroring Task 15's `attachSchema()`:

```ts
function attachSchema() {
  const choice = prompt("Path or URL to a JSON Schema file:");
  if (!choice) return;
  const source = choice.startsWith("http://") || choice.startsWith("https://")
    ? { Url: choice }
    : { Local: choice };
  closeSheets();
  snap = send({ SetSchema: { source } });
  render();
}
```

Wire it into the menu sheet's button list following that list's existing pattern (each item is a button with a click handler calling a function like this one — mirror an existing item such as the language picker's entry).

- [ ] **Step 3: Manual smoke test**

Run: `cd web && node build.mjs && node serve.mjs`, open `touch.html` in a narrow viewport (or device emulation), attach a schema via the ⋯ menu.
Expected: same round-trip as desktop — violations surface after the attach completes.

- [ ] **Step 4: Commit**

```bash
git add web/touch/app.ts
git commit -m "feat(touch): resolve schema fetch requests and add Attach Schema menu action"
```

---

### Task 17: Touch enum-picker sheet

**Files:**
- Modify: `web/touch/app.ts`

**Interfaces:**
- Consumes: `snap.mode`'s `SchemaEnum` variant (Task 13's `ModeView` addition), `Intent::{SchemaEnumMove, SchemaEnumCommit}` (Task 7).

- [ ] **Step 1: Add `openSchemaEnumSheet`, modeled on `openKindSheet`**

In `web/touch/app.ts`, add a function directly parallel to `openKindSheet` (quoted verbatim in this plan's grounding research):

```ts
function openSchemaEnumSheet(path: Path, options: string[]) {
  const cells = options
    .map(
      (label, i) =>
        `<button class="add-cell kind-opt" data-idx="${i}"><span class="dotc" style="background:var(--warn)"></span>${esc(label)}</button>`,
    )
    .join("");
  sheets.kind.innerHTML =
    '<div class="grab"></div>' +
    `<div class="sheet-head"><h3>Schema value</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
    `<div class="sheet-body"><div class="addgrid">${cells}</div></div>`;
  sheets.kind.querySelectorAll<HTMLElement>(".kind-opt").forEach((b) => {
    b.addEventListener("click", () => {
      const idx = Number(b.dataset.idx);
      closeSheets();
      const after = sendR({ SchemaEnumCommit: null }); // cursor already set to idx below
      toast(after.error ?? "Value changed");
    });
  });
  openSheet("kind");
}
```

Since `SchemaEnumCommit` commits whatever `cursor` currently is (not an explicit index), fix the click handler to move first, then commit:

```ts
    b.addEventListener("click", () => {
      const idx = Number(b.dataset.idx);
      const current = snap && "SchemaEnum" in snap.mode ? snap.mode.SchemaEnum.cursor : 0;
      send({ SchemaEnumMove: idx - current });
      closeSheets();
      const after = sendR("SchemaEnumCommit");
      toast(after.error ?? "Value changed");
    });
```

- [ ] **Step 2: Trigger it from `handleTap`/`openPanel`**

`begin_inline_edit` (Task 6) already routes into `Mode::SchemaEnum` core-side whenever the tapped node is enum-constrained — touch's job is only to *render* that mode when it becomes active. In `render()` (same function extended in Task 16), add:

```ts
  if (snap.mode && "SchemaEnum" in snap.mode) {
    const cur = snap.rows.find((r) => r.is_cursor);
    if (cur) openSchemaEnumSheet(cur.path, snap.mode.SchemaEnum.options);
  }
```

(Placed alongside the other mode-driven-surface checks already in `render()`, e.g. the `TypeFilter`/`Convert`/`Prompt` tag checks quoted in this plan's grounding research.)

- [ ] **Step 3: Manual smoke test**

Run: `cd web && node build.mjs && node serve.mjs`, open `touch.html` with a schema-constrained document, double-tap an enum field.
Expected: a bottom sheet titled "Schema value" lists the enum options; tapping one commits and closes the sheet.

- [ ] **Step 4: Commit**

```bash
git add web/touch/app.ts
git commit -m "feat(touch): add the schema-constrained enum/const picker sheet"
```

---

### Task 18: Touch `.schema-warn` CSS + row wiring

**Files:**
- Modify: `web/touch/render.ts`
- Modify: `web/touch/style.css`

**Interfaces:**
- Consumes: `ViewRow.schema_warn` (Task 11). Touch has its **own** stylesheet (`web/touch/style.css`, confirmed independent from `web/style.css` per this plan's grounding research) — this task duplicates Task 14's CSS intent there, not a shared import.

- [ ] **Step 1: Add the CSS**

In `web/touch/style.css`, add (near the existing `.row.selected > .row-main` rule):

```css
.row.schema-warn > .row-main { outline: 1.5px dashed var(--warn); outline-offset: -2px; }
```

Add the `--warn` variable to this file's own root theme block (touch has its own copy per this file's header comment noting it "ports" desktop's chrome rather than importing it — find the `:root { ... }` block here and add the same `--warn: #d9a441;` line Task 14 added to `web/style.css`).

- [ ] **Step 2: Wire the class in `rowHTML`**

In `web/touch/render.ts`'s `rowHTML` (quoted verbatim in this plan's grounding research), the `cls` string is built from several booleans. Add:

```ts
    (r.schema_warn ? " schema-warn" : "");
```

appended to the existing `cls` concatenation chain (alongside `(r.read_only ? " readonly" : "")`).

- [ ] **Step 3: Manual smoke test**

Run: `cd web && node build.mjs && node serve.mjs`, open `touch.html` with a schema-violating document.
Expected: the violating row shows the dashed amber outline in the touch layout too.

- [ ] **Step 4: Commit**

```bash
git add web/touch/render.ts web/touch/style.css
git commit -m "feat(touch): soft schema-violation row indicator"
```

---

## Phase 5 — Tauri verification + wrap-up

### Task 19: Verify Tauri `fs:scope` covers schema reads; document the Android limitation

**Files:**
- Modify: `TAURI.md`

**Interfaces:** none — this task is verification + documentation, no new code (spec: "expected zero code change beyond what step 3 wires through the shared `fs.ts`").

- [ ] **Step 1: Manual verification on desktop**

Build and run the Tauri desktop shell (`cd crates/confy-tauri && cargo tauri dev`, or per this repo's existing dev-run convention documented in `TAURI.md`), open a TOML file with a `#:schema ./s.json` hint and a real sibling `s.json`, confirm the row-warning indicators from Task 14 appear (same code path as web desktop — Tauri ships the same `web/` bundle per `tauri.conf.json`'s `frontendDist`).
Expected: works with no capability errors (confirms `fs:scope: "**"` already covers the sibling read, per this plan's grounding research — no `capabilities/default.json` change needed).

- [ ] **Step 2: Document the Android limitation**

In `TAURI.md`, find the existing Android/mobile section (documenting the `confy-picker` write-access rationale, per this plan's grounding research) and add a subsection:

```markdown
### JSON Schema on Android

A local/relative-path schema hint (`#:schema ./s.json`, a bare relative-path
`$schema` value, or an "Attach schema…" pick of a local file) cannot resolve
on Android: `tauri-plugin-confy-picker`'s only commands (`pick_writable`,
`create_writable`) grant a persistable SAF URI to exactly the *document
being opened*, not a directory — there is no way to read a second file
relative to it. This degrades soft (`SchemaStatus.load_error`, editing
unaffected) — see ADR 0001 for why `pick_writable` exists (a durability
gap, not a read/write capability gap) and
`docs/superpowers/specs/2026-08-10-json-schema-support-design.md`'s
Tauri/Android section for the full reasoning. **URL-based hints and the
"Attach schema…" action's URL path work identically to desktop** — no new
capability needed (plain `fetch()`, already used by "Open from URL…").
```

- [ ] **Step 3: Commit**

```bash
git add TAURI.md
git commit -m "docs(tauri): confirm schema reads work under fs:scope; document Android gap"
```

---

### Task 20: CHANGELOG entry

**Files:**
- Modify: `CHANGELOG.md`

**Interfaces:** none.

- [ ] **Step 1: Add the Unreleased entry**

Per this repo's `CLAUDE.md` convention ("Append an Unreleased Update entry to `CHANGELOG.md` with the timestamp and a description matching the commit message"), add to the top of `CHANGELOG.md`'s `### Added` section under `## Unreleased` (create that heading if it doesn't already exist at the top of the file — match the file's existing heading level/format exactly, e.g. the `### Added` sections shown in this plan's earlier grounding research at `CHANGELOG.md` lines ~794-798):

```markdown
### Added
- JSON Schema support — in-file hint detection ($schema key, yaml-language-server modeline, TOML `#:schema` comment) with explicit override, `jsonschema`-crate-backed validation surfaced as soft (never-blocking) per-row warnings, and constrained enum/const inline editing (TUI popup, web `<select>`, touch bottom sheet) across TOML/JSON/YAML on every surface (TUI, web desktop, touch, Tauri desktop/mobile).
```

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs: changelog entry for JSON Schema support"
```

---

## Post-plan verification (run once, after all tasks)

- [ ] `cargo test --workspace` — full Rust suite (confy-core, confy-tui) passes.
- [ ] `cd web && npx tsc --noEmit` — TypeScript compiles clean.
- [ ] `cd web && node build.mjs` — web bundle builds.
- [ ] Manual smoke test (already covered per-task above) confirms the end-to-end flow on at least TUI + web desktop: open a TOML file with a `#:schema` hint referencing a schema with an `enum` field and a `type` mismatch elsewhere → soft warning renders, enum field opens as a picker, popup/`$EDITOR` editing remains fully unconstrained.
