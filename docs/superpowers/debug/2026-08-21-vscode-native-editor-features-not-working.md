# Debug Handoff: VS Code Native-Editor Features Not Working

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
