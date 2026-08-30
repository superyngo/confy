// Shared `ViewRow` lookups and predicates (value-hue labels, plus row-anatomy
// predicates isCommentRow/isPositional/isExpanded) — previously duplicated
// across render.ts, panel.ts and touch/render.ts.
//
// The kind badge's friendly label + notation-suffix note (previously
// NOTATION_SHORT/CONTAINER_NOTE/KIND_SHORT/notationGlyph/kindLabelParts here)
// now live in core as `ViewRow.badge_label`/`.badge_note`
// (session/status_fmt.rs's `badge_label_note`), so every host renders the
// identical badge without re-deriving type/format heuristics.
import type { ViewRow } from "./types.js";

// Value-type hue token (design `--t-*` without the prefix); "" when unknown.
export function valueHue(r: ViewRow): string {
  switch (r.scalar_type) {
    case "String":
      return "string";
    case "Integer":
    case "Float":
      return "number";
    case "Bool":
      return "bool";
    case "Null":
      return "null";
    case "OffsetDatetime":
    case "LocalDatetime":
    case "LocalDate":
    case "LocalTime":
      return "date";
    default:
      return "";
  }
}

// Value-type color class (design tokens `--t-*`). Numbers share one hue.
export function valueTypeClass(r: ViewRow): string {
  const hue = valueHue(r);
  return hue ? `t-${hue}` : "";
}

// A comment node is identified by its kind label (core sets `type_label` to
// "comment"; it also fills both `key` and `value` with the comment text, so a
// key/value heuristic is unreliable — use the label). Previously duplicated
// across render.ts, touch/render.ts and panel.ts.
export function isCommentRow(r: ViewRow): boolean {
  return r.type_label === "comment";
}

// A positional node (array element / AoT entry) is addressed by `Seg::Index`;
// it is keyless — core hands it a display key like "[0]", which hosts render
// faintly instead of as a real key. Previously duplicated across render.ts,
// touch/render.ts and panel.ts.
export function isPositional(r: ViewRow): boolean {
  const last = r.path[r.path.length - 1];
  return last !== undefined && "Index" in last;
}

// A branch is open iff the next visible row is one level deeper (the
// snapshot only carries visible rows, so there's no `.expanded` flag to read
// directly). Previously duplicated across render.ts and touch/render.ts.
export function isExpanded(rows: ViewRow[], idx: number): boolean {
  const next = rows[idx + 1];
  return next !== undefined && next.depth > rows[idx].depth;
}
