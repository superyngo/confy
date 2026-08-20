# VS Code `DocumentSymbolProvider` (Outline / Breadcrumbs) — Design

Status: approved for planning (design phase)
Date: 2026-08-20

## Goal

Expose confy's own AST (the lossless CST projection already computed by
`confy-core`) as VS Code's native Outline panel / `Cmd+Shift+O` / breadcrumbs
for TOML and YAML files, via a `vscode.DocumentSymbolProvider`.

## Non-goals (this pass)

- Enhancing confy's own webview breadcrumb bar (`web/breadcrumb.ts`) with span
  data. The core/FFI span fields are designed to be reusable by that consumer
  later, but no webview-side change ships in this pass.
- JSON. VS Code's built-in JSON language already provides outline/breadcrumbs;
  registering a second provider risks duplicate/conflicting entries for no
  incremental value.
- Any change inside confy's own custom editor (`viewType: "confy.editor"`).
  VS Code's native breadcrumbs/Outline cannot attach to a Custom Editor
  webview tab (platform limitation, not something this feature can fix) — see
  "Platform constraint" below. This feature only benefits the plain "default"
  text editor tab (`confy.openTextBeside` / `confy.reopenAsText`).

## Platform constraint (why scope is what it is)

`vscode.DocumentSymbolProvider` binds to a real `vscode.TextEditor`. confy's
primary VS Code UI is a `customEditors` webview (`editors/vscode/package.json`
`viewType: "confy.editor"`); VS Code does not route Outline/breadcrumbs to
custom-editor webview tabs (microsoft/vscode#97095, longstanding, marked
out-of-scope by VS Code core). confy's own `web/breadcrumb.ts` exists
specifically because of this gap (`WEBUI.md` §"Breadcrumb bar + mini-tree").
This feature therefore only takes effect when a file is open in VS Code's
native text editor, not confy's own editor.

## Architecture

Three modules, bottom-up, each additive/read-only — zero behavior change to
existing functionality.

### 1. `confy-core` — `Node` gains two span fields

```rust
pub struct Node {
    // ...existing fields unchanged...
    /// Byte range (half-open, UTF-8 byte offsets into the source text) of the
    /// whole node, including its key and value/children.
    pub span: Range<usize>,
    /// Byte range of just the key token; `None` for keyless nodes (array
    /// elements, AoT entries, Root, comments) — same nodes where `key_sign`
    /// is already `KeySign::None`.
    pub key_span: Option<Range<usize>>,
}
```

All three format backends (TOML via `taplo`, YAML, JSON) are rowan-based
lossless CSTs; every `rowan::SyntaxNode`/`SyntaxToken` already carries
`text_range()`. The three existing `walk()` functions
(`model/cst_project.rs`, `model/json/project.rs`, `model/yaml/project.rs`)
each read `text_range()` off the syntax node/token they're already visiting
and populate the two new fields when constructing a `Node`. No new parsing,
no new tree shape — this is a field-population change at three existing call
sites, not an AST redesign.

`NodeTree` is unchanged (still `{ root: Node }`).

### 2. `confy-ffi` — one new read-only method + one new transport type

```rust
/// Read-only outline transport — deliberately separate from the internal
/// `Node`/`NodeKind` wire shape, matching the existing `ChildView`/
/// `KindOption` convention of small dedicated FFI-boundary types.
pub struct OutlineNode {
    pub key: String,
    pub path: Path,
    pub type_label: String, // same vocabulary as ViewRow::type_label
    pub span: (u32, u32),
    pub key_span: Option<(u32, u32)>,
    pub children: Vec<OutlineNode>,
}
```

```rust
impl ConfySession {
    /// Read-only symbol tree for editor outline/breadcrumb integrations.
    /// Root itself is not included as a wrapping symbol — this returns the
    /// Root's children (mirrors VS Code's own JSON outline, which does not
    /// synthesize a whole-document symbol). `Comment` nodes are omitted.
    pub fn outline(&self) -> Result<JsValue, JsValue> { ... }
}
```

No new wasm build target. `wasm-pack build --target web` stays the only
build; `cf-build.sh` / CI / the `rebuild-wasm-web-after-core-change` workflow
are unaffected in shape (still "rebuild after core change", nothing new to
maintain).

### 3. `editors/vscode` — new `src/outlineProvider.ts`

**Loading the wasm in the extension host (Node.js), not the webview:**
the generated `--target web` glue (`confy_ffi.js`) only calls `fetch()` when
its `init(module_or_path)` receives a string/URL/Request; passing raw bytes
(`Uint8Array`) instead makes it call `WebAssembly.instantiate(bytes, imports)`
directly — identical API in Node and the browser. Confirmed by reading the
generated `crates/confy-ffi/pkg/confy_ffi.js` in this repo (`__wbg_init`,
`__wbg_load`). Extension host code:

```ts
const bytes = fs.readFileSync(
  vscode.Uri.joinPath(context.extensionUri, "media/pkg/confy_ffi_bg.wasm").fsPath,
);
const ffi = await import("../media/pkg/confy_ffi.js"); // dynamic import of ESM from the CJS-bundled extension.js; Node 18 supports this
await ffi.default(bytes);
```

Module-level singleton init (first call wins), no per-request re-init.

**Provider registration:**
- `{ pattern: "**/*.toml" }` and `{ pattern: "**/*.yaml" }` / `"**/*.yml"` —
  **pattern selectors, not `language` selectors.** VS Code core ships no
  built-in TOML language; an unmodified install assigns `.toml` files
  `languageId: "plaintext"`, so a `{ language: "toml" }` selector would never
  fire without the user separately installing a TOML language extension.
  Pattern selectors are language-id-independent and always match.
- JSON/JSONC excluded (Non-goals).

**`provideDocumentSymbols(document, token)`:**
1. Reuse the existing filename→format detection already in
   `editorProvider.ts` (currently a private helper there) — extract it to a
   shared module rather than duplicating the extension-list logic.
2. `new ConfySession(document.getText(), format)`, call `.outline()`.
3. Recursively map `OutlineNode` → `vscode.DocumentSymbol`:
   - `range` from `span`, `selectionRange` from `key_span` (falls back to
     `span` when `key_span` is `None`, e.g. array elements).
   - Byte offsets (UTF-8) → `vscode.Position` needs a UTF-16 code-unit
     conversion (`TextDocument.positionAt` expects UTF-16 offsets; rowan's
     `text_range()` is UTF-8 bytes) — a single-pass pure helper function,
     independently unit-testable (covers multi-byte content: CJK, emoji).
   - `SymbolKind` mapping: `Table`/`InlineTable` → `Object`;
     `Array`/`ArrayOfTables` → `Array`; `Scalar(ScalarType)` → `String` /
     `Number` / `Boolean` / `Null` / `Constant` (datetime variants).
4. On any parse/FFI error (e.g. mid-edit invalid document): catch and return
   `[]` — never throw into VS Code's UI. Respect `token.isCancellationRequested`.

**Performance:** no session caching in v1 — a fresh `ConfySession` per
request is cheap for config-file-sized documents (KB scale). Not optimizing
ahead of a demonstrated need (YAGNI).

## Data flow

```
.toml/.yaml file (native text editor)
  -> VS Code calls outlineProvider.provideDocumentSymbols(document)
  -> extension host: ConfySession::from_text(document.getText(), format)  [wasm, bytes-init]
  -> session.outline() -> OutlineNode[]  (byte spans)
  -> TS: byte->UTF16 offset conversion, OutlineNode -> vscode.DocumentSymbol[]
  -> VS Code renders Outline panel / breadcrumbs / Go to Symbol
```

## Error handling

- confy-core: span population cannot fail (pure field reads off the syntax
  tree already being walked).
- confy-ffi: `outline()` only reachable on a live `ConfySession` (post
  successful `from_text`); no new failure mode.
- editors/vscode: wasm init failure, parse failure, or any exception inside
  `provideDocumentSymbols` → caught, logged, returns `[]`. No user-facing
  error surface — an empty Outline is an acceptable degraded state for a
  read-only convenience feature.

## Testing

- `confy-core`: extend each backend's existing test module
  (`cst_project.rs`/`json/project.rs`/`yaml/project.rs` or their `tests` mods)
  with assertions that `span`/`key_span` byte ranges slice out the expected
  substrings of representative fixtures (nested tables, arrays, AoT, dotted
  keys, YAML block/flow forms).
- `confy-ffi`: extend `functional_smoke.mjs` (currently 92 checks) with
  `outline()` shape + span-value checks against known sample documents.
- `editors/vscode`: no existing test runner (`package.json` has no `test`
  script) — add one standalone `node:test` unit test for the byte→UTF-16
  offset helper (pure function, covers ASCII + multi-byte content). Everything
  else verified via `tsc --noEmit` plus a **manual** pass in the Extension
  Development Host: open a real `.toml` and a real `.yaml` file in the native
  text editor, confirm Outline panel / `Cmd+Shift+O` / breadcrumbs populate
  correctly (per repo convention — WEBUI/plan docs note manual browser/host
  verification is done by the user, not automated).

## Open implementation details (left to the implementation plan, not blocking design approval)

- Exact esbuild handling of the dynamic `import()` of `confy_ffi.js` from the
  CJS-bundled `extension.js` (likely needs marking external or verifying
  esbuild's dynamic-import-from-CJS output works under Node 18 as bundled).
- Whether `outline()` is called once per keystroke-debounced VS Code request
  or whether VS Code itself throttles `DocumentSymbolProvider` calls
  sufficiently that no extra debouncing is needed on the extension side.
