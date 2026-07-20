// Breadcrumb bar + mini-tree picker — the VS Code-style symbol path for the
// cursor node, rendered between the filter row and the tree (visible in every
// host, including the VS Code webview, whose native breadcrumb shows only the
// file segment for custom editors).
//
// Bar anatomy: ⌂ root, then one segment per cursor-path `Seg` (`Key` → name,
// `Index` → `[i]`), each with a type glyph. Clicking a **segment** Reveals it
// (`RevealPath` — expand ancestors + set cursor + select) AND toggles open the
// mini-tree popup at that segment — but the *displayed* bar path freezes at
// its pre-click state while browsing: hopping to another segment keeps
// jump-selecting without moving the bar. The bar only catches up (unfreezes)
// when the user re-clicks the same segment (closing the popup) or picks a row
// inside the mini-tree (which also Reveals + closes). Clicking a `›`
// separator (including the trailing one after the current node) instead opens
// the mini-tree as a pure browse — no jump-select, bar untouched — fed by the
// ffi `children(path)` query, pre-expanded along the highlighted path; row
// carets expand/collapse freely, clicking a row body Reveals it and closes the
// popup + finalizes the bar. The popup's expand state is ephemeral — every
// open resets (Q7.1). The mini-tree shows the same node set as the main tree
// — comments and read-only nodes included (Q3/A). Pure render-from-snapshot
// aside from the popup + its ephemeral expand set and the frozen-path latch;
// any re-render, outside pointerdown, or a capture-phase Escape closes the
// popup (Escape is swallowed so it doesn't also peel filter state — panel.ts
// stopPropagation precedent) — closing this way does not finalize a frozen
// bar path.
import type { ChildView, Path, Seg, SessionSnapshot } from "./types.js";
import { escapeHtml } from "./escape.js";
import { pathEq } from "./path-utils.js";
import { t } from "./i18n.js";

export interface CrumbDeps {
  /** Immediate children of the node at `path` (ffi `children`), lazy. */
  children(path: Path): ChildView[];
  /** Dispatch `RevealPath` (the Reveal operation). */
  jump(path: Path): void;
}

// VS Code-style text glyph + value-type hue token per core type_label (Q6/A).
const GLYPHS: Record<string, [glyph: string, hue: string]> = {
  table: ["{}", "branch"],
  inline: ["{}", "branch"],
  array: ["[]", "branch"],
  "array-of-tables": ["[]", "branch"],
  string: ["abc", "string"],
  integer: ["123", "number"],
  float: ["123", "number"],
  bool: ["tf", "bool"],
  null: ["??", "null"],
  offsetdatetime: ["@", "date"],
  localdatetime: ["@", "date"],
  localdate: ["@", "date"],
  localtime: ["@", "date"],
  comment: ["#", "null"],
};

function glyphHTML(typeLabel: string): string {
  const [g, hue] = GLYPHS[typeLabel] ?? ["··", "null"];
  return `<span class="crumb-glyph mono t-${hue}">${escapeHtml(g)}</span>`;
}

function segLabel(seg: Seg): string {
  return "Key" in seg ? escapeHtml(seg.Key) : `[${seg.Index}]`;
}

// ---- popup state (ephemeral per open) ----
let openMenu: HTMLElement | null = null;
let treeExpanded = new Set<string>(); // JSON.stringify(path) keys

// A segment click jump-selects immediately but freezes the *displayed* bar
// path until the interaction is finalized (re-clicking the same segment, or
// picking a row in the mini-tree) — so browsing between segments doesn't
// thrash the breadcrumb path underfoot. `null` means "not frozen": the bar
// tracks the live cursor as usual.
let frozenPath: Path | null = null;

function closeMenu(): void {
  openMenu?.remove();
  openMenu = null;
}

// ---- bar ----
export function renderCrumbs(bar: HTMLElement, snap: SessionSnapshot, deps: CrumbDeps): void {
  closeMenu();
  const cur = frozenPath ?? snap.cursor;
  const parts: string[] = [
    `<button class="crumb${cur.length === 0 ? " current" : ""}" data-i="0" title="${t("web.crumbs.root.title")}">⌂</button>`,
  ];
  for (let i = 0; i < cur.length; i++) {
    // A segment's own type comes from its parent's children list (the bar has
    // only `Seg`s; ChildView carries the type). Depth is small — one lazy
    // query per level per render is fine.
    const self = cur.slice(0, i + 1);
    const info = deps.children(cur.slice(0, i)).find((k) => pathEq(k.path, self));
    parts.push(`<button class="crumb-sep" data-i="${i}" title="${t("web.crumbs.browse.title")}">›</button>`);
    parts.push(
      `<button class="crumb${i === cur.length - 1 ? " current" : ""}" data-i="${i + 1}">` +
        (info ? glyphHTML(info.type_label) : "") +
        `<span>${segLabel(cur[i])}</span></button>`,
    );
  }
  // Trailing separator: opens the mini-tree at the current node (and is the
  // only mini-tree entry when the cursor sits on the root).
  parts.push(`<button class="crumb-sep" data-i="${cur.length}" title="${t("web.crumbs.browse.title")}">›</button>`);
  bar.innerHTML = parts.join("");
  bar.querySelectorAll<HTMLElement>("button.crumb").forEach((b) =>
    b.addEventListener("click", (ev) => {
      ev.stopPropagation();
      const path = cur.slice(0, Number(b.dataset.i));
      if (openMenu && openMenu.dataset.kind === "node" && openMenu.dataset.i === b.dataset.i) {
        // Re-clicking the same segment: close the panel and finalize —
        // the bar now catches up to wherever the cursor ended up.
        closeMenu();
        frozenPath = null;
        return;
      }
      // First segment click of a browsing session: freeze the bar at its
      // current (pre-jump) path so hopping between segments doesn't move it.
      if (frozenPath === null) frozenPath = cur;
      deps.jump(path); // synchronous dispatch + re-render (bar stays frozen)
      const freshAnchor = bar.querySelector<HTMLElement>(`button.crumb[data-i="${b.dataset.i}"]`);
      if (freshAnchor) openTree(freshAnchor, deps, path, "node");
    }),
  );
  bar.querySelectorAll<HTMLElement>("button.crumb-sep").forEach((b) =>
    b.addEventListener("click", (ev) => {
      ev.stopPropagation();
      openTree(b, deps, cur.slice(0, Number(b.dataset.i)), "sep");
    }),
  );
  // Keep the tail (current node) in view when a deep path overflows.
  bar.scrollLeft = bar.scrollWidth;
}

// ---- mini-tree popup ----
function openTree(anchor: HTMLElement, deps: CrumbDeps, highlight: Path, kind: "node" | "sep"): void {
  if (openMenu?.dataset.i === anchor.dataset.i && openMenu?.dataset.kind === kind) {
    closeMenu(); // second click on the same trigger toggles the popup off
    return;
  }
  closeMenu();
  // Ephemeral expand state: reset to "expanded along the highlighted path"
  // (Q7.1) — every prefix of `highlight` (expanding a leaf is a harmless
  // no-op — it has no children).
  treeExpanded = new Set<string>();
  for (let i = 0; i <= highlight.length; i++) {
    treeExpanded.add(JSON.stringify(highlight.slice(0, i)));
  }

  const menu = document.createElement("div");
  menu.className = "crumb-menu";
  menu.dataset.i = anchor.dataset.i!;
  menu.dataset.kind = kind;
  renderTreeRows(menu, deps, highlight);
  menu.addEventListener("click", (ev) => {
    const rowEl = (ev.target as Element).closest<HTMLElement>(".crumb-row");
    if (!rowEl) return;
    if ((ev.target as Element).closest(".crumb-caret:not(.none)")) {
      const key = rowEl.dataset.path!;
      if (treeExpanded.has(key)) treeExpanded.delete(key);
      else treeExpanded.add(key);
      const scroll = menu.scrollTop;
      renderTreeRows(menu, deps, highlight);
      menu.scrollTop = scroll;
      return;
    }
    const path = JSON.parse(rowEl.dataset.path!) as Path;
    closeMenu();
    frozenPath = null; // picking a mini-tree row finalizes the breadcrumb path
    deps.jump(path);
  });
  document.body.appendChild(menu);
  const r = anchor.getBoundingClientRect();
  menu.style.left = `${Math.max(8, Math.min(r.left, window.innerWidth - menu.offsetWidth - 8))}px`;
  menu.style.top = `${r.bottom + 4}px`;
  openMenu = menu;
  menu.querySelector(".current")?.scrollIntoView({ block: "nearest" });
}

function renderTreeRows(menu: HTMLElement, deps: CrumbDeps, highlight: Path): void {
  const rows: string[] = [
    // Root row first — jumpable like the main tree's root row.
    `<div class="crumb-row${highlight.length === 0 ? " current" : ""}" data-path="[]" style="--d:0">` +
      `<span class="crumb-caret none"></span>` +
      `<span class="crumb-glyph mono t-branch">⌂</span>` +
      `<span class="crumb-label">${t("web.crumbs.root.title")}</span></div>`,
  ];
  const walk = (path: Path, depth: number): void => {
    for (const k of deps.children(path)) {
      const key = JSON.stringify(k.path);
      const open = treeExpanded.has(key);
      const last = k.path[k.path.length - 1];
      // Keyless children (array elements, comments) fall back to `[i]`.
      const label = k.key !== "" ? escapeHtml(k.key) : last ? segLabel(last) : "…";
      rows.push(
        `<div class="crumb-row${pathEq(k.path, highlight) ? " current" : ""}" data-path="${escapeHtml(key)}" style="--d:${depth}">` +
          (k.is_branch
            ? `<button class="crumb-caret" aria-expanded="${open}">${open ? "⌄" : "›"}</button>`
            : `<span class="crumb-caret none"></span>`) +
          glyphHTML(k.type_label) +
          `<span class="crumb-label">${label}</span></div>`,
      );
      if (k.is_branch && open) walk(k.path, depth + 1);
    }
  };
  walk([], 1);
  menu.innerHTML = rows.join("");
}

// ---- dismissal (wired once) ----
let dismissWired = false;
export function wireCrumbDismiss(): void {
  if (dismissWired) return;
  dismissWired = true;
  document.addEventListener("pointerdown", (ev) => {
    if (
      openMenu &&
      !openMenu.contains(ev.target as Node) &&
      !(ev.target as Element).closest(".crumbs")
    ) {
      closeMenu();
    }
  });
  // Capture phase so an open popup swallows Escape before the app's global
  // peel handler (same precedent as panel.ts's stopPropagation).
  document.addEventListener(
    "keydown",
    (ev) => {
      if (ev.key === "Escape" && openMenu) {
        closeMenu();
        ev.stopPropagation();
        ev.preventDefault();
      }
    },
    true,
  );
}
