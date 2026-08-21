# VS Code Schema Hints (Diagnostics + Hover) — Design

Status: approved for planning (design phase)
Date: 2026-08-21 (revised after grilling round)

## Goal

Surface confy-core's existing JSON Schema support (`crates/confy-core/src/schema/`)
inside VS Code's **native** TOML/YAML text editor as **Problems-panel
diagnostics** (schema violations, plus a schema-load-failure notice) and
**hover tooltips** (type/enum/const/bounds at the cursor's node), the same
way `outlineProvider.ts` surfaces the core's AST as Outline/breadcrumbs.

## Non-goals (this pass)

- **JSON.** Same reasoning as the outline provider: VS Code's built-in JSON
  language already does schema-aware diagnostics/hover.
- **Completion.** Cursor-position key/value intent detection is materially
  harder than diagnostics/hover and is deferred to a future pass.
- **Any change inside confy's own custom editor** (`viewType: "confy.editor"`).
  Same platform constraint as the outline provider — this only affects VS
  Code's native text editor tab.
- **Reacting to extensions installed while a document tab is already open.**
  Coexistence deferral is re-checked every time a document is (re)opened
  (see below) — fully dynamic for newly-opened tabs — but confy does not
  subscribe to `vscode.extensions.onDidChange` to retroactively re-evaluate
  tabs that are already open when a new extension is installed mid-session.
  Reopening the file (or reloading the window) picks up the change, matching
  normal VS Code extension-interaction expectations.
- **`load_error` surfacing on web/TUI/touch.** Investigating this pass
  surfaced that `SchemaStatus.load_error` has *no visible UI anywhere today*,
  not even in the web host (`web/ui.ts` only renders `violation_count`).
  That is a real, cross-platform gap, but out of scope here — this pass adds
  a `load_error` Diagnostic **for VS Code only** (cheap, and the Diagnostics
  plumbing is already being built); web/TUI/touch parity is tracked as a
  separate follow-up, not bundled into this design.
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

**Decision:** on every `SchemaSessionManager.open(document)` call (i.e. every
time a matching document is opened — fully dynamic, not cached once at
`activate()`), check `vscode.extensions.getExtension(id) !== undefined` for
both — **installed**, not `isActive`. `isActive` was rejected: confy's own
`activationEvents` is `["onStartupFinished"]` while Even Better TOML /
redhat.vscode-yaml typically activate lazily (`onLanguage:*`); reading
`isActive` at a fixed point risks a false negative if confy's check runs
before the other extension has finished activating. "Installed" is a
synchronous, static property lookup with no such race, and it's the only
property that actually matters for the decision being made ("should this
language's Diagnostics be fully deferred to that extension"), not "is it
currently running."

If installed for a given language, confy **defers**: it skips
registering/running Diagnostics for that language entirely for that
document (no `ConfySession` is even constructed for diagnostics purposes).
**Hover always registers regardless** — hover content from multiple
providers stacks harmlessly in the same popup, and the risk/annoyance
profile is much lower than duplicate red squiggles in Problems.

## Architecture

Three layers, bottom-up, all additive.

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
    /// **Not** idempotent in core — verified against `session.rs:1489`:
    /// `detect_and_request_schema()` unconditionally returns `Some(source)`
    /// whenever an in-document hint is found, with no comparison against
    /// `self.schema`'s current source, and `apply_schema_text()`
    /// unconditionally recompiles the `jsonschema::Validator` every call.
    /// Any dedup ("don't re-fetch/re-compile if the hint didn't change")
    /// is therefore the **host's** responsibility (see `SchemaSessionManager`
    /// below) — this intent is a cheap text scan, not a guarded no-op.
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
    /// resolves (defensive; in the native-editor flow `schema_violations()`
    /// is always called against the same tree revision the violations were
    /// computed from, so this should not occur in practice — see "Open
    /// implementation details").
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
Naming note (surfaced while grilling this design, not changed here): the ffi
method `schema_hint` wraps `edit_hint`/`EditHint` (an editing-widget
constraint resolved for one node), which is a *different* concept from
`schema::hints::detect_hint` (in-document `#:schema`/yaml-modeline
detection, feeding `Intent::DetectSchema` above). Both are called "hint" in
existing production code; this doc uses full qualified names throughout to
avoid conflating them. `CONTEXT.md`'s Schema section defines neither term
today — left as-is; not a blocker for this design.

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

**Session lifecycle: one persistent `ConfySession` per open document,
updated in place — not rebuilt from scratch per edit.** This deliberately
diverges from the outline provider's per-request stateless pattern: schema
loading is a stateful async round trip (compiled `jsonschema::Validator`,
loaded schema text) that must not be thrown away and redone on every
debounced keystroke. See ADR 0007 for the full rationale — the short version:
`Intent::ApplyReplace { path: [], text }` (already used for external-edit
resolution, and already supporting whole-document reparse per
`cst_edit/mod.rs`'s empty-path `Replace` handling) lets the host feed each
edit's new text into the *same* session. On success, `SchemaState` (the
compiled validator) is untouched — only `revalidate_schema()` reruns
(cheap: re-`validate()`, not re-`compile()`). On failure (invalid mid-edit
syntax, `MutateError::Fragment`), the session's tree is left at its last
successfully-parsed state and `self.error` is set — nothing about `schema`
changes.

```ts
interface ManagedDoc {
  session: ConfySession;         // one live wasm session per open document
  format: string;
  diagnosticsDeferred: boolean;  // per Coexistence decision, fixed at open()
  loadedSchemaSource?: SchemaSource; // what's currently loaded in `session`'s SchemaState — compared against each DetectSchema result to decide whether to re-fetch/reload at all
  generation: number;            // bumped on every reparse; guards stale async fetch resolution (see step 4 below)
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

**`open(document)`** (first time a matching document is seen):
1. Resolve `diagnosticsDeferred` per the Coexistence decision (installed-check,
   done fresh here — not cached at `activate()`).
2. `new ConfySession(document.getText(), format)`.
3. Run the shared "sync schema" steps below (`fetchAndLoadSchemaIfNeeded`).
4. Update diagnostics (unless deferred) and store the `ManagedDoc`.

**`reparse(document)`** (debounced 300ms on `onDidChangeTextDocument`):
1. Bump `generation`.
2. `session.dispatch({ApplyReplace: {path: [], text: document.getText()}})`.
3. If the returned snapshot carries a (non-null) `error`: this was a
   mid-edit invalid-syntax reparse. **Clear diagnostics for this document**
   (Q7 decision below) and return early — do not run schema sync this cycle
   (nothing changed; `session.schema` and its violations are already stale
   relative to the *rejected* text, not worth re-deriving).
4. Otherwise run the shared "sync schema" steps, then update diagnostics
   (unless deferred).

**Shared "sync schema" steps** (`fetchAndLoadSchemaIfNeeded`, used by both
`open` and a successful `reparse`):
1. `session.dispatch({DetectSchema: null})` → read `schema_fetch_request`
   off the returned snapshot.
2. If absent, or it equals `loadedSchemaSource` (value equality) **and** a
   schema is already loaded (`SchemaStatus` shows no `load_error` and the
   source matches): nothing to do — the already-compiled validator in this
   session's `SchemaState` stays as-is. This is the dedup core doesn't do
   (see the `Intent::DetectSchema` doc comment above) — it must happen here.
3. Otherwise resolve the new/changed source, capturing the current
   `generation`:
   - **Local path** — `fs.promises.readFile`, resolved relative to
     `document.uri.fsPath`'s directory (mirrors `web/fs.ts`'s
     `readSiblingFile` resolution rule — bare/`./`/`../`-relative paths
     resolve against the open file's directory, absolute paths pass through
     untouched — reimplemented here since the extension host has direct
     `fs` access and does not need the webview's `read-schema-file` message
     round trip).
   - **URL** — global `fetch()` (Node 18, unsandboxed — no CSP concern here,
     unlike the webview's `read-schema-url` path).
4. When the fetch/read resolves: if `generation` has advanced since step 3
   captured it, **or** the `ManagedDoc` has been removed from `docs`
   (document closed) — discard the result, dispatch nothing. Otherwise
   `session.dispatch({SchemaLoaded: {source, text}})` and update
   `loadedSchemaSource`.

**`schemaProvider.ts`** — registration + the two providers:

```ts
export function registerSchemaFeatures(context: vscode.ExtensionContext) {
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
    // manager's document-lifecycle listeners; diagnostics-update logic reads
    // `ManagedDoc.diagnosticsDeferred` before doing any work
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
- **Diagnostics update** (skipped entirely when `diagnosticsDeferred`):
  `schema_violations()` → `vscode.Diagnostic[]`, **severity always
  `DiagnosticSeverity.Warning`** regardless of `Category` — matches
  `CONTEXT.md`'s "Soft constraint" principle (a Violation never blocks
  anything and every other surface — TUI `[WARN]` glyph, web/touch amber
  dot, web status text — already uses warning-level visual language, never
  error-level) — via the byte→UTF-16 range conversion already built for the
  outline provider (`byteToPosition.ts`, reused, not duplicated) →
  `diagnostics.set(uri, items)`. A violation whose `text_range` is `None`
  (see core note above) is dropped rather than guessed at. **Additionally**:
  if `SchemaStatus.load_error` is non-empty, append one more `Diagnostic`
  (range: line 0, `DiagnosticSeverity.Warning`, message: the `load_error`
  text) — the one piece of `load_error` UI shipping in this pass, VS-Code-only
  (see Non-goals).

## Data flow

```
.toml/.yaml file opened (native text editor)
  -> SchemaSessionManager.open(document)
     -> installed-check (Even Better TOML / redhat.vscode-yaml) -> diagnosticsDeferred
     -> new ConfySession(text, format)
     -> fetchAndLoadSchemaIfNeeded (dispatch DetectSchema -> [fetch/read if new/changed] -> dispatch SchemaLoaded)
     -> [unless deferred] schema_violations() -> byte->UTF16 -> diagnostics.set(uri, ... + load_error diagnostic if any)

cursor hover
  -> outline() [existing] -> range-hit-test -> Path
  -> schema_hint(path) [existing edit_hint] -> Markdown -> vscode.Hover

document edited (debounced 300ms)
  -> SchemaSessionManager.reparse(document)
     -> dispatch({ApplyReplace: {path: [], text}}) on the SAME session
     -> snap.error set (invalid mid-edit syntax)?
          yes -> diagnostics.set(uri, [])  [Q7: clear, don't guess at drifted positions]
          no  -> fetchAndLoadSchemaIfNeeded (usually a no-op: hint unchanged) -> diagnostics update

document closed -> SchemaSessionManager.close(uri); diagnostics.delete(uri)
```

## Error handling

- `confy-core`/`confy-ffi`: `schema_violations()` only reachable on a live
  `ConfySession`; a path that no longer resolves yields `text_range: None`
  (dropped by the host) rather than an error. No new fallible surface beyond
  what `apply_schema_text`/`edit_hint` already handle.
- `editors/vscode`, mid-edit invalid syntax (`ApplyReplace` → `snap.error`
  set): diagnostics for that document are **cleared**, not left stale —
  the session's own violations/`text_range` still describe the *last valid*
  text, and VS Code does not auto-shift diagnostic ranges as the live buffer
  changes underneath them, so displaying them risks pointing at the wrong
  line. Matches the outline provider's "empty/absent is an acceptable
  degraded state" convention. Hover returns `undefined` on any exception.
- Schema fetch/read failure: `SchemaStatus.load_error` set (existing
  mechanism) — this pass surfaces it as one VS-Code-only Diagnostic (see
  Architecture §3); no other platform changes.
- Stale async schema-fetch resolution (edit or close raced ahead of an
  in-flight `fs.readFile`/`fetch`): discarded via the `generation` counter /
  `docs` membership check in `fetchAndLoadSchemaIfNeeded` step 4 — never
  dispatched into a session that's moved on or been closed.

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
    `vscode` document/fs/fetch surface: open→edit(valid)→edit(invalid,
    diagnostics clear)→edit(valid again, diagnostics restored)→close
    transitions; debounce coalescing (rapid changes → one reparse); schema
    reuse (`loadedSchemaSource` unchanged → no second fetch/reload call);
    a fetch that resolves after the document closed is discarded (no
    dispatch, no error).
  - Manual verification in the Extension Development Host (per repo
    convention — user-performed, not automated): open a `.toml`/`.yaml` file
    with a `#:schema`/yaml-language-server modeline pointing at an invalid
    document, confirm Problems panel shows the violation (as a Warning) and
    hover shows type/enum info; point the hint at a nonexistent file, confirm
    the load-error Diagnostic appears; install/enable Even Better TOML and
    redhat.vscode-yaml, reopen the file, confirm confy's diagnostics don't
    appear for that language while hover still does.

## Open implementation details (left to the implementation plan, not blocking design approval)

- Exact debounce value (300ms proposed) — may need tuning against real
  keystroke cadence during manual verification.
- Whether `ViolationView.text_range` should just be non-`Option` given the
  "should not occur in practice" note above — kept `Option` defensively in
  this design; the implementation can drop it if it proves genuinely
  unreachable.
- Whether the coexistence "installed" check also needs a similar check
  against user-configured overrides (e.g. a hypothetical future confy
  setting to force-enable/disable regardless of what's installed) — no such
  setting exists today, not needed for this pass.

## Related decisions

- ADR 0007 (to be written on implementation): persistent-session-with-
  `ApplyReplace` vs per-request session rebuild, for hosts that need
  stateful async schema loading to survive live edits.
