# VS Code Schema Hints (Diagnostics + Hover) — Design

Status: approved for planning (design phase)
Date: 2026-08-21

## Goal

Surface confy-core's existing JSON Schema support (`crates/confy-core/src/schema/`)
inside VS Code's **native** TOML/YAML text editor as **Problems-panel
diagnostics** (schema violations) and **hover tooltips** (type/enum/const/
bounds at the cursor's node), the same way `outlineProvider.ts` surfaces the
core's AST as Outline/breadcrumbs.

## Non-goals (this pass)

- **JSON.** Same reasoning as the outline provider: VS Code's built-in JSON
  language already does schema-aware diagnostics/hover.
- **Completion.** Cursor-position key/value intent detection is materially
  harder than diagnostics/hover and is deferred to a future pass.
- **Any change inside confy's own custom editor** (`viewType: "confy.editor"`).
  Same platform constraint as the outline provider — this only affects VS
  Code's native text editor tab.
- **Dynamic re-detection of Even Better TOML / redhat.vscode-yaml** installs
  mid-session (`onDidChangeExtensions`). Coexistence deferral is decided once
  at `activate()`; installing/uninstalling those extensions requires a window
  reload to take effect, matching normal VS Code extension-interaction
  expectations.
- **Enhancing confy's own webview** (`web/breadcrumb.ts`, detail panel) with
  anything new. Zero webview-side change ships in this pass.

## Coexistence with existing TOML/YAML schema tooling

VS Code's provider APIs are additive, not exclusive: multiple
`CompletionItemProvider`/`HoverProvider` registrations for the same selector
have their results merged/stacked, and multiple `DiagnosticCollection`s are
shown side by side in Problems with no dedupe. Two extensions commonly
already do schema-aware work here:

- **`tamasfe.even-better-toml`** — the de facto standard TOML extension
  (taplo-based), supports its own schema associations.
- **`redhat.vscode-yaml`** — very commonly recommended for YAML, supports
  `yaml.schemas`.

**Decision:** at `activate()`, check
`vscode.extensions.getExtension(id)?.isActive` for both. If active for a
given language, confy **defers**: it skips registering/running Diagnostics
for that language entirely (no `ConfySession` is even constructed for
diagnostics purposes on files of that language). **Hover always registers
regardless** — hover content from multiple providers stacks harmlessly in
the same popup, and the risk/annoyance profile is much lower than duplicate
red squiggles in Problems.

## Architecture

Three layers, bottom-up, all additive/read-only — mirrors the outline
provider's layering.

### 1. `confy-core` — no new fields, one new intent, one new query

`Session::detect_and_request_schema()` and `Session::apply_schema_text()`
already exist (used by the web/TUI hosts today). Expose detection through the
existing `Intent`/`dispatch` channel instead of a bespoke ffi method, keeping
one command surface:

```rust
// crates/confy-core/src/session/intent.rs
pub enum Intent {
    // ...existing variants...
    /// Re-run `detect_and_request_schema()` against the current document.
    /// Idempotent no-op if a schema is already loaded and the in-document
    /// hint hasn't changed. Native-editor host analog of what the web/TUI
    /// hosts already do once after `Session::new`.
    DetectSchema,
}
```

New read-only query returning violations **with their source range already
resolved**, so the host never has to reimplement path→node lookup:

```rust
// crates/confy-core/src/schema/types.rs
pub struct ViolationView {
    pub path: Path,
    pub pointer: String,
    pub keyword: String,
    pub message: String,
    pub category: Category,
    /// Byte range of the violating node — `None` only if the path no longer
    /// resolves (document changed between validation and this call).
    pub text_range: Option<(u32, u32)>,
}
```

```rust
impl Session {
    /// Current schema violations, each carrying its node's `text_range` —
    /// the native-editor Diagnostics data source. Empty if no schema is
    /// loaded or there are no violations.
    pub fn schema_violations(&self) -> Vec<ViolationView> { ... }
}
```

`schema_hint(path)` (ffi) / `Session::edit_hint(path)` already returns
everything Hover needs (enum/const/bounds) — **no change**, reused as-is.

### 2. `confy-ffi` — one new query, `dispatch` already generic

```rust
impl ConfySession {
    /// Diagnostics data source — see `Session::schema_violations`.
    pub fn schema_violations(&self) -> Result<JsValue, JsValue> {
        to_value(&self.session.schema_violations()).map_err(js_serde_error)
    }
}
```

`Intent::DetectSchema` needs no new ffi method — it goes through the existing
generic `dispatch(intent)`. No new wasm build target; same `--target web`
artifact both the webview and the extension host already load.

### 3. `editors/vscode` — new `src/schemaSessionManager.ts` + `src/schemaProvider.ts`

**`SchemaSessionManager`** (new, stateful — unlike the outline provider's
per-request `ConfySession`, schema loading is an async round trip that must
land back on the *same* session instance):

```ts
interface ManagedDoc {
  session: ConfySession;      // live wasm session for this document
  format: string;
  schemaCache?: { source: SchemaSource; text: string }; // avoid re-fetching unchanged schema on every keystroke
}

class SchemaSessionManager {
  private docs = new Map<string /* uri.toString() */, ManagedDoc>();

  async open(document: vscode.TextDocument): Promise<void> { ... }
  // debounced (300ms) on onDidChangeTextDocument
  async reparse(document: vscode.TextDocument): Promise<void> { ... }
  close(uri: vscode.Uri): void { this.docs.delete(uri.toString()); }
  get(uri: vscode.Uri): ManagedDoc | undefined { ... }
}
```

`open`/`reparse` shared resolution steps:
1. `new ConfySession(document.getText(), format)`.
2. `session.dispatch({DetectSchema: null})` → read `schema_fetch_request` off
   the returned snapshot.
3. If present and it matches `schemaCache.source` (by value equality): reuse
   `schemaCache.text`, skip I/O, dispatch `SchemaLoaded` immediately.
4. Otherwise resolve the source:
   - **Local path** — `fs.promises.readFile`, resolved relative to
     `document.uri.fsPath`'s directory (mirrors `web/fs.ts`'s
     `readSiblingFile` resolution rule, reimplemented here since the
     extension host has direct `fs` access and does not need the webview's
     `read-schema-file` message round trip).
   - **URL** — global `fetch()` (Node 18, unsandboxed — no CSP concern here,
     unlike the webview's `read-schema-url` path).
   Cache `{source, text}` on success; on failure, dispatch `SchemaLoaded`
   with the error text (existing `Intent::SchemaLoaded { source, text:
   Result<String,String> }` shape already carries this).
5. `session.dispatch({SchemaLoaded: {source, text}})`.

**`schemaProvider.ts`** — registration + the two providers:

```ts
export function registerSchemaFeatures(context: vscode.ExtensionContext) {
  const tomlDeferred = vscode.extensions.getExtension("tamasfe.even-better-toml")?.isActive ?? false;
  const yamlDeferred = vscode.extensions.getExtension("redhat.vscode-yaml")?.isActive ?? false;

  const manager = new SchemaSessionManager();
  const diagnostics = vscode.languages.createDiagnosticCollection("confy-schema");

  // wiring: workspace.onDidOpenTextDocument / onDidChangeTextDocument (debounced)
  // / onDidCloseTextDocument, filtered to **/*.toml and **/*.{yaml,yml} pattern
  // matches (same rationale as outlineProvider.ts: pattern selectors, not
  // language selectors — VS Code assigns .toml plaintext without an extension).

  context.subscriptions.push(
    vscode.languages.registerHoverProvider(
      [{ pattern: "**/*.toml" }, { pattern: "**/*.yaml" }, { pattern: "**/*.yml" }],
      new ConfySchemaHoverProvider(manager),
    ),
    diagnostics,
    // manager's document-lifecycle listeners, diagnostics-update logic gated
    // on tomlDeferred / yamlDeferred per document language
  );
}
```

- **Hover** (`ConfySchemaHoverProvider.provideHover`): resolve the cursor's
  `Path` — reuse the outline tree's range-hit-test (walk `OutlineNode[]`,
  find the deepest node whose `text_range` contains the offset; this is a
  pure function, independently testable, and needs no new core API since
  `outline()` already exists), call `schema_hint(path)`, render Markdown
  (type / enum options / const / numeric bounds / schema `description` if
  present).
- **Diagnostics update** (called from `open`/`reparse`, skipped entirely
  when the document's language is deferred): `schema_violations()` →
  `vscode.Diagnostic[]` via the byte→UTF-16 range conversion already built
  for the outline provider (`byteToPosition.ts`, reused, not duplicated) →
  `diagnostics.set(uri, items)`. A violation whose `text_range` is `None`
  (stale path) is dropped rather than guessed at.

## Data flow

```
.toml/.yaml file opened (native text editor)
  -> SchemaSessionManager.open(document)
     -> new ConfySession(text, format)
     -> dispatch({DetectSchema}) -> schema_fetch_request?
        -> [cache miss] fs.readFile / fetch()  -> dispatch({SchemaLoaded})
        -> [cache hit]  cached text            -> dispatch({SchemaLoaded})
     -> schema_violations() -> ViolationView[] (byte text_range)
     -> byte->UTF16 conversion -> vscode.Diagnostic[] -> diagnostics.set(uri, ...)
        (skipped if language deferred to Even Better TOML / redhat.vscode-yaml)

cursor hover
  -> outline() [existing] -> range-hit-test -> Path
  -> schema_hint(path) [existing edit_hint] -> Markdown -> vscode.Hover

document edited (debounced 300ms)
  -> SchemaSessionManager.reparse(document) -> same flow as open(), schema
     source reused from cache when unchanged

document closed -> SchemaSessionManager.close(uri); diagnostics.delete(uri)
```

## Error handling

- `confy-core`/`confy-ffi`: `schema_violations()` only reachable on a live
  `ConfySession`; a path that no longer resolves yields `text_range: None`
  (dropped by the host) rather than an error. No new fallible surface beyond
  what `apply_schema_text`/`edit_hint` already handle (`SchemaStatus.load_error`).
- `editors/vscode`: any exception during open/reparse/hover — caught,
  diagnostics for that document cleared (or left at their last-good state if
  the failure was transient parse noise mid-keystroke — implementation
  detail, not blocking), hover returns `undefined`. No user-facing error
  surface, consistent with the outline provider's "empty/absent is an
  acceptable degraded state" convention. Schema fetch failures surface only
  as the existing `SchemaStatus.load_error` semantics (no violations
  computed, no new diagnostic for "schema failed to load" in this pass).

## Testing

- `confy-core`: new `schema_violations()` tested in `schema_headless.rs`
  (existing suite) — asserts returned `text_range` matches the violating
  node's known `Node.text_range` for representative fixtures (nested table
  violation, array-element violation, `required`-on-parent violation).
- `confy-ffi`: extend `functional_smoke.mjs` with a `schema_violations()`
  shape check against a known invalid document + schema pair.
- `editors/vscode`:
  - Pure-function unit tests (plain `node --experimental-strip-types`,
    matching `byteToPosition.spec`'s convention): the outline range-hit-test
    helper used by Hover.
  - `SchemaSessionManager` lifecycle unit tests against a mocked minimal
    `vscode` document/fs/fetch surface: open→change→close transitions,
    debounce coalescing (rapid changes → one reparse), schema-cache reuse
    (unchanged source → no second fetch call).
  - Manual verification in the Extension Development Host (per repo
    convention — user-performed, not automated): open a `.toml`/`.yaml` file
    with a `#:schema`/yaml-language-server modeline pointing at an invalid
    document, confirm Problems panel shows the violation and hover shows
    type/enum info; install/enable Even Better TOML and redhat.vscode-yaml,
    reload window, confirm confy's diagnostics disappear for that language
    while hover still appears.

## Open implementation details (left to the implementation plan, not blocking design approval)

- Exact debounce value (300ms proposed) — may need tuning against real
  keystroke cadence during manual verification.
- Whether a transient parse failure mid-keystroke should clear diagnostics
  immediately or hold the last-good set until the next successful reparse
  (minor UX polish, not architecturally significant).
- Whether `DetectSchema`'s "idempotent no-op if hint unchanged" check
  compares the raw hint string or something structural — an implementation
  detail of `detect_and_request_schema`'s existing dirty-checking, not new
  surface this design introduces.
