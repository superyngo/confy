// Shared floating "actions / paste" action button (FAB) — behavior and
// markup used by both `touch/app.ts` and `ui.ts` (desktop). Visuals are
// duplicated per-stylesheet (`style.css` / `touch/style.css`) rather than
// shared, since each shell links exactly one CSS file; this module is the
// single source of truth for the glyphs and markup. Unarmed, it opens the
// centralized Action menu (design doc
// `docs/superpowers/specs/2026-08-30-action-menu-design.md`); armed, it
// pastes — that decision is one clipboard_count check, made per-host inline.

// Icons ported verbatim from `touch/render.ts`'s `IC` table / `touch/app.ts`'s
// `PASTE_IC` so both surfaces render byte-identical glyphs.
// Three stacked dots (the standard "open an actions list" glyph) rather than
// "+": the button no longer means "add a node", it opens the Action menu for
// whatever's selected — "+" implied create-only and misled here.
export const FAB_ACTIONS_IC =
  '<svg viewBox="0 0 24 24" fill="currentColor"><circle cx="12" cy="5.5" r="2.1"/><circle cx="12" cy="12" r="2.1"/><circle cx="12" cy="18.5" r="2.1"/></svg>';
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
    `<button${fabId} class="fab" data-act="actions" aria-label="actions">${FAB_ACTIONS_IC}</button>` +
    // Small ✕ floating above the paste FAB — clears the clipboard / exits paste
    // mode (shown only while armed, via the host's `.paste-mode` class).
    `<button${clearId} class="fab-clear" data-act="pastecancel" aria-label="exit paste mode">${FAB_CLOSE_IC}</button>`
  );
}

/** Applies the armed-clipboard appearance to an already-rendered FAB. */
export function syncFab(fab: HTMLElement, armed: boolean, cut: boolean): void {
  fab.classList.toggle("paste-copy", armed && !cut);
  fab.classList.toggle("paste-cut", armed && cut);
  fab.innerHTML = armed ? FAB_PASTE_IC : FAB_ACTIONS_IC;
  fab.setAttribute("aria-label", armed ? "paste" : "actions");
}

