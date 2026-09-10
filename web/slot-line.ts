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
