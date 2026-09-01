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
import type { ViewRow, Intent, SessionSnapshot, EditHint, Path } from "./types";
import { escapeHtml as esc } from "./escape.js";
import { isCommentRow, isPositional, valueHue } from "./kind-labels.js";
import { t, tArgs } from "./i18n.js";

// Whether a scalar value edits through the host's popup editor rather than a
// one-line input. Mirrors core's `edit_target_kind` scalar rule (multiline string
// formats route External); the `\n` check is a fallback for any embedded newline.
const MULTILINE_FORMATS = ["MultilineBasic", "MultilineLiteral", "LiteralBlock", "Folded"];
function isMultilineValue(r: ViewRow): boolean {
  return MULTILINE_FORMATS.includes(r.format) || (r.value ?? "").includes("\n");
}

// Touch swipe-to-nudge over a *focused* Integer/Float value field (i.e. once
// it has entered inline-edit) mirrors the desktop wheel / TUI arrow-key nudge,
// without opening the keyboard. Gated to pointerType==="touch" in the
// document-level pointerdown handler below so desktop mouse drag-to-select-
// text is untouched; a swipe starting ANYWHERE (not just on the input) tracks
// while the field holds focus. No Intent is dispatched per tick: the nudged
// text is written straight into the input via a stateless core query
// (`nudge_repr`) and only committed on the normal Enter/blur `commit` path.
// State lives at module scope, not inside `wirePanel`, because the gesture
// must keep tracking even though the panel may re-render mid-gesture —
// `document`-level pointermove/up/cancel listeners (installed once, guarded
// by `nudgeListenersWired`) keep tracking the same physical touch contact
// across that DOM swap, mirroring `web/touch/app.ts`'s reorder/paste-drag
// convention of reading live pointer coordinates rather than relying on
// the original target element.
const VALUE_NUDGE_DEADZONE_PX = 8; // matches touch/app.ts's existing tap-vs-drag dead zone
const VALUE_NUDGE_STEP_PX = 24; // px of horizontal drag per one Nudge(±1)-equivalent step
interface ValueNudgeGesture {
  pointerId: number;
  originX: number;
  originY: number;
  lastStep: number;
  engaged: boolean;
  path: Path;
  input: HTMLInputElement;
  nudgeRepr: (path: Path, text: string, delta: number) => string | undefined;
}
let nudgeGesture: ValueNudgeGesture | null = null;
let nudgeListenersWired = false;
function installValueNudgeListeners(): void {
  if (nudgeListenersWired) return;
  nudgeListenersWired = true;
  document.addEventListener(
    "pointermove",
    (e) => {
      if (!nudgeGesture || e.pointerId !== nudgeGesture.pointerId) return;
      const dx = e.clientX - nudgeGesture.originX;
      const dy = e.clientY - nudgeGesture.originY;
      if (!nudgeGesture.engaged) {
        if (Math.abs(dx) < VALUE_NUDGE_DEADZONE_PX || Math.abs(dx) < Math.abs(dy)) return;
        nudgeGesture.engaged = true;
      }
      e.preventDefault();
      const step = Math.trunc(dx / VALUE_NUDGE_STEP_PX);
      if (step === nudgeGesture.lastStep) return;
      const delta = step - nudgeGesture.lastStep;
      nudgeGesture.lastStep = step;
      const { input, path, nudgeRepr } = nudgeGesture;
      const next = nudgeRepr(path, input.value, delta);
      if (next === undefined) return;
      input.value = next;
      const n = input.value.length;
      input.setSelectionRange(n, n);
    },
    { passive: false },
  );
  const endGesture = (e: PointerEvent) => {
    if (!nudgeGesture || e.pointerId !== nudgeGesture.pointerId) return;
    if (nudgeGesture.engaged) e.preventDefault(); // swallow the trailing click/focus after a real drag
    nudgeGesture = null;
  };
  document.addEventListener("pointerup", endGesture);
  document.addEventListener("pointercancel", endGesture);
}

// Kind-notation glyph + value-hue lookups are shared (`kind-labels.ts`).

// Human dotted/bracketed path: `{Key:n}` → `.n` (no leading dot on the first
// segment), `{Index:i}` → `[i]`. e.g. `server.host`, `servers[1].port`.
// Prefers the server-computed `row.path_display` (core's `Session::human_path`),
// which wraps a quoted YAML key segment in `"…"` — mirroring TOML's/JSON's
// already-literal-quoted `Path` — since a plain client-side join of
// `row.path` can't know which ancestor keys were quoted (`Seg::Key` decodes
// YAML keys). Falls back to a plain client-side join when `path_display` is
// absent (e.g. older/synthetic `ViewRow` fixtures).
function humanPath(row: ViewRow): string {
  if (row.path_display !== undefined) return row.path_display;
  let s = "";
  for (const seg of row.path) {
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
  schemaInfo?: string,
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
    h += `<dl><dt>${t("web.panel.field.path")}</dt><dd>${esc(humanPath(r))}</dd></dl>`;
    h += "</div>";
    return h;
  }

  // Key (array-element index is positional, not renamable). Like the tree row
  // and the rename input, this shows the key's **authored spelling** — the
  // editable field must round-trip what the file actually holds, or committing
  // an untouched panel would silently restyle a quoted key to bare.
  const keyText = r.key_literal ?? r.key;
  h += `<div class="field-label">${t("web.panel.field.key")}</div>`;
  if (elem) {
    h += `<input class="v-edit" value="${esc(keyText)}" disabled />`;
    h += `<div class="hint-line">${t("web.panel.hint.positionalKey")}</div>`;
  } else if (!r.read_only) {
    h += `<input class="k-edit" data-field="name" value="${esc(keyText)}" autocomplete="off" spellcheck="false" />`;
  } else {
    h += `<input class="v-edit" value="${esc(keyText)}" disabled />`;
  }

  // Value (scalars only). A constrained value swaps in the picker select once
  // BeginEdit resolves `Mode::SchemaEnum` (mirrors the tree's `renderValue`'s
  // `schemaEnum` branch exactly, same option/cursor shape). Before that — and
  // for a multi-line value — it's a clickable trigger that dispatches BeginEdit
  // and lets core pick the destination (external popup, or the picker for an
  // enum-constrained field / any `bool` scalar). The `bool` case must be
  // predicted host-side here, exactly like the enum one: this panel is touch's
  // only value-edit surface, so a plain `<input>` would keep the true/false
  // picker unreachable there (`CommitEdit` deliberately never re-enters it).
  if (!branch) {
    h += `<div class="field-label">${esc(tArgs("web.panel.field.value", [r.type_label]))}</div>`;
    const v = r.value ?? "";
    if (!r.read_only && schemaEnum) {
      const opts = schemaEnum.options
        .map((label, i) => `<option value="${i}"${i === schemaEnum.cursor ? " selected" : ""}>${esc(label)}</option>`)
        .join("");
      h += `<select class="v-edit" data-field="value-enum">${opts}</select>`;
    } else if (
      !r.read_only &&
      (isMultilineValue(r) ||
        r.scalar_type === "Bool" ||
        (editHint && editHint !== "None" && "Enum" in editHint))
    ) {
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
    const note = r.badge_note;
    const noteStr = note && note !== r.type_label ? ` · ${esc(note)}` : "";
    h += `<div class="field-label">${t("web.panel.field.kind")}</div>`;
    h += `<button class="btn kindbtn" data-act="kindswitch"><span class="dotc" style="background:var(--t-${hue})"></span>${esc(r.type_label)}${noteStr}</button>`;
  }

  // Meta: Path (human form) / Children (branches) / Sign.
  h += `<dl><dt>${t("web.panel.field.path")}</dt><dd>${esc(humanPath(r))}</dd>`;
  if (branch) h += `<dt>${t("web.panel.field.children")}</dt><dd>${r.child_count}</dd>`;
  h += `<dt>${t("web.panel.field.sign")}</dt><dd>${esc(r.key_sign ?? t("web.panel.sign.none"))}</dd>`;
  h += "</dl>";

  // Schema — proactive non-widget info (`description`/`type`/`format`/
  // `pattern`, `schemaInfo`), proactive constraint description (`editHint`'s
  // `describe()` equivalent, `schemaHintText`), plus any violation messages
  // for this row. Mirrors the TUI Detail popup's `Schema:` section exactly
  // (same sources, same "only render when there's something to say" rule)
  // so the information available in the panel doesn't drift by platform.
  // Rendered right after Meta (Path/Children/Sign), before Actions, so the
  // action buttons stay the panel's fixed trailing element. Class names are
  // `schema-*-msg` (not bare `.schema-violation`) so they can't collide with
  // the tree row's `.row.schema-violation` marker.
  const hintText = editHint ? schemaHintText(editHint) : "";
  const violations = r.violations ?? [];
  if (schemaInfo || hintText || violations.length) {
    h += `<div class="field-label">${t("web.panel.field.schema")}</div>`;
    h += `<div class="schema-info${violations.length ? " has-violation" : ""}">`;
    for (const line of schemaInfo ? schemaInfo.split("\n") : []) {
      h += `<div class="schema-hint-msg">${esc(line)}</div>`;
    }
    if (hintText) h += `<div class="schema-hint-msg">${esc(hintText)}</div>`;
    for (const msg of violations) {
      h += `<div class="schema-violation-msg">${esc(msg)}</div>`;
    }
    h += "</div>";
  }

  // Comment advisory — a document-format note (not a schema constraint),
  // shown when this row is a comment/trailing-comment carrier inside a
  // `strict_json` document. Same "only render when there's something to
  // say" convention as the Schema block, placed right after it.
  if (r.comment_advisory) {
    h += `<div class="field-label">${t("web.panel.field.advisory")}</div>`;
    h += `<div class="comment-advisory"><div class="comment-advisory-msg">${esc(r.comment_advisory)}</div></div>`;
  }

  h += "</div>";
  return h;
}

// Wire the rendered panel's controls to intents.
//  - send(intent): dispatches and returns the new snapshot (we read its notice).
//  - nudgeRepr(path, text, delta): stateless nudge preview for the focused
//    value field's live text — written straight into the input (wheel/swipe
//    nudge), no dispatch, no re-render; committed via the normal commit path.
//  - batch(fn): optional host batcher — dispatches every send inside `fn` with a
//    single re-render at the end (perf: multi-intent handlers render once).
export function wirePanel(
  container: HTMLElement,
  row: ViewRow,
  send: (intent: Intent) => SessionSnapshot,
  nudgeRepr: (path: Path, text: string, delta: number) => string | undefined,
  openKind: (row: ViewRow) => void,
  onError: (msg: string) => void,
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
  const ev = container.querySelector<HTMLElement>("[data-act=editvalue]");

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
    // Mouse-wheel nudges the value only once the field is focused (entering
    // inline-edit); once armed, every wheel tick anywhere on the page nudges
    // it (not just while the pointer hovers the field) until it blurs. No
    // Intent dispatch, no re-render — the nudged text is written straight
    // into `ve` and only committed via the normal Enter/blur `commit` path.
    const st = row.scalar_type;
    if (st === "Integer" || st === "Float") {
      let onWheel: ((e: WheelEvent) => void) | null = null;
      ve.addEventListener("focus", () => {
        onWheel = (e: WheelEvent) => {
          e.preventDefault();
          const next = nudgeRepr(path, ve.value, e.deltaY < 0 ? 1 : -1);
          if (next === undefined) return;
          ve.value = next;
          const n = ve.value.length;
          ve.setSelectionRange(n, n);
        };
        document.addEventListener("wheel", onWheel, { passive: false, capture: true });
      });
      ve.addEventListener("blur", () => {
        if (onWheel) document.removeEventListener("wheel", onWheel, { capture: true });
        onWheel = null;
      });
    }
    // Touch: once the value field is focused (inline-edit), a horizontal
    // swipe starting ANYWHERE on the page begins the swipe-to-nudge gesture
    // (see module-level state above `humanPath`). Bool excluded — it already
    // has a dedicated true/false picker sheet on touch, and a bounded slide
    // has no natural two-value mapping.
    if (st === "Integer" || st === "Float") {
      ve.style.touchAction = "pan-y"; // let vertical sheet/page scroll pass through natively; only horizontal is intercepted below
      document.addEventListener(
        "pointerdown",
        (e) => {
          if (e.pointerType !== "touch") return; // desktop mouse drag keeps native text selection
          if (document.activeElement !== ve) return; // only while this field is the active inline edit
          installValueNudgeListeners();
          nudgeGesture = {
            pointerId: e.pointerId,
            originX: e.clientX,
            originY: e.clientY,
            lastStep: 0,
            engaged: false,
            path,
            input: ve,
            nudgeRepr,
          };
        },
        { capture: true },
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