// Pure `SessionSnapshot → DOM` for the tree (WEBUI.md / PORTING §8.3). Renders
// the full web-native row anatomy from `design_index_model.html`: rotating
// caret, key / `=` / value (value-type colored) or `—` element / item count, a
// per-row **kind badge** (type + notation + chevron), comment / trailing
// decoration, and a hover drag-grip, flush right (mirrors touch's
// row-actions layout; node operations live in the centralized Action menu,
// not per-row chrome). Each row
// carries `data-path` (JSON-encoded `Path`) + `data-index` so the pointer layer
// (web/ui.ts, later web/select.ts / web/dnd.ts) maps a click back to a node
// without re-deriving tree structure. No editing logic lives here — it renders
// the snapshot and nothing else; the affordances it draws are wired in later
// phases (kind popover, context menu, drag-reparent).
import type { EditView, PasteSlot, SessionSnapshot, ViewRow } from "./types.js";
import { escapeHtml } from "./escape.js";
import { isCommentRow, isExpanded, isPositional, valueTypeClass } from "./kind-labels.js";
import { t } from "./i18n.js";
import { highlightHtml } from "./highlight.js";

// Re-export so existing importers (ui.ts / typefilter.ts / convert-dialog.ts)
// keep their entry point; the single quote-safe escaper lives in escape.ts.
export { escapeHtml } from "./escape.js";

// --- inline SVGs (mirrors the design's IC table) ---
export const IC_CARET =
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M9 6l6 6-6 6"/></svg>`;
const IC_CHEV =
  `<svg class="chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M6 9l6 6 6-6"/></svg>`;
const IC_GRIP =
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="9" cy="6" r="1.4"/><circle cx="15" cy="6" r="1.4"/><circle cx="9" cy="12" r="1.4"/><circle cx="15" cy="12" r="1.4"/><circle cx="9" cy="18" r="1.4"/><circle cx="15" cy="18" r="1.4"/></svg>`;
// Schema-warning triangle — same glyph as the TUI's ▲/△ (filled = this row
// violates, hollow = a branch summarizing a descendant violation), drawn as
// SVG rather than a CSS circle so both states render crisply at 7px.
const IC_WARN_FILL =
  `<svg class="warn-dot warn-dot-fill" viewBox="0 0 10 10" width="7" height="7"><polygon points="5,0 10,9 0,9"/></svg>`;
const IC_WARN_HOLLOW =
  `<svg class="warn-dot warn-dot-hollow" viewBox="0 0 10 10" width="7" height="7"><polygon points="5,0.9 9.2,8.6 0.8,8.6"/></svg>`;

// KIND_SHORT / NOTATION_SHORT / CONTAINER_NOTE now live in core as
// `ViewRow.badge_label`/`.badge_note` (session/status_fmt.rs); this file only
// renders them.

// Plain-text "label · notation" for the kind popup's disabled "Current:" header
// (design's `目前：…` row). Suppresses a notation that just repeats the label.
export function currentKindLabel(r: ViewRow): string {
  const { badge_label: label, badge_note: note } = r;
  return note ? `${label} · ${note}` : label;
}

// When the cursor row is in `Value` edit mode, the value cell becomes a live
// `<input>` (ui.ts focuses it and commits on Enter/blur via `CommitEdit`).
// Core bundles a scalar's trailing comment into the inline-edit buffer as
// `value␠␠# comment` (so the TUI edits both at once). The web edits the comment
// *separately* (a dedicated comment cell), so the value `<input>` is seeded with
// the value portion only — strip the `␠␠<trailing>` suffix core appended. ui.ts
// re-appends the unchanged comment on commit so a value edit never drops it.
function valueEditSeed(r: ViewRow, buffer: string): string {
  const tc = r.trailing_comment;
  if (tc && buffer.endsWith(tc)) {
    return buffer.slice(0, buffer.length - tc.length).replace(/\s+$/, "");
  }
  return buffer;
}

// Inline-editor width sized to its content, so the editor opens at the text's
// own length (CSS min/max-width still clamp; ui.ts re-applies the same formula
// on input so it grows while typing).
export function editWidthCh(text: string): string {
  return `${Math.max(6, text.length + 2)}ch`;
}
const editWidthStyle = (text: string): string => `width:${editWidthCh(text)}`;

function renderValue(
  r: ViewRow,
  edit: EditView | null,
  schemaEnum: { options: string[]; cursor: number } | null,
  filter: string,
): string {
  if (schemaEnum && r.is_cursor) {
    const opts = schemaEnum.options
      .map((label, i) => `<option value="${i}"${i === schemaEnum.cursor ? " selected" : ""}>${escapeHtml(label)}</option>`)
      .join("");
    return `<select class="cell-input mono" data-editing="value" data-schema-enum="1">${opts}</select>`;
  }
  if (edit && r.is_cursor && edit.field === "Value") {
    const seed = valueEditSeed(r, edit.buffer);
    return `<input class="cell-input mono" data-editing="value" style="${editWidthStyle(seed)}" value="${escapeHtml(seed)}" />`;
  }
  // Collapse newlines so a multiline value stays on one row (it would otherwise
  // break the flexbox and push the kind badge off, making it unclickable). The
  // `.val` cell also clamps with ellipsis (style.css). The filter's matched chars
  // are marked here as well as in the key — core's haystack spans path + value +
  // comment, so a row can be a value-only match (TUI does the same).
  return highlightHtml((r.value ?? "").replace(/\r?\n/g, " ↵ "), filter);
}

// The per-row kind badge: friendly kind label + notation suffix + chevron.
//
// The Root's badge (core gives it `⌂` + the document format) is **inert**
// (ADR 0013 D12): a document has no kind to switch to, so it is a `<span>`
// with no `data-kind` hook and no chevron — clicking it does nothing rather
// than opening an empty `K` picker.
function renderKindBadge(r: ViewRow, isRoot: boolean): string {
  const { badge_label: label, badge_note: note } = r;
  const suffix = note ? `<span class="kind-note">·${escapeHtml(note)}</span>` : "";
  if (isRoot) return `<span class="kind inert">${escapeHtml(label)}${suffix}</span>`;
  return `<button class="kind" data-kind="1">${escapeHtml(label)}${suffix} ${IC_CHEV}</button>`;
}

export function renderRow(
  r: ViewRow,
  idx: number,
  rows: ViewRow[],
  edit: EditView | null,
  schemaEnum: { options: string[]; cursor: number } | null,
  clip: "" | " clip-copy" | " clip-cut",
  pasteInto: boolean = false,
  docFormat: string = "Toml",
  /** Live filter query (`SessionSnapshot.filter`); "" = no filter, no marks. */
  filter: string = "",
): string {
  const pathAttr = escapeHtml(JSON.stringify(r.path));
  const comment = isCommentRow(r);
  // The document Root, drawn only in root-visible mode (desktop web, TUI).
  // It is a real row — cursor, expand/collapse, hover ⋮ — but it can never be
  // selected, moved, or kind-switched (ADR 0013 D1/D12).
  const isRoot = r.path.length === 0;
  const expanded = r.is_branch && isExpanded(rows, idx);
  const cls =
    `row${r.is_branch ? " branch" : ""}${expanded ? " open" : ""}` +
    `${r.is_cursor ? " cursor" : ""}${r.selected ? " selected" : ""}` +
    `${r.read_only ? " readonly" : ""}${comment ? " comment-row" : ""}${clip}` +
    `${r.violations ? " schema-violation" : ""}${pasteInto ? " paste-target" : ""}` +
    `${r.is_branch && r.has_descendant_violation ? " warn-branch" : ""}` +
    `${isRoot ? " root-row" : ""}`;
  let s = `<div class="${cls}" data-path="${pathAttr}" data-index="${idx}">`;
  // Indentation: a single spacer whose width scales with depth (the design's
  // `indent.style.width = depth*22`). `r.depth` is used verbatim — core owns
  // the shift: in root-hidden mode (touch, VS Code) it drops the Root row and
  // re-bases every remaining depth, so top-level nodes arrive at 0 either way
  // (ADR 0013 D5). In root-visible mode the drawn Root row costs one level,
  // exactly as the TUI draws it (D8).
  s += `<span class="indent" style="width:calc(var(--indent) * ${r.depth})"></span>`;
  // Disclosure caret (rotates on expand); leaves get an aligned hidden caret.
  s += `<button class="caret${r.is_branch ? "" : " leaf"}" data-caret="1">${IC_CARET}</button>`;
  // Schema-warning triangle: rides in the flex flow right after the caret,
  // so its horizontal position tracks the row's own indentation instead of
  // a fixed offset from the tree's left edge. Filled = this exact row
  // violates (`r.violations`); hollow = a collapsed/expanded branch merely
  // summarizing a violation somewhere in its subtree
  // (`has_descendant_violation`). A branch that both violates itself and
  // has violating descendants shows filled — its own problem outranks the
  // summary. Matches the TUI's ▲/△ glyphs (`crates/confy-tui/src/tui/ui.rs`).
  if (r.violations) {
    s += IC_WARN_FILL;
  } else if (r.is_branch && r.has_descendant_violation) {
    s += IC_WARN_HOLLOW;
  }

  if (comment) {
    if (edit && r.is_cursor && edit.field === "Value") {
      // Single-line comment → inline editor (multi-line routes to the popup).
      s += `<input class="cell-input mono comment-input" data-editing="comment" style="${editWidthStyle(edit.buffer)}" value="${escapeHtml(edit.buffer)}" />`;
    } else {
      // Show only the first line in the row; the full multi-line text lives in
      // the detail panel (i). A trailing `…` marks a comment that continues.
      const full = r.value ?? "";
      const nl = full.search(/\r?\n/);
      const head = nl === -1 ? full : full.slice(0, nl);
      const more = nl !== -1;
      // `comment_advisory` (a `strict_json` document — comments aren't
      // standard JSON) takes priority over the plain "press i" hint when
      // both apply; a wavy underline marks it visually (desktop hover only,
      // matching the schema hover-tooltip convention — touch has no hover).
      const advisoryTitle = r.comment_advisory ?? (more ? "multi-line comment — press i for full text" : "");
      const advisoryCls = r.comment_advisory ? " comment-advisory" : "";
      s +=
        `<span class="comment mono${advisoryCls}" data-edit="comment"${advisoryTitle ? ` title="${escapeHtml(advisoryTitle)}"` : ""}>` +
        `${highlightHtml(head, filter)}${more ? '<span class="comment-more"> …</span>' : ""}</span>`;
    }
  } else {
    // Key. Positional array/AoT elements are keyless; core gives them the index
    // label "[0]"/"[1]" which we keep (informative) but render faintly. A keyed
    // node in `Name` edit mode becomes a live rename `<input>`.
    if (isPositional(r)) {
      s += `<span class="key elem">${highlightHtml(r.key, filter)}</span>`;
    } else if (edit && r.is_cursor && edit.field === "Name") {
      // The rename/edit buffer carries the key's authored spelling itself
      // (seeded from core's `ViewRow.key_literal`), so no separate quote
      // decoration is drawn here — it would double the quotes. Plain input,
      // same as any other key.
      s += `<input class="cell-input key-input mono" data-editing="name" style="${editWidthStyle(edit.buffer)}" value="${escapeHtml(edit.buffer)}" />`;
    } else if (isRoot) {
      // The document's filename, supplied by the host via `SetFilename`. Not
      // a rename target: renaming the Root would mean renaming the file, which
      // is the host's Save-As flow, not a document mutation.
      s += `<span class="key">${highlightHtml(r.key, filter)}</span>`;
    } else if (r.key) {
      s += `<span class="key" data-edit="key">${highlightHtml(r.key_literal ?? r.key, filter)}</span>`;
    }
    if (r.is_branch) {
      s += `<span class="count">${r.child_count} ${r.child_count === 1 ? t("web.render.item.one") : t("web.render.item.many")}</span>`;
    } else {
      const vcls = valueTypeClass(r);
      const editingValue =
        (schemaEnum && r.is_cursor) || (edit !== null && r.is_cursor && edit.field === "Value");
      s += `<span class="eq">=</span>`;
      s += `<span class="val ${vcls}${editingValue ? " editing" : ""} mono" data-edit="val">${renderValue(r, edit, schemaEnum, filter)}</span>`;
    }
    // Kind badge (type + notation + chevron).
    if (!r.read_only) s += renderKindBadge(r, isRoot);
    // Trailing same-line comment.
    if (r.trailing_comment) {
      const advisoryCls = r.comment_advisory ? " comment-advisory" : "";
      const titleAttr = r.comment_advisory ? ` title="${escapeHtml(r.comment_advisory)}"` : "";
      s += `<span class="comment mono${advisoryCls}" data-edit="note"${titleAttr}>${escapeHtml(r.trailing_comment)}</span>`;
    }
  }

  // Hover action buttons: drag grip only (⋮ replaced by the centralized
  // Action menu — desktop.paste-mode hides row-actions wholesale, so the
  // grip now disappears with the rest during an armed clipboard, same as
  // it did with the old ⋮ button).
  //
  // The Root row gets NO grip: `Mutation::Move` on the document is
  // `Unsupported`, so a draggable grip there could only ever fail (D12 — the
  // row keeps its hover ⋮, which core dims per action).
  s += `<span class="row-actions">`;
  if (!isRoot) s += `<span class="drag-handle" data-grip="1" draggable="true">${IC_GRIP}</span>`;
  s += `</span>`;

  s += `</div>`;
  return s;
}

/** The inline `<select>` shown in a row's value cell for `Mode::SchemaEnum` —
 * or `null` when that mode is not a *value* pick at all.
 *
 * A `from_kind_switch` picker is the ADR 0012 datetime **type** list: those are
 * kind options, and the host draws them in its kind-option surfaces
 * (`#kindMenu` popover / `#overlay` list, `ui.ts`), so drawing them here too
 * would double the widget — which is exactly what it used to do, leaving one
 * kind list looking like an inline dropdown while every other one was a list
 * box. Exported so the gate is testable without a DOM. */
export function valuePicker(
  mode: SessionSnapshot["mode"],
): { options: string[]; cursor: number } | null {
  if (typeof mode !== "object" || !("SchemaEnum" in mode)) return null;
  const se = mode.SchemaEnum;
  return se.from_kind_switch ? null : { options: se.options, cursor: se.cursor };
}

/** Render the whole tree into `treeEl` and scroll the cursor row into view.
 *
 * Reconciles by `data-path` key instead of rebuilding `innerHTML`
 * unconditionally: a row whose freshly-rendered HTML is byte-identical to
 * what's cached on its existing element (`data-html`) is left untouched —
 * no DOM write, no reflow — everything else is created/replaced/reordered
 * in place. This is the reason a live-focused inline `<input>` (edit mode)
 * survives a render that fires for an unrelated reason (e.g. an async
 * schema fetch resolving): `renderRow` derives the row from `edit.buffer`,
 * which core only updates on commit, so the row's output is unchanged while
 * typing and the element — including its focus and in-progress keystrokes —
 * is reused verbatim. A caller that mutates a row's DOM out of band (see
 * `beginTrailingEdit` in ui.ts) must invalidate that row's `data-html` so
 * this can't skip restoring it on the next render. */
export function renderTree(
  treeEl: HTMLElement,
  snap: SessionSnapshot,
  edit: EditView | null,
): void {
  const rows = snap.rows;
  const schemaEnum = valuePicker(snap.mode);
  // Clipboard source rows get a distinct class (copy vs cut) so they read
  // differently from the selection box.
  const clipKeys = new Set(snap.clipboard_paths.map((p) => JSON.stringify(p)));
  const clipCls: " clip-copy" | " clip-cut" = snap.clipboard_cut
    ? " clip-cut"
    : " clip-copy";
  // Armed-paste `Into` target, keyed so the loop above can compare per row
  // (ADR 0004 §1) — `After` is a cross-row line, drawn separately in
  // `ui.ts`'s `renderConfirmedPasteCue` since it isn't any single row's own
  // class. Falls back to `After(cursor)` when `paste_slot` is unset, same
  // as core's own `effective_paste_slot()`, so this row's own render
  // reflects the confirmed target the instant the clipboard is armed.
  const effectivePasteSlot: PasteSlot | null =
    (snap.clipboard_count ?? 0) > 0 ? snap.paste_slot ?? { After: snap.cursor } : null;
  const pasteIntoPath =
    effectivePasteSlot && "Into" in effectivePasteSlot
      ? JSON.stringify(effectivePasteSlot.Into)
      : null;
  // Every row core hands us is drawn — including the Root row, which core
  // omits entirely in root-hidden mode (ADR 0013 D5), so no host-side filter
  // is needed or wanted. `idx` is the real `snap.rows` index either way, so a
  // click maps back to the right node.
  const next: { key: string; html: string }[] = [];
  rows.forEach((r, idx) => {
    next.push({
      key: JSON.stringify(r.path),
      html: renderRow(
        r,
        idx,
        rows,
        edit,
        schemaEnum,
        clipKeys.has(JSON.stringify(r.path)) ? clipCls : "",
        pasteIntoPath !== null && JSON.stringify(r.path) === pasteIntoPath,
        snap.doc_format,
        snap.filter ?? "",
      ),
    });
  });

  const existing = new Map<string, HTMLElement>();
  for (const child of Array.from(treeEl.children)) {
    const key = (child as HTMLElement).dataset.path;
    if (key) existing.set(key, child as HTMLElement);
  }
  const tpl = document.createElement("template");
  let cursor: ChildNode | null = treeEl.firstChild;
  for (const { key, html } of next) {
    const old = existing.get(key);
    if (old) existing.delete(key);
    let node: HTMLElement;
    if (old && old.dataset.html === html) {
      node = old; // unchanged — reuse verbatim, no DOM write
    } else {
      tpl.innerHTML = html;
      node = tpl.content.firstElementChild as HTMLElement;
      node.dataset.html = html;
    }
    if (cursor === node) {
      cursor = node.nextSibling;
    } else {
      // `cursor` may currently *be* `old` (the stale node this replaces,
      // sitting exactly where `node` belongs) — step past it first so
      // removing `old` below can't invalidate the reference `insertBefore`
      // needs.
      if (cursor === old) cursor = cursor.nextSibling;
      // Inserting an already-attached node moves it (detaching it from its
      // old position first) — this one call handles create, replace-in-
      // place, and reorder uniformly.
      treeEl.insertBefore(node, cursor);
    }
    if (old && old !== node) old.remove();
  }
  // Anything left in `existing` is a row that no longer appears — drop it.
  for (const stale of existing.values()) stale.remove();
  // A **root-hidden** host (VS Code) can legitimately render ZERO rows: an
  // empty document has no Root row to stand on (ADR 0013 D11 — core gets no
  // zero-children exception, the empty state is the host's). The keyboard
  // still has `a`; this is the pointer's only way in. Root-visible hosts never
  // see it — the Root row is always drawn there.
  const emptyEl = treeEl.querySelector(".tree-empty");
  if (rows.length === 0) {
    if (!emptyEl) {
      treeEl.insertAdjacentHTML(
        "beforeend",
        `<div class="tree-empty"><p>${escapeHtml(t("web.tree.empty"))}</p>` +
          `<button data-act="addroot">${escapeHtml(t("web.tree.empty.add"))}</button></div>`,
      );
    }
  } else {
    emptyEl?.remove();
  }

  const cur = treeEl.querySelector(".row.cursor") as HTMLElement | null;
  cur?.scrollIntoView({ block: "nearest" });
}
