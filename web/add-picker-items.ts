// Shared Add-type picker item rendering. The desktop popup (web/ui.ts) and
// the touch sheet (web/touch/app.ts) both call this — mirrors
// `web/action-menu-items.ts`'s split exactly, minus enabled/danger/separator
// (every option `add_picker_options` emits is already legal; illegal ones are
// omitted, not disabled).
import { escapeHtml } from "./escape.js";

export function addPickerItemHTML(label: string, index: number, active = false): string {
  const sel = active ? " sel" : "";
  return `<button class="menu-item${sel}" data-i="${index}">${escapeHtml(label)}</button>`;
}
