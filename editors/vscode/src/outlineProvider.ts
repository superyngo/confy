import * as vscode from "vscode";
import { readFileSync } from "node:fs";
import { formatFromName } from "./formatFromName.js";
import { byteOffsetsToRange } from "./byteToPosition.js";
// The wasm-pack `--target web` glue for the core, staged into media/ by
// build.mjs from web/dist. Imported statically rather than the design doc's
// sketch of a dynamic import(): whether esbuild's dynamic-import-from-CJS
// output reliably works under the Node-18-bundled extension host was an
// explicit open implementation detail of the design spec, and the static
// form sidesteps it — esbuild inlines the glue at build time, and importing
// the glue has no side effects, so wasm instantiation stays deferred to
// loadConfySession below.
import * as ffi from "../media/pkg/confy_ffi.js";

interface OutlineNode {
  key: string;
  path: unknown;
  type_label: string;
  value: string | null;
  text_range: [number, number];
  key_text_range: [number, number] | undefined;
  children: OutlineNode[];
}

interface ConfySessionCtor {
  new (text: string, format: string): { outline(): OutlineNode[] };
}

let ffiInit: Promise<ConfySessionCtor> | undefined;

// Loading the wasm in the extension host (Node.js), not the webview: the
// generated `--target web` glue only calls `fetch()` when `init()` receives a
// string/URL/Request; passing raw bytes makes it call
// `WebAssembly.instantiate(bytes, imports)` directly — identical API in Node
// and the browser (confirmed against the generated confy_ffi.js). Module-level
// singleton: first call wins, no per-request re-init.
async function loadConfySession(context: vscode.ExtensionContext): Promise<ConfySessionCtor> {
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

// Keys are `OutlineNode.type_label` strings — the `node_type_label`/
// `node_type_label_str` vocabulary from confy-core's status_fmt.rs (the same
// one web/kind-labels.ts uses): "inline" and "array-of-tables", hyphenated.
const KIND_MAP: Record<string, vscode.SymbolKind> = {
  table: vscode.SymbolKind.Object,
  inline: vscode.SymbolKind.Object,
  array: vscode.SymbolKind.Array,
  "array-of-tables": vscode.SymbolKind.Array,
  string: vscode.SymbolKind.String,
  integer: vscode.SymbolKind.Number,
  float: vscode.SymbolKind.Number,
  bool: vscode.SymbolKind.Boolean,
  null: vscode.SymbolKind.Null,
};

function symbolKindFor(typeLabel: string): vscode.SymbolKind {
  return KIND_MAP[typeLabel] ?? vscode.SymbolKind.Constant; // datetime variants etc.
}

function toDocumentSymbol(node: OutlineNode, document: vscode.TextDocument): vscode.DocumentSymbol {
  const range = byteOffsetsToRange(document, node.text_range[0], node.text_range[1]);
  const selectionRange = node.key_text_range
    ? byteOffsetsToRange(document, node.key_text_range[0], node.key_text_range[1])
    : range;
  const detail = node.value ?? ""; // scalar leaves only (spec Q3); containers stay empty.
  const symbol = new vscode.DocumentSymbol(
    node.key,
    detail,
    symbolKindFor(node.type_label),
    range,
    selectionRange,
  );
  symbol.children = node.children.map((c) => toDocumentSymbol(c, document));
  return symbol;
}

export class ConfyOutlineProvider implements vscode.DocumentSymbolProvider {
  constructor(private readonly context: vscode.ExtensionContext) {}

  async provideDocumentSymbols(
    document: vscode.TextDocument,
    token: vscode.CancellationToken,
  ): Promise<vscode.DocumentSymbol[]> {
    try {
      const ConfySession = await loadConfySession(this.context);
      if (token.isCancellationRequested) return [];
      const format = formatFromName(document.fileName);
      const session = new ConfySession(document.getText(), format);
      const outline = session.outline();
      if (token.isCancellationRequested) return [];
      return outline.map((n) => toDocumentSymbol(n, document));
    } catch {
      // Never throw into VS Code's UI — an empty Outline is an acceptable
      // degraded state for a read-only convenience feature (e.g. mid-edit
      // invalid document, or wasm init failure).
      return [];
    }
  }
}
