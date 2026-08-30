// Shared Action menu item rendering (design doc
// `docs/superpowers/specs/2026-08-30-action-menu-design.md` §2, ADR 0009).
// The desktop popup (web/ui.ts) and the touch sheet (web/touch/app.ts) both
// call this; only the surrounding chrome (popup vs sheet head/scrim) differs
// per host. Distinct from `web/menu.ts`, which is the Tauri native OS menu
// bar — an unrelated, unrestyleable surface exempted from this item model
// (design doc §9).
import type { ActionItemView } from "./types.js";
import { escapeHtml } from "./escape.js";

/** One `<button class="menu-item">` row, with `.danger` and a preceding
 *  `<div class="menu-sep">` when `separator_before` is set. `index` is the
 *  item's position in `ModeView::ActionMenu.items`, stashed in `data-i` so
 *  the click handler can look the id back up without re-deriving it.
 *  `active` (desktop keyboard nav only — touch is tap-only) marks the item
 *  under the core cursor with the same `.sel` convention the Help/Kind
 *  overlays use (`#overlay .opt.sel`). */
export function actionItemHTML(item: ActionItemView, index: number, active = false): string {
  const sep = item.separator_before ? '<div class="menu-sep"></div>' : "";
  const danger = item.danger ? " danger" : "";
  const sel = active ? " sel" : "";
  const disabled = item.enabled ? "" : " disabled";
  return `${sep}<button class="menu-item${danger}${sel}" data-i="${index}"${disabled}>${escapeHtml(item.label)}</button>`;
}
