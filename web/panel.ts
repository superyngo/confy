// Shared node-detail/edit panel, rendered identically for the touch UI and the
// desktop UI. Pure DOM + string HTML (framework-free), mirroring the touch UI's
// `detailHTML`/`wireDetail` conventions (`data-field` for inputs, `data-act` for
// buttons, `.field-label`/`.btn`/`<dl>` structure) so it drops into either host.
//
// Differences from the old per-UI panels (the approved Section B fixes):
//   · Field order is LOCKED: Key → Value → Trailing comment → Kind → Path →
//     Children → Sign.
//   · The Kind button label is `type_label · «notation glyph»` (e.g.
//     `string · "…"`, `integer · 0x`, `table · dotted`) — a SHORT glyph, so it
//     doesn't break layout the way the old verbose "· switch notation" did.
//   · Path renders the human dotted/bracketed form (e.g. `servers[1].port`),
//     not `JSON.stringify(path)`.
//   · A structured "Sign" field exposes `key_sign`.
//   · The Delete and Duplicate buttons — rendered-but-dead in the old touch
//     `wireDetail` — are actually wired here.
//   · Every `send(...)` result is inspected for `SessionSnapshot.error`; a
//     non-empty error is surfaced via `onError` (no more silent failures).
import type { ViewRow, Intent, SessionSnapshot, Path } from "./types";

// Self-contained attribute-safe escaper (matches touch render.ts `esc`).
function esc(s: string): string {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function isComment(r: ViewRow): boolean {
  return r.type_label === "comment";
}

// Whether a scalar value edits through the host's popup editor rather than a
// one-line input. Mirrors core's `edit_target_kind` scalar rule (multiline string
// formats route External); the `\n` check is a fallback for any embedded newline.
const MULTILINE_FORMATS = ["MultilineBasic", "MultilineLiteral", "LiteralBlock", "Folded"];
function isMultilineValue(r: ViewRow): boolean {
  return MULTILINE_FORMATS.includes(r.format) || (r.value ?? "").includes("\n");
}

function isPositional(r: ViewRow): boolean {
  const last = r.path[r.path.length - 1];
  return !!last && "Index" in last;
}

// Short notation glyph for the Kind button — mirrors render.ts NOTATION_SHORT /
// CONTAINER_NOTE so the panel surfaces a scalar's string/radix/exponent style and
// a container's scope/dotted/inline/flow notation. "" when the type label already
// says it all.
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
const CONTAINER_NOTE: Record<string, string> = {
  Scope: "scope",
  Dotted: "dotted",
  Inline: "inline",
  Multiline: "multi",
  Block: "block",
  Flow: "flow",
};
function kindNotation(r: ViewRow): string {
  if (r.is_branch) return CONTAINER_NOTE[r.format] ?? "";
  const s = NOTATION_SHORT[r.format];
  if (s) return s;
  // A plain float shares `Format::Plain` with bool/datetime/null; resolve by type.
  if (r.scalar_type === "Float" && r.format === "Plain") return "dec";
  return "";
}

// Value-type hue token (design `--t-*`); branches fall back to "branch".
function valueHue(r: ViewRow): string {
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
      return "branch";
  }
}

// Human dotted/bracketed path: `{Key:n}` → `.n` (no leading dot on the first
// segment), `{Index:i}` → `[i]`. e.g. `server.host`, `servers[1].port`.
function humanPath(path: Path): string {
  let s = "";
  for (const seg of path) {
    if ("Key" in seg) s += s === "" ? seg.Key : "." + seg.Key;
    else s += `[${seg.Index}]`;
  }
  return s === "" ? "(root)" : s;
}

// Pure HTML string for the panel body. Field order is LOCKED:
//   Key → Value → Trailing comment → Kind → Path → Children → Sign
export function panelHTML(row: ViewRow): string {
  const r = row;
  const branch = r.is_branch;
  const comment = isComment(r);
  const elem = isPositional(r);
  let h = '<div class="detail">';

  // Standalone comment node: comment text + path + delete (its own layout). A
  // multi-line comment can't live in a one-line input → render it as a button that
  // opens the host popup editor (BeginEdit → external edit), same as a value.
  if (comment) {
    h += '<div class="field-label">Comment</div>';
    if (!r.read_only && isMultilineValue(r)) {
      const oneLine = (r.value ?? "").replace(/\r?\n/g, " ↵ ") || "(multi-line — tap to edit)";
      h += `<button class="c-edit v-multiline" data-act="editvalue" style="text-align:left;cursor:pointer;display:block;max-width:100%;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${esc(oneLine)}</button>`;
    } else {
      h += `<input class="c-edit" data-field="comment-node" value="${esc(r.value ?? "")}" autocomplete="off" spellcheck="false" />`;
    }
    h += `<dl><dt>Path</dt><dd>${esc(humanPath(r.path))}</dd></dl>`;
    h += '<div class="row-btns"><button class="btn danger" data-act="del">Delete</button></div></div>';
    return h;
  }

  // Key (array-element index is positional, not renamable).
  h += '<div class="field-label">Key</div>';
  if (elem) {
    h += `<input class="v-edit" value="${esc(r.key)}" disabled />`;
    h += '<div class="hint-line">Array-element index is positional — drag the grip to reorder.</div>';
  } else if (!r.read_only) {
    h += `<input class="k-edit" data-field="name" value="${esc(r.key)}" autocomplete="off" spellcheck="false" />`;
  } else {
    h += `<input class="v-edit" value="${esc(r.key)}" disabled />`;
  }

  // Value (scalars only). A multi-line value can't live in a one-line <input>;
  // render it as a clickable button that opens the host's popup editor (click →
  // BeginEdit → external_edit), mirroring the tree's multiline routing.
  if (!branch) {
    h += `<div class="field-label">Value (${esc(r.type_label)})</div>`;
    const v = r.value ?? "";
    if (!r.read_only && isMultilineValue(r)) {
      const oneLine = v.replace(/\r?\n/g, " ↵ ") || "(multi-line — tap to edit)";
      h += `<button class="v-edit v-multiline" data-act="editvalue" style="text-align:left;cursor:pointer;display:block;max-width:100%;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${esc(oneLine)}</button>`;
    } else {
      h += `<input class="v-edit" data-field="value" value="${esc(v)}"${r.read_only ? " disabled" : ""} />`;
    }
  }

  // Trailing comment.
  if (!r.read_only) {
    h += '<div class="field-label">Trailing comment</div>';
    h += `<input class="c-edit" data-field="trailing" value="${esc(r.trailing_comment ?? "")}" placeholder="add a comment…" autocomplete="off" spellcheck="false" />`;
  }

  // Kind switch — label is `type_label · «notation glyph»` (the glyph is dropped
  // when it would merely repeat the label, e.g. an inline table).
  if (!r.read_only) {
    const hue = branch ? "branch" : valueHue(r);
    const note = kindNotation(r);
    const noteStr = note && note !== r.type_label ? ` · ${esc(note)}` : "";
    h += '<div class="field-label">Kind</div>';
    h += `<button class="btn kindbtn" data-act="kindswitch"><span class="dotc" style="background:var(--t-${hue})"></span>${esc(r.type_label)}${noteStr}</button>`;
  }

  // Meta: Path (human form) / Children (branches) / Sign.
  h += `<dl><dt>Path</dt><dd>${esc(humanPath(r.path))}</dd>`;
  if (branch) h += `<dt>Children</dt><dd>${r.child_count}</dd>`;
  h += `<dt>Sign</dt><dd>${esc(r.key_sign ?? "none")}</dd>`;
  h += "</dl>";

  // Actions. Copy/Cut arm the clipboard (paste via the host's paste affordance);
  // Delete removes the node.
  if (!r.read_only) {
    h +=
      '<div class="row-btns">' +
      '<button class="btn" data-act="copy">Copy</button>' +
      '<button class="btn" data-act="cut">Cut</button>' +
      '<button class="btn danger" data-act="del">Delete</button></div>';
  }
  h += "</div>";
  return h;
}

// Wire the rendered panel's controls to intents.
//  - send(intent): dispatches and returns the new snapshot (we read its error).
//  - openKind(row): host opens its kind-switch surface (sheet / popover).
//  - onError(msg): host shows a message (toast/status) when a send errors.
//  - afterMutation(msg): host confirms + dismisses the panel after a successful
//    Delete / Copy / Cut (e.g. toast the message and close the detail surface).
export function wirePanel(
  container: HTMLElement,
  row: ViewRow,
  send: (intent: Intent) => SessionSnapshot,
  openKind: (row: ViewRow) => void,
  onError: (msg: string) => void,
  afterMutation?: (msg: string) => void,
): void {
  const path = row.path;

  // Dispatch and surface any error the snapshot reports (no silent failures).
  const fire = (intent: Intent): void => {
    const snap = send(intent);
    if (snap && snap.error) onError(snap.error);
  };

  const commit = (el: HTMLInputElement, fn: () => void) => {
    el.addEventListener("change", fn);
    el.addEventListener("keydown", (e) => {
      if ((e as KeyboardEvent).key === "Enter") el.blur();
    });
  };

  const ke = container.querySelector<HTMLInputElement>('[data-field="name"]');
  const ve = container.querySelector<HTMLInputElement>('[data-field="value"]');
  const te = container.querySelector<HTMLInputElement>('[data-field="trailing"]');
  const cn = container.querySelector<HTMLInputElement>('[data-field="comment-node"]');
  const kb = container.querySelector<HTMLElement>("[data-act=kindswitch]");
  const del = container.querySelector<HTMLElement>("[data-act=del]");
  const cp = container.querySelector<HTMLElement>("[data-act=copy]");
  const ct = container.querySelector<HTMLElement>("[data-act=cut]");
  const ev = container.querySelector<HTMLElement>("[data-act=editvalue]");

  if (ke)
    commit(ke, () => {
      fire({ SetCursor: path });
      fire({ CommitEdit: { value: null, name: ke.value } });
    });
  if (ve) {
    commit(ve, () => {
      fire({ SetCursor: path });
      fire({ CommitEdit: { value: ve.value, name: null } });
    });
    // Mouse-wheel over the value field adjusts it (matches the tree gesture): a
    // bool toggles (trailing comment preserved), a number nudges ±1 (up = +1).
    const st = row.scalar_type;
    if (st === "Bool" || st === "Integer" || st === "Float") {
      ve.addEventListener(
        "wheel",
        (e) => {
          e.preventDefault();
          // Nudge handles all three (bool toggles, int/float ±1) and — unlike
          // CommitEdit — keeps the host in Detail mode, so the panel stays open.
          fire({ SetCursor: path });
          fire({ Nudge: e.deltaY < 0 ? 1 : -1 });
        },
        { passive: false },
      );
    }
  }
  // Multi-line value button → open the host's popup editor via core's edit flow.
  if (ev)
    ev.addEventListener("click", () => {
      fire({ SetCursor: path });
      fire("BeginEdit");
    });
  if (te)
    commit(te, () => {
      fire({ SetTrailing: { path, comment: te.value || null } });
    });
  if (cn)
    commit(cn, () => {
      fire({ ApplyEditComment: { path, text: cn.value } });
    });
  if (kb) kb.addEventListener("click", () => openKind(row));

  // Delete / Copy / Cut: select this row, run the action, then — on success —
  // confirm + dismiss the panel via `afterMutation` (errors still go to onError).
  const act = (intent: Intent, okMsg: string) => {
    fire({ SetCursor: path });
    fire({ SetSelection: { paths: [path] } });
    const snap = send(intent);
    if (snap && snap.error) onError(snap.error);
    else afterMutation?.(okMsg);
  };
  if (del) del.addEventListener("click", () => act("DeleteSelected", "Deleted"));
  // Copy / Cut arm the clipboard; the host's paste affordance (FAB / paste-mode
  // click) commits the paste at the new cursor.
  if (cp) cp.addEventListener("click", () => act("CopySelected", "Copied — paste to place it"));
  if (ct) ct.addEventListener("click", () => act("CutSelected", "Cut — paste to move it"));
}
