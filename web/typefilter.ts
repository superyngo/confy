// Shared `f` type-filter facet grid, rendered identically for the desktop and
// touch UIs. Pure string-HTML + handler-wiring over the design markup (the
// `.tf-head`/`.menu-label`/`.tf-grid`/`.tf-cell` class names the per-page CSS
// styles). The host owns the popover container (`#tfInner`) and its open/place
// logic; this module only fills the inner HTML and wires the per-cell clicks.
import type { Intent, TypeFilterRow, TypeFilterView } from "./types.js";
import { escapeHtml } from "./render.js";
import { t } from "./i18n.js";

// The check glyph inside a facet cell's `.box` (design markup; CSS reveals it
// only for `data-state="On"`).
const TF_CHECK = `<span class="box"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3"><path d="M5 12l5 5 9-11"/></svg></span>`;

function isHeader(row: TypeFilterRow): row is { Header: string } {
  return "Header" in row;
}

// Inner HTML for the type-filter popover: a `.tf-head` with the live-active hint
// and a `Clear` button (no Apply/Cancel — toggles filter live and persists
// when the popup closes), `.menu-label` group headers, and `.tf-grid` rows of
// `.tf-cell` buttons carrying `data-state`/`data-r`/`data-c`.
export function typeFilterHTML(grid: TypeFilterView): string {
  let cellRow = -1;
  let html =
    `<div class="tf-head"><span class="menu-label">${t("web.typefilter.label")}${grid.active ? ` <span class='tf-active'>${t("web.typefilter.active")}</span>` : ""}</span>` +
    `<button class="tf-clear" data-tf="clear" title="${t("web.typefilter.clear.title")}">${t("web.typefilter.clear")}</button></div>`;
  for (const row of grid.rows) {
    if (isHeader(row)) {
      html += `<div class="menu-label">${escapeHtml(row.Header)}</div>`;
      continue;
    }
    cellRow++;
    html +=
      `<div class="tf-grid">` +
      row.Cells.map(
        (c, col) =>
          `<button class="tf-cell${c.is_cursor ? " cursor" : ""}" data-state="${c.state}" data-r="${cellRow}" data-c="${col}">` +
          `${TF_CHECK}${escapeHtml(c.label)}</button>`,
      ).join("") +
      `</div>`;
  }
  return html;
}

// Wire the per-cell clicks (move the core cursor to that cell then toggle it) and
// the `×` clear button (`ExitTypeFilter`) within an already-filled container.
export function wireTypeFilter(
  container: HTMLElement,
  grid: TypeFilterView,
  { send }: { send: (i: Intent) => void },
): void {
  container.querySelectorAll<HTMLElement>("[data-r]").forEach((b) => {
    b.onclick = () => {
      const dr = Number(b.dataset.r) - grid.cursor_row;
      const dc = Number(b.dataset.c) - grid.cursor_col;
      if (dr || dc) send({ TypeFilterMove: [dr, dc] });
      send("TypeFilterToggle");
    };
  });
  // Clear resets the filter *and* closes the popup; clicking outside closes it
  // keeping the filter (wired by the host).
  const clear = container.querySelector('[data-tf="clear"]') as HTMLElement | null;
  if (clear) clear.onclick = () => send("ExitTypeFilter");
}
