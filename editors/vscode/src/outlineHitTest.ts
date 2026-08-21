import type { Path } from "../../../web/types.js";
import type { OutlineNode } from "./wasmSession.js";

function contains(range: [number, number], byteOffset: number): boolean {
  return byteOffset >= range[0] && byteOffset <= range[1];
}

/** The deepest `OutlineNode` whose `text_range` contains `byteOffset`,
 * returned as its `Path` — the hover provider's cursor→node lookup. Walks
 * the outline tree confy's own `outline()` already produces rather than
 * adding a new core query (spec §"Hover"). */
export function findPathAtByteOffset(nodes: OutlineNode[], byteOffset: number): Path | undefined {
  for (const n of nodes) {
    if (!contains(n.text_range, byteOffset)) continue;
    return findPathAtByteOffset(n.children, byteOffset) ?? n.path;
  }
  return undefined;
}
