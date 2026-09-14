import type { PasteSlot } from "./types.js";

// Horizontal placement of a `PasteSlot::After` insertion line — ADR 0010.
//
// `After(p)` does **not** always mean "a sibling below `p`": when `p` is an
// expanded branch, core's `resolve_target` lands the insert as `p`'s *first
// child* (`crates/confy-core/src/session/insertion.rs`). The TUI has always
// said so visually, indenting its green line one level deeper for that case
// (`paste_line_row`'s `row.depth + 1`, `crates/confy-tui/src/tui/ui.rs`); the
// web `#dropLine`/`#pasteTargetLine` and the touch `.reorder-line` all used
// the hovered row's own indent unconditionally, so every "gap under an
// expanded `[table]`" cue was drawn one level too shallow — the visual half of
// the mismatch ADR 0010 fixes functionally.
//
// Only this decision is shared: the three call sites live in different
// coordinate systems (`#treeWrap` + `scrollTop` vs. the touch tree's own rect)
// and measure their own indentation differently (an `.indent` spacer span vs.
// `.row-main`'s `padding-left`), so each passes the row indent it measured and
// gets back the indent the *line* should use.
export function slotLineIndentPx(rowEl: HTMLElement, rowIndentPx: number): number {
  // `branch` + `open` are the classes both renderers already put on an
  // expanded container row (`web/render.ts`, `web/touch/render.ts`), so this
  // needs no snapshot lookup and stays correct mid-drag, when nothing has
  // re-rendered.
  if (!(rowEl.classList.contains("branch") && rowEl.classList.contains("open"))) {
    return rowIndentPx;
  }
  // One level = the live `--indent` custom property (`:root` in both
  // `web/style.css` — 22px, 16px under the ≤680px query — and
  // `web/touch/style.css` — 18px), so the line tracks whatever scale the rows
  // are currently drawn at instead of a hard-coded copy.
  const step = parseFloat(getComputedStyle(rowEl).getPropertyValue("--indent"));
  return rowIndentPx + (Number.isFinite(step) ? step : 0);
}

// Which row edge an insertion line for a **root-anchored** `PasteSlot` sits on.
// `vRow` gives the vertical anchor, `edge` which of its edges, `hRow` the row
// whose indent the line takes (always a top-level one — the insert lands at
// root depth however deep `vRow` sits).
export interface DocumentEdgeLine {
  vRow: HTMLElement;
  edge: "top" | "bottom";
  hRow: HTMLElement;
}

// The insertion line for one of the two **document-edge** slots, which are
// anchored on the Root and so have no row of their own on ANY host (ADR 0013
// D1/D6; the TUI draws them at its viewport edges for the same reason) —
// `null` for every other slot, leaving the caller's normal per-row lookup
// untouched.
//
// Both edge slots are reachable and legal, so both need a cue: the keyboard
// steps onto them (`paste_slots()` emits `After([])` first and `Into([])`
// last, in screen order) and the *pointer* resolves the first drawn row's top
// band to `After([])`, the only route to "drop above everything".
//
// - `After([])` resolves to root index 0 (`resolve_target`): the document's
//   very top, so the line sits on the FIRST row's *top* edge.
// - `Into([])` resolves to `children.len()` (`slot_target`): append at the
//   document's very end, so the line sits on the LAST row's *bottom* edge.
export function documentEdgeLine(
  treeEl: { querySelectorAll(sel: string): ArrayLike<HTMLElement> },
  slot: PasteSlot | null | undefined,
): DocumentEdgeLine | null {
  if (!slot) return null;
  const path = "Into" in slot ? slot.Into : slot.After;
  if (path.length !== 0) return null;
  const rows = treeEl.querySelectorAll(".row");
  const first = rows[0];
  const last = rows[rows.length - 1];
  if (!first || !last) return null;
  return "Into" in slot
    ? { vRow: last, edge: "bottom", hRow: first }
    : { vRow: first, edge: "top", hRow: first };
}
