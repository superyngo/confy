import * as vscode from "vscode";
import { formatFromName } from "./formatFromName.js";
import { byteOffsetsToRange } from "./byteToPosition.js";
import { loadConfySession, type OutlineNode } from "./wasmSession.js";

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
