// Shared `Path` helpers (previously re-declared in select.ts / dnd.ts /
// touch/render.ts / touch/app.ts).
import type { Path, SessionSnapshot, ViewRow } from "./types.js";

export function pathEq(a: Path, b: Path): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export const parentOf = (p: Path): Path => p.slice(0, -1);

// Neither web host draws the **root row** (empty path): `render.ts` and
// `touch/render.ts` both skip it, because the document node has no key/value
// of its own to show. Core, however, can legitimately leave the cursor there
// — `visible_nodes()` starts at the root, so `cursor_home` (`g`/Home) lands on
// it, `cursor_up` from the first drawn row steps onto it, and a fresh session
// starts there (the TUI *does* draw the root, so core is right for its own
// host). A cursor on an undrawn row is an invisible focus cursor, which no
// amount of scrolling can reveal, so both web hosts re-target the first drawn
// row after a keyboard nav dispatch. `null` = nothing to re-target.
export function drawnCursorFallback(snap: SessionSnapshot): Path | null {
  if (snap.cursor.length > 0) return null;
  return snap.rows.find((r) => r.path.length > 0)?.path ?? null;
}

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
