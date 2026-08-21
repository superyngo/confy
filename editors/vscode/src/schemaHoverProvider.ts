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
  constructor(private readonly getManager: () => Promise<SchemaSessionManager>) {}

  async provideHover(
    document: vscode.TextDocument,
    position: vscode.Position,
  ): Promise<vscode.Hover | undefined> {
    try {
      // Lazy: the wasm session manager is only awaited here, on the first
      // actual hover — activation must not force wasm instantiation.
      const manager = await this.getManager();
      const key = document.uri.toString();
      const outline = manager.outline(key);
      if (!outline) return undefined;
      const byteOffset = utf16OffsetToUtf8ByteOffset(document.getText(), document.offsetAt(position));
      const path = findPathAtByteOffset(outline, byteOffset);
      if (!path) return undefined;
      const hint = manager.schemaHint(key, path);
      if (!hint) return undefined;
      const text = renderEditHint(hint);
      return text ? new vscode.Hover(new vscode.MarkdownString(text)) : undefined;
    } catch {
      // Never throw into VS Code's UI (ConfyOutlineProvider convention).
      return undefined;
    }
  }
}
