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
