// Shared `Path` helpers (previously re-declared in select.ts / dnd.ts /
// touch/render.ts / touch/app.ts).
import type { Path, ViewRow } from "./types.js";

export function pathEq(a: Path, b: Path): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export const parentOf = (p: Path): Path => p.slice(0, -1);

// Index of `p` among the visible rows that share its parent (= core's
// full-child-sequence index, since an expanded parent shows all its children).
export function siblingIndex(rows: ViewRow[], p: Path): number {
  const par = parentOf(p);
  let i = 0;
  for (const r of rows) {
    if (r.path.length === p.length && pathEq(parentOf(r.path), par)) {
      if (pathEq(r.path, p)) return i;
      i++;
    }
  }
  return i;
}
