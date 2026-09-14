// T8 (R14/R15/R29): the Raw breadcrumb jump's shared byte-offset helpers,
// kept pure and DOM-free so both the Raw view and Raw write selection
// branches in ui.ts use one conversion, and so it is directly importable by
// a spec without going through ui.ts's wasm-dependent module graph.
import type { OutlineNode, Path } from "./types.js";
import { pathEq } from "./path-utils.js";

// `OutlineNode.text_range` is UTF-8 byte offsets over `serialize()`; DOM
// APIs (`Range`, `<textarea>.setSelectionRange`) index by UTF-16 code unit.
// A document with any non-ASCII byte before the target makes a naive
// `slice(byteOffset)` land mid-character — measured and quantified in the
// design record's F5/T2.3 (CJK+emoji fixture, drift === 8).
export function byteToCodeUnit(text: string, byteOffset: number): number {
  let bytes = 0;
  for (let i = 0; i < text.length; i++) {
    if (bytes >= byteOffset) return i;
    const code = text.charCodeAt(i);
    if (code < 0x80) bytes += 1;
    else if (code < 0x800) bytes += 2;
    else if (code >= 0xd800 && code <= 0xdbff) {
      bytes += 4; // astral code point: one UTF-16 surrogate pair, 4 UTF-8 bytes
      i++; // consume the low surrogate too (the `for` loop's own ++ finishes the pair)
    } else bytes += 3;
  }
  return text.length;
}

// Depth-first lookup by exact path match, mirroring the outline's own
// nesting (F5: the whole member — key included — not a value-only node).
export function findOutlineByPath(nodes: OutlineNode[], path: Path): OutlineNode | undefined {
  for (const n of nodes) {
    if (pathEq(n.path, path)) return n;
    if (path.length > n.path.length) {
      const hit = findOutlineByPath(n.children, path);
      if (hit) return hit;
    }
  }
  return undefined;
}
