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
//   · Every `send(...)` result is inspected for `SessionSnapshot.notice` (Error severity);
//     a non-empty error is surfaced via `onError` (no more silent failures).
import type { ViewRow, Intent, SessionSnapshot, Path, EditHint } from "./types";
import { escapeHtml as esc } from "./escape.js";
import { isCommentRow, isPositional, notationGlyph, valueHue } from "./kind-labels.js";
import { t, tArgs } from "./i18n.js";

// Whether a scalar value edits through the host's popup editor rather than a
// one-line input. Mirrors core's `edit_target_kind` scalar rule (multiline string
// formats route External); the `\n` check is a fallback for any embedded newline.
const MULTILINE_FORMATS = ["MultilineBasic", "MultilineLiteral", "LiteralBlock", "Folded"];
function isMultilineValue(r: ViewRow): boolean {
  return MULTILINE_FORMATS.includes(r.format) || (r.value ?? "").includes("\n");
}

// Kind-notation glyph + value-hue lookups are shared (`kind-labels.ts`).

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
//
// `parentInline`: true when `row`'s immediate parent is a single-line
// container (TOML inline table, JSON single-line object/array, YAML flow
// map/seq — core's `Format::Inline`). Such containers can't hold comments, so
// the Trailing-comment input is disabled instead of failing on commit. The
// host computes this from its own `SessionSnapshot.rows` (no parent lookup
// lives in `ViewRow` itself).
export function panelHTML(
  row: ViewRow,
  parentInline = false,
  editHint?: EditHint,
  schemaEnum?: { options: string[]; cursor: number },
): string {
  const r = row;
  const branch = r.is_branch;
  const comment = isCommentRow(r);
  const elem = isPositional(r);
  let h = '<div class="detail">';

  // Standalone comment node: comment text + path + delete (its own layout). A
  // multi-line comment can't live in a one-line input → render it as a button that
  // opens the host popup editor (BeginEdit → external edit), same as a value.
  if (comment) {
    h += `<div class="field-label">${t("web.panel.field.comment")}</div>`;
    if (!r.read_only && isMultilineValue(r)) {
      const oneLine = (r.value ?? "").replace(/\r?\n/g, " ↵ ") || t("web.panel.multilinePlaceholder");
      h += `<button class="c-edit v-multiline" data-act="editvalue" style="text-align:left;cursor:pointer;display:block;max-width:100%;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${esc(oneLine)}</button>`;
    } else {
      h += `<input class="c-edit" data-field="comment-node" value="${esc(r.value ?? "")}" autocomplete="off" spellcheck="false" />`;
    }
    h += `<dl><dt>${t("web.panel.field.path")}</dt><dd>${esc(humanPath(r.path))}</dd></dl>`;
    h +=
      '<div class="row-btns">' +
      `<button class="btn" data-act="editexternal">${t("web.panel.editExternal")}</button>` +
      `<button class="btn" data-act="copy">${t("web.common.copy")}</button>` +
      `<button class="btn" data-act="cut">${t("web.common.cut")}</button>` +
      `<button class="btn danger" data-act="del">${t("web.common.delete")}</button></div></div>`;
    return h;
  }

  // Key (array-element index is positional, not renamable).
  h += `<div class="field-label">${t("web.panel.field.key")}</div>`;
  if (elem) {
    h += `<input class="v-edit" value="${esc(r.key)}" disabled />`;
    h += `<div class="hint-line">${t("web.panel.hint.positionalKey")}</div>`;
  } else if (!r.read_only) {
    h += `<input class="k-edit" data-field="name" value="${esc(r.key)}" autocomplete="off" spellcheck="false" />`;
  } else {
    h += `<input class="v-edit" value="${esc(r.key)}" disabled />`;
  }

  // Value (scalars only). A schema-enum-constrained value swaps in the
  // picker select once BeginEdit resolves `Mode::SchemaEnum` (mirrors the
  // tree's `renderValue`'s `schemaEnum` branch exactly, same option/cursor
  // shape). Before that — and for a multi-line value — it's a clickable
  // trigger that dispatches BeginEdit and lets core pick the destination
  // (external popup, or the schema picker for an enum-constrained field).
  if (!branch) {
    h += `<div class="field-label">${esc(tArgs("web.panel.field.value", [r.type_label]))}</div>`;
    const v = r.value ?? "";
    if (!r.read_only && schemaEnum) {
      const opts = schemaEnum.options
        .map((label, i) => `<option value="${i}"${i === schemaEnum.cursor ? " selected" : ""}>${esc(label)}</option>`)
        .join("");
      h += `<select class="v-edit" data-field="value-enum">${opts}</select>`;
    } else if (!r.read_only && (isMultilineValue(r) || (editHint && editHint !== "None" && "Enum" in editHint))) {
      const oneLine = v.replace(/\r?\n/g, " ↵ ") || t("web.panel.multilinePlaceholder");
      h += `<button class="v-edit v-multiline" data-act="editvalue" style="text-align:left;cursor:pointer;display:block;max-width:100%;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${esc(oneLine)}</button>`;
    } else {
      h += `<input class="v-edit" data-field="value" value="${esc(v)}"${r.read_only ? " disabled" : ""} />`;
    }
  }

  // Trailing comment. Disabled on a member of an inline/flow container — core
  // rejects the mutation (comments can't live inside `{…}`/`[…]`).
  if (!r.read_only) {
    h += `<div class="field-label">${t("web.panel.field.trailing")}</div>`;
    const disabledAttr = parentInline
      ? ` disabled title="${t("web.panel.trailing.disabledTitle")}"`
      : "";
    // The placeholder states the reason when disabled — touch has no hover
    // tooltip, so the title attribute alone wouldn't surface it.
    const ph = parentInline ? t("web.panel.trailing.disabledPlaceholder") : t("web.panel.trailing.placeholder");
    h += `<input class="c-edit" data-field="trailing" value="${esc(r.trailing_comment ?? "")}" placeholder="${ph}" autocomplete="off" spellcheck="false"${disabledAttr} />`;
  }

  // Kind switch — label is `type_label · «notation glyph»` (the glyph is dropped
  // when it would merely repeat the label, e.g. an inline table).
  if (!r.read_only) {
    const hue = branch ? "branch" : valueHue(r) || "branch";
    const note = notationGlyph(r);
    const noteStr = note && note !== r.type_label ? ` · ${esc(note)}` : "";
    h += `<div class="field-label">${t("web.panel.field.kind")}</div>`;
    h += `<button class="btn kindbtn" data-act="kindswitch"><span class="dotc" style="background:var(--t-${hue})"></span>${esc(r.type_label)}${noteStr}</button>`;
  }

  // Meta: Path (human form) / Children (branches) / Sign.
  h += `<dl><dt>${t("web.panel.field.path")}</dt><dd>${esc(humanPath(r.path))}</dd>`;
  if (branch) h += `<dt>${t("web.panel.field.children")}</dt><dd>${r.child_count}</dd>`;
  h += `<dt>${t("web.panel.field.sign")}</dt><dd>${esc(r.key_sign ?? t("web.panel.sign.none"))}</dd>`;
  h += "</dl>";

  // Actions. Copy/Cut arm the clipboard (paste via the host's paste affordance);
  // Delete removes the node.
  if (!r.read_only) {
    h +=
      '<div class="row-btns">' +
      `<button class="btn" data-act="editexternal">${t("web.panel.editExternal")}</button>` +
      `<button class="btn" data-act="copy">${t("web.common.copy")}</button>` +
      `<button class="btn" data-act="cut">${t("web.common.cut")}</button>` +
      `<button class="btn danger" data-act="del">${t("web.common.delete")}</button></div>`;
  }
  h += "</div>";
  return h;
}

// Wire the rendered panel's controls to intents.
//  - send(intent): dispatches and returns the new snapshot (we read its notice).
//  - openKind(row): host opens its kind-switch surface (sheet / popover).
//  - onError(msg): host shows a message (toast/status) when a send errors.
//  - afterMutation(msg): host confirms + dismisses the panel after a successful
//    Delete / Copy / Cut (e.g. toast the message and close the detail surface).
//  - batch(fn): optional host batcher — dispatches every send inside `fn` with a
//    single re-render at the end (perf: multi-intent handlers render once).
export function wirePanel(
  container: HTMLElement,
  row: ViewRow,
  send: (intent: Intent) => SessionSnapshot,
  openKind: (row: ViewRow) => void,
  onError: (msg: string) => void,
  afterMutation?: (msg: string) => void,
  batch?: (fn: () => void) => void,
  schemaEnum?: { options: string[]; cursor: number },
): void {
  const path = row.path;
  const run = batch ?? ((fn: () => void) => fn());

  // Dispatch and surface any error the snapshot reports (no silent failures).
  const fire = (intent: Intent): void => {
    const snap = send(intent);
    if (snap?.notice?.severity === "error") onError(snap.notice.text);
  };

  // Commit on change (blur / Enter→blur); Esc cancels — restoring the value to
  // what it was when the input gained focus means the browser's own "change"
  // comparison sees no difference, so blur() doesn't re-fire a commit.
  const commit = (el: HTMLInputElement, fn: () => void) => {
    const orig = el.value;
    el.addEventListener("change", fn);
    el.addEventListener("keydown", (e) => {
      const k = (e as KeyboardEvent).key;
      if (k === "Enter") {
        // Commit-then-blur can synchronously open a confirm prompt (type
        // change / collision) whose y/n the desktop `onKey` reads straight
        // off Enter — without stopping propagation here, this same keydown
        // bubbles past the now-blurred input (no longer an INPUT, so the
        // host's "don't hijack text entry" guard no longer applies) and
        // auto-answers "y" before the prompt is ever visible.
        e.stopPropagation();
        el.blur();
      } else if (k === "Escape") {
        e.stopPropagation(); // cancel this edit only — don't peel host surfaces
        el.value = orig;
        el.blur();
      }
    });
  };

  const ke = container.querySelector<HTMLInputElement>('[data-field="name"]');
  const ve = container.querySelector<HTMLInputElement>('[data-field="value"]');
  const ven = container.querySelector<HTMLSelectElement>('[data-field="value-enum"]');
  const te = container.querySelector<HTMLInputElement>('[data-field="trailing"]');
  const cn = container.querySelector<HTMLInputElement>('[data-field="comment-node"]');
  const kb = container.querySelector<HTMLElement>("[data-act=kindswitch]");
  const del = container.querySelector<HTMLElement>("[data-act=del]");
  const cp = container.querySelector<HTMLElement>("[data-act=copy]");
  const ct = container.querySelector<HTMLElement>("[data-act=cut]");
  const ev = container.querySelector<HTMLElement>("[data-act=editvalue]");
  const ext = container.querySelector<HTMLElement>("[data-act=editexternal]");

  // NOTE: read the field value BEFORE the first `fire` — a `SetCursor` dispatch
  // rebuilds the host panel's innerHTML, detaching this input, so reading
  // `el.value` afterward is unreliable (the edit silently no-ops).
  if (ke)
    commit(ke, () => {
      const name = ke.value;
      run(() => {
        fire({ SetCursor: path });
        fire({ CommitEdit: { value: null, name } });
      });
    });
  if (ve) {
    commit(ve, () => {
      const value = ve.value;
      run(() => {
        fire({ SetCursor: path });
        fire({ CommitEdit: { value, name: null } });
      });
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
          run(() => {
            fire({ SetCursor: path });
            fire({ Nudge: e.deltaY < 0 ? 1 : -1 });
          });
        },
        { passive: false },
      );
    }
  }
  // Schema-enum picker select (active once BeginEdit resolves
  // Mode::SchemaEnum) → SchemaEnumMove/SchemaEnumCommit, mirroring the
  // tree's focusSchemaEnumSelect exactly (same idx-current delta
  // convention). Escape cancels the picker.
  if (ven) {
    ven.addEventListener("change", () => {
      const idx = Number(ven.value);
      const current = schemaEnum?.cursor ?? 0;
      run(() => {
        fire({ SchemaEnumMove: idx - current });
        fire("SchemaEnumCommit");
      });
    });
    ven.addEventListener("keydown", (e) => {
      if ((e as KeyboardEvent).key === "Escape") {
        e.stopPropagation();
        fire("Escape");
      }
    });
  }
  // "Edit in editor" — unconditionally forces the external popup editor,
  // bypassing the schema-select branch BeginEdit takes for an
  // enum-constrained scalar (req 1).
  if (ext)
    ext.addEventListener("click", () => {
      run(() => {
        fire({ SetCursor: path });
        fire("BeginEditExternal");
      });
    });
  // Multi-line value button → open the host's popup editor via core's edit flow.
  if (ev)
    ev.addEventListener("click", () => {
      run(() => {
        fire({ SetCursor: path });
        fire("BeginEdit");
      });
    });
  if (te && !te.disabled)
    commit(te, () => {
      fire({ SetTrailing: { path, comment: te.value || null } });
    });
  if (cn)
    commit(cn, () => {
      fire({ ApplyEditComment: { path, text: cn.value } });
    });
  // (te/cn read their value inline in the single dispatch — no re-render between.)
  if (kb) kb.addEventListener("click", () => openKind(row));

  // Delete / Copy / Cut: select this row, run the action, then — on success —
  // confirm + dismiss the panel via `afterMutation` (errors still go to onError).
  const act = (intent: Intent, okMsg: string) => {
    let out: SessionSnapshot | undefined;
    run(() => {
      fire({ SetCursor: path });
      fire({ SetSelection: { paths: [path] } });
      out = send(intent);
    });
    if (out?.notice?.severity === "error") onError(out.notice.text);
    else afterMutation?.(okMsg);
  };
  if (del) del.addEventListener("click", () => act("DeleteSelected", "Deleted"));
  // Copy / Cut arm the clipboard; the host's paste affordance (FAB / paste-mode
  // click) commits the paste at the new cursor.
  if (cp) cp.addEventListener("click", () => act("CopySelected", "Copied — paste to place it"));
  if (ct) ct.addEventListener("click", () => act("CutSelected", "Cut — paste to move it"));
}

// Format a resolved `EditHint` into a localized advisory sentence —
// shared by every surface that shows the schema constraint for the current node: the
// desktop hover tooltip, and the desktop/TUI-mirroring idle status-line hint
// on both desktop and touch (see `ui.ts`/`touch/app.ts` `render()`).
// Localized via core.hint.* catalog keys.
export function schemaHintText(hint: EditHint): string {
  if (hint === "None") return "";
  if ("Enum" in hint) {
    const labels = hint.Enum.map(([label]) => label);
    return labels.length ? tArgs("core.hint.enum", [labels.join(", ")]) : "";
  }
  const { minimum, maximum, multiple_of } = hint.Bounded;
  const parts: string[] = [];
  if (minimum !== undefined && maximum !== undefined) {
    parts.push(tArgs("core.hint.bounded", [String(minimum), String(maximum)]));
  } else if (minimum !== undefined) {
    parts.push(tArgs("core.hint.min", [String(minimum)]));
  } else if (maximum !== undefined) {
    parts.push(tArgs("core.hint.max", [String(maximum)]));
  }
  if (multiple_of !== undefined) {
    parts.push(tArgs("core.hint.multiple", [String(multiple_of)]));
  }
  return parts.join(", ");
}