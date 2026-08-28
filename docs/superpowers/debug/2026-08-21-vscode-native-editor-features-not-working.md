# Debug Handoff: VS Code Native-Editor Features Not Working

✅ **Resolved — historical reference.** The behavior investigated below now works; see `CHANGELOG.md`. Kept for the development record, not as an open issue.

**Date:** 2026-08-21  
**Branch/Commit:** `main` @ `3bd4060`  
**Features affected:** DocumentSymbolProvider (Outline/breadcrumbs) + Schema Diagnostics/Hover

---

## Symptom

Two native-text-editor features — both implemented in the `vscode-schema-hints` feature branch and merged to `main` — are **silently doing nothing** in the installed `.vsix`:

1. **Outline / breadcrumbs** (`DocumentSymbolProvider` for TOML/YAML in the native text editor):  
   - `⇧⌘O` (Go to Symbol in File) shows **"在 'tasks.toml' 中找不到任何符號"** (No symbols found)  
   - Breadcrumb bar shows only the file path (`scripts > tasks.toml`), never a key path  

2. **Schema diagnostics + hover** (`onDidOpenTextDocument` pipeline + `HoverProvider`):  
   - Problems panel is **completely empty** even with a `#:schema ./tasks.schema.json` hint in the file that the confy custom editor loads correctly  
   - Hovering over keys shows nothing  

**Confirmed working:** confy's custom editor (webview) correctly loads the local schema file and shows schema violations inside the confy UI — this is a completely separate code path (webview-side wasm fetch), so it proves the extension IS activating and the wasm artifact is not corrupted.

---

## Environment

- macOS, VS Code (not Insiders), GitHub Copilot installed  
- Extension installed as `.vsix` (`confy-vscode-0.20.0.vsix`, built `2026-08-21 09:41`)  
- Test file: `tasks.toml` with `#:schema ./tasks.schema.json` on line 1, opened as **native text editor** (confirmed via "文字編輯器" label in title bar)  
- `tamasfe.even-better-toml` is **NOT installed** (so coexistence deferral is not the cause)  
- VS Code Output panel log sources: Git, GitHub, GitHub Auth, Copilot Chat, Copilot Log, JSON Language Server, Microsoft Auth — **no "confy" entry** (expected — extension has no output channel)

---

## What Has Been Ruled Out

| Hypothesis | Status | Evidence |
|---|---|---|
| wasm artifact corrupted | ❌ Ruled out | Plain Node test: `readFileSync` + `ffi.default({module_or_path: bytes})` → `outline()` works |
| Packaged vsix missing wasm | ❌ Ruled out | `unzip` + Node test against extracted vsix also works |
| `package.json` / `activationEvents` malformed | ❌ Ruled out | Valid JSON, `"activationEvents": ["onStartupFinished"]`, `"main": "./dist/extension.js"` |
| Extension not activating | ❌ Ruled out | Custom editor (webview) works → `activate()` ran |
| User testing in wrong editor | ❌ Ruled out | "文字編輯器" confirmed in title bar, ⇧⌘O explicitly tested |
| Even Better TOML coexistence deferral | ❌ Ruled out | Not installed |
| `esbuild import.meta` causing crash at load time | ❌ Ruled out | `import_meta.url` path only executes when `module_or_path === undefined`; we always pass `{module_or_path: bytes}` |
| `ConfySession`/`wasm` scoping in CJS bundle | ❌ Ruled out | Both are `var`-hoisted at top of bundle scope, `wasm` is set by `__wbg_finalize_init` before any session methods are called |

---

## Root Cause Hypothesis

**`loadConfySession()` fails silently inside the VS Code extension host.**

`loadConfySession` is the shared singleton initializer used by BOTH features:

```ts
// editors/vscode/src/wasmSession.ts
let ffiInit: Promise<ConfySessionCtor> | undefined;

export async function loadConfySession(context: vscode.ExtensionContext): Promise<ConfySessionCtor> {
  if (!ffiInit) {
    ffiInit = (async () => {
      const bytes = readFileSync(
        vscode.Uri.joinPath(context.extensionUri, "media/pkg/confy_ffi_bg.wasm").fsPath,
      );
      await ffi.default({ module_or_path: bytes });
      return ffi.ConfySession as unknown as ConfySessionCtor;
    })();
  }
  return ffiInit;
}
```

If this IIFE throws/rejects once, `ffiInit` stores a rejected Promise forever — all subsequent calls return the same rejection. Since both callers swallow errors silently (see below), nothing is visible:

- `outlineProvider.ts:provideDocumentSymbols` — try/catch returns `[]`  
- `extension.ts:openDoc()` — called with `void openDoc(document)`, rejection discarded  
- `schemaHoverProvider.ts:provideHover` — try/catch returns `undefined`  

### Most Likely Sub-causes (unconfirmed, need runtime logging to confirm)

1. **`readFileSync` path resolution failure**: `context.extensionUri.fsPath` might not resolve to a valid filesystem path in some VS Code configurations (e.g., VS Code virtual workspaces, or the installed extension directory having unexpected structure)
2. **`WebAssembly.instantiate` rejected**: Some Electron versions or security policies might reject wasm instantiation from raw bytes in the extension host process
3. **`__wbg_get_imports()` failure**: The wasm import object generation might throw if certain browser/Node globals expected by wasm-bindgen aren't available in the extension host

---

## Recommended Fix: Add Error Visibility

**The blocker for diagnosis is that all error paths are silently swallowed.**

### Step 1: Add logging to `wasmSession.ts`

```ts
export async function loadConfySession(context: vscode.ExtensionContext): Promise<ConfySessionCtor> {
  if (!ffiInit) {
    ffiInit = (async () => {
      try {
        const wasmPath = vscode.Uri.joinPath(context.extensionUri, "media/pkg/confy_ffi_bg.wasm").fsPath;
        console.log("[confy] wasm path:", wasmPath);
        const bytes = readFileSync(wasmPath);
        console.log("[confy] wasm bytes read:", bytes.length);
        await ffi.default({ module_or_path: bytes });
        console.log("[confy] wasm initialized OK");
        return ffi.ConfySession as unknown as ConfySessionCtor;
      } catch (e) {
        console.error("[confy] loadConfySession FAILED:", e);
        throw e;
      }
    })();
  }
  return ffiInit;
}
```

### Step 2: Rebuild + reinstall the vsix

```bash
cd /Volumes/Home/Users/wen/repos/confy/editors/vscode
npx tsc --noEmit
node build.mjs
npx vsce package --allow-missing-repository
# Install confy-vscode-0.20.0.vsix in VS Code
```

### Step 3: Check Developer Tools console

In VS Code: **Help > Toggle Developer Tools > Console tab**  
Filter by `[confy]`. The `console.log`/`console.error` output from extension host code appears here.

Expected success path:
```
[confy] wasm path: /Users/wen/.vscode/extensions/wenanlin.confy-vscode-0.20.0/media/pkg/confy_ffi_bg.wasm
[confy] wasm bytes read: 2803110
[confy] wasm initialized OK
```

If any of those lines is missing or followed by `FAILED:`, the error message will identify the exact failure point.

---

## Code Locations

| File | Purpose | Key lines |
|---|---|---|
| `editors/vscode/src/wasmSession.ts` | Shared wasm loader (singleton) | `loadConfySession()` |
| `editors/vscode/src/outlineProvider.ts` | DocumentSymbolProvider | `provideDocumentSymbols()` catch block eats errors |
| `editors/vscode/src/extension.ts` | Wires all providers | `void openDoc(document)` discards async errors |
| `editors/vscode/src/schemaHoverProvider.ts` | Hover provider | try/catch eats errors |
| `editors/vscode/dist/extension.js` | Bundled output | `loadConfySession` @ line ~1004, `wasm` var @ line 939 |

---

## Additional Context

- `context.extensionUri` for an installed `.vsix` should be `file:///Users/wen/.vscode/extensions/wenanlin.confy-vscode-0.20.0/`
- The wasm path the code constructs: `<extensionUri>/media/pkg/confy_ffi_bg.wasm`
- The vsix was verified to contain the wasm at `extension/media/pkg/confy_ffi_bg.wasm` (6 files in pkg/)
- wasm file size: 2,803,110 bytes
- Build system: esbuild CJS bundle, `external: ["vscode"]` only, `confy_ffi.js` is inlined
- The `import.meta.url` → `import_meta.url` (empty object) esbuild substitution is benign because the `module_or_path === undefined` branch that uses it is never taken when bytes are passed

---

## Research: Can the Agent Directly Control VS Code?

Short answer: **partially, but not full GUI automation of VS Code itself**.

### What the coding agent can already do in this environment

1. Edit source files and run build/test commands in terminal.
2. Use VS Code language-service style operations (e.g. diagnostics/symbol-aware operations via tooling integration).
3. Run repeatable scripts/tests and report exact output.

### What it cannot directly do here

1. Click VS Code UI controls (Outline panel, breadcrumb bar, Command Palette) directly.
2. Interact with VS Code desktop chrome like a human tester without a dedicated VS Code automation harness.

### Practical way to achieve "direct VS Code control" for debugging

Use **VS Code extension integration tests** (`@vscode/test-electron`) so the agent can drive a real Extension Host process programmatically.

This gives scriptable control of the exact native-editor APIs we care about:

1. `vscode.workspace.openTextDocument`
2. `vscode.window.showTextDocument`
3. `vscode.commands.executeCommand("vscode.executeDocumentSymbolProvider", uri)`
4. Diagnostic assertions via `vscode.languages.getDiagnostics(uri)`
5. Hover assertions via `vscode.executeHoverProvider`

In other words: not mouse-click automation, but **API-level native VS Code runtime automation**, which is the right level for this regression.

---

## Recommended Debugging Upgrade (Actionable)

### A. Add a small extension-host integration test target

In `editors/vscode`, add a test runner based on `@vscode/test-electron` that:

1. Opens fixture `tasks.toml` with `#:schema ./tasks.schema.json`
2. Waits for extension activation
3. Asserts `DocumentSymbolProvider` returns non-empty symbols
4. Asserts diagnostics include expected schema warnings
5. Asserts hover on a key returns `Allowed values` or bounded hint text

### B. Keep runtime logs visible (temporary)

Keep the temporary `[confy]` logging in `loadConfySession()` while investigating:

1. wasm absolute path
2. bytes length
3. init success/failure

### C. Prefer extension-host logs over silent fallback during debug

Current providers intentionally degrade silently (`[]` / `undefined`). During debug, also emit `console.error` before fallback return so failures are observable.

---

## Notes on Log Surfaces

For installed extensions, also check:

1. **Output panel** → `Log (Extension Host)`
2. **Developer Tools Console** (still useful, but not the only channel)

This avoids a false negative where no custom output channel named `confy` exists but errors were still written to extension-host logs.

---

## 2026-08-21 Follow-up: API-level Reproduction Result

A new extension-host integration harness (`@vscode/test-electron`) was added and run against this workspace. It reproduces the native-editor regression programmatically (no manual GUI clicking required).

### Reproduced failure

`vscode.executeDocumentSymbolProvider` returns no symbols, and runtime logs now reveal concrete errors:

1. `openDoc failed Error: serde error: unknown variant DetectSchema ...`
2. `outline provider failed TypeError: session.outline is not a function`

### Interpretation

This is no longer a "silent failure" mystery. The extension host successfully loads wasm, but the loaded wasm API does not match the TypeScript caller expectations:

1. TS calls `dispatch("DetectSchema")`, but wasm's `Intent` enum in that artifact does not contain `DetectSchema`.
2. TS expects `session.outline()` to exist, but the loaded `ConfySession` object does not provide it.

### Most likely root cause

**Stale/incorrect `media/pkg` artifact drift** (extension bundle copied an older `web/dist/pkg` output), not VS Code API registration failure.

Observed in this run:

1. wasm loaded from `editors/vscode/media/pkg/confy_ffi_bg.wasm`
2. wasm size logged as `1,015,212` bytes (much smaller than the previously verified `~2,803,110` bytes in the original handoff)

### Next verification steps

1. Rebuild `crates/confy-ffi` wasm artifact.
2. Rebuild `web/dist` (ensuring fresh `pkg/*`).
3. Re-run `editors/vscode/node build.mjs` to restage `media/`.
4. Re-run integration harness and confirm:
  - symbols are non-empty,
  - diagnostics appear,
  - hover contains allowed-values/bounds hint.

---

## 2026-08-21 Final Resolution

The regression was fixed in two steps, both required:

1. **Artifact freshness fix**
  - `web/build.mjs` now assembles a fresh `web/dist` every build (including `dist/pkg/*`).
  - This prevents `editors/vscode/build.mjs` from copying stale `media/pkg` artifacts.

2. **Cleaner extension-host wasm loader fix**
  - `editors/vscode/src/wasmSession.ts` was changed to dynamically load
    `media/pkg/confy_ffi.js` via an absolute file URL (`pathToFileURL(...)`).
  - This avoids CJS-bundle static capture of the wasm ESM glue and removes the
    build-time `import.meta` warning while preserving runtime behavior.

### Why this second fix matters

After artifact freshness was fixed, integration tests still failed with:

- `LinkError: WebAssembly.instantiate(): Import #4 "./confy_ffi_bg.js" ... requires a callable`

That error came from bundling boundary mismatch (CJS host vs ESM wasm glue), not schema logic.
Runtime dynamic import by file URL resolves the module in Node/Electron exactly as ESM, so its
generated import map is intact.

### Final verification evidence

Executed locally in `editors/vscode`:

1. `npm run build`
  - Result: success, **no `import.meta` warning**.
2. `npm run integration-test`
  - Result: success (exit code `0`).
  - Extension-host logs show:
    - `[confy-vscode] wasm bytes: 2799135`
    - `[confy-vscode] wasm initialized`

This confirms native-editor symbol/diagnostic/hover pipelines are no longer blocked by the
wasm loader path.
