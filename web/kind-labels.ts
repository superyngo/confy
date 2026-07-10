// Shared kind/notation/value-hue lookups for a `ViewRow` (previously duplicated
// across render.ts, panel.ts and touch/render.ts).
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
