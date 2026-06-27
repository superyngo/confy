// confy touch UI — orchestrator.
//
// Boots the shared confy-core Session (the same wasm contract the desktop UI
// uses), generates the prototype's app shell, and re-points every prototype
// gesture to a single `Intent` → `session.dispatch` → full re-render. Unlike the
// prototype (which mutated its DOM-as-state directly), this is STATELESS: the
// DOM is always a projection of the latest `SessionSnapshot`.
//
// Deliberate scope mappings (the core's vocabulary, not the prototype's):
//   · FAB → `AddNode` (parameterless, like the desktop `a` key) — the
//     prototype's add-type sheet is dropped; the new node's type is changed
//     afterwards via the kind badge / detail.
//   · Swipe "Dup" → `CopySelected` + `Paste` (a real duplicate; there is no
//     dedicated duplicate Intent).
//   · Read-only / opaque rows (`ViewRow.read_only`) render without grip/kind/
//     swipe affordances and reject edits — mirroring core.
//   · Type-filter & Convert sheets are driven by `snapshot.mode`
//     (`TypeFilterView` / `ConvertView`), never local UI state.
import { load, Session } from "../confy.js";
import type {
  Intent,
  Path,
  Seg,
  SessionSnapshot,
  ViewRow,
  ModeView,
  TypeFilterView,
  ConvertView,
} from "../types.js";
import {
  fsAccessAvailable,
  pickOpenFile,
  pickSaveFile,
  writeFile,
  downloadText,
  extFor,
  type OpenedFile,
} from "../fs.js";
import { IC, esc, treeHTML, isExpanded, pathEq } from "./render.js";
import { panelHTML, wirePanel } from "../panel.js";
import { typeFilterHTML, wireTypeFilter } from "../typefilter.js";
import {
  type ConvertRefs,
  extForTag,
  renderConvertDialog as renderConvertDialogShared,
  wireConvertDialog,
} from "../convert-dialog.js";

type FsHandle = OpenedFile["handle"];

// Workspace version stamped by esbuild (see build.mjs `define`); falls back to
// "dev" when bundled without the define.
declare const __APP_VERSION__: string;
const APP_VERSION =
  typeof __APP_VERSION__ !== "undefined" ? __APP_VERSION__ : "dev";

// Built-in welcome sample — identical content to the desktop UI (`web/ui.ts`
// SAMPLES), so both surfaces boot the same tree. All three carry the *same* tree
// (identical keys/values/comments); only the dialect's notation and comment
// marker differ, so cycling the header pill shows one config wearing three
// outfits. The pill cycles these while the doc is the unsaved sample
// (`sampleMode`); opening or saving a real file leaves sample mode and freezes it.
const SAMPLES: Record<"toml" | "json" | "yaml", string> = {
  toml: `# 👋 Welcome to confy — a lossless editor for TOML · JSON · YAML
# Click a row to select · drag the ⠿ grip to reparent · ⌘S to save

[about]
name = "confy"
pitch = "Three config dialects, one tidy tree 🌳"
version = "${APP_VERSION}"
lossless = true    # untouched bytes round-trip byte-for-byte

[basics]
select = ["click = one", "shift-click = range", "cmd-click = toggle"]
add_child = "hover a branch, hit the ＋"
undo_redo = "z and y — we all fat-finger 🙃"

[formats]
toml = "tables, dotted keys, datetimes"
json = "// comments quietly upgrade it to JSONC"
yaml = "block + flow, plain-where-safe"

[fun]
emoji_welcome = true
brackets_collected = ["{ }", "[ ]", "< >"]
coffees_per_config = 3
`,
  json: `// 👋 Welcome to confy — a lossless editor for TOML · JSON · YAML
// Click a row to select · drag the ⠿ grip to reparent · ⌘S to save
{
  "about": {
    "name": "confy",
    "pitch": "Three config dialects, one tidy tree 🌳",
    "version": "${APP_VERSION}",
    "lossless": true    // untouched bytes round-trip byte-for-byte
  },
  "basics": {
    "select": ["click = one", "shift-click = range", "cmd-click = toggle"],
    "add_child": "hover a branch, hit the ＋",
    "undo_redo": "z and y — we all fat-finger 🙃"
  },
  "formats": {
    "toml": "tables, dotted keys, datetimes",
    "json": "// comments quietly upgrade it to JSONC",
    "yaml": "block + flow, plain-where-safe"
  },
  "fun": {
    "emoji_welcome": true,
    "brackets_collected": ["{ }", "[ ]", "< >"],
    "coffees_per_config": 3
  }
}
`,
  yaml: `# 👋 Welcome to confy — a lossless editor for TOML · JSON · YAML
# Click a row to select · drag the ⠿ grip to reparent · ⌘S to save

about:
  name: confy
  pitch: Three config dialects, one tidy tree 🌳
  version: "${APP_VERSION}"
  lossless: true    # untouched bytes round-trip byte-for-byte

basics:
  select: ["click = one", "shift-click = range", "cmd-click = toggle"]
  add_child: hover a branch, hit the ＋
  undo_redo: z and y — we all fat-finger 🙃

formats:
  toml: tables, dotted keys, datetimes
  json: "// comments quietly upgrade it to JSONC"
  yaml: block + flow, plain-where-safe

fun:
  emoji_welcome: true
  brackets_collected: ["{ }", "[ ]", "< >"]
  coffees_per_config: 3
`,
};
// Pill-cycle order.
const SAMPLE_ORDER: Array<"toml" | "json" | "yaml"> = ["toml", "json", "yaml"];

const FS_AVAILABLE = fsAccessAvailable();

// ---- module state ----
let session: Session | null = null;
let snap: SessionSnapshot | null = null;
let fileHandle: FsHandle | null = null;
let fileName: string | null = "sample";
// True while the open doc is the built-in sample (no backing file) — enables the
// format-pill cycle. Opening or saving a real file leaves sample mode.
let sampleMode = false;
let sampleFormat: "toml" | "json" | "yaml" = "toml";
let rawView = false;
let searchTimer: number | undefined;

// ---- DOM refs (cached after the shell mounts) ----
let app: HTMLElement;
let treePane: HTMLElement;
let treeEl: HTMLElement;
let rawEl: HTMLElement;
let scrim: HTMLElement;
let dpBody: HTMLElement;
let statusEl: HTMLElement;
let selBadge: HTMLElement;
let clipBadge: HTMLElement;
let searchInput: HTMLInputElement;
let fmtPill: HTMLElement;
let docNameEl: HTMLElement;
let dirtyDot: HTMLElement;
let filterBtn: HTMLElement;
let toastEl: HTMLElement;
let fabEl: HTMLElement;
const sheets: Record<string, HTMLElement> = {};

// Clipboard glyph for the paste-armed FAB (vs IC.plus when adding).
const PASTE_IC =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="8" y="3" width="8" height="4" rx="1"/><path d="M9 5H6a1 1 0 0 0-1 1v14a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V6a1 1 0 0 0-1-1h-3"/><path d="M12 11v6M9 14l3 3 3-3"/></svg>';

// ---- helpers ----
function modeTag(m: ModeView): string {
  return typeof m === "string" ? m : Object.keys(m)[0];
}
function send(i: Intent) {
  if (!session) return;
  snap = session.dispatch(i);
  render();
}
// Dispatch and return the resulting snapshot (the shared panel.ts contract reads
// `snapshot.error`). `send` already triggered the re-render.
function sendR(i: Intent): SessionSnapshot {
  send(i);
  return snap!;
}
const openKindRow = (r: ViewRow) => openKindSheet(r.path);
function pathOf(row: HTMLElement | null): Path | null {
  return row?.dataset.path ? (JSON.parse(row.dataset.path) as Path) : null;
}
function rowFor(p: Path): ViewRow | undefined {
  return snap?.rows.find((r) => pathEq(r.path, p));
}
function cursorRow(): ViewRow | undefined {
  return snap?.rows.find((r) => r.is_cursor);
}
const parentOf = (p: Path): Path => p.slice(0, -1);
function startsWith(p: Path, prefix: Path): boolean {
  if (prefix.length > p.length) return false;
  for (let i = 0; i < prefix.length; i++) {
    if (JSON.stringify(p[i]) !== JSON.stringify(prefix[i])) return false;
  }
  return true;
}
// Index of `p` among visible rows that share its parent (= core's full-child
// sequence index, since an expanded parent shows all its children). Mirrors dnd.ts.
function siblingIndex(p: Path): number {
  let i = 0;
  const par = parentOf(p);
  for (const r of snap!.rows) {
    if (r.path.length === p.length && pathEq(parentOf(r.path), par)) {
      if (pathEq(r.path, p)) return i;
      i++;
    }
  }
  return i;
}
function lastKey(p: Path): string {
  const s = p[p.length - 1] as Seg | undefined;
  if (!s) return "";
  return "Key" in s ? s.Key : `[${s.Index}]`;
}

// ---- app shell (ported from the prototype's appHTML, minus the OS-status frame
// and the add-sheet; plus a convert sheet and an external-edit modal) ----
// Desktop chrome SVGs (copied verbatim from `web/index.html`) so the toolbar /
// filter row read identically to the desktop UI; they carry `class="ic"` so the
// ported `.tbtn .ic` / `.icon-btn .ic` rules size them.
const TIC = {
  open: '<svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 7h6l2 2h10v10H3z"/></svg>',
  save: '<svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M5 3h11l3 3v15H5z"/><path d="M8 3v6h7"/></svg>',
  undo: '<svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M9 7L4 12l5 5"/><path d="M4 12h11a5 5 0 0 1 0 10h-3"/></svg>',
  redo: '<svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M15 7l5 5-5 5"/><path d="M20 12H9a5 5 0 0 0 0 10h3"/></svg>',
  theme: '<svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 12.8A9 9 0 1 1 11.2 3 7 7 0 0 0 21 12.8z"/></svg>',
  more: '<svg class="ic" viewBox="0 0 24 24" fill="currentColor" stroke="none"><circle cx="5" cy="12" r="1.7"/><circle cx="12" cy="12" r="1.7"/><circle cx="19" cy="12" r="1.7"/></svg>',
  search: '<svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/></svg>',
  close: '<svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M6 6l12 12M18 6 6 18"/></svg>',
  filter: '<svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 5h18l-7 8v6l-4 2v-8z"/></svg>',
  expand: '<svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M7 10l5 5 5-5"/><path d="M7 4l5 5 5-5"/></svg>',
  collapse: '<svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M7 14l5-5 5 5"/><path d="M7 20l5-5 5 5"/></svg>',
};

function appHTML(): string {
  return (
    '<div class="app">' +
    // ---- toolbar (mirrors desktop index.html) ----
    '<header class="toolbar">' +
    '<div class="brand"><span class="logo">cy</span><span class="doc-name"></span></div>' +
    '<button class="fmt-pill" data-act="cyclefmt" title="document format"></button>' +
    '<span class="dirty-dot"></span>' +
    '<span class="spacer"></span>' +
    `<button class="tbtn" data-act="open" title="Open file">${TIC.open}<span class="label-hide">Open</span></button>` +
    `<button class="tbtn primary" data-act="save" title="Save / Convert…">${TIC.save}<span class="label-hide">Save</span></button>` +
    '<div class="tgroup edit-grp">' +
    `<button class="icon-btn" data-act="undo" title="Undo">${TIC.undo}</button>` +
    `<button class="icon-btn" data-act="redo" title="Redo">${TIC.redo}</button>` +
    `<button class="icon-btn" data-act="theme" title="Toggle theme">${TIC.theme}</button>` +
    "</div>" +
    `<button class="tbtn more-btn" data-act="menu" title="More actions">${TIC.more}</button>` +
    "</header>" +
    // ---- filter row (mirrors desktop index.html) ----
    '<div class="filterbar">' +
    `<div class="search">${TIC.search}` +
    '<input type="search" placeholder="search keys or values…" autocomplete="off" spellcheck="false" />' +
    `<button class="clear" data-act="searchclear" title="clear">${TIC.close}</button></div>` +
    `<button class="tbtn tf-btn" data-act="filter" title="Type filter">${TIC.filter}<span class="label-hide">Type filter</span><span class="dot"></span></button>` +
    '<div class="tgroup nav-grp">' +
    `<button class="icon-btn" data-act="expandall" title="Expand all">${TIC.expand}</button>` +
    `<button class="icon-btn" data-act="collapseall" title="Collapse all">${TIC.collapse}</button>` +
    "</div>" +
    // Single toggle button (label = the view it switches TO); folds into ⋯.
    '<div class="tgroup viewtabs">' +
    '<button class="tbtn viewtoggle" data-act="toggleview" title="Toggle Tree / Raw view">Raw</button>' +
    "</div>" +
    "</div>" +
    '<div class="body">' +
    '<div class="tree-pane"><div class="tree"></div></div>' +
    '<pre class="raw-view"></pre>' +
    '<div class="splitter" data-splitter></div>' +
    '<div class="detail-pane"><div class="dp-head"><h3>Node detail</h3></div>' +
    '<div class="dp-body"><div class="dp-empty">Tap any node<br>to edit its value and metadata here</div></div></div>' +
    "</div>" +
    '<div class="statusbar"><span class="status">ready</span>' +
    '<span class="badge sel-badge">none</span><span class="badge clip-badge">clipboard 0</span></div>' +
    `<button class="fab" data-act="add" aria-label="add node">${IC.plus}</button>` +
    // Small ✕ floating above the paste FAB — clears the clipboard / exits paste
    // mode (shown only while armed, via `.app.paste-mode`).
    `<button class="fab-clear" data-act="pastecancel" aria-label="exit paste mode">${IC.close}</button>` +
    '<div class="toast"></div>' +
    '<div class="scrim" data-act="scrim"></div>' +
    '<div class="sheet detail-sheet"></div>' +
    '<div class="sheet menu-sheet"></div>' +
    '<div class="sheet filter-sheet"></div>' +
    '<div class="sheet kind-sheet"></div>' +
    // Save / Convert sheet (shared form via convert-dialog.ts, hosted in a bottom
    // sheet like every other touch panel; the #conv* children match the refs).
    '<div class="sheet convert-sheet">' +
    '<div class="grab"></div>' +
    `<div class="sheet-head"><h3>Save / Convert</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
    '<div class="sheet-body">' +
    '<p class="dlg-sub">Save a copy in the current format, or convert the whole tree to another.</p>' +
    '<div class="field"><label for="convFmt">Format</label>' +
    '<select id="convFmt"><option value="Toml">TOML</option><option value="Json">JSON</option><option value="Yaml">YAML</option></select></div>' +
    '<div class="field"><label for="convPath">Output path</label>' +
    '<input id="convPath" type="text" /></div>' +
    '<div class="warns hide" id="convWarns"></div>' +
    '<div class="row-btns"><button class="btn" id="convCancel">Cancel</button>' +
    '<button class="btn primary" id="convRun">Convert &amp; save</button></div>' +
    "</div></div>" +
    // external-edit sheet (multi-line value / comment) — built on demand by
    // `openExternalEdit` (a touch-native bottom sheet, NOT the desktop modal).
    '<div class="sheet ext-sheet"></div>' +
    "</div>"
  );
}

// ---- toast ----
let toastT: number | undefined;
function toast(msg: string) {
  toastEl.textContent = msg;
  toastEl.classList.add("show");
  clearTimeout(toastT);
  toastT = window.setTimeout(() => toastEl.classList.remove("show"), 1600);
}

// ---- sheets ----
function openSheet(name: string) {
  Object.keys(sheets).forEach((k) => {
    if (k !== name) sheets[k].classList.remove("open");
  });
  scrim.classList.add("show");
  sheets[name].classList.add("open");
}
function closeSheets() {
  scrim.classList.remove("show");
  Object.keys(sheets).forEach((k) => sheets[k].classList.remove("open"));
}
function isWide(): boolean {
  return app.clientWidth >= 600;
}
// Tree ↔ Raw view toggle. The button label/active state is reflected in render();
// reused by the toggle button and the folded ⋯ menu item.
function setRawView(raw: boolean) {
  rawView = raw;
  render();
}

// ---- render ----
function render() {
  if (!snap || !session) return;
  fmtPill.textContent = snap.doc_format.toUpperCase();
  fmtPill.classList.toggle("toggleable", sampleMode);
  fmtPill.title = sampleMode ? "Sample — tap to switch format" : "document format";
  docNameEl.textContent = fileName ?? "config";
  dirtyDot.style.opacity = snap.is_dirty ? "1" : "0";
  statusEl.textContent = snap.error ? snap.error : snap.status ?? "ready";
  const cur = cursorRow();
  selBadge.textContent = cur && cur.path.length ? lastKey(cur.path) : "none";
  const armed = (snap.clipboard_count ?? 0) > 0;
  clipBadge.textContent = `clipboard ${snap.clipboard_count ?? 0}`;
  clipBadge.classList.toggle("armed", armed);
  // Paste mode: the clipboard freezes the source selection — de-emphasize it and
  // show the cursor row as the live paste target instead (CSS keys off this class).
  app.classList.toggle("paste-mode", armed);
  // Paste-armed FAB: paste glyph + copy/cut accent (tap pastes; see "add" case).
  fabEl.classList.toggle("paste-copy", armed && !snap.clipboard_cut);
  fabEl.classList.toggle("paste-cut", armed && snap.clipboard_cut);
  fabEl.innerHTML = armed ? PASTE_IC : IC.plus;
  fabEl.setAttribute("aria-label", armed ? "paste" : "add node");
  // View toggle: label is the view tapping switches TO; `active` while in Raw.
  const vt = app.querySelector<HTMLElement>(".viewtoggle");
  if (vt) {
    vt.textContent = rawView ? "Tree" : "Raw";
    vt.classList.toggle("active", rawView);
  }

  if (rawView) {
    rawEl.textContent = session.serialize();
    app.classList.add("raw");
  } else {
    // Preserve the tree scroll position across the full innerHTML rebuild —
    // otherwise every tap (re-render) snaps the pane back to the top.
    const st = treePane.scrollTop;
    treeEl.innerHTML = treeHTML(snap);
    treePane.scrollTop = st;
    // The rebuild detaches any swipe-opened row — drop the stale reference.
    openSwipeMain = null;
    app.classList.remove("raw");
  }

  // Persistent side-pane detail (≥600px). Narrow uses the detail sheet on
  // double-tap. The shared panel.ts renders/wires the body identically to desktop.
  if (isWide() && !rawView) {
    if (cur && cur.path.length) {
      dpBody.innerHTML = panelHTML(cur);
      wirePanel(dpBody, cur, sendR, openKindRow, toast, afterPanelMutation);
    } else {
      dpBody.innerHTML = '<div class="dp-empty">Tap any node<br>to edit its value and metadata here</div>';
    }
  }

  // Mode-driven surfaces: TypeFilter → the shared grid in the filter sheet;
  // Convert → the shared native dialog (no scrim/sheet).
  const tag = modeTag(snap.mode);
  if (tag === "TypeFilter") renderFilterSheet((snap.mode as { TypeFilter: TypeFilterView }).TypeFilter);
  else sheets.filter.classList.remove("open");
  if (tag === "Convert") renderConvertDialogShared(convRefs(), (snap.mode as { Convert: ConvertView }).Convert, snap);
  else if (sheets.convert.classList.contains("open")) closeSheets();
  if (tag !== "TypeFilter" && !anySheetOpen()) scrim.classList.remove("show");

  // Active type-filter indicator on the funnel button.
  filterBtn.classList.toggle("on", typeFilterActive(snap.mode));

  // Async host I/O the snapshot requested.
  if (snap.external_edit) openExternalEdit(snap.external_edit);
  if (snap.convert_write) void doConvertWrite(snap.convert_write[0], snap.convert_write[1]);
}
function anySheetOpen(): boolean {
  return Object.keys(sheets).some((k) => sheets[k].classList.contains("open"));
}
function typeFilterActive(m: ModeView): boolean {
  // Reflect persisted filter: rows are filtered when fewer show than the doc has;
  // cheaper proxy — the funnel stays lit while the TypeFilter mode reports active.
  if (typeof m === "object" && "TypeFilter" in m) return m.TypeFilter.active;
  return filterBtn.classList.contains("on");
}

// ---- selection / detail panel ----
// Single tap = select only (cursor + selection); the wide-mode side pane
// reactively shows it. The detail sheet opens on double-tap (openPanel).
function selectOnly(path: Path) {
  send({ SetCursor: path });
  send({ SetSelection: { paths: [path] } });
}
// After a successful panel Delete / Copy / Cut: confirm via a toast and dismiss
// the detail sheet (a no-op in wide split-pane mode, which re-tracks the new
// cursor on the next render).
function afterPanelMutation(msg: string) {
  toast(msg);
  closeSheets();
}
// Double-tap (narrow) opens the bottom-sheet panel. Wide mode keeps the
// persistent side pane (render() refreshed it), so no sheet is needed.
function openPanel(path: Path) {
  selectOnly(path);
  const r = rowFor(path);
  if (!r) return;
  if (!isWide()) {
    // A comment node fills `key` with the whole (possibly multi-line) comment text,
    // which would blow up the title — use a fixed label; otherwise the node key.
    // The `.sheet-head h3` CSS truncates a long key to one line (ellipsis).
    const title = r.type_label === "comment" ? "Comment" : r.key || lastKey(path);
    sheets.detail.innerHTML =
      '<div class="grab"></div>' +
      `<div class="sheet-head"><h3>${esc(title)}</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
      `<div class="sheet-body detail-wrap">${panelHTML(r)}</div>`;
    wirePanel(sheets.detail, r, sendR, openKindRow, toast, afterPanelMutation);
    openSheet("detail");
  }
}

// ---- kind sheet (from session.kindOptions) ----
function openKindSheet(path: Path) {
  if (!session) return;
  send({ SetCursor: path });
  const opts = session.kindOptions(path);
  if (!opts.length) {
    toast("No notation switches available");
    return;
  }
  const cells = opts
    .map(
      (o) =>
        `<button class="add-cell kind-opt" data-target="${esc(o.target)}"><span class="dotc" style="background:var(--accent)"></span>${esc(o.label)}</button>`,
    )
    .join("");
  sheets.kind.innerHTML =
    '<div class="grab"></div>' +
    `<div class="sheet-head"><h3>Switch kind — ${esc(lastKey(path))}</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
    `<div class="sheet-body"><div class="addgrid">${cells}</div></div>`;
  sheets.kind.querySelectorAll<HTMLElement>(".kind-opt").forEach((b) => {
    b.addEventListener("click", () => {
      const target = b.dataset.target!;
      closeSheets();
      send({ CommitKind: { path, target } });
      toast("Kind changed");
    });
  });
  openSheet("kind");
}

// ---- menu sheet ----
function mi(ic: string, label: string, sc: string, id: string): string {
  return `<button class="menu-item" data-mi="${id}"><span class="ic">${ic}</span>${label}${sc ? `<span class="sc">${sc}</span>` : ""}</button>`;
}
// The collapsible toolbar/filter controls, in display order. The ⋯ menu lists
// only the ones currently folded away (their toolbar control is hidden), so it
// tracks the responsive breakpoints instead of hardcoding a fixed set. Each item
// names its toolbar selector + the action to run when picked.
const MENU_CANDIDATES: Array<{ sel: string; ic: string; label: string; run: () => void }> = [
  { sel: '[data-act="undo"]', ic: IC.undo, label: "Undo", run: () => send("Undo") },
  { sel: '[data-act="redo"]', ic: IC.redo, label: "Redo", run: () => send("Redo") },
  { sel: '[data-act="theme"]', ic: IC.sun, label: "Toggle light / dark", run: toggleTheme },
  { sel: '[data-act="expandall"]', ic: IC.expand, label: "Expand all", run: () => send("ExpandAll") },
  { sel: '[data-act="collapseall"]', ic: IC.collapse, label: "Collapse all", run: () => send("CollapseAll") },
  { sel: '[data-act="toggleview"]', ic: IC.open, label: "Toggle Tree / Raw view", run: () => setRawView(!rawView) },
];
// A toolbar control is "folded" (→ belongs in the menu) when it's not laid out
// (its group is display:none, so offsetParent is null).
function isFolded(sel: string): boolean {
  const el = app.querySelector<HTMLElement>(sel);
  return !!el && el.offsetParent === null;
}
function openMenuSheet() {
  const folded = MENU_CANDIDATES.filter((c) => isFolded(c.sel));
  sheets.menu.innerHTML =
    '<div class="grab"></div>' +
    `<div class="sheet-head"><h3>More actions</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
    '<div class="sheet-body">' +
    folded.map((c, i) => mi(c.ic, c.label, "", String(i))).join("") +
    "</div>";
  sheets.menu.querySelectorAll<HTMLElement>(".menu-item").forEach((it) => {
    it.addEventListener("click", () => {
      const c = folded[Number(it.dataset.mi)];
      if (c.run !== toggleTheme) closeSheets();
      c.run();
    });
  });
  openSheet("menu");
}

// ---- type-filter sheet (driven by snapshot.mode TypeFilterView) ----
// The grid markup + per-cell wiring is shared with the desktop UI
// (`typefilter.ts`); the sheet shell (grab / head) + open-close logic and the
// funnel `.on` indicator stay here. No "Done" button — the grid toggles live and
// has its own ✕ clear; the sheet closes via grab / scrim / header ×.
function renderFilterSheet(grid: TypeFilterView) {
  // Preserve the body's scroll position — toggling a cell re-renders the whole
  // sheet, which would otherwise snap a scrolled grid back to the top.
  const prevBody = sheets.filter.querySelector<HTMLElement>(".sheet-body");
  const st = prevBody ? prevBody.scrollTop : 0;
  sheets.filter.innerHTML =
    '<div class="grab"></div>' +
    `<div class="sheet-head"><h3>Type filter</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
    `<div class="sheet-body tf-body"><div class="tf">${typeFilterHTML(grid)}</div></div>`;
  wireTypeFilter(sheets.filter, grid, { send });
  const body = sheets.filter.querySelector<HTMLElement>(".sheet-body");
  if (body) body.scrollTop = st;
  if (!sheets.filter.classList.contains("open")) openSheet("filter");
}

// The Save/Convert form's five children plus a sheet-backed `ConvertSurface`, so
// the shared convert-dialog module drives a bottom sheet here (vs the desktop
// `<dialog>`). Dismiss (scrim / grab / ×) routes through `dismissSheets`, which
// sends `ExitConvert` to peel core's Convert mode.
function convRefs(): ConvertRefs {
  return {
    surface: {
      isOpen: () => sheets.convert.classList.contains("open"),
      open: () => openSheet("convert"),
      close: () => closeSheets(),
      onCancel: () => {
        /* sheet dismissal is handled by dismissSheets → ExitConvert */
      },
    },
    fmt: document.getElementById("convFmt") as HTMLSelectElement,
    path: document.getElementById("convPath") as HTMLInputElement,
    warns: document.getElementById("convWarns")!,
    run: document.getElementById("convRun")!,
    cancel: document.getElementById("convCancel")!,
  };
}

// ---- external edit (multi-line value/comment): a dedicated touch bottom sheet
// (built fresh per session, styled like the other sheets — NOT the desktop modal).
// Guard: while the sheet is already open for this session, render() re-calls this
// every snapshot — return early so the textarea/buttons aren't clobbered mid-edit.
function openExternalEdit(ext: { initial: string; kind: unknown }) {
  if (sheets.ext.classList.contains("open")) return;
  const kind = ext.kind as { Value?: { path: Path }; Comment?: { path: Path } };
  const isComment = !!kind.Comment;
  const path = (kind.Value ?? kind.Comment)!.path;
  const title = isComment ? "Edit comment" : `Edit ${esc(lastKey(path) || "value")}`;
  sheets.ext.innerHTML =
    '<div class="grab"></div>' +
    `<div class="sheet-head"><h3>${title}</h3><button class="close" data-act="extcancel">${IC.close}</button></div>` +
    '<div class="sheet-body">' +
    '<textarea class="ext-text" spellcheck="false" autocomplete="off" autocapitalize="off"></textarea>' +
    '<div class="row-btns"><button class="btn" data-act="extcancel">Cancel</button>' +
    '<button class="btn primary ext-apply">Apply</button></div>' +
    "</div>";
  const txt = sheets.ext.querySelector<HTMLTextAreaElement>(".ext-text")!;
  txt.value = ext.initial;
  // Apply is wired directly (no data-act) so the shell delegation never double-fires.
  sheets.ext.querySelector<HTMLElement>(".ext-apply")!.onclick = () => {
    closeSheets();
    if (kind.Value) send({ ApplyReplace: { path, text: txt.value } });
    else send({ ApplyEditComment: { path, text: txt.value } });
  };
  openSheet("ext");
  txt.focus();
}

// ---- grip reorder + tap (pointer flow; horizontal swipe removed) ----
// Tap-vs-scroll tracking on a row (grip-drag reorder is handled separately).
let sx = 0,
  sy = 0,
  dragRow: HTMLElement | null = null,
  dragging = false,
  moved = false;
// Double-tap detection (item 6): same path within DOUBLE_TAP_MS opens the panel.
let lastTapKey: string | null = null;
let lastTapTime = 0;
const DOUBLE_TAP_MS = 300;

// Swipe-to-delete: a horizontal left-swipe on a row's `.row-main` slides it open
// to reveal a single Delete action (`.row-del`). One row is open at a time.
let swiping = false;
let swipeMain: HTMLElement | null = null;
let swipeBase = 0;
let swipeOff = 0;
let openSwipeMain: HTMLElement | null = null;
const SWIPE_W = 96;

// reorder state
let reordering = false;
let reRow: HTMLElement | null = null;
let reStartY = 0;
let reMoved = false;
let reTarget: HTMLElement | null = null;
let reMode: "before" | "after" | "into" = "before";
let reInto: HTMLElement | null = null;
let reLine: HTMLElement | null = null;
let reSrcPath: Path | null = null;

function clearInto() {
  if (reInto) {
    reInto.classList.remove("drop-into");
    reInto = null;
  }
}
function startReorder(e: PointerEvent, row: HTMLElement) {
  reordering = true;
  reMoved = false;
  reRow = row;
  reTarget = null;
  reMode = "before";
  reSrcPath = pathOf(row);
  reStartY = e.clientY;
  reLine = treeEl.querySelector(".reorder-line");
  row.classList.add("dragging");
  try {
    treeEl.setPointerCapture(e.pointerId);
  } catch (_) {
    /* ignore */
  }
}
function onReorderMove(y: number) {
  if (!reLine || !reSrcPath) return;
  if (Math.abs(y - reStartY) < 6 && !reMoved) {
    reLine.style.display = "none";
    clearInto();
    return;
  }
  reMoved = true;
  // Candidates: visible rows that are neither the dragged row nor its descendants.
  const rows = Array.prototype.filter.call(
    treeEl.querySelectorAll<HTMLElement>(".row"),
    (r: HTMLElement) => {
      const p = pathOf(r);
      return p !== null && !startsWith(p, reSrcPath!) && r.offsetHeight > 0;
    },
  ) as HTMLElement[];
  if (!rows.length) {
    reTarget = null;
    reLine.style.display = "none";
    clearInto();
    return;
  }
  let hit: HTMLElement | null = null,
    nearest: HTMLElement | null = null,
    nd = Infinity;
  for (const r of rows) {
    const rect = r.getBoundingClientRect();
    if (y >= rect.top && y <= rect.bottom) {
      hit = r;
      break;
    }
    const d = y < rect.top ? rect.top - y : y - rect.bottom;
    if (d < nd) {
      nd = d;
      nearest = r;
    }
  }
  let resolved = false;
  if (!hit) {
    hit = nearest!;
    const nr = hit.getBoundingClientRect();
    reMode = y < (nr.top + nr.bottom) / 2 ? "before" : "after";
    resolved = true;
  }
  const hr = hit.getBoundingClientRect();
  if (!resolved) {
    const isBranch = hit.classList.contains("branch");
    const rel = (y - hr.top) / (hr.height || 1);
    if (isBranch) reMode = rel < 0.28 ? "before" : rel > 0.72 ? "after" : "into";
    else reMode = rel < 0.5 ? "before" : "after";
  }
  reTarget = hit;
  const treeTop = treeEl.getBoundingClientRect().top;
  if (reMode === "into") {
    reLine.style.display = "none";
    if (reInto !== hit) {
      clearInto();
      reInto = hit;
      hit.classList.add("drop-into");
    }
  } else {
    clearInto();
    reLine.style.display = "block";
    reLine.style.top = (reMode === "before" ? hr.top - treeTop : hr.bottom - treeTop) + "px";
  }
}
function endReorder() {
  reordering = false;
  if (reLine) reLine.style.display = "none";
  clearInto();
  if (reRow) reRow.classList.remove("dragging");
  if (reMoved && reTarget && reSrcPath) {
    const tgtPath = pathOf(reTarget);
    if (tgtPath && !pathEq(tgtPath, reSrcPath)) {
      const sources = [reSrcPath];
      if (reMode === "into") {
        const idx = rowFor(tgtPath)?.child_count ?? 0;
        send({ MoveSelectionTo: { sources, target: tgtPath, index: idx } });
      } else {
        const sib = siblingIndex(tgtPath);
        send({
          MoveSelectionTo: {
            sources,
            target: parentOf(tgtPath),
            index: reMode === "after" ? sib + 1 : sib,
          },
        });
      }
    }
  }
  reRow = null;
  reTarget = null;
  reMoved = false;
  reMode = "before";
  reSrcPath = null;
}

function installTreeGestures() {
  treeEl.addEventListener("pointerdown", (e) => {
    const grip = (e.target as HTMLElement).closest<HTMLElement>(".drag-handle");
    if (grip) {
      const row = grip.closest<HTMLElement>(".row");
      if (row) startReorder(e, row);
      return;
    }
    const t = e.target as HTMLElement;
    // A tap can land on the visible row-main OR (when already swiped open) on the
    // revealed `.row-del` behind it — both map to the same row.
    const main = t.closest<HTMLElement>(".row-main") ?? t.closest<HTMLElement>(".row-del");
    const rowEl = main?.closest<HTMLElement>(".row");
    if (!rowEl) return;
    dragRow = rowEl;
    // Swipe only when the row carries a Delete action (read-only rows don't).
    swipeMain = rowEl.querySelector<HTMLElement>(".row-del") ? rowEl.querySelector<HTMLElement>(".row-main") : null;
    swipeBase = swipeMain && openSwipeMain === swipeMain ? -SWIPE_W : 0;
    sx = e.clientX;
    sy = e.clientY;
    dragging = true;
    moved = false;
    swiping = false;
  });
  treeEl.addEventListener("pointermove", (e) => {
    if (reordering) {
      e.preventDefault();
      onReorderMove(e.clientY);
      return;
    }
    if (!dragging || !dragRow) return;
    const dx = e.clientX - sx;
    const dy = e.clientY - sy;
    // Lock the axis once the gesture is decisive: horizontal → swipe, vertical →
    // a scroll (which also cancels the pending tap).
    if (!swiping && !moved) {
      if (swipeMain && Math.abs(dx) > 8 && Math.abs(dx) > Math.abs(dy)) {
        swiping = true;
        if (openSwipeMain && openSwipeMain !== swipeMain) {
          openSwipeMain.style.transform = "";
          openSwipeMain = null;
        }
        if (swipeMain) swipeMain.style.transition = "none";
      } else if (Math.abs(dy) > 8) {
        moved = true;
      }
    }
    if (swiping && swipeMain) {
      e.preventDefault();
      swipeOff = Math.max(-SWIPE_W, Math.min(0, swipeBase + dx));
      swipeMain.style.transform = `translateX(${swipeOff}px)`;
    }
  });
  treeEl.addEventListener("pointerup", (e) => {
    if (reordering) {
      endReorder();
      return;
    }
    if (swiping && swipeMain) {
      // Snap open / closed past the halfway point (CSS transition animates it).
      swipeMain.style.transition = "";
      const open = swipeOff < -SWIPE_W / 2;
      swipeMain.style.transform = open ? `translateX(${-SWIPE_W}px)` : "";
      openSwipeMain = open ? swipeMain : null;
    } else if (dragging && dragRow && !moved) {
      handleTap(e.target as HTMLElement, dragRow);
    }
    dragging = false;
    dragRow = null;
    swiping = false;
    swipeMain = null;
  });
  treeEl.addEventListener("pointercancel", () => {
    if (reordering) {
      endReorder();
      return;
    }
    if (swiping && swipeMain) {
      swipeMain.style.transition = "";
      swipeMain.style.transform = openSwipeMain === swipeMain ? `translateX(${-SWIPE_W}px)` : "";
    }
    dragging = false;
    dragRow = null;
    swiping = false;
    swipeMain = null;
  });
}

// Single tap = select only; double tap (same row within DOUBLE_TAP_MS) opens the
// panel. The caret toggles expand; the kind badge now behaves like a normal tap
// (kind switching lives inside the edit panel).
function handleTap(target: HTMLElement, row: HTMLElement) {
  const path = pathOf(row);
  if (!path) return;
  const actBtn = target.closest<HTMLElement>("[data-act]");
  if (actBtn) {
    const act = actBtn.dataset.act;
    if (act === "grip") return;
    // Revealed Delete (swipe-to-delete): remove this row, then re-render closes it.
    if (act === "rowdel") {
      openSwipeMain = null;
      send({ SetCursor: path });
      send({ SetSelection: { paths: [path] } });
      const after = sendR("DeleteSelected");
      toast(after.error ?? "Deleted");
      return;
    }
    if (act === "caret") {
      send({ SetCursor: path });
      send("ToggleExpand");
      return;
    }
  }
  // A tap while a row is swiped open just closes it (no selection change).
  if (openSwipeMain) {
    const wasOpen = openSwipeMain;
    openSwipeMain.style.transform = "";
    openSwipeMain = null;
    if (wasOpen === row.querySelector(".row-main")) return;
  }
  const key = JSON.stringify(path);
  const now = Date.now();
  const isDouble = key === lastTapKey && now - lastTapTime < DOUBLE_TAP_MS;
  lastTapKey = key;
  lastTapTime = now;
  if (isDouble) openPanel(path);
  // In paste mode the clipboard freezes the selection, so a tap only moves the
  // cursor (= the paste target); `.app.paste-mode .row.cursor` highlights it.
  else if ((snap?.clipboard_count ?? 0) > 0) send({ SetCursor: path });
  else selectOnly(path);
}

// ---- context-aware add (FAB) ----
// On an expanded branch → AddChild; on a scalar or collapsed branch → AddSibling.
// No cursor row → fall back to AddNode (the cursor-relative default).
function addContextual() {
  if (!snap) return;
  const idx = snap.rows.findIndex((r) => r.is_cursor);
  if (idx < 0) {
    send("AddNode");
    toast("Node added");
    return;
  }
  const r = snap.rows[idx];
  if (r.is_branch && isExpanded(snap.rows, idx)) {
    send("AddChild");
    toast("Child added");
  } else {
    send("AddSibling");
    toast("Sibling added");
  }
}

// ---- theme ----
type Theme = "dark" | "light";
function initTheme() {
  const stored = localStorage.getItem("confy-theme");
  document.documentElement.dataset.theme = stored === "light" ? "light" : "dark";
}
function toggleTheme() {
  const cur = document.documentElement.dataset.theme === "light" ? "light" : "dark";
  const next: Theme = cur === "dark" ? "light" : "dark";
  localStorage.setItem("confy-theme", next);
  document.documentElement.dataset.theme = next;
}

// ---- sample doc ----
// Load the built-in sample in `format`, entering sample mode (pill toggle on).
function loadSample(format: "toml" | "json" | "yaml") {
  sampleFormat = format;
  openText(SAMPLES[format], format, null, "sample", true);
}
// Cycle the sample doc to the next backend (pill tap while in sample mode).
function cycleSampleFormat() {
  if (!sampleMode) return;
  const next = SAMPLE_ORDER[(SAMPLE_ORDER.indexOf(sampleFormat) + 1) % SAMPLE_ORDER.length];
  loadSample(next);
}

// ---- file I/O (host-owned, via fs.ts) ----
function openText(
  text: string,
  format: "toml" | "json" | "yaml" | "yml",
  handle: FsHandle | null,
  name: string | null,
  asSample = false,
) {
  session?.free();
  try {
    session = Session.fromText(text, format);
  } catch (e) {
    statusEl.textContent = String((e as Error).message ?? e);
    return;
  }
  fileHandle = handle;
  fileName = name;
  sampleMode = asSample;
  snap = session.snapshot();
  rawView = false;
  render();
}
function formatFromName(name: string): "toml" | "json" | "yaml" {
  return name.endsWith(".json") || name.endsWith(".jsonc")
    ? "json"
    : name.endsWith(".yaml") || name.endsWith(".yml")
      ? "yaml"
      : "toml";
}
async function doOpen() {
  if (FS_AVAILABLE) {
    const opened = await pickOpenFile();
    if (!opened) return;
    openText(opened.text, formatFromName(opened.name), opened.handle, opened.name);
    return;
  }
  const input = document.getElementById("fileInput") as HTMLInputElement;
  input.value = "";
  input.onchange = async () => {
    const file = input.files?.[0];
    if (!file) return;
    const text = await file.text();
    openText(text, formatFromName(file.name), null, file.name);
  };
  input.click();
}
// File stem (no dir, no extension) for seeding the convert dialog's output path.
function fileStem(): string {
  const base = (fileName ?? "config").split("/").pop()!;
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(0, dot) : base;
}
// Open the unified Save / Convert panel from the root node (mirrors desktop):
// `OpenConvert` leaves `target` = the current format (a plain save-as default);
// seed the output name from the open file's stem.
function openSaveConvert() {
  send({ SetCursor: [] });
  send("OpenConvert");
  send({ SetConvertPath: fileStem() + extForTag(snap?.doc_format ?? "Toml") });
}
// Faithful "save a copy" of the live document (byte-for-byte `serialize()`),
// used when the panel's format equals the open format.
async function doSaveAsCopy(path: string) {
  if (!session || !snap) return;
  const text = session.serialize();
  const fmt = snap.doc_format;
  const baseName = path.split("/").pop() || "confy-export" + extFor(fmt);
  send("ExitConvert");
  if (FS_AVAILABLE) {
    const handle = await pickSaveFile(fmt, baseName);
    if (!handle) return;
    try {
      await writeFile(handle, text);
      toast(`Saved copy → ${(await handle.getFile()).name}`);
    } catch (e) {
      statusEl.textContent = `save failed: ${String((e as Error).message ?? e)}`;
    }
    return;
  }
  downloadText(baseName, text);
  toast("Downloaded");
}
async function doConvertWrite(path: string, text: string) {
  closeSheets();
  const baseName = path.split("/").pop() ?? "confy-converted";
  if (FS_AVAILABLE) {
    const fmt = snap?.doc_format ?? "Toml";
    const outExt = extFor(path.endsWith(".json") ? "Json" : path.endsWith(".yaml") || path.endsWith(".yml") ? "Yaml" : "Toml");
    const handle = await pickSaveFile(fmt, baseName.endsWith(outExt) ? baseName : baseName + outExt);
    if (!handle) return;
    try {
      await writeFile(handle, text);
      toast(`Converted → ${(await handle.getFile()).name}`);
    } catch (e) {
      statusEl.textContent = `convert write failed: ${String((e as Error).message ?? e)}`;
    }
    return;
  }
  downloadText(baseName, text);
  toast("Converted (downloaded)");
}

// ---- shell-level click delegation (toolbar / footer / scrim / sheets) ----
function installShellHandlers() {
  app.addEventListener("click", (e) => {
    const b = (e.target as HTMLElement).closest<HTMLElement>("[data-act]");
    if (!b) return;
    if (treeEl.contains(b)) return; // tree handled by the pointer flow
    const act = b.dataset.act;
    switch (act) {
      case "menu":
        openMenuSheet();
        break;
      case "filter":
        send("EnterTypeFilter");
        break;
      case "open":
        void doOpen();
        break;
      case "add":
        // Paste-armed (after Copy/Cut) → the FAB pastes at the cursor; otherwise
        // it adds a node contextually.
        if ((snap?.clipboard_count ?? 0) > 0) send("Paste");
        else addContextual();
        break;
      case "cyclefmt":
        cycleSampleFormat(); // no-op unless in sample mode
        break;
      case "save":
        openSaveConvert();
        break;
      case "undo":
        send("Undo");
        break;
      case "redo":
        send("Redo");
        break;
      case "theme":
        toggleTheme();
        break;
      case "expandall":
        send("ExpandAll");
        break;
      case "collapseall":
        send("CollapseAll");
        break;
      case "scrim":
      case "closesheet":
        dismissSheets();
        break;
      case "extcancel":
        closeSheets();
        send("Escape");
        break;
      case "pastecancel":
        send("Escape"); // clear clipboard / exit paste mode
        break;
      case "toggleview":
        setRawView(!rawView);
        break;
      case "searchclear":
        searchInput.value = "";
        searchInput.parentElement!.classList.remove("has-val");
        send({ SetFilter: "" });
        break;
    }
  });

  // Search → debounced SetFilter.
  searchInput.addEventListener("input", () => {
    searchInput.parentElement!.classList.toggle("has-val", !!searchInput.value);
    clearTimeout(searchTimer);
    searchTimer = window.setTimeout(() => send({ SetFilter: searchInput.value }), 180);
  });

  // Sheet drag-to-dismiss (grab handle / header).
  Object.keys(sheets).forEach((name) => {
    const sheet = sheets[name];
    let sy0 = 0,
      dy = 0,
      drag = false;
    sheet.addEventListener("pointerdown", (e) => {
      if (!(e.target as HTMLElement).closest(".grab") && !(e.target as HTMLElement).closest(".sheet-head")) return;
      drag = true;
      sy0 = e.clientY;
      dy = 0;
      sheet.style.transition = "none";
    });
    sheet.addEventListener("pointermove", (e) => {
      if (!drag) return;
      dy = Math.max(0, e.clientY - sy0);
      sheet.style.transform = `translateY(${dy}px)`;
    });
    const end = () => {
      if (!drag) return;
      drag = false;
      sheet.style.transition = "";
      sheet.style.transform = "";
      if (dy > 90) dismissSheets();
    };
    sheet.addEventListener("pointerup", end);
    sheet.addEventListener("pointercancel", end);
  });
}

// ---- tablet splitter (≥600px): drag the divider to resize the detail pane ----
const DETAIL_W_KEY = "confy-detail-w";
const DETAIL_W_MIN = 240;
const DETAIL_W_MAX = 520;
function restoreDetailWidth() {
  const v = Number(localStorage.getItem(DETAIL_W_KEY));
  if (v >= DETAIL_W_MIN && v <= DETAIL_W_MAX) app.style.setProperty("--detail-w", v + "px");
}
function installSplitter() {
  const sp = app.querySelector<HTMLElement>("[data-splitter]");
  if (!sp) return;
  let spDrag = false;
  sp.addEventListener("pointerdown", (e) => {
    spDrag = true;
    sp.classList.add("dragging");
    try {
      sp.setPointerCapture(e.pointerId);
    } catch (_) {
      /* ignore */
    }
    e.preventDefault();
  });
  sp.addEventListener("pointermove", (e) => {
    if (!spDrag) return;
    const w = Math.max(DETAIL_W_MIN, Math.min(DETAIL_W_MAX, app.getBoundingClientRect().right - e.clientX));
    app.style.setProperty("--detail-w", w + "px");
  });
  const end = () => {
    if (!spDrag) return;
    spDrag = false;
    sp.classList.remove("dragging");
    const cur = parseInt(app.style.getPropertyValue("--detail-w"), 10);
    if (cur) localStorage.setItem(DETAIL_W_KEY, String(cur));
  };
  sp.addEventListener("pointerup", end);
  sp.addEventListener("pointercancel", end);
}

// Dismiss whatever sheet is open. Mode-driven sheets must peel their core mode so
// the next render() doesn't immediately re-open them: TypeFilter commits, Convert
// exits, and an open external-edit sheet sends Escape (clears `external_edit`).
function dismissSheets() {
  const tag = snap ? modeTag(snap.mode) : "Normal";
  if (sheets.ext.classList.contains("open")) {
    closeSheets();
    return send("Escape");
  }
  if (tag === "TypeFilter") return send("CommitTypeFilter");
  if (tag === "Convert") return send("ExitConvert");
  closeSheets();
}

// ---- boot ----
async function main() {
  initTheme();
  const root = document.getElementById("root")!;
  root.innerHTML = appHTML();
  app = root.querySelector(".app")!;
  treePane = app.querySelector(".tree-pane")!;
  treeEl = app.querySelector(".tree")!;
  rawEl = app.querySelector(".raw-view")!;
  scrim = app.querySelector(".scrim")!;
  dpBody = app.querySelector(".dp-body")!;
  statusEl = app.querySelector(".status")!;
  selBadge = app.querySelector(".sel-badge")!;
  clipBadge = app.querySelector(".clip-badge")!;
  searchInput = app.querySelector(".search input")!;
  fmtPill = app.querySelector(".fmt-pill")!;
  docNameEl = app.querySelector(".brand .doc-name")!;
  dirtyDot = app.querySelector(".dirty-dot")!;
  filterBtn = app.querySelector(".tf-btn")!;
  toastEl = app.querySelector(".toast")!;
  fabEl = app.querySelector(".fab")!;
  // Tap the clip badge while armed → cancel the copy/cut (clears the clipboard).
  clipBadge.title = "tap to clear clipboard";
  clipBadge.addEventListener("click", () => {
    if ((snap?.clipboard_count ?? 0) > 0) send("Escape");
  });
  sheets.detail = app.querySelector(".detail-sheet")!;
  sheets.menu = app.querySelector(".menu-sheet")!;
  sheets.filter = app.querySelector(".filter-sheet")!;
  sheets.kind = app.querySelector(".kind-sheet")!;
  sheets.convert = app.querySelector(".convert-sheet")!;
  sheets.ext = app.querySelector(".ext-sheet")!;

  restoreDetailWidth();
  installTreeGestures();
  installShellHandlers();
  installSplitter();
  wireConvertDialog(convRefs(), { send, fileStem, doSaveAsCopy, getSnap: () => snap });

  const wasmUrl = new URL("../pkg/confy_ffi_bg.wasm", import.meta.url);
  await load(wasmUrl);
  loadSample("toml");
}

void main();
