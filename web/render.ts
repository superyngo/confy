// Pure `SessionSnapshot → DOM` for the tree (WEBUI.md / PORTING §8.3). Renders
// the full web-native row anatomy from `design_index_model.html`: drag grip,
// rotating caret, key / `=` / value (value-type colored) or `—` element / item
// count, a per-row **kind badge** (type + notation + chevron), comment / trailing
// decoration, and hover action buttons (`＋` add on branches, `⋮` more). Each row
// carries `data-path` (JSON-encoded `Path`) + `data-index` so the pointer layer
// (web/ui.ts, later web/select.ts / web/dnd.ts) maps a click back to a node
// without re-deriving tree structure. No editing logic lives here — it renders
// the snapshot and nothing else; the affordances it draws are wired in later
// phases (kind popover, context menu, drag-reparent).
import type { EditView, SessionSnapshot, ViewRow } from "./types.js";

// A positional node (array element / AoT entry) is addressed by `Seg::Index`; it
// is keyless and renders as the faint "—" placeholder (core hands us a display
// key like "[0]", which the design replaces with the dash).
function isPositional(r: ViewRow): boolean {
  const last = r.path[r.path.length - 1];
  return last !== undefined && "Index" in last;
}

export function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

// Escape a string for use inside a double-quoted HTML attribute. `escapeHtml`
// alone leaves `"` intact, which truncates an attribute like
// `data-path="[{"Key":…}]"` at the first quote — so `dataset.path` came back as
// `[{` and `JSON.parse` threw, silently killing every row click. We additionally
// encode `"` so the JSON survives the round-trip through the DOM.
function escapeAttr(s: string): string {
  return escapeHtml(s).replace(/"/g, "&quot;");
}

// Value-type color class (design tokens `--t-*`). Numbers share one hue.
export function valueTypeClass(r: ViewRow): string {
  switch (r.scalar_type) {
    case "String":
      return "t-string";
    case "Integer":
    case "Float":
      return "t-number";
    case "Boolean":
      return "t-bool";
    case "Null":
      return "t-null";
    case "Datetime":
      return "t-date";
    default:
      return "";
  }
}

// --- inline SVGs (mirrors the design's IC table) ---
const IC_CARET =
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M9 6l6 6-6 6"/></svg>`;
const IC_CHEV =
  `<svg class="chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M6 9l6 6 6-6"/></svg>`;
const IC_GRIP =
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="9" cy="6" r="1.4"/><circle cx="15" cy="6" r="1.4"/><circle cx="9" cy="12" r="1.4"/><circle cx="15" cy="12" r="1.4"/><circle cx="9" cy="18" r="1.4"/><circle cx="15" cy="18" r="1.4"/></svg>`;
const IC_ADD =
  `<svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14"/></svg>`;
const IC_MORE =
  `<svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="5" r="1.6"/><circle cx="12" cy="12" r="1.6"/><circle cx="12" cy="19" r="1.6"/></svg>`;

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
const CONTAINER_NOTE: Record<string, string> = {
  Scope: "scope", // TOML standard [header] table
  Dotted: "dotted", // TOML dotted-key table (a.b.c)
  Inline: "inline", // TOML inline table / inline array
  Multiline: "multi", // TOML multiline array
  Block: "block", // YAML block map/seq
  Flow: "flow", // YAML flow map/seq
};

// The bare notation glyph for a row (no markup), or "" when the type label is
// already complete. Shared by the kind badge and the popup's "current" header.
function notationGlyph(r: ViewRow): string {
  if (r.is_branch) return CONTAINER_NOTE[r.format] ?? "";
  const s = NOTATION_SHORT[r.format];
  if (s) return s;
  // A plain float shares `Format::Plain` with bool/datetime/null (each a
  // single-style scalar), so it can't be keyed by format alone — resolve it by
  // scalar type. The single-style scalars stay bare (the type label is complete).
  if (r.scalar_type === "Float" && r.format === "Plain") return "dec";
  return "";
}

function notationSuffix(r: ViewRow): string {
  const s = notationGlyph(r);
  return s ? `<span class="kind-note">·${escapeHtml(s)}</span>` : "";
}

// Plain-text "label · notation" for the kind popup's disabled "Current:" header
// (design's `目前：…` row). Suppresses a notation that just repeats the label.
export function currentKindLabel(r: ViewRow): string {
  const label = KIND_SHORT[r.type_label] ?? r.type_label;
  const note = CONTAINER_NOTE[r.format] === label ? "" : notationGlyph(r);
  return note ? `${label} · ${note}` : label;
}

// The disclosure caret as an inline SVG (rotated 90° via `.row.open > .caret`).
function isExpanded(rows: ViewRow[], idx: number): boolean {
  const r = rows[idx];
  const next = rows[idx + 1];
  return next !== undefined && next.depth > r.depth;
}

// When the cursor row is in `Value` edit mode, the value cell becomes a live
// `<input>` (ui.ts focuses it and commits on Enter/blur via `CommitEdit`).
function renderValue(r: ViewRow, edit: EditView | null): string {
  if (edit && r.is_cursor && edit.field === "Value") {
    return `<input class="cell-input mono" data-editing="value" value="${escapeAttr(edit.buffer)}" />`;
  }
  // Collapse newlines so a multiline value stays on one row (it would otherwise
  // break the flexbox and push the kind badge off, making it unclickable). The
  // `.val` cell also clamps with ellipsis (style.css).
  return escapeHtml((r.value ?? "").replace(/\r?\n/g, " ↵ "));
}

// A comment node is identified by its kind label (core sets `type_label` to
// "comment"; it also fills both `key` and `value` with the comment text, so the
// old key/value heuristic is unreliable — use the label).
function isCommentRow(r: ViewRow): boolean {
  return r.type_label === "comment";
}

// The per-row kind badge: friendly kind label + notation suffix + chevron.
function renderKindBadge(r: ViewRow): string {
  const label = KIND_SHORT[r.type_label] ?? r.type_label;
  // Suppress a suffix that just repeats the label (an inline table's label is
  // already "inline", so "inline·inline" is noise).
  const suffix = CONTAINER_NOTE[r.format] === label ? "" : notationSuffix(r);
  return `<button class="kind" data-kind="1">${escapeHtml(label)}${suffix} ${IC_CHEV}</button>`;
}

function renderRow(
  r: ViewRow,
  idx: number,
  rows: ViewRow[],
  edit: EditView | null,
  clip: "" | " clip-copy" | " clip-cut",
): string {
  const pathAttr = escapeAttr(JSON.stringify(r.path));
  const comment = isCommentRow(r);
  const expanded = r.is_branch && isExpanded(rows, idx);
  const cls =
    `row${r.is_branch ? " branch" : ""}${expanded ? " open" : ""}` +
    `${r.is_cursor ? " cursor" : ""}${r.selected ? " selected" : ""}` +
    `${r.read_only ? " readonly" : ""}${comment ? " comment-row" : ""}${clip}`;
  let s = `<div class="${cls}" data-path="${pathAttr}" data-index="${idx}">`;
  // Indentation: a single spacer whose width scales with depth (the design's
  // `indent.style.width = depth*22`). The synthetic root (depth 0) is not drawn,
  // so real top-level nodes (depth 1) sit flush-left and each deeper level adds
  // one `--indent` step. (A zero-width span per level — the previous approach —
  // left every node flush-left with no level hint.)
  const level = Math.max(0, r.depth - 1);
  s += `<span class="indent" style="width:calc(var(--indent) * ${level})"></span>`;
  // Drag grip.
  s += `<span class="drag-handle" data-grip="1" draggable="true">${IC_GRIP}</span>`;
  // Disclosure caret (rotates on expand); leaves get an aligned hidden caret.
  s += `<button class="caret${r.is_branch ? "" : " leaf"}" data-caret="1">${IC_CARET}</button>`;

  if (comment) {
    s += `<span class="comment mono" data-edit="comment">${escapeHtml(r.value ?? "")}</span>`;
  } else {
    // Key. Positional array/AoT elements are keyless; core gives them the index
    // label "[0]"/"[1]" which we keep (informative) but render faintly. A keyed
    // node in `Name` edit mode becomes a live rename `<input>`.
    if (isPositional(r)) {
      s += `<span class="key elem">${escapeHtml(r.key)}</span>`;
    } else if (edit && r.is_cursor && edit.field === "Name") {
      s += `<input class="cell-input key-input mono" data-editing="name" value="${escapeAttr(edit.buffer)}" />`;
    } else if (r.key) {
      s += `<span class="key" data-edit="key">${escapeHtml(r.key)}</span>`;
    }
    if (r.is_branch) {
      s += `<span class="count">${r.child_count} ${r.child_count === 1 ? "item" : "items"}</span>`;
    } else {
      const vcls = valueTypeClass(r);
      s += `<span class="eq">=</span>`;
      s += `<span class="val ${vcls} mono" data-edit="val">${renderValue(r, edit)}</span>`;
    }
    // Kind badge (type + notation + chevron).
    if (!r.read_only) s += renderKindBadge(r);
    // Trailing same-line comment.
    if (r.trailing_comment) {
      s += `<span class="comment mono" data-edit="note">${escapeHtml(r.trailing_comment)}</span>`;
    }
  }

  // Hover action buttons (＋ add on branches, ⋮ more) — wired in later phases.
  s += `<span class="row-actions">`;
  if (r.is_branch) s += `<button title="Add child" data-act="add">${IC_ADD}</button>`;
  s += `<button title="More actions" data-act="menu">${IC_MORE}</button>`;
  s += `</span>`;

  s += `</div>`;
  return s;
}

/** Render the whole tree into `treeEl` and scroll the cursor row into view. */
export function renderTree(
  treeEl: HTMLElement,
  snap: SessionSnapshot,
  edit: EditView | null,
): void {
  const rows = snap.rows;
  // Clipboard source rows get a distinct class (copy vs cut) so they read
  // differently from the selection box.
  const clipKeys = new Set(snap.clipboard_paths.map((p) => JSON.stringify(p)));
  const clipCls: " clip-copy" | " clip-cut" = snap.clipboard_cut
    ? " clip-cut"
    : " clip-copy";
  // The synthetic root (empty path) is not rendered; `idx` stays the real
  // `snap.rows` index so a click maps back to the right node.
  treeEl.innerHTML = rows
    .map((r, idx) =>
      r.path.length === 0
        ? ""
        : renderRow(
            r,
            idx,
            rows,
            edit,
            clipKeys.has(JSON.stringify(r.path)) ? clipCls : "",
          ),
    )
    .join("");
  const cur = treeEl.querySelector(".row.cursor") as HTMLElement | null;
  cur?.scrollIntoView({ block: "nearest" });
}
