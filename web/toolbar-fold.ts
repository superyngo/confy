// Shared toolbar/overflow-menu registry mechanism, used identically by the
// desktop (`ui.ts`) and touch (`touch/app.ts`) chrome. Each toolbar control
// that can be responsively hidden registers ONE `ToolbarEntry` here; the "⋯"
// overflow menu is then derived from that single list via `foldedEntries`
// instead of a hand-maintained parallel array — the class of bug this exists
// to prevent is a button that folds away in CSS but was never added to the
// menu's candidate list (or vice versa).
//
// Kept intentionally tiny and DOM-free: `isFolded` is injected by the caller
// (desktop checks `document.getElementById(key).offsetParent === null`,
// touch checks `app.querySelector(key).offsetParent === null`), which makes
// `foldedEntries` a pure function unit-testable without a DOM/jsdom.
export interface ToolbarEntry<Ctx = void> {
  // Unique identifier the caller's `isFolded` predicate resolves — desktop
  // uses the element id (e.g. "btnUndo"), touch uses a CSS selector on
  // `data-act` (e.g. '[data-act="undo"]'). Caller's choice; just a lookup key.
  key: string;
  // i18n catalog key for the menu-row label.
  labelKey: string;
  // Optional icon markup/glyph for the menu row.
  icon?: string;
  run: (ctx: Ctx) => void;
}

export function foldedEntries<Ctx>(
  entries: ToolbarEntry<Ctx>[],
  isFolded: (key: string) => boolean,
): ToolbarEntry<Ctx>[] {
  return entries.filter((e) => isFolded(e.key));
}
