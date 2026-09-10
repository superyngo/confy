// Shared `Path` helpers (previously re-declared in select.ts / dnd.ts /
// touch/render.ts / touch/app.ts).
import type { Intent, Path, SessionSnapshot, ViewRow } from "./types.js";

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

// The paste-mode analogue of `drawnCursorFallback`, and web-only for the same
// reason. While a clipboard is armed the arrow keys step `paste_slots()`,
// whose FIRST entry is the root row's `Into` — which `slot_target` resolves to
// `children.len()`, an append at the document's **end**. The TUI draws the
// root row, so stepping onto it highlights a real, visible row and reads
// correctly there; neither web host draws it, so `↑`/`k`/PageUp/Home from the
// top of the tree instead threw the insertion point to the opposite end of the
// document and clamped there (index 0 cannot step further) — an invisible
// move, above everything, in the one direction the user was not aiming.
//
// `true` = the nav overshot onto that slot and the host should step one slot
// back down, onto `After(root)` (the document's top, which is what the upward
// key meant). Downward navigation and `End` are left alone: reaching the
// append slot from below is exactly right, and both hosts draw it at the last
// row's bottom edge (`rootSlotLine`).
export function overshotUndrawnRootSlot(i: Intent, snap: SessionSnapshot): boolean {
  const upward =
    typeof i === "string"
      ? i === "CursorUp" || i === "CursorHome"
      : typeof i === "object" && i !== null && "PageUp" in i;
  if (!upward) return false;
  const slot = snap.paste_slot as { Into?: Path } | null | undefined;
  return !!slot && !!slot.Into && slot.Into.length === 0;
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
