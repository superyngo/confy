// Breadcrumb bar + mini-tree picker — the VS Code-style symbol path for the
// cursor node, rendered between the filter row and the tree (visible in every
// host, including the VS Code webview, whose native breadcrumb shows only the
// file segment for custom editors).
//
// Bar anatomy: ⌂ root, then one segment per cursor-path `Seg` (`Key` → name,
// `Index` → `[i]`), each with a type glyph. Clicking a segment Reveals it
// directly (`RevealPath` — expand ancestors + set cursor + select;
// CONTEXT.md §Operations "Reveal"). Clicking a `›` separator (including the
// trailing one after the current node) opens the mini-tree popup: a lazy mini
// document tree fed by the ffi `children(path)` query, pre-expanded along the
// cursor path, highlighted at the segment left of the separator; row carets
// expand/collapse freely, clicking a row body Reveals it and closes the popup. The
// popup's expand state is ephemeral — every open resets (Q7.1). The mini-tree
// shows the same node set as the main tree — comments and read-only nodes
// included (Q3/A). Pure render-from-snapshot; the only module state is the
// open popup + its ephemeral expand set, which any re-render, outside
// pointerdown, or a capture-phase Escape closes (Escape is swallowed so it
// doesn't also peel filter state — panel.ts stopPropagation precedent).
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

function closeMenu(): void {
  openMenu?.remove();
  openMenu = null;
}

// ---- bar ----
export function renderCrumbs(bar: HTMLElement, snap: SessionSnapshot, deps: CrumbDeps): void {
  closeMenu();
  const cur = snap.cursor;
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
      closeMenu();
      deps.jump(cur.slice(0, Number(b.dataset.i)));
    }),
  );
  bar.querySelectorAll<HTMLElement>("button.crumb-sep").forEach((b) =>
    b.addEventListener("click", (ev) => {
      ev.stopPropagation();
      openTree(b, snap, deps, cur.slice(0, Number(b.dataset.i)));
    }),
  );
  // Keep the tail (current node) in view when a deep path overflows.
  bar.scrollLeft = bar.scrollWidth;
}

// ---- mini-tree popup ----
function openTree(anchor: HTMLElement, snap: SessionSnapshot, deps: CrumbDeps, highlight: Path): void {
  if (openMenu?.dataset.i === anchor.dataset.i) {
    closeMenu(); // second click on the same segment toggles the popup off
    return;
  }
  closeMenu();
  // Ephemeral expand state: reset to "expanded along the cursor path" (Q7.1).
  // Every prefix INCLUDING the cursor node itself (expanding a leaf is a
  // harmless no-op — it has no children), plus the clicked segment.
  treeExpanded = new Set<string>();
  for (let i = 0; i <= snap.cursor.length; i++) {
    treeExpanded.add(JSON.stringify(snap.cursor.slice(0, i)));
  }
  treeExpanded.add(JSON.stringify(highlight));

  const menu = document.createElement("div");
  menu.className = "crumb-menu";
  menu.dataset.i = anchor.dataset.i!;
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
