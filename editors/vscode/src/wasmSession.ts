import * as vscode from "vscode";
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import type { EditHint, Intent, Path, SessionSnapshot, ViolationView } from "../../../web/types.js";

// Do not statically import the generated wasm glue into the CJS-bundled
// extension entrypoint. The generated `--target web` module expects to be
// loaded as an ESM module and resolves its internal `./confy_ffi_bg.js` import
// at runtime; bundling it into the CJS extension host breaks that import shim,
// yielding the "requires a callable" LinkError seen in integration tests.
// We therefore import the runtime module lazily and pass raw bytes to its
// default initializer, exactly as the browser host does.

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
const LOG_PREFIX = "[confy-vscode]";

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
      try {
        const wasmPath = vscode.Uri.joinPath(context.extensionUri, "media/pkg/confy_ffi_bg.wasm").fsPath;
        const bytes = readFileSync(wasmPath);
        const ffiJsPath = vscode.Uri.joinPath(context.extensionUri, "media/pkg/confy_ffi.js").fsPath;
        const ffi = await import(pathToFileURL(ffiJsPath).href);
        console.log(`${LOG_PREFIX} loading wasm from: ${wasmPath}`);
        console.log(`${LOG_PREFIX} wasm bytes: ${bytes.length}`);
        // Object form: the glue warns the bare-bytes form is deprecated.
        await ffi.default({ module_or_path: bytes });
        console.log(`${LOG_PREFIX} wasm initialized`);
        return ffi.ConfySession as unknown as ConfySessionCtor;
      } catch (error) {
        console.error(`${LOG_PREFIX} loadConfySession failed`, error);
        // Clear failed singleton so a later call can retry after environment fixes.
        ffiInit = undefined;
        throw error;
      }
    })();
  }
  return ffiInit;
}
