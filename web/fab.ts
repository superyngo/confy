// Shared floating "add / paste" action button (FAB) — behavior and markup
// used by both `touch/app.ts` and `ui.ts` (desktop). Visuals are duplicated
// per-stylesheet (`style.css` / `touch/style.css`) rather than shared, since
// each shell links exactly one CSS file; this module is the single source of
// truth for the glyphs, markup, and the add/paste decision logic instead.
import type { SessionSnapshot } from "./types.js";
import { isExpanded } from "./kind-labels.js";

// Icons ported verbatim from `touch/render.ts`'s `IC` table / `touch/app.ts`'s
// `PASTE_IC` so both surfaces render byte-identical glyphs.
export const FAB_PLUS_IC =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>';
export const FAB_CLOSE_IC =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>';
export const FAB_PASTE_IC =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="8" y="3" width="8" height="4" rx="1"/><path d="M9 5H6a1 1 0 0 0-1 1v14a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V6a1 1 0 0 0-1-1h-3"/><path d="M12 11v6M9 14l3 3 3-3"/></svg>';

/** Markup for the FAB pair. `idAttrs` lets the desktop shell add `id="fab"` /
 *  `id="fabClear"` so it can grab them with its `$()` helper; touch passes
 *  nothing and keeps addressing them via `data-act`. */
export function fabHTML(idAttrs?: { fab?: string; clear?: string }): string {
  const fabId = idAttrs?.fab ? ` id="${idAttrs.fab}"` : "";
  const clearId = idAttrs?.clear ? ` id="${idAttrs.clear}"` : "";
  return (
    `<button${fabId} class="fab" data-act="add" aria-label="add node">${FAB_PLUS_IC}</button>` +
    // Small ✕ floating above the paste FAB — clears the clipboard / exits paste
    // mode (shown only while armed, via the host's `.paste-mode` class).
    `<button${clearId} class="fab-clear" data-act="pastecancel" aria-label="exit paste mode">${FAB_CLOSE_IC}</button>`
  );
}

/** Applies the armed-clipboard appearance to an already-rendered FAB. */
export function syncFab(fab: HTMLElement, armed: boolean, cut: boolean): void {
  fab.classList.toggle("paste-copy", armed && !cut);
  fab.classList.toggle("paste-cut", armed && cut);
  fab.innerHTML = armed ? FAB_PASTE_IC : FAB_PLUS_IC;
  fab.setAttribute("aria-label", armed ? "paste" : "add node");
}

export type FabAdd =
  | { kind: "locked" }
  | { kind: "add"; intent: "AddNode" | "AddChild" | "AddSibling"; noticeKey: string };

/** Pure: what the `+` press should do for this snapshot. `null` = no session yet.
 *  Mirrors `touch/app.ts`'s `addContextual` decision without touching the DOM
 *  or dispatching. */
export function fabAddAction(snap: SessionSnapshot | null): FabAdd | null {
  if (!snap) return null;
  if ((snap.clipboard_count ?? 0) > 0) return { kind: "locked" };
  const idx = snap.rows.findIndex((r) => r.is_cursor);
  if (idx < 0) {
    return { kind: "add", intent: "AddNode", noticeKey: "web.host.add.node" };
  }
  const r = snap.rows[idx];
  if (r.is_branch && isExpanded(snap.rows, idx)) {
    return { kind: "add", intent: "AddChild", noticeKey: "web.host.add.child" };
  }
  return { kind: "add", intent: "AddSibling", noticeKey: "web.host.add.sibling" };
}
