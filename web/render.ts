// Pure `SessionSnapshot → DOM` for the tree (WEBUI.md / PORTING §8.3). Renders
// the full web-native row anatomy from `design_index_model.html`: rotating
// caret, key / `=` / value (value-type colored) or `—` element / item count, a
// per-row **kind badge** (type + notation + chevron), comment / trailing
// decoration, and hover action buttons — drag grip + `⋮` more, flush right
// (mirrors touch's row-actions layout; ⋮'s "Add child"/"Append sibling" cover
// the standalone ＋ this replaced). Each row
// carries `data-path` (JSON-encoded `Path`) + `data-index` so the pointer layer
// (web/ui.ts, later web/select.ts / web/dnd.ts) maps a click back to a node
// without re-deriving tree structure. No editing logic lives here — it renders
// the snapshot and nothing else; the affordances it draws are wired in later
// phases (kind popover, context menu, drag-reparent).
import type { EditView, SessionSnapshot, ViewRow } from "./types.js";
import { escapeHtml } from "./escape.js";
import { displayKey, isCommentRow, isExpanded, isPositional, kindLabelParts, valueTypeClass } from "./kind-labels.js";
import { t } from "./i18n.js";

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
const IC_MORE =
  `<svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="5" r="1.6"/><circle cx="12" cy="12" r="1.6"/><circle cx="12" cy="19" r="1.6"/></svg>`;
// Schema-warning triangle — same glyph as the TUI's ▲/△ (filled = this row
// violates, hollow = a branch summarizing a descendant violation), drawn as
// SVG rather than a CSS circle so both states render crisply at 7px.
const IC_WARN_FILL =
  `<svg class="warn-dot warn-dot-fill" viewBox="0 0 10 10" width="7" height="7"><polygon points="5,0 10,9 0,9"/></svg>`;
const IC_WARN_HOLLOW =
  `<svg class="warn-dot warn-dot-hollow" viewBox="0 0 10 10" width="7" height="7"><polygon points="5,0.9 9.2,8.6 0.8,8.6"/></svg>`;

// KIND_SHORT / NOTATION_SHORT / CONTAINER_NOTE / kindLabelParts / valueTypeClass
// live in the shared kind-labels.ts (also used by panel.ts and touch/render.ts).

// Plain-text "label · notation" for the kind popup's disabled "Current:" header
// (design's `目前：…` row). Suppresses a notation that just repeats the label.
export function currentKindLabel(r: ViewRow): string {
  const { label, note } = kindLabelParts(r);
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

function renderValue(r: ViewRow, edit: EditView | null, schemaEnum: { options: string[]; cursor: number } | null): string {
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
  // `.val` cell also clamps with ellipsis (style.css).
  return escapeHtml((r.value ?? "").replace(/\r?\n/g, " ↵ "));
}

// The per-row kind badge: friendly kind label + notation suffix + chevron.
function renderKindBadge(r: ViewRow): string {
  const { label, note } = kindLabelParts(r);
  const suffix = note ? `<span class="kind-note">·${escapeHtml(note)}</span>` : "";
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
): string {
  const pathAttr = escapeHtml(JSON.stringify(r.path));
  const comment = isCommentRow(r);
  const expanded = r.is_branch && isExpanded(rows, idx);
  const cls =
    `row${r.is_branch ? " branch" : ""}${expanded ? " open" : ""}` +
    `${r.is_cursor ? " cursor" : ""}${r.selected ? " selected" : ""}` +
    `${r.read_only ? " readonly" : ""}${comment ? " comment-row" : ""}${clip}` +
    `${r.violations ? " schema-violation" : ""}${pasteInto ? " drag-over-into" : ""}` +
    `${r.is_branch && r.has_descendant_violation ? " warn-branch" : ""}`;
  let s = `<div class="${cls}" data-path="${pathAttr}" data-index="${idx}">`;
  // Indentation: a single spacer whose width scales with depth (the design's
  // `indent.style.width = depth*22`). The synthetic root (depth 0) is not drawn,
  // so real top-level nodes (depth 1) sit flush-left and each deeper level adds
  // one `--indent` step. (A zero-width span per level — the previous approach —
  // left every node flush-left with no level hint.)
  const level = Math.max(0, r.depth - 1);
  s += `<span class="indent" style="width:calc(var(--indent) * ${level})"></span>`;
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
        `${escapeHtml(head)}${more ? '<span class="comment-more"> …</span>' : ""}</span>`;
    }
  } else {
    // Key. Positional array/AoT elements are keyless; core gives them the index
    // label "[0]"/"[1]" which we keep (informative) but render faintly. A keyed
    // node in `Name` edit mode becomes a live rename `<input>`.
    if (isPositional(r)) {
      s += `<span class="key elem">${escapeHtml(r.key)}</span>`;
    } else if (edit && r.is_cursor && edit.field === "Name") {
      s += `<input class="cell-input key-input mono" data-editing="name" style="${editWidthStyle(edit.buffer)}" value="${escapeHtml(edit.buffer)}" />`;
    } else if (r.key) {
      s += `<span class="key" data-edit="key">${escapeHtml(displayKey(r, docFormat))}</span>`;
    }
    if (r.is_branch) {
      s += `<span class="count">${r.child_count} ${r.child_count === 1 ? t("web.render.item.one") : t("web.render.item.many")}</span>`;
    } else {
      const vcls = valueTypeClass(r);
      const editingValue =
        (schemaEnum && r.is_cursor) || (edit !== null && r.is_cursor && edit.field === "Value");
      s += `<span class="eq">=</span>`;
      s += `<span class="val ${vcls}${editingValue ? " editing" : ""} mono" data-edit="val">${renderValue(r, edit, schemaEnum)}</span>`;
    }
    // Kind badge (type + notation + chevron).
    if (!r.read_only) s += renderKindBadge(r);
    // Trailing same-line comment.
    if (r.trailing_comment) {
      const advisoryCls = r.comment_advisory ? " comment-advisory" : "";
      const titleAttr = r.comment_advisory ? ` title="${escapeHtml(r.comment_advisory)}"` : "";
      s += `<span class="comment mono${advisoryCls}" data-edit="note"${titleAttr}>${escapeHtml(r.trailing_comment)}</span>`;
    }
  }

  // Hover action buttons: drag grip (replaces the former standalone left-side
  // grip AND the branch-only ＋, whose "add child" is already reachable via
  // the ⋮ menu — desktop.paste-mode hides row-actions wholesale, so the grip
  // now disappears with the rest during an armed clipboard, same as ⋮/menu).
  s += `<span class="row-actions">`;
  s += `<span class="drag-handle" data-grip="1" draggable="true">${IC_GRIP}</span>`;
  s += `<button title="${t("web.render.moreActions.title")}" data-act="menu">${IC_MORE}</button>`;
  s += `</span>`;

  s += `</div>`;
  return s;
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
  const schemaEnum =
    typeof snap.mode === "object" && "SchemaEnum" in snap.mode
      ? { options: snap.mode.SchemaEnum.options, cursor: snap.mode.SchemaEnum.cursor }
      : null;
  // Clipboard source rows get a distinct class (copy vs cut) so they read
  // differently from the selection box.
  const clipKeys = new Set(snap.clipboard_paths.map((p) => JSON.stringify(p)));
  const clipCls: " clip-copy" | " clip-cut" = snap.clipboard_cut
    ? " clip-cut"
    : " clip-copy";
  // Armed-paste `Into` target, keyed so the loop above can compare per row
  // (ADR 0004 §1) — `After` is a cross-row line, drawn separately in
  // `ui.ts`'s `renderPasteSlotCue` since it isn't any single row's own class.
  const pasteIntoPath =
    snap.paste_slot && "Into" in snap.paste_slot ? JSON.stringify(snap.paste_slot.Into) : null;
  // The synthetic root (empty path) is not rendered; `idx` stays the real
  // `snap.rows` index so a click maps back to the right node.
  const next: { key: string; html: string }[] = [];
  rows.forEach((r, idx) => {
    if (r.path.length === 0) return;
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

  const cur = treeEl.querySelector(".row.cursor") as HTMLElement | null;
  cur?.scrollIntoView({ block: "nearest" });
}
