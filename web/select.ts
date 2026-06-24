// Pointer selection gestures → `SetSelection` intent (WEBUI.md / PORTING §8.4).
// Pure logic only: given the current `SessionSnapshot` and a click (with
// modifier keys) or a marquee rectangle, compute the *full* next selection set.
// The Session is the source of truth — we never track selection here beyond a
// shift-range `anchor`; the resolved set is handed to core via `SetSelection`,
// which normalizes it and moves the cursor to the focal (last) path.
import type { Path, SessionSnapshot } from "./types.js";

const eq = (a: Path, b: Path) => JSON.stringify(a) === JSON.stringify(b);

function visiblePaths(snap: SessionSnapshot): Path[] {
  return snap.rows.filter((r) => r.path.length > 0).map((r) => r.path);
}
function selectedPaths(snap: SessionSnapshot): Path[] {
  return snap.rows.filter((r) => r.selected).map((r) => r.path);
}

// Anchor for shift-range selection: the last focal (non-shift) click. Reset by
// a plain or ⌘/Ctrl click; consulted by a shift click.
let anchor: Path | null = null;
// Selection to preserve across a shift round: the committed set at the moment
// the anchor was last set. A shift-range unions onto this, so prior segments
// (e.g. an earlier 1–3) survive while a new 5–7 range is dragged out.
let base: Path[] = [];

/**
 * Reset the shift-range anchor and the preserved base (e.g. after a marquee or
 * a programmatic move). `keep` becomes the set a following shift-range unions
 * onto.
 */
export function setAnchor(p: Path | null, keep: Path[] = []): void {
  anchor = p;
  base = keep;
}

interface Mods {
  shiftKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
}

/**
 * Resolve a row-body click (with modifiers) into the next full selection set,
 * ordered so the focal path is **last** (core's cursor follows it):
 *   - plain        → just the clicked row (anchor ← clicked)
 *   - ⇧ (shift)    → contiguous range anchor…clicked over the visible rows
 *   - ⌘/Ctrl       → toggle the clicked row in/out of the current set
 */
export function resolveClick(
  snap: SessionSnapshot,
  clicked: Path,
  ev: Mods,
): Path[] {
  const all = visiblePaths(snap);
  if (ev.shiftKey && anchor) {
    const ai = all.findIndex((p) => eq(p, anchor!));
    const ti = all.findIndex((p) => eq(p, clicked));
    if (ai >= 0 && ti >= 0) {
      const [lo, hi] = ai <= ti ? [ai, ti] : [ti, ai];
      const range = ai <= ti ? all.slice(lo, hi + 1) : all.slice(lo, hi + 1).reverse();
      // Union the range onto the preserved base so earlier segments survive;
      // re-shift-clicking from the same anchor redefines (can shrink) the range.
      const baseKept = base.filter((p) => !range.some((q) => eq(p, q)));
      // Keep the clicked end last so the cursor lands there.
      return [...baseKept, ...range];
    }
  }
  if (ev.ctrlKey || ev.metaKey) {
    const cur = selectedPaths(snap);
    const without = cur.filter((p) => !eq(p, clicked));
    const added = without.length === cur.length;
    // ⌘/Ctrl-click sets a new anchor without clearing: a following shift-range
    // unions onto this toggled set.
    const next = added ? [...without, clicked] : without;
    anchor = clicked;
    base = next;
    return next;
  }
  anchor = clicked;
  base = [];
  return [clicked];
}

/** Paths of every `.row` whose bounding box intersects the marquee `rect`. */
export function rowsInRect(treeEl: HTMLElement, rect: DOMRect): Path[] {
  const out: Path[] = [];
  treeEl.querySelectorAll<HTMLElement>(".row").forEach((el) => {
    const r = el.getBoundingClientRect();
    const miss =
      r.bottom < rect.top ||
      r.top > rect.bottom ||
      r.right < rect.left ||
      r.left > rect.right;
    if (!miss && el.dataset.path) out.push(JSON.parse(el.dataset.path) as Path);
  });
  return out;
}
