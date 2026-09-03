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

// The **single** value-type hue table (design `--t-*` tokens without the
// prefix), keyed by core's `type_label`. It covers branches and comments too,
// which `valueHue` alone cannot: a branch/comment row has no `scalar_type`.
// The glyph itself is core-owned (`ViewRow.kind_glyph`) and the hue is
// web-owned — see ADR 0011. Previously this table was forked in
// `breadcrumb.ts` (as `GLYPHS`, carrying the glyph as well) while rows went
// through `valueTypeClass`, so the two surfaces could disagree.
export function hueFor(typeLabel: string): string {
  switch (typeLabel) {
    case "table":
    case "inline":
    case "array":
    case "array-of-tables":
      return "branch";
    case "string":
      return "string";
    case "integer":
    case "float":
      return "number";
    case "bool":
      return "bool";
    case "null":
      return "null";
    case "offsetdatetime":
    case "localdatetime":
    case "localdate":
    case "localtime":
      return "date";
    // A comment carries no type; it borrows the dimmest token, matching the
    // TUI's DarkGray comment tag.
    case "comment":
      return "null";
    default:
      return "";
  }
}

// Value-type hue token for a scalar row; "" for branches/comments/unknown.
// Kept as the scalar-only entry point (callers that must *not* colour a
// branch, e.g. the `.val` cell) — a thin wrapper over `hueFor`.
export function valueHue(r: ViewRow): string {
  return r.scalar_type === undefined || r.scalar_type === null
    ? ""
    : hueFor(r.type_label);
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
