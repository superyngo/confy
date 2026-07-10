// Pure SessionSnapshot → HTML for the touch UI's tree. The row anatomy is ported
// from the prototype's `rowHTML` (caret / key / `=` / typed value / count / kind
// badge / comment / grip / swipe actions), but every row is sourced from a real
// `ViewRow` (confy-core), never the prototype's fake `TREE`. Like the desktop
// `render.ts`, each row carries an attribute-safe `data-path` so the pointer
// layer can map a tap back to a node. Stateless: the orchestrator re-renders the
// whole tree from each snapshot.
import type { SessionSnapshot, ViewRow } from "../types.js";
import { escapeHtml as esc } from "../escape.js";
import { kindLabelParts, valueTypeClass } from "../kind-labels.js";

// The shared quote-safe escaper, under this module's traditional short name.
export { esc };

// --- inline SVG icons (ported verbatim from the prototype's `I` table) ---
export const IC = {
  chev: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M9 6l6 6-6 6"/></svg>',
  search: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/></svg>',
  filter: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 5h18l-7 8v6l-4 2v-8z"/></svg>',
  save: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M5 3h11l3 3v15H5z"/><path d="M8 3v6h7"/></svg>',
  more: '<svg viewBox="0 0 24 24" fill="currentColor"><circle cx="12" cy="5" r="1.9"/><circle cx="12" cy="12" r="1.9"/><circle cx="12" cy="19" r="1.9"/></svg>',
  close: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>',
  plus: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>',
  edit: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 20h4L19 9l-4-4L4 16z"/><path d="M14 6l4 4"/></svg>',
  dup: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h8"/></svg>',
  del: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/></svg>',
  check: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12l5 5L19 7"/></svg>',
  undo: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 7L4 12l5 5"/><path d="M4 12h11a5 5 0 0 1 0 10h-3"/></svg>',
  redo: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 7l5 5-5 5"/><path d="M20 12H9a5 5 0 0 0 0 10h3"/></svg>',
  sun: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="4.5"/><path d="M12 2v2M12 20v2M4 12H2M22 12h-2M5 5l1.5 1.5M17.5 17.5 19 19M19 5l-1.5 1.5M6.5 17.5 5 19"/></svg>',
  open: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M3 7h6l2 2h10v10H3z"/></svg>',
  convert: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M7 8h11l-3-3M17 16H6l3 3"/></svg>',
  expand: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M7 10l5 5 5-5M7 4l5 5 5-5"/></svg>',
  collapse: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M7 14l5-5 5 5M7 20l5-5 5 5"/></svg>',
  help: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><path d="M9.5 9a2.5 2.5 0 1 1 3.4 2.3c-.8.4-1.4 1-1.4 2"/><path d="M12 17h.01"/></svg>',
  grip: '<svg viewBox="0 0 24 24" fill="currentColor"><circle cx="9" cy="6" r="1.5"/><circle cx="15" cy="6" r="1.5"/><circle cx="9" cy="12" r="1.5"/><circle cx="15" cy="12" r="1.5"/><circle cx="9" cy="18" r="1.5"/><circle cx="15" cy="18" r="1.5"/></svg>',
};

export function isComment(r: ViewRow): boolean {
  return r.type_label === "comment";
}

export function isPositional(r: ViewRow): boolean {
  const last = r.path[r.path.length - 1];
  return !!last && "Index" in last;
}

// A branch is open iff the next visible row is one level deeper (mirrors the
// desktop `isExpanded`; the snapshot only carries visible rows).
export function isExpanded(rows: ViewRow[], idx: number): boolean {
  const next = rows[idx + 1];
  return next !== undefined && next.depth > rows[idx].depth;
}

function containerKind(r: ViewRow): "array" | "table" {
  return /array|seq/i.test(r.type_label) ? "array" : "table";
}

// The touch kind badge shares the desktop's friendly label + notation note
// (kind-labels.ts), so both surfaces read `str·"…"` / `table·scope` identically.
function kindBadgeText(r: ViewRow): string {
  const { label, note } = kindLabelParts(r);
  return note ? `${label}·${note}` : label;
}

function rowHTML(r: ViewRow, idx: number, rows: ViewRow[]): string {
  const branch = r.is_branch;
  const comment = isComment(r);
  const pad = 10 + Math.max(0, r.depth - 1) * 18;
  const expanded = branch && isExpanded(rows, idx);
  const type = branch ? containerKind(r) : r.scalar_type ?? "string";
  const dataPath = esc(JSON.stringify(r.path));
  const cls =
    "row" +
    (branch ? " branch" : "") +
    (expanded ? " open" : "") +
    (r.selected ? " selected" : "") +
    (r.is_cursor ? " cursor" : "") +
    (r.read_only ? " readonly" : "");
  let h = `<div class="${cls}" data-type="${esc(String(type))}" data-path="${dataPath}">`;
  h += `<div class="row-main" style="padding-left:${pad}px">`;
  h += `<button class="caret ${branch ? "" : "leaf"}" data-act="caret" aria-label="expand">${IC.chev}</button>`;

  if (comment) {
    // Standalone comment node (no analogue in the prototype): show the text
    // faint, full-width; swipe still reveals Edit/Delete.
    h += `<span class="comment" style="flex:1 1 auto;margin-left:0">${esc(r.value ?? "")}</span>`;
  } else {
    h += `<span class="key${isPositional(r) ? " elem" : ""}">${esc(r.key)}</span>`;
    if (branch) {
      h += `<span class="count">${r.child_count}</span>`;
      h += `<span class="kind" data-act="kind">${esc(kindBadgeText(r))}</span>`;
      // Core's `trailing_comment` already carries its marker (`#` / `//`) —
      // render it raw, exactly like the desktop render.ts.
      h += `<span class="comment">${esc(r.trailing_comment ?? "")}</span>`;
    } else {
      h += `<span class="eq">=</span>`;
      h += `<span class="val ${valueTypeClass(r)}">${esc(r.value ?? "")}</span>`;
      h += `<span class="comment">${esc(r.trailing_comment ?? "")}</span>`;
      if (!r.read_only) h += `<span class="kind" data-act="kind">${esc(kindBadgeText(r))}</span>`;
    }
  }
  // Drag grip (omitted on read-only/opaque rows — they reject moves in core).
  if (!r.read_only)
    h += `<button class="drag-handle" data-act="grip" aria-label="reorder">${IC.grip}</button>`;
  h += "</div>"; // row-main
  // Swipe-reveal Delete action, hidden behind row-main (revealed by a left-swipe;
  // omitted on read-only rows, which reject deletes in core).
  if (!r.read_only)
    h += `<button class="row-del" data-act="rowdel" tabindex="-1" aria-label="delete">${IC.del}</button>`;
  h += "</div>"; // row
  return h;
}

// Flat tree HTML (the snapshot is already the visible-row projection; collapsed
// branches simply omit their descendants, so no `.children` nesting is needed).
// The root row (empty path) is not drawn. A trailing `.reorder-line` is kept for
// the grip-drag indicator.
export function treeHTML(snap: SessionSnapshot): string {
  const rows = snap.rows;
  return (
    rows
      .map((r, idx) => (r.path.length === 0 ? "" : rowHTML(r, idx, rows)))
      .join("") + '<div class="reorder-line"></div>'
  );
}
