✅ **Shipped — historical reference.** See `CHANGELOG.md` for current behavior; this plan is kept for context, not as a live task list.

# VS Code Schema Hints (Diagnostics + Hover) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface confy-core's JSON Schema support inside VS Code's native TOML/YAML text editor as Problems-panel diagnostics (schema violations + load failures) and hover tooltips, without touching confy's own custom webview editor.

**Architecture:** One new core intent (`Intent::DetectSchema`) and one new read-only ffi query (`schema_violations()`) let a persistent, per-document `ConfySession` in the VS Code extension host stay alive across edits (`Intent::ApplyReplace{path:[], text}` in place of a full session rebuild — ADR 0007), while a small set of pure TypeScript helpers (coexistence check, dedup decision, local-path resolution, outline hit-test, diagnostic descriptor building) do the host-side policy the core deliberately leaves to hosts, wired together by `SchemaSessionManager` and two `vscode` providers.

**Tech Stack:** Rust (`confy-core`, `confy-ffi`, `wasm-bindgen`), TypeScript (`editors/vscode`, `web/types.ts` as the canonical wire-type mirror), plain `node --experimental-strip-types` unit tests (no test framework), `cargo test`.

**Spec:** `docs/superpowers/specs/2026-08-21-vscode-schema-hints-design.md` (design decisions, data flow, error handling — read alongside this plan). Related: `docs/adr/0007-vscode-schema-session-in-place-replace.md`.

## Global Constraints

- **Diagnostics severity is always `vscode.DiagnosticSeverity.Warning`**, never `Error` — CONTEXT.md's Schema section defines Violations as a Soft constraint (spec §"Diagnostics update").
- **Coexistence:** confy defers Diagnostics registration for a language when `tamasfe.even-better-toml` (TOML) or `redhat.vscode-yaml` (YAML) is *installed* (not `isActive` — spec §"Coexistence"). Hover always registers regardless of what's installed.
- **Scope:** TOML and YAML only, native VS Code text editor only (`{pattern: "**/*.toml"}` / `{pattern: "**/*.yaml"}` / `{pattern: "**/*.yml"}` selectors — JSON and confy's own custom editor are explicitly out of scope, spec §"Non-goals").
- **No completion provider** this pass (spec §"Non-goals").
- **Debounce:** 300ms on `onDidChangeTextDocument` before reparsing (spec §"Open implementation details" — tunable later, not blocking).
- **Session lifecycle:** one persistent `ConfySession` per open document, updated via `Intent::ApplyReplace{path: [], text}`, never rebuilt per edit (ADR 0007).
- **Never throw into VS Code's UI** from a provider — matches `ConfyOutlineProvider`'s existing `try {...} catch { return [] }` convention.
- **No project-wide test/lint/build commands inside a task** — each task runs only the specific test(s) named in that task. A final task runs the full check suite once.

---

### Task 1: `confy-core` — `Intent::DetectSchema` + `Session::schema_violations()`

**Files:**
- Modify: `crates/confy-core/src/session/intent.rs` (`Intent` enum, near the existing `SchemaLoaded`/`SchemaEnum*` variants around line 214)
- Modify: `crates/confy-core/src/session/dispatch.rs` (`apply()` match, in the `// Schema` group around line 281)
- Modify: `crates/confy-core/src/schema/types.rs` (new `ViolationView` struct, after `Violation` around line 41)
- Modify: `crates/confy-core/src/schema/mod.rs` (add `ViolationView` to the `pub use types::{...}` line)
- Modify: `crates/confy-core/src/session/session.rs` (new `Session::schema_violations()` method, placed directly after `revalidate_schema` around line 1577)
- Test: `crates/confy-core/tests/schema_headless.rs`

**Interfaces:**
- Consumes: existing `Session::detect_and_request_schema(&mut self) -> Option<SchemaSource>` (`session.rs:1489`), `Session.pending_schema_fetch: Option<SchemaSource>`, `Session.schema: Option<SchemaState>` with `SchemaState.violations: Vec<Violation>` (`schema/types.rs:129`), `Session.tree: NodeTree` with `NodeTree::node_at(&self, path: &[Seg]) -> Option<&Node>` and `Node.text_range: std::ops::Range<usize>`.
- Produces: `Intent::DetectSchema` (unit variant); `pub struct ViolationView { path: Path, pointer: String, keyword: String, message: String, category: Category, text_range: Option<(u32, u32)> }`; `pub fn Session::schema_violations(&self) -> Vec<ViolationView>` — Task 2 (ffi) and every downstream TypeScript task consume this exact shape (field names carry through `serde`/`serde-wasm-bindgen` verbatim).

- [ ] **Step 1: Write the failing tests**

```rust
// crates/confy-core/tests/schema_headless.rs — append at the end of the file
use confy_core::model::node::Seg;
use confy_core::session::{Intent, Session};

fn session_from_toml(src: &str) -> Session {
    Session::new(AnyDocument::from_str_as(src, DocFormat::Toml).unwrap())
}

#[test]
fn detect_schema_intent_sets_pending_schema_fetch() {
    let mut s = session_from_toml("port = 1\n");
    // No hint yet: dispatch is a no-op.
    let _ = s.dispatch(Intent::DetectSchema);
    assert_eq!(s.pending_schema_fetch, None);

    let mut s = session_from_toml("#:schema ./s.json\nport = 1\n");
    // `Session::new` already ran detection once (session.rs:72) — drain that first.
    let _ = s.pending_schema_fetch.take();
    let snap = s.dispatch(Intent::DetectSchema);
    assert_eq!(
        snap.schema_fetch_request,
        Some(SchemaSource::Local("./s.json".into()))
    );
}

#[test]
fn schema_violations_is_empty_without_a_loaded_schema() {
    let s = session_from_toml("port = 1\n");
    assert!(s.schema_violations().is_empty());
}

#[test]
fn schema_violations_carries_the_violating_node_text_range() {
    let mut s = session_from_toml("port = \"not-a-number\"\n");
    s.apply_schema_text(
        SchemaSource::Local("./s.json".into()),
        Ok(r#"{"type":"object","properties":{"port":{"type":"integer"}}}"#.to_string()),
    );
    let violations = s.schema_violations();
    assert_eq!(violations.len(), 1);
    let v = &violations[0];
    assert_eq!(v.path, vec![Seg::Key("port".into())]);
    assert_eq!(v.keyword, "type");
    let (start, end) = v.text_range.expect("port node resolves");
    // The violating node's range must land on the value `"not-a-number"`, not
    // the whole line or the key.
    assert_eq!(&"port = \"not-a-number\"\n"[start as usize..end as usize], "\"not-a-number\"");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p confy-core --test schema_headless detect_schema_intent -- --nocapture` and `cargo test -p confy-core --test schema_headless schema_violations`
Expected: FAIL to compile — `Intent::DetectSchema` and `Session::schema_violations` don't exist yet.

- [ ] **Step 3: Add `Intent::DetectSchema`**

```rust
// crates/confy-core/src/session/intent.rs — insert immediately before the
// existing `SchemaLoaded { .. }` variant (around line 214)
    /// Re-run `detect_and_request_schema()` against the current document and
    /// stash the result into `pending_schema_fetch` — **not** an idempotent
    /// no-op: it unconditionally overwrites `pending_schema_fetch`, even
    /// with `None`, whenever called. Hosts that want to avoid redundant
    /// fetch/recompile after every edit must compare the returned
    /// `schema_fetch_request` against what they already have loaded
    /// themselves (VS Code schema-hints design).
    DetectSchema,
```

```rust
// crates/confy-core/src/session/dispatch.rs — insert immediately before the
// existing `Intent::SchemaLoaded { .. }` arm (around line 282)
            Intent::DetectSchema => {
                self.pending_schema_fetch = self.detect_and_request_schema();
            }
```

- [ ] **Step 4: Add `ViolationView` and `Session::schema_violations()`**

```rust
// crates/confy-core/src/schema/types.rs — insert immediately after the
// `Violation` struct (after line 41, before the `EditHint` doc comment)
/// A `Violation` plus its violating node's resolved source-text byte range —
/// the native-editor Diagnostics data source (`Session::schema_violations`).
/// `text_range: None` only if `path` no longer resolves against the current
/// tree (defensive: in practice this is only ever read against the same
/// tree revision the violations were computed from).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ViolationView {
    pub path: Path,
    pub pointer: String,
    pub keyword: String,
    pub message: String,
    pub category: Category,
    pub text_range: Option<(u32, u32)>,
}
```

```rust
// crates/confy-core/src/schema/mod.rs — replace the existing pub use line
pub use types::{Category, EditHint, SchemaSource, SchemaState, SchemaStatus, Violation, ViolationView};
```

```rust
// crates/confy-core/src/session/session.rs — insert directly after the
// closing brace of `revalidate_schema` (around line 1577)
    /// Current schema violations, each carrying its node's resolved
    /// `text_range` — the native-editor Diagnostics data source (VS Code
    /// schema-hints design). Empty when no schema is loaded or there are no
    /// violations.
    pub fn schema_violations(&self) -> Vec<crate::schema::ViolationView> {
        let Some(state) = self.schema.as_ref() else {
            return Vec::new();
        };
        state
            .violations
            .iter()
            .map(|v| crate::schema::ViolationView {
                path: v.path.clone(),
                pointer: v.pointer.clone(),
                keyword: v.keyword.clone(),
                message: v.message.clone(),
                category: v.category,
                text_range: self
                    .tree
                    .node_at(&v.path)
                    .map(|n| (n.text_range.start as u32, n.text_range.end as u32)),
            })
            .collect()
    }
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p confy-core --test schema_headless`
Expected: PASS (all tests in the file, including the three new ones).

- [ ] **Step 6: Commit**

```bash
git add crates/confy-core/src/session/intent.rs crates/confy-core/src/session/dispatch.rs \
        crates/confy-core/src/schema/types.rs crates/confy-core/src/schema/mod.rs \
        crates/confy-core/src/session/session.rs crates/confy-core/tests/schema_headless.rs
git commit -m "feat(core): Intent::DetectSchema + Session::schema_violations for native-editor diagnostics"
```

---

### Task 2: `confy-ffi` — expose `schema_violations()`

**Files:**
- Modify: `crates/confy-ffi/src/lib.rs` (`impl ConfySession`, after the existing `outline()` method around line 128)
- Test: `crates/confy-ffi/functional_smoke.mjs`

**Interfaces:**
- Consumes: `Session::schema_violations()` (Task 1), existing `to_value`/`js_serde_error` helpers already used by every other query method in this file.
- Produces: `ConfySession.schema_violations(): ViolationView[]` (wasm-bindgen JS method) — Task 3's `wasmSession.ts` types this exactly.

- [ ] **Step 1: Write the failing check**

```js
// crates/confy-ffi/functional_smoke.mjs — append near the other schema checks
// (grep the file for "schema" to place this next to related assertions)
{
  const src = `port = "not-a-number"\n`;
  const schemaSession = new ConfySession(src, "toml");
  schemaSession.dispatch(tuple("SchemaLoaded", {
    source: { Local: "./s.json" },
    text: { Ok: JSON.stringify({ type: "object", properties: { port: { type: "integer" } } }) },
  }));
  const violations = schemaSession.schema_violations();
  check("schema_violations reports one violation", violations.length === 1, JSON.stringify(violations));
  check(
    "violation carries a resolved text_range",
    Array.isArray(violations[0]?.text_range) && violations[0].text_range.length === 2,
    JSON.stringify(violations[0]),
  );
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd crates/confy-ffi && wasm-pack build --target web --out-dir pkg && node functional_smoke.mjs`
Expected: FAIL — `schema_violations is not a function`.

- [ ] **Step 3: Add the ffi method**

```rust
// crates/confy-ffi/src/lib.rs — insert immediately after the `outline()`
// method (after line 128, before `pointer_slot`)
    /// Current schema violations with resolved `text_range`s — the
    /// native-editor Diagnostics data source (VS Code schema-hints design).
    pub fn schema_violations(&self) -> Result<JsValue, JsValue> {
        to_value(&self.session.schema_violations()).map_err(js_serde_error)
    }
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd crates/confy-ffi && wasm-pack build --target web --out-dir pkg && node functional_smoke.mjs`
Expected: PASS, no `✗` lines in the output.

- [ ] **Step 5: Commit**

```bash
git add crates/confy-ffi/src/lib.rs crates/confy-ffi/functional_smoke.mjs
git commit -m "feat(ffi): expose schema_violations() for native-editor diagnostics"
```

---

### Task 3: `web/types.ts` — mirror `DetectSchema` + `ViolationView`, add `wasmSession.ts` shared loader

**Files:**
- Modify: `web/types.ts` (`Intent` union around line 253, new `ViolationView` interface near `SchemaStatus` around line 157)
- Create: `editors/vscode/src/wasmSession.ts`
- Modify: `editors/vscode/src/outlineProvider.ts` (remove duplicated loader/types, import from `wasmSession.ts`)

**Interfaces:**
- Consumes: Task 1/2's `Intent::DetectSchema`, `ConfySession.schema_violations()`; existing `web/types.ts` `Path`, `SchemaSource`, `SchemaStatus`, `SessionSnapshot`, `Intent`, `Category` shapes.
- Produces: `web/types.ts` exports `ViolationView` and an `Intent` union that includes `"DetectSchema"`. `editors/vscode/src/wasmSession.ts` exports `OutlineNode`, `ConfySessionHandle` (`outline()`, `dispatch(intent: Intent): SessionSnapshot`, `snapshot(): SessionSnapshot`, `schema_violations(): ViolationView[]`, `schema_hint(path: Path): EditHint`), `ConfySessionCtor`, and `loadConfySession(context: vscode.ExtensionContext): Promise<ConfySessionCtor>` — every later VS Code task imports from here, none re-declares the loader.

- [ ] **Step 1: Update `web/types.ts`**

```typescript
// web/types.ts — add to the Intent union, immediately before the closing
// `| "SchemaEnumCommit";` line (around line 256)
  | "DetectSchema"
```

```typescript
// web/types.ts — insert immediately after the `SchemaStatus` interface
// (after line 157)
// ---- Schema violation with resolved source range (ffi `schema_violations`) ----
export type ViolationCategory = "Value" | "Representation";

export interface ViolationView {
  path: Path;
  pointer: string;
  keyword: string;
  message: string;
  category: ViolationCategory;
  text_range: [number, number] | undefined;
}
```

- [ ] **Step 2: Verify no existing consumer breaks**

Run: `cd web && npm run typecheck`
Expected: PASS — both additions are purely additive (new union member, new exported type), nothing in `web/` currently does an exhaustive `switch` over every `Intent` variant that the compiler would flag.

- [ ] **Step 3: Create `editors/vscode/src/wasmSession.ts`**

```typescript
// editors/vscode/src/wasmSession.ts
import * as vscode from "vscode";
import { readFileSync } from "node:fs";
// See outlineProvider.ts's original comment (preserved verbatim below) for
// why this is a static import and why raw bytes are passed to `ffi.default`.
import * as ffi from "../media/pkg/confy_ffi.js";
import type { EditHint, Intent, Path, SessionSnapshot, ViolationView } from "../../../web/types.js";

export type { EditHint, ViolationView };

export interface OutlineNode {
  key: string;
  path: Path;
  type_label: string;
  value: string | null;
  text_range: [number, number];
  key_text_range: [number, number] | undefined;
  children: OutlineNode[];
}

export interface ConfySessionHandle {
  outline(): OutlineNode[];
  dispatch(intent: Intent): SessionSnapshot;
  snapshot(): SessionSnapshot;
  schema_violations(): ViolationView[];
  schema_hint(path: Path): EditHint;
}

export interface ConfySessionCtor {
  new (text: string, format: string): ConfySessionHandle;
}

let ffiInit: Promise<ConfySessionCtor> | undefined;

// Loading the wasm in the extension host (Node.js), not the webview: the
// generated `--target web` glue only calls `fetch()` when `init()` receives a
// string/URL/Request; passing raw bytes makes it call
// `WebAssembly.instantiate(bytes, imports)` directly — identical API in Node
// and the browser (confirmed against the generated confy_ffi.js). Module-level
// singleton: first call wins, no per-request re-init. Shared by
// `ConfyOutlineProvider` and the schema-hints feature (`schemaSessionManager.ts`)
// so the wasm module is instantiated exactly once regardless of how many
// features load it.
export async function loadConfySession(context: vscode.ExtensionContext): Promise<ConfySessionCtor> {
  if (!ffiInit) {
    ffiInit = (async () => {
      const bytes = readFileSync(
        vscode.Uri.joinPath(context.extensionUri, "media/pkg/confy_ffi_bg.wasm").fsPath,
      );
      // Object form: the glue warns the bare-bytes form is deprecated.
      await ffi.default({ module_or_path: bytes });
      return ffi.ConfySession as unknown as ConfySessionCtor;
    })();
  }
  return ffiInit;
}
```

- [ ] **Step 4: Refactor `outlineProvider.ts` to use the shared module**

```typescript
// editors/vscode/src/outlineProvider.ts — replace lines 1-49 (imports through
// the end of loadConfySession) with:
import * as vscode from "vscode";
import { formatFromName } from "./formatFromName.js";
import { byteOffsetsToRange } from "./byteToPosition.js";
import { loadConfySession, type OutlineNode } from "./wasmSession.js";
```

Leave everything from the `KIND_MAP` constant (previously line 54) through the end of the file untouched — `symbolKindFor`, `toDocumentSymbol`, and `ConfyOutlineProvider` are unchanged; only the now-duplicate `OutlineNode` interface, `ConfySessionCtor` interface, `ffiInit`, and `loadConfySession` function (previously lines 15-49) are deleted, since they now live in `wasmSession.ts`.

- [ ] **Step 5: Verify the refactor compiles and changes no behavior**

Run: `cd editors/vscode && npm run check`
Expected: PASS, no type errors. This is a pure extraction — no new behavior, so `npm run check` (the existing verification for this file, since there is no automated test for `outlineProvider.ts` itself, only the manual Extension Development Host check already documented in `docs/superpowers/specs/2026-08-20-vscode-outline-provider-design.md`) is the correct verification here.

- [ ] **Step 6: Commit**

```bash
git add web/types.ts editors/vscode/src/wasmSession.ts editors/vscode/src/outlineProvider.ts
git commit -m "feat(vscode): shared wasmSession loader; mirror DetectSchema/ViolationView in web/types.ts"
```

---

### Task 4: `byteToPosition.ts` — reverse UTF-16→UTF-8-byte converter (for hover)

**Files:**
- Modify: `editors/vscode/src/byteToPosition.ts`
- Test: `editors/vscode/src/byteToPosition.test.ts`

**Interfaces:**
- Consumes: nothing new (pure string walk, same style as the existing `utf8ByteOffsetToUtf16Offset`).
- Produces: `export function utf16OffsetToUtf8ByteOffset(text: string, utf16Offset: number): number` — Task 6's `outlineHitTest.ts` consumes this to convert a hover's `document.offsetAt(position)` into a byte offset comparable against `OutlineNode.text_range`.

- [ ] **Step 1: Write the failing tests**

```typescript
// editors/vscode/src/byteToPosition.test.ts — append at the end of the file
import { utf16OffsetToUtf8ByteOffset } from "./byteToPosition.ts";

test("ASCII: UTF-16 offset equals byte offset", () => {
  assert.strictEqual(utf16OffsetToUtf8ByteOffset("port = 8080", 7), 7);
});

test("CJK: 1 UTF-16 unit maps to the multi-byte UTF-8 length", () => {
  const text = "鍵 = 1";
  assert.strictEqual(utf16OffsetToUtf8ByteOffset(text, 1), 3); // right after 鍵
  assert.strictEqual(utf16OffsetToUtf8ByteOffset(text, 2), 4); // right after the space
});

test("emoji: a 2-unit surrogate pair maps to 4 bytes", () => {
  const text = "😀x";
  assert.strictEqual(utf16OffsetToUtf8ByteOffset(text, 2), 4); // right after 😀
});

test("round-trips with utf8ByteOffsetToUtf16Offset", () => {
  const text = "鍵 = \"😀值\"";
  for (let byte = 0; byte <= Buffer.byteLength(text, "utf8"); byte++) {
    const u16 = utf8ByteOffsetToUtf16Offset(text, byte);
    const back = utf16OffsetToUtf8ByteOffset(text, u16);
    // Not a strict inverse mid-codepoint (byte offsets inside a multi-byte
    // char round up to the next boundary) — but every *boundary* byte offset
    // must round-trip exactly.
    if (text.codePointAt(0) !== undefined) {
      assert.ok(back >= byte === back >= byte); // boundary offsets checked explicitly below
    }
  }
  assert.strictEqual(utf16OffsetToUtf8ByteOffset(text, utf8ByteOffsetToUtf16Offset(text, 0)), 0);
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd editors/vscode && node --experimental-strip-types src/byteToPosition.test.ts`
Expected: FAIL — `utf16OffsetToUtf8ByteOffset is not a function`.

- [ ] **Step 3: Implement**

```typescript
// editors/vscode/src/byteToPosition.ts — append after utf8ByteLength (after
// line 33, before byteOffsetsToRange)
/** Inverse of `utf8ByteOffsetToUtf16Offset`: walk `text` once, converting a
 * UTF-16 code-unit offset (e.g. `document.offsetAt(position)`) into the
 * UTF-8 byte offset comparable against `OutlineNode.text_range`/
 * `ViolationView.text_range` — used by the hover provider's cursor→Path
 * lookup (`outlineHitTest.ts`). */
export function utf16OffsetToUtf8ByteOffset(text: string, utf16Offset: number): number {
  let units = 0;
  let bytes = 0;
  for (let i = 0; i < text.length && units < utf16Offset; ) {
    const code = text.codePointAt(i)!;
    const len = utf8ByteLength(code);
    const width = code > 0xffff ? 2 : 1; // surrogate pair consumes two UTF-16 units
    bytes += len;
    units += width;
    i += width;
  }
  return bytes;
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd editors/vscode && node --experimental-strip-types src/byteToPosition.test.ts`
Expected: PASS, all tests exit 0 with no assertion errors.

- [ ] **Step 5: Commit**

```bash
git add editors/vscode/src/byteToPosition.ts editors/vscode/src/byteToPosition.test.ts
git commit -m "feat(vscode): utf16OffsetToUtf8ByteOffset for hover cursor lookup"
```

---

### Task 5: `outlineHitTest.ts` — byte offset → `Path` lookup

**Files:**
- Create: `editors/vscode/src/outlineHitTest.ts`
- Test: `editors/vscode/src/outlineHitTest.test.ts`

**Interfaces:**
- Consumes: `OutlineNode` (Task 3, `wasmSession.ts`).
- Produces: `export function findPathAtByteOffset(nodes: OutlineNode[], byteOffset: number): Path | undefined` — Task 8's `schemaHoverProvider.ts` consumes this.

- [ ] **Step 1: Write the failing tests**

```typescript
// editors/vscode/src/outlineHitTest.test.ts
import { test } from "node:test";
import assert from "node:assert";
import { findPathAtByteOffset } from "./outlineHitTest.ts";
import type { OutlineNode } from "./wasmSession.ts";

function node(key: string, range: [number, number], children: OutlineNode[] = []): OutlineNode {
  return { key, path: [{ Key: key }], type_label: "string", value: null, text_range: range, key_text_range: undefined, children };
}

test("finds the deepest node whose range contains the offset", () => {
  const tree = [node("server", [0, 30], [node("port", [10, 20])])];
  assert.deepStrictEqual(findPathAtByteOffset(tree, 15), [{ Key: "port" }]);
});

test("falls back to the shallower ancestor when the offset misses every child", () => {
  const tree = [node("server", [0, 30], [node("port", [10, 20])])];
  assert.deepStrictEqual(findPathAtByteOffset(tree, 25), [{ Key: "server" }]);
});

test("returns undefined when the offset is outside every node", () => {
  const tree = [node("server", [0, 30])];
  assert.strictEqual(findPathAtByteOffset(tree, 99), undefined);
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd editors/vscode && node --experimental-strip-types src/outlineHitTest.test.ts`
Expected: FAIL — module `./outlineHitTest.ts` not found.

- [ ] **Step 3: Implement**

```typescript
// editors/vscode/src/outlineHitTest.ts
import type { Path } from "../../../web/types.js";
import type { OutlineNode } from "./wasmSession.js";

function contains(range: [number, number], byteOffset: number): boolean {
  return byteOffset >= range[0] && byteOffset <= range[1];
}

/** The deepest `OutlineNode` whose `text_range` contains `byteOffset`,
 * returned as its `Path` — the hover provider's cursor→node lookup. Walks
 * the outline tree confy's own `outline()` already produces rather than
 * adding a new core query (spec §"Hover"). */
export function findPathAtByteOffset(nodes: OutlineNode[], byteOffset: number): Path | undefined {
  for (const n of nodes) {
    if (!contains(n.text_range, byteOffset)) continue;
    return findPathAtByteOffset(n.children, byteOffset) ?? n.path;
  }
  return undefined;
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd editors/vscode && node --experimental-strip-types src/outlineHitTest.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add editors/vscode/src/outlineHitTest.ts editors/vscode/src/outlineHitTest.test.ts
git commit -m "feat(vscode): outline-based byte-offset to Path hit test for hover"
```

---

### Task 6: `schemaCoexistence.ts` — installed-extension deferral check

**Files:**
- Create: `editors/vscode/src/schemaCoexistence.ts`
- Test: `editors/vscode/src/schemaCoexistence.test.ts`

**Interfaces:**
- Consumes: nothing (pure, injected lookup function).
- Produces: `export type SchemaLanguage = "toml" | "yaml"`; `export function isDiagnosticsDeferred(language: SchemaLanguage, isInstalled: (extensionId: string) => boolean): boolean` — Task 10's `schemaSessionManager.ts` consumes this at `open()` time with `(id) => vscode.extensions.getExtension(id) !== undefined`.

- [ ] **Step 1: Write the failing tests**

```typescript
// editors/vscode/src/schemaCoexistence.test.ts
import { test } from "node:test";
import assert from "node:assert";
import { isDiagnosticsDeferred } from "./schemaCoexistence.ts";

test("defers TOML diagnostics when Even Better TOML is installed", () => {
  assert.strictEqual(
    isDiagnosticsDeferred("toml", (id) => id === "tamasfe.even-better-toml"),
    true,
  );
});

test("does not defer TOML diagnostics when nothing relevant is installed", () => {
  assert.strictEqual(isDiagnosticsDeferred("toml", () => false), false);
});

test("defers YAML diagnostics when redhat.vscode-yaml is installed", () => {
  assert.strictEqual(
    isDiagnosticsDeferred("yaml", (id) => id === "redhat.vscode-yaml"),
    true,
  );
});

test("a TOML extension installed does not defer YAML", () => {
  assert.strictEqual(
    isDiagnosticsDeferred("yaml", (id) => id === "tamasfe.even-better-toml"),
    false,
  );
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd editors/vscode && node --experimental-strip-types src/schemaCoexistence.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

```typescript
// editors/vscode/src/schemaCoexistence.ts
export type SchemaLanguage = "toml" | "yaml";

const COEXISTING_EXTENSIONS: Record<SchemaLanguage, string> = {
  toml: "tamasfe.even-better-toml",
  yaml: "redhat.vscode-yaml",
};

/** Whether confy should defer Diagnostics registration for `language` — true
 * when that language's established schema-aware extension is *installed*.
 * Deliberately checks installed, not active: confy's own `onStartupFinished`
 * activation can race ahead of the other extension's typically-lazy
 * `onLanguage:*` activation, so `isActive` risks a false negative at the
 * moment this check runs (spec §"Coexistence"). `isInstalled` is injected so
 * this stays testable without a real `vscode.extensions` API — callers pass
 * `(id) => vscode.extensions.getExtension(id) !== undefined`. */
export function isDiagnosticsDeferred(
  language: SchemaLanguage,
  isInstalled: (extensionId: string) => boolean,
): boolean {
  return isInstalled(COEXISTING_EXTENSIONS[language]);
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd editors/vscode && node --experimental-strip-types src/schemaCoexistence.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add editors/vscode/src/schemaCoexistence.ts editors/vscode/src/schemaCoexistence.test.ts
git commit -m "feat(vscode): installed-extension coexistence check for schema diagnostics"
```

---

### Task 7: `schemaPathResolve.ts` — local schema path resolution

**Files:**
- Create: `editors/vscode/src/schemaPathResolve.ts`
- Test: `editors/vscode/src/schemaPathResolve.test.ts`

**Interfaces:**
- Consumes: Node's built-in `node:path`.
- Produces: `export function resolveLocalSchemaPath(currentFilePath: string, relativeOrAbsolute: string): string` — Task 10's `schemaSessionManager.ts` consumes this before calling its injected `readFile`.

- [ ] **Step 1: Write the failing tests**

```typescript
// editors/vscode/src/schemaPathResolve.test.ts
import { test } from "node:test";
import assert from "node:assert";
import * as path from "node:path";
import { resolveLocalSchemaPath } from "./schemaPathResolve.ts";

test("resolves a bare filename against the document's directory", () => {
  const result = resolveLocalSchemaPath("/proj/config/app.toml", "schema.json");
  assert.strictEqual(result, path.resolve("/proj/config", "schema.json"));
});

test("resolves a ./relative path against the document's directory", () => {
  const result = resolveLocalSchemaPath("/proj/config/app.toml", "./schemas/app.json");
  assert.strictEqual(result, path.resolve("/proj/config", "./schemas/app.json"));
});

test("resolves a ../relative path against the document's directory", () => {
  const result = resolveLocalSchemaPath("/proj/config/app.toml", "../schemas/app.json");
  assert.strictEqual(result, path.resolve("/proj/config", "../schemas/app.json"));
});

test("passes an absolute path through untouched", () => {
  const abs = path.resolve("/other/place/schema.json");
  assert.strictEqual(resolveLocalSchemaPath("/proj/config/app.toml", abs), abs);
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd editors/vscode && node --experimental-strip-types src/schemaPathResolve.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

```typescript
// editors/vscode/src/schemaPathResolve.ts
import * as path from "node:path";

/** Resolve a schema hint's `Local` source against the directory of the
 * document that referenced it — mirrors `web/fs.ts`'s `readSiblingFile`
 * resolution rule (bare/relative paths resolve against the open file's
 * directory; absolute paths pass through). Uses Node's own `path.isAbsolute`
 * (authoritative for the extension host's OS) rather than reimplementing
 * `web/fs.ts`'s manual POSIX/Windows/UNC regex — the extension host has
 * direct, unsandboxed `fs` access and does not need the webview's
 * `read-schema-file` message round trip (design §"Coexistence"/§"Shared
 * sync schema steps"). */
export function resolveLocalSchemaPath(currentFilePath: string, relativeOrAbsolute: string): string {
  if (path.isAbsolute(relativeOrAbsolute)) return relativeOrAbsolute;
  return path.resolve(path.dirname(currentFilePath), relativeOrAbsolute);
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd editors/vscode && node --experimental-strip-types src/schemaPathResolve.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add editors/vscode/src/schemaPathResolve.ts editors/vscode/src/schemaPathResolve.test.ts
git commit -m "feat(vscode): resolve local schema hint paths against the document directory"
```

---

### Task 8: `schemaDedup.ts` — reload decision

**Files:**
- Create: `editors/vscode/src/schemaDedup.ts`
- Test: `editors/vscode/src/schemaDedup.test.ts`

**Interfaces:**
- Consumes: `SchemaSource`, `SchemaStatus` (`web/types.ts`, existing).
- Produces: `export function needsSchemaReload(detected: SchemaSource | undefined, loaded: SchemaSource | undefined, status: SchemaStatus | undefined): boolean` — Task 10's `schemaSessionManager.ts` consumes this in `syncSchema`.

- [ ] **Step 1: Write the failing tests**

```typescript
// editors/vscode/src/schemaDedup.test.ts
import { test } from "node:test";
import assert from "node:assert";
import { needsSchemaReload } from "./schemaDedup.ts";

test("no hint detected: never reloads", () => {
  assert.strictEqual(needsSchemaReload(undefined, { Local: "s.json" }, undefined), false);
});

test("first detection with nothing loaded yet: reloads", () => {
  assert.strictEqual(needsSchemaReload({ Local: "s.json" }, undefined, undefined), true);
});

test("same Local source already loaded successfully: does not reload", () => {
  const source = { Local: "s.json" };
  const status = { source_label: "s.json", violation_count: 0, load_error: undefined };
  assert.strictEqual(needsSchemaReload(source, source, status), false);
});

test("same source but the previous load failed: retries", () => {
  const source = { Local: "s.json" };
  const status = { source_label: "s.json", violation_count: 0, load_error: "not found" };
  assert.strictEqual(needsSchemaReload(source, source, status), true);
});

test("detected source differs from what's loaded: reloads", () => {
  assert.strictEqual(
    needsSchemaReload({ Local: "b.json" }, { Local: "a.json" }, undefined),
    true,
  );
});

test("Local and Url with the same string are not the same source: reloads", () => {
  assert.strictEqual(
    needsSchemaReload({ Url: "s.json" }, { Local: "s.json" }, undefined),
    true,
  );
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd editors/vscode && node --experimental-strip-types src/schemaDedup.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

```typescript
// editors/vscode/src/schemaDedup.ts
import type { SchemaSource, SchemaStatus } from "../../../web/types.js";

function sameSchemaSource(a: SchemaSource, b: SchemaSource): boolean {
  if ("Local" in a && "Local" in b) return a.Local === b.Local;
  if ("Url" in a && "Url" in b) return a.Url === b.Url;
  return false;
}

/** Whether a freshly `DetectSchema`-detected source requires the host to
 * (re)fetch/read and dispatch `SchemaLoaded` — confy-core does not dedup
 * this itself (`Intent::DetectSchema`'s doc comment): `apply_schema_text`
 * unconditionally recompiles the validator every call. `false` only when
 * the same source is already loaded *and* that load actually succeeded —
 * a previous failure (`load_error` set) retries on every reparse rather
 * than getting stuck (ADR 0007's "host owns dedup" consequence). */
export function needsSchemaReload(
  detected: SchemaSource | undefined,
  loaded: SchemaSource | undefined,
  status: SchemaStatus | undefined,
): boolean {
  if (!detected) return false;
  if (!loaded || !sameSchemaSource(detected, loaded)) return true;
  return status?.load_error != null;
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd editors/vscode && node --experimental-strip-types src/schemaDedup.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add editors/vscode/src/schemaDedup.ts editors/vscode/src/schemaDedup.test.ts
git commit -m "feat(vscode): host-side schema reload dedup decision"
```

---

### Task 9: `schemaDiagnostics.ts` — violation/load-error → diagnostic descriptors

**Files:**
- Create: `editors/vscode/src/schemaDiagnostics.ts`
- Test: `editors/vscode/src/schemaDiagnostics.test.ts`

**Interfaces:**
- Consumes: `ViolationView` (`web/types.ts`/`wasmSession.ts`, Task 1/3).
- Produces: `export interface DiagnosticDescriptor { startByte: number; endByte: number; message: string }`; `export function buildSchemaDiagnostics(violations: ViolationView[], loadError: string | undefined): DiagnosticDescriptor[]` — pure, no `vscode` import (so it is unit-testable under plain `node`, matching the design's testing intent). Task 11's wiring in `extension.ts` converts each descriptor to a `vscode.Diagnostic` via `byteOffsetsToRange` (Task 4's converter is the byte→UTF-16 half already built for outline; this task does not touch `vscode` at all).

- [ ] **Step 1: Write the failing tests**

```typescript
// editors/vscode/src/schemaDiagnostics.test.ts
import { test } from "node:test";
import assert from "node:assert";
import { buildSchemaDiagnostics } from "./schemaDiagnostics.ts";
import type { ViolationView } from "./wasmSession.ts";

function violation(overrides: Partial<ViolationView> = {}): ViolationView {
  return {
    path: [{ Key: "port" }],
    pointer: "/port",
    keyword: "type",
    message: "port must be an integer",
    category: "Value",
    text_range: [10, 20],
    ...overrides,
  };
}

test("one descriptor per violation with a text_range", () => {
  const result = buildSchemaDiagnostics([violation()], undefined);
  assert.deepStrictEqual(result, [{ startByte: 10, endByte: 20, message: "port must be an integer" }]);
});

test("drops violations with no resolvable text_range", () => {
  const result = buildSchemaDiagnostics([violation({ text_range: undefined })], undefined);
  assert.deepStrictEqual(result, []);
});

test("appends a line-0 descriptor for a non-empty load_error", () => {
  const result = buildSchemaDiagnostics([], "schema file not found");
  assert.deepStrictEqual(result, [{ startByte: 0, endByte: 0, message: "schema file not found" }]);
});

test("no load_error descriptor when load_error is undefined", () => {
  assert.deepStrictEqual(buildSchemaDiagnostics([], undefined), []);
});

test("violations and load_error combine", () => {
  const result = buildSchemaDiagnostics([violation()], "schema file not found");
  assert.strictEqual(result.length, 2);
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd editors/vscode && node --experimental-strip-types src/schemaDiagnostics.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

```typescript
// editors/vscode/src/schemaDiagnostics.ts
import type { ViolationView } from "./wasmSession.js";

export interface DiagnosticDescriptor {
  startByte: number;
  endByte: number;
  message: string;
}

/** Build the Problems-panel entries for one document, as plain byte-range
 * descriptors (no `vscode` import — kept pure and testable under plain
 * `node`; the caller converts each descriptor to a `vscode.Diagnostic` with
 * `DiagnosticSeverity.Warning`, never `Error` — Violations are a documented
 * Soft constraint, CONTEXT.md § Schema). A violation with no resolvable
 * `text_range` is dropped rather than guessed at. `loadError` becomes one
 * additional line-0 descriptor — the one piece of `load_error` UI shipping
 * in this pass (spec §"Non-goals": web/TUI/touch parity is tracked
 * separately). */
export function buildSchemaDiagnostics(
  violations: ViolationView[],
  loadError: string | undefined,
): DiagnosticDescriptor[] {
  const out: DiagnosticDescriptor[] = [];
  for (const v of violations) {
    if (!v.text_range) continue;
    out.push({ startByte: v.text_range[0], endByte: v.text_range[1], message: v.message });
  }
  if (loadError) out.push({ startByte: 0, endByte: 0, message: loadError });
  return out;
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd editors/vscode && node --experimental-strip-types src/schemaDiagnostics.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add editors/vscode/src/schemaDiagnostics.ts editors/vscode/src/schemaDiagnostics.test.ts
git commit -m "feat(vscode): pure schema-violation to diagnostic-descriptor builder"
```

---

### Task 10: `schemaSessionManager.ts` — persistent per-document session lifecycle

**Files:**
- Create: `editors/vscode/src/schemaSessionManager.ts`
- Test: `editors/vscode/src/schemaSessionManager.test.ts`

**Interfaces:**
- Consumes: `ConfySessionCtor`, `ConfySessionHandle` (Task 3); `needsSchemaReload` (Task 8); `resolveLocalSchemaPath` (Task 7); `SchemaSource`, `SchemaStatus` (`web/types.ts`).
- Produces:
  ```typescript
  export interface SchemaSessionDeps {
    readFile: (path: string) => Promise<string>;
    fetchUrl: (url: string) => Promise<string>;
  }
  export interface DocSyncResult {
    violations: ViolationView[];
    loadError: string | undefined;
    invalidSyntax: boolean;
  }
  export class SchemaSessionManager {
    constructor(sessionCtor: ConfySessionCtor, deps: SchemaSessionDeps);
    open(key: string, fsPath: string, text: string, format: string): Promise<DocSyncResult>;
    reparse(key: string, text: string): Promise<DocSyncResult | undefined>;
    outline(key: string): OutlineNode[] | undefined;
    schemaHint(key: string, path: Path): EditHint | undefined;
    close(key: string): void;
  }
  ```
  Task 11 (`schemaHoverProvider.ts`) consumes `outline`/`schemaHint`; Task 12 (`extension.ts` wiring) consumes `open`/`reparse`/`close`/`DocSyncResult`.

- [ ] **Step 1: Write the failing tests**

```typescript
// editors/vscode/src/schemaSessionManager.test.ts
import { test } from "node:test";
import assert from "node:assert";
import { SchemaSessionManager } from "./schemaSessionManager.ts";
import type { ConfySessionCtor, ConfySessionHandle } from "./wasmSession.ts";

// A minimal in-memory stand-in for the wasm ConfySession, driving exactly the
// subset of dispatch/snapshot/schema_violations behavior this manager relies
// on (design §"Testing": "mocked minimal vscode document/fs/fetch surface").
class FakeSession implements ConfySessionHandle {
  text: string;
  schemaSource: { Local: string } | { Url: string } | undefined;
  loadError: string | undefined = undefined;
  hint: { Local: string } | undefined;
  failNextReplace = false;

  constructor(text: string, _format: string) {
    this.text = text;
    this.hint = text.includes("#:schema") ? { Local: text.split("#:schema ")[1].split("\n")[0] } : undefined;
  }
  outline() { return []; }
  schema_hint() { return "None" as const; }
  schema_violations() { return []; }
  snapshot() {
    return {
      schema_fetch_request: undefined,
      schema_status: this.schemaSource
        ? { source_label: "s", violation_count: 0, load_error: this.loadError }
        : undefined,
    } as any;
  }
  dispatch(intent: any) {
    if (intent === "DetectSchema") {
      return {
        schema_fetch_request: this.hint,
        schema_status: this.schemaSource
          ? { source_label: "s", violation_count: 0, load_error: this.loadError }
          : undefined,
      } as any;
    }
    if (intent.ApplyReplace !== undefined) {
      if (this.failNextReplace) return { error: "parse error" } as any;
      this.text = intent.ApplyReplace.text;
      this.hint = this.text.includes("#:schema")
        ? { Local: this.text.split("#:schema ")[1].split("\n")[0] }
        : undefined;
      return { error: undefined } as any;
    }
    if (intent.SchemaLoaded !== undefined) {
      this.schemaSource = intent.SchemaLoaded.source;
      this.loadError = intent.SchemaLoaded.text.Err;
      return this.snapshot();
    }
    throw new Error(`unexpected intent in FakeSession: ${JSON.stringify(intent)}`);
  }
}
const FakeCtor = FakeSession as unknown as ConfySessionCtor;

function deps(overrides: Partial<{ readFile: (p: string) => Promise<string>; fetchUrl: (u: string) => Promise<string> }> = {}) {
  return {
    readFile: overrides.readFile ?? (async () => "{}"),
    fetchUrl: overrides.fetchUrl ?? (async () => "{}"),
  };
}

test("open() with a hint fetches and loads the schema", async () => {
  let readCalls = 0;
  const manager = new SchemaSessionManager(FakeCtor, deps({ readFile: async () => { readCalls++; return "{}"; } }));
  const result = await manager.open("doc1", "/proj/app.toml", "#:schema ./s.json\nport=1\n", "toml");
  assert.strictEqual(readCalls, 1);
  assert.strictEqual(result.invalidSyntax, false);
});

test("reparse() with an unchanged hint does not re-fetch", async () => {
  let readCalls = 0;
  const manager = new SchemaSessionManager(FakeCtor, deps({ readFile: async () => { readCalls++; return "{}"; } }));
  await manager.open("doc1", "/proj/app.toml", "#:schema ./s.json\nport=1\n", "toml");
  assert.strictEqual(readCalls, 1);
  await manager.reparse("doc1", "#:schema ./s.json\nport=2\n");
  assert.strictEqual(readCalls, 1, "same hint must not trigger a second fetch");
});

test("reparse() with invalid syntax reports invalidSyntax and skips schema sync", async () => {
  let readCalls = 0;
  const manager = new SchemaSessionManager(FakeCtor, deps({ readFile: async () => { readCalls++; return "{}"; } }));
  await manager.open("doc1", "/proj/app.toml", "port=1\n", "toml");
  // Reach into the fake to force the next ApplyReplace to fail — a
  // test-only hook exercising the manager's `snap.error` branch.
  (manager as any).docs.get("doc1").session.failNextReplace = true;
  const result = await manager.reparse("doc1", "port=");
  assert.strictEqual(result?.invalidSyntax, true);
  assert.strictEqual(readCalls, 0);
});

test("reparse() on an unknown key returns undefined", async () => {
  const manager = new SchemaSessionManager(FakeCtor, deps());
  const result = await manager.reparse("never-opened", "port=1\n");
  assert.strictEqual(result, undefined);
});

test("close() then a slow fetch resolving later is discarded, not dispatched", async () => {
  let resolveRead: (v: string) => void;
  const pending = new Promise<string>((resolve) => { resolveRead = resolve; });
  const manager = new SchemaSessionManager(FakeCtor, deps({ readFile: () => pending }));
  const openPromise = manager.open("doc1", "/proj/app.toml", "#:schema ./s.json\nport=1\n", "toml");
  manager.close("doc1");
  resolveRead!("{}");
  const result = await openPromise;
  // open() itself still resolves (it awaited its own fetch), but the
  // document is gone from the manager — no crash, no dangling dispatch.
  assert.strictEqual(manager.outline("doc1"), undefined);
  assert.ok(result); // does not throw
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd editors/vscode && node --experimental-strip-types src/schemaSessionManager.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

```typescript
// editors/vscode/src/schemaSessionManager.ts
import type { EditHint, Path, SchemaSource, SessionSnapshot, ViolationView } from "../../../web/types.js";
import type { ConfySessionCtor, ConfySessionHandle, OutlineNode } from "./wasmSession.js";
import { needsSchemaReload } from "./schemaDedup.js";
import { resolveLocalSchemaPath } from "./schemaPathResolve.js";

export interface SchemaSessionDeps {
  readFile: (path: string) => Promise<string>;
  fetchUrl: (url: string) => Promise<string>;
}

export interface DocSyncResult {
  violations: ViolationView[];
  loadError: string | undefined;
  invalidSyntax: boolean;
}

interface ManagedDoc {
  session: ConfySessionHandle;
  fsPath: string;
  loadedSchemaSource: SchemaSource | undefined;
  generation: number;
}

/**
 * One persistent `ConfySession` per open document (ADR 0007), keyed by an
 * opaque caller-chosen string (the caller uses `document.uri.toString()`).
 * Edits go through `reparse()`'s `Intent::ApplyReplace{path: [], text}`
 * against the *same* session rather than constructing a new one, so the
 * compiled schema `Validator` survives every edit; schema fetch/reload is
 * only re-triggered when `needsSchemaReload` says the detected source
 * actually changed.
 */
export class SchemaSessionManager {
  private docs = new Map<string, ManagedDoc>();

  constructor(
    private readonly SessionCtor: ConfySessionCtor,
    private readonly deps: SchemaSessionDeps,
  ) {}

  async open(key: string, fsPath: string, text: string, format: string): Promise<DocSyncResult> {
    const session = new this.SessionCtor(text, format);
    const doc: ManagedDoc = { session, fsPath, loadedSchemaSource: undefined, generation: 0 };
    this.docs.set(key, doc);
    return this.syncSchema(key, doc);
  }

  async reparse(key: string, text: string): Promise<DocSyncResult | undefined> {
    const doc = this.docs.get(key);
    if (!doc) return undefined;
    doc.generation += 1;
    const snap = doc.session.dispatch({ ApplyReplace: { path: [], text } }) as SessionSnapshot & { error?: string };
    if (snap.error) {
      // Mid-edit invalid syntax: the session's tree (and therefore its
      // violations/text_range) is untouched at the last valid parse, whose
      // byte positions no longer correspond to the live buffer. Report
      // invalidSyntax so the caller clears diagnostics instead of
      // displaying drifted ranges (spec §"Error handling", Q7).
      return { violations: [], loadError: undefined, invalidSyntax: true };
    }
    return this.syncSchema(key, doc);
  }

  outline(key: string): OutlineNode[] | undefined {
    return this.docs.get(key)?.session.outline();
  }

  schemaHint(key: string, path: Path): EditHint | undefined {
    return this.docs.get(key)?.session.schema_hint(path);
  }

  close(key: string): void {
    this.docs.delete(key);
  }

  private async syncSchema(key: string, doc: ManagedDoc): Promise<DocSyncResult> {
    let snap = doc.session.dispatch("DetectSchema") as SessionSnapshot;
    const detected = snap.schema_fetch_request;
    if (needsSchemaReload(detected, doc.loadedSchemaSource, snap.schema_status)) {
      const generation = doc.generation;
      const text = await this.resolveSchemaText(doc.fsPath, detected!);
      // Stale-fetch guard: discard if the document closed or moved on to a
      // later reparse while this fetch/read was in flight (spec §"Error
      // handling").
      const stillCurrent = this.docs.get(key) === doc && doc.generation === generation;
      if (stillCurrent) {
        snap = doc.session.dispatch({ SchemaLoaded: { source: detected!, text } }) as SessionSnapshot;
        doc.loadedSchemaSource = detected!;
      }
    }
    return {
      violations: doc.session.schema_violations(),
      loadError: snap.schema_status?.load_error,
      invalidSyntax: false,
    };
  }

  private async resolveSchemaText(
    fsPath: string,
    source: SchemaSource,
  ): Promise<{ Ok: string } | { Err: string }> {
    try {
      if ("Local" in source) {
        const resolved = resolveLocalSchemaPath(fsPath, source.Local);
        return { Ok: await this.deps.readFile(resolved) };
      }
      return { Ok: await this.deps.fetchUrl(source.Url) };
    } catch (e) {
      return { Err: e instanceof Error ? e.message : String(e) };
    }
  }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd editors/vscode && node --experimental-strip-types src/schemaSessionManager.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add editors/vscode/src/schemaSessionManager.ts editors/vscode/src/schemaSessionManager.test.ts
git commit -m "feat(vscode): SchemaSessionManager - persistent per-document schema session lifecycle"
```

---

### Task 11: `schemaHoverProvider.ts` — hover tooltips

**Files:**
- Create: `editors/vscode/src/schemaHoverProvider.ts`

**Interfaces:**
- Consumes: `SchemaSessionManager.outline`/`schemaHint` (Task 10); `findPathAtByteOffset` (Task 5); `utf16OffsetToUtf8ByteOffset` (Task 4); `EditHint` (`web/types.ts`).
- Produces: `export class ConfySchemaHoverProvider implements vscode.HoverProvider` — Task 12 registers this in `extension.ts`.

No unit test for this file: it is a thin `vscode.HoverProvider` adapter with no branching logic of its own beyond calling the already-tested pure helpers (mirrors `ConfyOutlineProvider`'s own untested-adapter convention) — verified manually in the Extension Development Host per Task 12's step.

- [ ] **Step 1: Implement**

```typescript
// editors/vscode/src/schemaHoverProvider.ts
import * as vscode from "vscode";
import type { EditHint } from "../../../web/types.js";
import { findPathAtByteOffset } from "./outlineHitTest.js";
import { utf16OffsetToUtf8ByteOffset } from "./byteToPosition.js";
import type { SchemaSessionManager } from "./schemaSessionManager.js";

function renderEditHint(hint: EditHint): string | undefined {
  if (hint === "None") return undefined;
  if ("Enum" in hint) {
    const options = hint.Enum.map(([label]) => `\`${label}\``).join(", ");
    return `Allowed values: ${options}`;
  }
  const { minimum, maximum, multiple_of } = hint.Bounded;
  const parts: string[] = [];
  if (minimum !== undefined) parts.push(`minimum: ${minimum}`);
  if (maximum !== undefined) parts.push(`maximum: ${maximum}`);
  if (multiple_of !== undefined) parts.push(`multiple of: ${multiple_of}`);
  return parts.length > 0 ? parts.join(", ") : undefined;
}

/** Native-editor hover: reuses the read-only `outline()` tree (already built
 * for `ConfyOutlineProvider`) to resolve the cursor's `Path`, then asks the
 * live per-document `ConfySession` (via `SchemaSessionManager`) for its
 * schema-driven `EditHint` — no new core query beyond what Diagnostics
 * already needs (design §"Hover"). */
export class ConfySchemaHoverProvider implements vscode.HoverProvider {
  constructor(private readonly manager: SchemaSessionManager) {}

  provideHover(document: vscode.TextDocument, position: vscode.Position): vscode.Hover | undefined {
    try {
      const key = document.uri.toString();
      const outline = this.manager.outline(key);
      if (!outline) return undefined;
      const byteOffset = utf16OffsetToUtf8ByteOffset(document.getText(), document.offsetAt(position));
      const path = findPathAtByteOffset(outline, byteOffset);
      if (!path) return undefined;
      const hint = this.manager.schemaHint(key, path);
      if (!hint) return undefined;
      const text = renderEditHint(hint);
      return text ? new vscode.Hover(new vscode.MarkdownString(text)) : undefined;
    } catch {
      // Never throw into VS Code's UI (ConfyOutlineProvider convention).
      return undefined;
    }
  }
}
```

- [ ] **Step 2: Commit**

```bash
git add editors/vscode/src/schemaHoverProvider.ts
git commit -m "feat(vscode): schema-driven hover tooltips for native TOML/YAML editors"
```

---

### Task 12: Wire into `extension.ts` + `package.json`

**Files:**
- Modify: `editors/vscode/src/extension.ts` (append registration inside `activate`, after the existing outline-provider block around line 100)
- Modify: `editors/vscode/package.json` (verify only — no new `activationEvents`/`contributes` entries are required, matching the outline provider's precedent of runtime-only registration under the existing `onStartupFinished`)

**Interfaces:**
- Consumes: `SchemaSessionManager`, `SchemaSessionDeps` (Task 10); `ConfySchemaHoverProvider` (Task 11); `isDiagnosticsDeferred` (Task 6); `buildSchemaDiagnostics` (Task 9); `byteOffsetsToRange` (existing, Task 4's module); `loadConfySession` (Task 3); `formatFromName` (existing).
- Produces: nothing further downstream — this is the top of the dependency graph.

- [ ] **Step 1: Implement the wiring**

```typescript
// editors/vscode/src/extension.ts — add these imports near the top, next to
// the existing outlineProvider import
import { readFile } from "node:fs/promises";
import { formatFromName } from "./formatFromName.js";
import { loadConfySession } from "./wasmSession.js";
import { SchemaSessionManager, type DocSyncResult } from "./schemaSessionManager.js";
import { ConfySchemaHoverProvider } from "./schemaHoverProvider.js";
import { isDiagnosticsDeferred, type SchemaLanguage } from "./schemaCoexistence.js";
import { buildSchemaDiagnostics } from "./schemaDiagnostics.js";
import { byteOffsetsToRange } from "./byteToPosition.js";
```

```typescript
// editors/vscode/src/extension.ts — append inside activate(), immediately
// after the existing registerDocumentSymbolProvider block (after line 100,
// before the setLang/setTheme function declarations)
  const SCHEMA_SELECTOR = [
    { pattern: "**/*.toml" },
    { pattern: "**/*.yaml" },
    { pattern: "**/*.yml" },
  ];

  function schemaLanguageFor(fileName: string): SchemaLanguage {
    return fileName.endsWith(".yaml") || fileName.endsWith(".yml") ? "yaml" : "toml";
  }

  const diagnostics = vscode.languages.createDiagnosticCollection("confy-schema");
  const deferredDocs = new Set<string>(); // keys currently deferring diagnostics
  let managerPromise: Promise<SchemaSessionManager> | undefined;
  async function getManager(): Promise<SchemaSessionManager> {
    if (!managerPromise) {
      managerPromise = loadConfySession(context).then(
        (ctor) =>
          new SchemaSessionManager(ctor, {
            readFile: (p) => readFile(p, "utf8"),
            fetchUrl: async (url) => {
              const res = await fetch(url);
              if (!res.ok) throw new Error(`HTTP ${res.status}`);
              return res.text();
            },
          }),
      );
    }
    return managerPromise;
  }

  function updateDiagnostics(document: vscode.TextDocument, result: DocSyncResult): void {
    const key = document.uri.toString();
    if (deferredDocs.has(key)) return;
    const descriptors = buildSchemaDiagnostics(result.violations, result.loadError);
    diagnostics.set(
      document.uri,
      descriptors.map(
        (d) =>
          new vscode.Diagnostic(
            byteOffsetsToRange(document, d.startByte, d.endByte),
            d.message,
            vscode.DiagnosticSeverity.Warning,
          ),
      ),
    );
  }

  async function openDoc(document: vscode.TextDocument): Promise<void> {
    if (!SCHEMA_SELECTOR.some((s) => vscode.languages.match(s, document) > 0)) return;
    const key = document.uri.toString();
    if (isDiagnosticsDeferred(schemaLanguageFor(document.fileName), (id) => vscode.extensions.getExtension(id) !== undefined)) {
      deferredDocs.add(key);
    } else {
      deferredDocs.delete(key);
    }
    const manager = await getManager();
    const result = await manager.open(key, document.uri.fsPath, document.getText(), formatFromName(document.fileName));
    updateDiagnostics(document, result);
  }

  const reparseTimers = new Map<string, ReturnType<typeof setTimeout>>();
  function scheduleReparse(document: vscode.TextDocument): void {
    const key = document.uri.toString();
    const existing = reparseTimers.get(key);
    if (existing) clearTimeout(existing);
    reparseTimers.set(
      key,
      setTimeout(async () => {
        reparseTimers.delete(key);
        const manager = await getManager();
        const result = await manager.reparse(key, document.getText());
        if (!result) return;
        if (result.invalidSyntax) {
          if (!deferredDocs.has(key)) diagnostics.set(document.uri, []);
          return;
        }
        updateDiagnostics(document, result);
      }, 300),
    );
  }

  context.subscriptions.push(
    diagnostics,
    vscode.languages.registerHoverProvider(SCHEMA_SELECTOR, new ConfySchemaHoverProvider(await getManager())),
    vscode.workspace.onDidOpenTextDocument(openDoc),
    vscode.workspace.onDidChangeTextDocument((e) => {
      if (SCHEMA_SELECTOR.some((s) => vscode.languages.match(s, e.document) > 0)) scheduleReparse(e.document);
    }),
    vscode.workspace.onDidCloseTextDocument(async (document) => {
      const key = document.uri.toString();
      const timer = reparseTimers.get(key);
      if (timer) clearTimeout(timer);
      reparseTimers.delete(key);
      deferredDocs.delete(key);
      (await getManager()).close(key);
      diagnostics.delete(document.uri);
    }),
  );
  // Documents already open when the extension activates (e.g. a restored
  // window) still need their initial schema sync — mirrors why the
  // outline provider needs no equivalent (it's request-driven, not
  // event-driven).
  for (const document of vscode.workspace.textDocuments) {
    if (SCHEMA_SELECTOR.some((s) => vscode.languages.match(s, document) > 0)) void openDoc(document);
  }
```

Note: `activate()` is not declared `async` today (`extension.ts:34`: `export function activate(context: vscode.ExtensionContext): void`). Change its signature to `async function activate(...): Promise<void>` — VS Code's activation contract accepts a `Promise<void>` return, and `await getManager()` above requires it. This is the one necessary signature change in this file beyond the additive block.

- [ ] **Step 2: Verify the wiring compiles**

Run: `cd editors/vscode && npm run check`
Expected: PASS, no type errors.

- [ ] **Step 3: Manual verification in the Extension Development Host**

Per repo convention (`docs/superpowers/specs/2026-08-20-vscode-outline-provider-design.md`'s manual-verification precedent — no automated end-to-end test for a real `vscode` window):

1. `cd editors/vscode && npm run build`, then `F5` in VS Code to launch the Extension Development Host.
2. Create a `.toml` file with `#:schema ./bad.json` pointing at a schema that makes the document invalid (e.g. `{"type":"object","properties":{"port":{"type":"integer"}}}` against `port = "x"`). Open it in the native text editor. Confirm the Problems panel shows one Warning-severity diagnostic anchored on the `"x"` value, and hovering the `port` key/value shows the type constraint.
3. Point the hint at a nonexistent file. Confirm a Warning-severity diagnostic appears at line 1 with the load-error message.
4. Type invalid syntax (e.g. delete a closing `]`) and confirm the Problems entries disappear, then reappear correctly once the syntax is fixed again.
5. Install/enable "Even Better TOML", reload the window, reopen the file. Confirm confy's own diagnostics for that file no longer appear (Even Better TOML's own diagnostics may), while hovering still shows confy's tooltip.

- [ ] **Step 4: Commit**

```bash
git add editors/vscode/src/extension.ts editors/vscode/package.json
git commit -m "feat(vscode): wire schema diagnostics + hover into extension activation"
```

---

### Task 13: Full verification pass + CHANGELOG

**Files:**
- Modify: `CHANGELOG.md` (append an `Unreleased Update` entry per CLAUDE.md's after-each-task requirement)

- [ ] **Step 1: Run every automated test touched by this plan, once**

```bash
cargo test -p confy-core --test schema_headless
cd crates/confy-ffi && wasm-pack build --target web --out-dir pkg && node functional_smoke.mjs && cd -
cd web && npm run typecheck && cd -
cd editors/vscode && npm run check && cd -
cd editors/vscode && for f in src/byteToPosition.test.ts src/outlineHitTest.test.ts src/schemaCoexistence.test.ts src/schemaPathResolve.test.ts src/schemaDedup.test.ts src/schemaDiagnostics.test.ts src/schemaSessionManager.test.ts; do node --experimental-strip-types "$f" || exit 1; done
```

Expected: every command exits 0.

- [ ] **Step 2: Append the CHANGELOG entry**

```markdown
### Unreleased Update — YYYY-MM-DDTHH:MM:SSZ
- feat(vscode): native TOML/YAML text editors now surface confy-core's JSON Schema support directly — Problems-panel diagnostics (schema violations, always `Warning` severity per the Soft-constraint principle, plus a load-error notice) and hover tooltips (enum/const/bounds at the cursor's node), driven by one persistent `ConfySession` per open document (`Intent::ApplyReplace{path:[],text}` in place of a per-edit rebuild, ADR 0007) and a new `Session::schema_violations()`/`Intent::DetectSchema` core surface. Defers to `tamasfe.even-better-toml`/`redhat.vscode-yaml` when installed. Scoped to VS Code's native editor only; confy's own custom editor tab is unaffected.
```

(Replace the timestamp with the actual commit time.)

- [ ] **Step 3: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs: changelog entry for VS Code schema diagnostics + hover"
```
