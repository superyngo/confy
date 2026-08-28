// Shared `ViewRow` lookups and predicates (kind/notation/value-hue labels,
// plus row-anatomy predicates isCommentRow/isPositional/isExpanded) —
// previously duplicated across render.ts, panel.ts and touch/render.ts.
import type { ViewRow } from "./types.js";

// Short notation glyph for a scalar's `Format` (design's NOTATION_SHORT). Plain
// notations (Basic/Decimal/Plain/Block…) carry no badge suffix.
const NOTATION_SHORT: Record<string, string> = {
  BasicString: '"…"',
  Decimal: "dec",
  Literal: "'…'",
  MultilineBasic: '"""',
  MultilineLiteral: "'''",
  Multiline: '"""',
  Hex: "0x",
  Octal: "0o",
  Binary: "0b",
  Exponent: "1e",
  SingleQuoted: "'…'",
  DoubleQuoted: '"…"',
  LiteralBlock: "|",
  Folded: ">",
  Inf: "inf",
  Nan: "nan",
};

// Short notation glyph for a *container's* `Format` — the TUI's [T/S], [T/D],
// [A/M] etc. distinctions. A container's notation isn't implied by its type
// label alone (a TOML table can be a `[header]` scope or a dotted `a.b` table),
// so we surface it as a suffix just like scalars.
export const CONTAINER_NOTE: Record<string, string> = {
  Scope: "scope", // TOML standard [header] table
  Dotted: "dotted", // TOML dotted-key table (a.b.c)
  Inline: "inline", // TOML inline table / inline array
  Multiline: "multi", // TOML multiline array
  Block: "block", // YAML block map/seq
  Flow: "flow", // YAML flow map/seq
};

// The bare notation glyph for a row (no markup), or "" when the type label is
// already complete. Shared by the kind badge, the kind popup's "current" header
// and the panel's Kind button.
export function notationGlyph(r: ViewRow): string {
  if (r.is_branch) return CONTAINER_NOTE[r.format] ?? "";
  const s = NOTATION_SHORT[r.format];
  if (s) return s;
  // A plain float shares `Format::Plain` with bool/datetime/null (each a
  // single-style scalar), so it can't be keyed by format alone — resolve it by
  // scalar type. The single-style scalars stay bare (the type label is complete).
  if (r.scalar_type === "Float" && r.format === "Plain") return "dec";
  return "";
}

// Friendly short label for the kind badge (design's KIND_SHORT, keyed by the
// core `type_label`).
const KIND_SHORT: Record<string, string> = {
  table: "table",
  inline: "inline",
  array: "array",
  "array-of-tables": "AoT",
  string: "str",
  integer: "int",
  float: "float",
  bool: "bool",
  null: "null",
  offsetdatetime: "date",
  localdatetime: "date",
  localdate: "date",
  localtime: "time",
};

// The kind badge's two plain-text pieces: friendly label + notation note (note
// is "" when it would just repeat the label — an inline table's label is
// already "inline", so "inline·inline" is noise). One source for the desktop
// badge, the kind popup's "Current:" header and the touch badge.
export function kindLabelParts(r: ViewRow): { label: string; note: string } {
  const label = KIND_SHORT[r.type_label] ?? r.type_label;
  const note = CONTAINER_NOTE[r.format] === label ? "" : notationGlyph(r);
  return { label, note };
}

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

// Wrap a quoted YAML key in display-only `"…"` so the tree row matches TOML's
// existing (accidental but established) behavior of showing quote marks for a
// quoted key — informational only, never fed back into rename/edit/collision
// logic (those read `r.path`'s raw `Seg::Key`, untouched by this). TOML
// already carries its quotes inside `r.key` itself (taplo lexes a quoted
// key's raw text, quotes included); JSON keys are unconditionally quoted, so
// wrapping them would just be noise on every row. YAML is the only backend
// that decodes its key to a bare string, hiding the quoting from the tree.
// Mirrors the TUI's `display_key` (crates/confy-tui/src/tui/ui.rs).
export function isQuotedYamlKey(r: ViewRow, docFormat: string): boolean {
  return docFormat === "Yaml" && r.key_sign === "quoted";
}

export function displayKey(r: ViewRow, docFormat: string): string {
  return isQuotedYamlKey(r, docFormat) ? `"${r.key}"` : r.key;
}

// A branch is open iff the next visible row is one level deeper (the
// snapshot only carries visible rows, so there's no `.expanded` flag to read
// directly). Previously duplicated across render.ts and touch/render.ts.
export function isExpanded(rows: ViewRow[], idx: number): boolean {
  const next = rows[idx + 1];
  return next !== undefined && next.depth > rows[idx].depth;
}
