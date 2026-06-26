// Shared node-detail/edit panel, rendered identically for the touch UI and the
// desktop UI. Pure DOM + string HTML (framework-free), mirroring the touch UI's
// `detailHTML`/`wireDetail` conventions (`data-field` for inputs, `data-act` for
// buttons, `.field-label`/`.btn`/`<dl>` structure) so it drops into either host.
//
// Differences from the old per-UI panels (the approved Section B fixes):
//   · Field order is LOCKED: Key → Value → Trailing comment → Kind → Path →
//     Children → Sign.
//   · The Kind button label is ONLY `type_label` — the old "· switch notation"
//     suffix is dropped (it broke layout).
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

function isPositional(r: ViewRow): boolean {
  const last = r.path[r.path.length - 1];
  return !!last && "Index" in last;
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

  // Standalone comment node: comment text + path + delete (its own layout).
  if (comment) {
    h += '<div class="field-label">Comment</div>';
    h += `<input class="c-edit" data-field="comment-node" value="${esc(r.value ?? "")}" autocomplete="off" spellcheck="false" />`;
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

  // Value (scalars only).
  if (!branch) {
    h += `<div class="field-label">Value (${esc(r.type_label)})</div>`;
    h += `<input class="v-edit" data-field="value" value="${esc(r.value ?? "")}"${r.read_only ? " disabled" : ""} />`;
  }

  // Trailing comment.
  if (!r.read_only) {
    h += '<div class="field-label">Trailing comment</div>';
    h += `<input class="c-edit" data-field="trailing" value="${esc(r.trailing_comment ?? "")}" placeholder="add a comment…" autocomplete="off" spellcheck="false" />`;
  }

  // Kind switch — label is ONLY `type_label` (no "· switch notation" suffix).
  if (!r.read_only) {
    const hue = branch ? "branch" : valueHue(r);
    h += '<div class="field-label">Kind</div>';
    h += `<button class="btn kindbtn" data-act="kindswitch"><span class="dotc" style="background:var(--t-${hue})"></span>${esc(r.type_label)}</button>`;
  }

  // Meta: Path (human form) / Children (branches) / Sign.
  h += `<dl><dt>Path</dt><dd>${esc(humanPath(r.path))}</dd>`;
  if (branch) h += `<dt>Children</dt><dd>${r.child_count}</dd>`;
  h += `<dt>Sign</dt><dd>${esc(r.key_sign ?? "none")}</dd>`;
  h += "</dl>";

  // Actions.
  if (!r.read_only) {
    h +=
      '<div class="row-btns">' +
      '<button class="btn" data-act="dup">Duplicate</button>' +
      '<button class="btn danger" data-act="del">Delete</button></div>';
  }
  h += "</div>";
  return h;
}

// Wire the rendered panel's controls to intents.
//  - send(intent): dispatches and returns the new snapshot (we read its error).
//  - openKind(row): host opens its kind-switch surface (sheet / popover).
//  - onError(msg): host shows a message (toast/status) when a send errors.
export function wirePanel(
  container: HTMLElement,
  row: ViewRow,
  send: (intent: Intent) => SessionSnapshot,
  openKind: (row: ViewRow) => void,
  onError: (msg: string) => void,
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
  const dup = container.querySelector<HTMLElement>("[data-act=dup]");

  if (ke)
    commit(ke, () => {
      fire({ SetCursor: path });
      fire({ CommitEdit: { value: null, name: ke.value } });
    });
  if (ve)
    commit(ve, () => {
      fire({ SetCursor: path });
      fire({ CommitEdit: { value: ve.value, name: null } });
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

  // Delete: select this row, then DeleteSelected.
  if (del)
    del.addEventListener("click", () => {
      fire({ SetCursor: path });
      fire({ SetSelection: { paths: [path] } });
      fire("DeleteSelected");
    });

  // Duplicate: select this row, copy, then paste (no dedicated duplicate Intent).
  if (dup)
    dup.addEventListener("click", () => {
      fire({ SetCursor: path });
      fire({ SetSelection: { paths: [path] } });
      fire("CopySelected");
      fire("Paste");
    });
}
