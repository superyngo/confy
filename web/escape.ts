// The one HTML escaper, shared by every UI module (desktop render/panel/
// typefilter/convert-dialog and touch render). Quote-safe: it also encodes `"`
// so the same function is safe inside double-quoted attributes — an attribute
// like `data-path="[{"Key":…}]"` would otherwise truncate at the first quote
// and `JSON.parse(dataset.path)` would throw, silently killing row clicks.
export function escapeHtml(s: string): string {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
