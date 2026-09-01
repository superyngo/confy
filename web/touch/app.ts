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
  PromptView,
  TypeFilterView,
  ConvertView,
  PasteSlot,
  Notice,
  ActionItemView,
  AddOptionView,
} from "../types.js";
import {
  canSaveAs,
  fsAccessAvailable,
  onTauriOpened,
  openTauriPath,
  pickOpenFile,
  isFirefoxIos,
  tauriOpenedUrls,
  type OpenedFile,
} from "../fs.js";
import { createBatcher, modeTag } from "../mode.js";
import {
  doConvertWrite,
  doQuickSave,
  doSaveAsCopy,
  fileStem,
  formatFromName,
  initTheme,
  openFromUrl,
  openSaveConvert,
  replaceSession,
  resolveSchemaFetchRequest,
  toggleTheme,
  type HostIo,
} from "../host-io.js";
import {
  cycleSampleFormat,
  inSampleMode,
  loadSample,
  setSampleMode,
  type SampleFormat,
} from "../samples.js";
import { IC, esc, treeHTML } from "./render.js";
import { fabHTML, syncFab } from "../fab.js";
import { parentOf, pathEq, siblingIndex } from "../path-utils.js";
import { resolveClick, resetAnchor, type Mods } from "../select.js";
import { panelHTML, wirePanel, schemaHintText } from "../panel.js";
import { bindPromptClicks, promptButtonsHTML, promptTitle } from "../prompt.js";
import { typeFilterHTML, wireTypeFilter } from "../typefilter.js";
import { helpBodyHTML } from "../help-content.js";
import { applyStaticI18n, availableLangs, getLang, LANG_DISPLAY_NAMES, setLang, t, tArgs } from "../i18n.js";
import type { Lang } from "../i18n.js";
import { foldedEntries, type ToolbarEntry } from "../toolbar-fold.js";
import {
  type ConvertRefs,
  renderConvertDialog as renderConvertDialogShared,
  runSaveConvert as runSaveConvertShared,
  wireConvertDialog,
} from "../convert-dialog.js";
import { resolveKeyIntent, navRowCount } from "../key-intent.js";
import { actionItemHTML } from "../action-menu-items.js";

type FsHandle = OpenedFile["handle"];

// The built-in welcome sample + sample-mode state live in the shared
// `samples.ts` (identical content to the desktop UI, so both surfaces boot
// the same tree).

const FS_AVAILABLE = fsAccessAvailable();

// ---- module state ----
let session: Session | null = null;
let snap: SessionSnapshot | null = null;
let fileHandle: FsHandle | null = null;
let fileName: string | null = "sample";
// Guards `schema_fetch_request` (§comment-advisory follow-up issue #2): see
// web/ui.ts's identical flag for the rationale.
let schemaFetchInFlight = false;
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

// ---- helpers ----
// Dispatch several intents with a single re-render at the end (mirrors ui.ts).
const { batch, isBatching } = createBatcher(render);
function send(i: Intent) {
  if (!session) return;
  const preClip = snap?.clipboard_count ?? 0;
  snap = session.dispatch(i);
  // Mirrors `web/ui.ts`'s `send()` (ROW_STATE_MODEL.md §6d): a paste that just
  // landed re-selects the pasted/moved batch so it stays visibly highlighted.
  // Purely client-side and ephemeral — safe here for the same reason it's safe
  // on desktop: every ordinary tap already collapses `Selection` to a single
  // path via `selectOnly()`, so this never outlives the tap that follows it.
  if (preClip > 0 && !(snap.clipboard_count ?? 0) && snap.notice?.severity !== "error" && snap.mode === "Normal") {
    const parent = snap.cursor.slice(0, -1);
    const siblings = session.children(parent).map((c) => c.path);
    const idx = siblings.findIndex((p) => JSON.stringify(p) === JSON.stringify(snap!.cursor));
    if (idx >= 0) {
      const pasted = siblings.slice(idx, idx + preClip);
      snap = session.dispatch({ SetSelection: { paths: pasted } });
    }
  }
  if (!isBatching()) render();
}
// Dispatch and return the resulting snapshot (the shared panel.ts contract reads
// `snapshot.notice`). `send` already triggered the re-render.
function sendR(i: Intent): SessionSnapshot {
  send(i);
  return snap!;
}

// The host surface the shared I/O flows (host-io.ts) are parameterized on.
// Feedback goes to the toast (success) / status line (failure); convert-writes
// first close the open sheet; download fallbacks toast + show the FxiOS hint.
const io: HostIo = {
  fsAvailable: FS_AVAILABLE,
  canSaveAs: canSaveAs(),
  getSnap: () => snap,
  send,
  batch,
  serialize: () => session?.serialize() ?? null,
  getFileName: () => fileName,
  getHandle: () => fileHandle,
  setHandle: (h) => {
    fileHandle = h;
  },
  ok: (msg) => send({ SetHostNotice: { key: "web.host.save-ok", args: [], source: "host-web" } }),
  err: (msg) => {
    statusEl.textContent = msg;
  },
  beforeConvertWrite: () => closeSheets(),
  afterDownload: (filename, msg) => {
    send({ SetHostNotice: { key: "web.host.download-ok", args: [], source: "host-web" } });
    firefoxIosSaveHint(filename);
  },
  adoptFile: (text, format, handle, name) => openText(text, format, handle, name),
};
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
// Whether `p`'s immediate parent is a single-line container (`Format::Inline`
// — TOML inline table, JSON single-line object/array, YAML flow map/seq).
// Mirrors desktop `ui.ts`'s `parentIsInline` (see panel.ts for why).
function parentIsInline(p: Path): boolean {
  if (p.length === 0) return false;
  return rowFor(parentOf(p))?.format === "Inline";
}
function startsWith(p: Path, prefix: Path): boolean {
  if (prefix.length > p.length) return false;
  for (let i = 0; i < prefix.length; i++) {
    if (JSON.stringify(p[i]) !== JSON.stringify(prefix[i])) return false;
  }
  return true;
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
  info: '<svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="9"/><path d="M12 11v6"/><path d="M12 7.5h.01"/></svg>',
  chevron: '<svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M6 9l6 6 6-6"/></svg>',
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
    `<button class="tbtn" data-act="open" data-i18n-title="web.toolbar.open.title" title="Open file">${TIC.open}<span class="label-hide" data-i18n="web.toolbar.open.label">Open</span></button>` +
    `<button class="tbtn primary" data-act="save" data-i18n-title="web.toolbar.save.label" title="Save">${TIC.save}<span class="label-hide" data-i18n="web.toolbar.save.label">Save</span></button>` +
    '<div class="tgroup edit-grp">' +
    `<button class="icon-btn" data-act="theme" data-i18n-title="web.toolbar.theme.title" title="Toggle theme" data-foldable="true">${TIC.theme}</button>` +
    `<button class="icon-btn" data-act="lang" data-i18n-title="web.toolbar.lang.title" title="Language" data-foldable="true"><span class="lang-label"></span></button>` +
    // Single toggle button (label = the view it switches TO); folds into ⋯.
    `<button class="tbtn viewtoggle" data-act="toggleview" data-i18n-title="web.toolbar.viewToggle.title" title="Toggle Tree / Raw view" data-foldable="true">Raw</button>` +
    `<button class="icon-btn" data-act="info" data-i18n-title="web.toolbar.info.title" title="Help / About" data-foldable="true">${TIC.info}</button>` +
    "</div>" +
    `<button class="tbtn more-btn" data-act="menu" data-i18n-title="web.toolbar.more.title" title="More actions">${TIC.more}</button>` +
    "</header>" +
    // ---- filter row (mirrors desktop index.html) ----
    '<div class="filterbar">' +
    `<div class="search">${TIC.search}` +
    `<input type="search" data-i18n-placeholder="web.search.placeholder" placeholder="search keys or values…" autocomplete="off" spellcheck="false" />` +
    `<button class="clear" data-act="searchclear" data-i18n-title="web.search.clear.title" title="clear">${TIC.close}</button></div>` +
    `<button class="tbtn tf-btn" data-act="filter" data-i18n-title="web.toolbar.typefilter.title" title="Type filter">${TIC.filter}<span class="label-hide" data-i18n="web.toolbar.typefilter.label">Type filter</span><span class="dot"></span></button>` +
    '<div class="tgroup nav-grp">' +
    `<button class="icon-btn" data-act="expandall" data-i18n-title="web.toolbar.expandAll.title" title="Expand all" data-foldable="true">${TIC.expand}</button>` +
    `<button class="icon-btn" data-act="collapseall" data-i18n-title="web.toolbar.collapseAll.title" title="Collapse all" data-foldable="true">${TIC.collapse}</button>` +
    "</div>" +
    '<div class="tgroup hist-grp">' +
    `<button class="icon-btn" data-act="undo" data-i18n-title="web.toolbar.undo.title" title="Undo" data-foldable="true">${TIC.undo}</button>` +
    `<button class="icon-btn" data-act="redo" data-i18n-title="web.toolbar.redo.title" title="Redo" data-foldable="true">${TIC.redo}</button>` +
    "</div>" +
    "</div>" +
    '<div class="body">' +
    '<div class="tree-pane"><div class="tree"></div></div>' +
    '<pre class="raw-view"></pre>' +
    '<div class="splitter" data-splitter></div>' +
    `<div class="detail-pane"><div class="dp-head"><h3 data-i18n="web.detail.title">Node detail</h3></div>` +
    '<div class="dp-body"><div class="dp-empty">Tap any node<br>to edit its value and metadata here</div></div></div>' +
    // Nested inside `.body` (position:relative, sits above `.statusbar` in
    // the `.app` flex column) rather than a sibling of `.statusbar`, so
    // `.fab`'s `bottom:18px` (touch/style.css) anchors above the status bar
    // instead of the full-screen edge — mirrors desktop's `.main`-nested fix.
    fabHTML() +
    "</div>" +
    `<div class="statusbar"><span class="status" data-i18n="web.status.ready">ready</span>` +
    '<span class="badge sel-badge">none</span><span class="badge clip-badge">clipboard 0</span></div>' +
    '<div class="toast"></div>' +
    '<div class="scrim" data-act="scrim"></div>' +
    '<div class="sheet detail-sheet"></div>' +
    '<div class="sheet menu-sheet"></div>' +
    '<div class="sheet actions-sheet"></div>' +
    '<div class="sheet filter-sheet"></div>' +
    '<div class="sheet kind-sheet"></div>' +
    '<div class="sheet lang-sheet"></div>' +
    // Save action-choice sheet (tap the toolbar Save button → pick Save vs
    // Save As/Convert); built on demand by `openSaveSheet`.
    '<div class="sheet save-sheet"></div>' +
    // Save / Convert sheet (shared form via convert-dialog.ts, hosted in a bottom
    // sheet like every other touch panel; the #conv* children match the refs).
    '<div class="sheet convert-sheet">' +
    '<div class="grab"></div>' +
    `<div class="sheet-head"><h3>${t("web.convert.title")}</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
    '<div class="sheet-body">' +
    `<p class="dlg-sub">${t("web.convert.subtitle")}</p>` +
    `<div class="field"><label for="convFmt">${t("web.convert.format.label")}</label>` +
    '<select id="convFmt"><option value="Toml">TOML</option><option value="Json">JSON</option><option value="Yaml">YAML</option></select></div>' +
    `<div class="field"><label for="convPath">${t("web.convert.path.label")}</label>` +
    '<input id="convPath" type="text" /></div>' +
    '<div class="warns hide" id="convWarns"></div>' +
    `<div class="row-btns"><button class="btn" id="convCancel">${t("web.common.cancel")}</button>` +
    `<button class="btn primary" id="convRun">${t("web.convert.run.convert")}</button></div>` +
    "</div></div>" +
    // external-edit sheet (multi-line value / comment) — built on demand by
    // `openExternalEdit` (a touch-native bottom sheet, NOT the desktop modal).
    '<div class="sheet ext-sheet"></div>' +
    // Help/About sheet (info button) — built on demand by `renderHelpSheet`.
    '<div class="sheet help-sheet"></div>' +
    // Open sheet (header Open button) — built on demand by `openOpenSheet`.
    '<div class="sheet url-sheet"></div>' +
    // Confirmation-prompt sheet (`Mode::Prompt` y/n → buttons) — rendered per
    // snapshot by `renderPromptSheet`.
    '<div class="sheet prompt-sheet"></div>' +
    "</div>"
  );
}

// ---- notice/toast ----
let toastT: number | undefined;
// Tracks the severity+text of whatever the toast last actually showed. render()
// re-invokes renderNotice(snap.notice) on every dispatched Intent, including
// pure navigation ones (cursor move, ToggleExpand, SetCursor/SetSelection) that
// the core Notice lifecycle deliberately leaves untouched (MESSAGES.md §1.1) —
// without this guard, tapping a different (valid) node or toggling expand while
// a stale paste/cut error notice is still sitting in Session.notice replays the
// toast's entrance animation and restarts its auto-hide timer, making one error
// look like it keeps popping back up. Only a genuinely new/changed notice (a
// fresh dispatch, or the notice clearing then reappearing) re-triggers it.
let lastNoticeKey: string | undefined;
function renderNotice(notice: Notice | undefined) {
  if (!notice) {
    lastNoticeKey = undefined;
    toastEl.classList.remove("show");
    return;
  }
  const key = `${notice.severity}|${notice.text}`;
  if (key === lastNoticeKey) return;
  lastNoticeKey = key;
  // Touch uses a simple toast for all severities (no separate error element like desktop).
  // Severity classes enable different styling if needed.
  toastEl.textContent = notice.text;
  toastEl.classList.remove("sev-info", "sev-success", "sev-warn", "sev-error");
  toastEl.classList.add(`sev-${notice.severity}`);
  toastEl.classList.add("show");
  clearTimeout(toastT);
  // Longer duration for warnings/errors
  const ms = notice.severity === "error" || notice.severity === "warn" ? 3000 : 1600;
  toastT = window.setTimeout(() => toastEl.classList.remove("show"), ms);
}

// Firefox iOS can't name extension-less downloads (.toml/.yaml); show a one-time
// hint to use Safari rather than leave the user puzzled by the garbage filename.
function firefoxIosSaveHint(filename: string) {
  if (!isFirefoxIos() || filename.endsWith(".json")) return;
  if (localStorage.getItem("confy.fxios-save-hint")) return;
  localStorage.setItem("confy.fxios-save-hint", "1");
  send({ SetHostNotice: { key: "web.host.fxios-save-hint", args: [], source: "host-web" } });
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

// The armed-paste `After` target reuses the same `.reorder-line` element
// drag-reorder already positions, and the `Into` target reuses the same
// `.drop-into` row class drag-reorder's own hover uses (ADR 0004 §1). Both
// survive a live reorder-drag ending: `endReorder()`'s cleanup (hiding
// `.reorder-line` + `clearInto()`) unconditionally wipes whichever half a
// drag last touched, even a drag unrelated to (or a no-op on) the armed
// clipboard — e.g. a grip tap that never crosses the move threshold, or a
// drag dropped back onto its own source — and in those cases no
// `MoveSelectionTo` is sent, so no subsequent `render()` restores the cue.
// `endReorder()` calls this again right after its own wipe (mirrors the
// desktop `dnd.ts` `onDragEnd` fix, ADR 0004 §1) — found and fixed the same
// way while implementing this hook, not part of the original brief.
function renderPasteSlotCue(snap: SessionSnapshot, slotOverride?: PasteSlot) {
  // Sweep any previously-classified row's `.drop-into` before applying the
  // new one — required once this function is called repeatedly mid-gesture
  // (onPasteDragMove's live preview, below) without an intervening full
  // render, where at most one stale row ever existed before. Mirrors
  // web/ui.ts's Phase 4 renderPasteSlotCue sweep (ADR 0004 §1).
  treeEl.querySelectorAll<HTMLElement>(".drop-into").forEach((el) => el.classList.remove("drop-into"));
  const slot = slotOverride ?? snap.paste_slot;
  if (slot && "Into" in slot) {
    treeEl
      .querySelector<HTMLElement>(`.row[data-path='${CSS.escape(JSON.stringify(slot.Into))}']`)
      ?.classList.add("drop-into");
  }
  const reorderLine = treeEl.querySelector<HTMLElement>(".reorder-line");
  if (reorderLine) {
    if (slot && "After" in slot) {
      const rowEl = treeEl.querySelector<HTMLElement>(
        `.row[data-path='${CSS.escape(JSON.stringify(slot.After))}']`,
      );
      if (rowEl) {
        const treeTop = treeEl.getBoundingClientRect().top;
        reorderLine.style.top = `${rowEl.getBoundingClientRect().bottom - treeTop}px`;
        reorderLine.style.display = "block";
      } else {
        reorderLine.style.display = "none";
      }
    } else if (!reordering) {
      reorderLine.style.display = "none";
    }
  }
}

// Renders the shared panel body into `container` (either the wide side pane's
// `.dp-body`, or the narrow bottom sheet's `.detail-wrap`) and re-wires it,
// preserving `scroller`'s own scroll position across the innerHTML replace —
// called on every render(), including the live SetCursor+Nudge dispatches a
// mid-drag swipe/wheel-nudge fires (`web/panel.ts`), so those steps don't
// snap the panel back to its own top every time. `scroller` is the element
// that actually has `overflow:auto` (touch/style.css): on wide layouts
// that's `.detail-pane`, one level *above* the padded `.dp-body` container
// itself (which never scrolls, so saving/restoring its own scrollTop is a
// no-op); on the narrow sheet, `.detail-wrap` doubles as `.sheet-body` and
// is both the container and the scroller.
function renderDetailBody(
  container: HTMLElement,
  scroller: HTMLElement,
  cur: ViewRow,
  schemaEnum: { options: string[]; cursor: number } | undefined,
): void {
  const hint = session!.schemaHint(cur.path);
  const info = session!.schemaInfo(cur.path);
  const st = scroller.scrollTop;
  container.innerHTML = panelHTML(cur, parentIsInline(cur.path), hint, schemaEnum, info);
  wirePanel(container, cur, sendR, openKindRow, (msg: string) => renderNotice({ severity: "error", text: msg, source: "core" }), undefined, schemaEnum);
  scroller.scrollTop = st;
}

// ---- render ----
function render() {
  if (!snap || !session) return;
  fmtPill.textContent = snap.doc_format.toUpperCase();
  fmtPill.classList.toggle("toggleable", inSampleMode());
  fmtPill.title = inSampleMode() ? t("web.toolbar.fmtPill.sampleTitle") : t("web.toolbar.fmtPill.title");
  docNameEl.textContent = fileName ?? "config";
  dirtyDot.style.opacity = snap.is_dirty ? "1" : "0";
  // Render notice (severity-driven toast)
  renderNotice(snap.notice);
  
  if (snap.notice) {
    statusEl.textContent = snap.notice.text;
  } else {
    // Idle schema hint — mirrors the TUI/desktop status line's dynamic
    // behavior (tooltip-like: appears while the cursor sits on a schema-
    // constrained node, clears the instant it moves off). Touch has no
    // hover, so this is its only way to see the constraint outside the
    // detail panel.
    statusEl.textContent = schemaHintText(session.schemaHint(snap.cursor)) || t("web.status.ready");
  }
  const cur = cursorRow();
  selBadge.textContent = cur && cur.path.length ? lastKey(cur.path) : t("web.badge.none");
  const armed = (snap.clipboard_count ?? 0) > 0;
  clipBadge.textContent = tArgs("web.badge.clipboard", [String(snap.clipboard_count ?? 0)]);
  clipBadge.classList.toggle("armed", armed);
  // Paste mode: the clipboard freezes the source selection — de-emphasize it and
  // show the cursor row as the live paste target instead (CSS keys off this class).
  app.classList.toggle("paste-mode", armed);
  // Paste-armed FAB: paste glyph + copy/cut accent (tap pastes; see "add" case).
  syncFab(fabEl, armed, !!snap.clipboard_cut);
  // Toolbar language label — mirrors desktop `#langLabel`.
  const langLabelEl = app.querySelector<HTMLElement>('[data-act="lang"] .lang-label');
  if (langLabelEl) langLabelEl.textContent = getLang() === "zh-TW" ? "繁" : "EN";
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
    renderPasteSlotCue(snap);
    treePane.scrollTop = st;
    // The rebuild detaches any swipe-opened row — drop the stale reference.
    openSwipeMain = null;
    openSwipeOff = 0;
    app.classList.remove("raw");
  }

  // Detail panel: the persistent side pane (≥600px) always reflects the
  // current cursor row live; the narrow bottom sheet (opened via double-tap,
  // `openPanel`) only needs a live refresh here while it's already open —
  // otherwise a swipe/wheel-nudge mid-drag (which dispatches SetCursor+Nudge
  // repeatedly without closing the sheet) silently changes the value while
  // the sheet keeps showing the pre-drag one, only visible after closing it.
  // `scrollTop` is captured/restored around the innerHTML replace on both
  // paths (mirrors the tree-pane preservation above and `renderFilterSheet`'s
  // `.sheet-body` preservation) — otherwise every nudge step snaps the panel's
  // own scroll back to the top.
  if (!rawView) {
    const schemaEnum =
      typeof snap.mode === "object" && "SchemaEnum" in snap.mode ? snap.mode.SchemaEnum : undefined;
    if (isWide()) {
      if (cur && cur.path.length) {
        renderDetailBody(dpBody, dpBody.parentElement!, cur, schemaEnum);
      } else {
        dpBody.innerHTML = '<div class="dp-empty">Tap any node<br>to edit its value and metadata here</div>';
      }
    } else if (sheets.detail.classList.contains("open") && cur && cur.path.length) {
      const wrap = sheets.detail.querySelector<HTMLElement>(".detail-wrap");
      if (wrap) renderDetailBody(wrap, wrap, cur, schemaEnum);
    }
  }

  // Mode-driven surfaces: TypeFilter → the shared grid in the filter sheet;
  // Convert → the shared native dialog (no scrim/sheet).
  const tag = modeTag(snap.mode);
  if (tag === "TypeFilter") renderFilterSheet((snap.mode as { TypeFilter: TypeFilterView }).TypeFilter);
  else sheets.filter.classList.remove("open");
  if (tag === "Convert") renderConvertDialogShared(convRefs(), (snap.mode as { Convert: ConvertView }).Convert, snap);
  else if (sheets.convert.classList.contains("open")) closeSheets();
  // Confirmation prompt (type change, paste collision, quit, …) → button sheet.
  // Without this, a `Mode::Prompt` would soft-lock the touch UI (no keyboard).
  if (tag === "Prompt") {
    const p = (snap.mode as { Prompt: { kind: PromptView; question: string } }).Prompt;
    renderPromptSheet(p.kind, p.question);
  } else sheets.prompt.classList.remove("open");
  // Constrained value picker (Mode::SchemaEnum): core enters this mode via
  // begin_inline_edit both for a schema enum/const field and for any `bool`
  // scalar (the schema-independent true/false fallback, `from_schema: false`);
  // render the bottom sheet of allowed values (mirrors the TypeFilter/Convert/
  // Prompt checks above).
  if (typeof snap.mode === "object" && "SchemaEnum" in snap.mode) {
    const se = snap.mode.SchemaEnum;
    const cur = snap.rows.find((r) => r.is_cursor);
    if (cur) openSchemaEnumSheet(cur.path, se.options, se.from_schema);
  }
  if (tag === "AddPicker") {
    const ap = (snap.mode as { AddPicker: { options: AddOptionView[]; cursor: number } }).AddPicker;
    openAddPickerSheet(ap.options, ap.cursor);
  }
  if (tag === "ActionMenu") {
    const am = (snap.mode as { ActionMenu: { items: ActionItemView[]; target_label: string } }).ActionMenu;
    openActionMenuSheet(am);
  } else if (sheets.actions.classList.contains("open")) {
    closeSheets();
  }
  renderHelpSheet();
  if (tag !== "TypeFilter" && !anySheetOpen()) scrim.classList.remove("show");

  // Active type-filter indicator on the funnel button.
  filterBtn.classList.toggle("on", snap.type_filter_active);

  // Async host I/O the snapshot requested.
  if (snap.external_edit) openExternalEdit(snap.external_edit);
  if (snap.convert_write) void doConvertWrite(io, snap.convert_write[0], snap.convert_write[1]);
  if (snap.schema_fetch_request && !schemaFetchInFlight) {
    schemaFetchInFlight = true;
    void resolveSchemaFetchRequest(io, session!, snap.schema_fetch_request, fileHandle?.path ?? null).then(
      (next) => {
        schemaFetchInFlight = false;
        snap = next;
        if (snap.schema_status?.load_error) {
          snap = session!.dispatch({
            SetHostNotice: {
              key: "web.host.schema.load-error",
              args: [snap.schema_status.load_error],
              source: "host-web",
            },
          });
        }
        render();
      },
    );
  }
}
function anySheetOpen(): boolean {
  return Object.values(sheets).some((s) => s.classList.contains("open"));
}
// Single tap = select only (cursor + selection); the wide-mode side pane
// reactively shows it. The detail sheet opens on double-tap (openPanel).
function selectOnly(path: Path) {
  send({ SetCursor: path });
  send({ SetSelection: { paths: [path] } });
}
// Double-tap (narrow) opens the bottom-sheet panel. Wide mode keeps the
// persistent side pane (render() refreshed it), so no sheet is needed.
function openPanel(path: Path) {
  if ((snap?.clipboard_count ?? 0) > 0) {
    send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
    return;
  }
  selectOnly(path);
  const r = rowFor(path);
  if (!r) return;
  if (!isWide()) {
    // A comment node fills `key` with the whole (possibly multi-line) comment text,
    // which would blow up the title — use a fixed label; otherwise the node key.
    // The `.sheet-head h3` CSS truncates a long key to one line (ellipsis).
    const title = r.type_label === "comment" ? "Comment" : r.key || lastKey(path);
    const hint = session!.schemaHint(r.path);
    const info = session!.schemaInfo(r.path);
    sheets.detail.innerHTML =
      '<div class="grab"></div>' +
      `<div class="sheet-head"><h3>${esc(title)}</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
      `<div class="sheet-body detail-wrap">${panelHTML(r, parentIsInline(r.path), hint, undefined, info)}</div>`;
    wirePanel(sheets.detail, r, sendR, openKindRow, (msg: string) => renderNotice({ severity: "error", text: msg, source: "core" }));
    openSheet("detail");
  }
}

// ---- confirmation-prompt sheet (Mode::Prompt) ----
// The question is `snap.mode.Prompt.question`;
// the buttons are the shared per-kind set (`prompt.ts`), answered as PromptKey
// via the delegated listener bound once in main(). The header × carries
// data-pk="n" so every dismissal answers the prompt (never just hides it).
function renderPromptSheet(kind: PromptView, question: string) {
  sheets.prompt.innerHTML =
    '<div class="grab"></div>' +
    `<div class="sheet-head"><h3>${esc(promptTitle(kind))}</h3><button class="close" data-pk="n">${IC.close}</button></div>` +
    `<div class="sheet-body"><p class="dlg-sub">${esc(question)}</p>${promptButtonsHTML(kind)}</div>`;
  if (!sheets.prompt.classList.contains("open")) openSheet("prompt");
}

// ---- kind sheet (from session.kindOptions) ----
function openKindSheet(path: Path) {
  if (!session) return;
  if ((snap?.clipboard_count ?? 0) > 0) {
    send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
    return;
  }
  send({ SetCursor: path });
  const opts = session.kindOptions(path);
  if (!opts.length) {
    send({ SetHostNotice: { key: "web.host.kind.no-options", args: [], source: "host-web" } });
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
    `<div class="sheet-head"><h3>${esc(tArgs("web.kind.switchTitle", [lastKey(path)]))}</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
    `<div class="sheet-body"><div class="addgrid">${cells}</div></div>`;
  sheets.kind.querySelectorAll<HTMLElement>(".kind-opt").forEach((b) => {
    b.addEventListener("click", () => {
      const target = b.dataset.target!;
      closeSheets();
      const after = sendR({ CommitKind: { path, target } });
      const isErr = after.notice?.severity === "error";
      send({ SetHostNotice: { key: isErr ? "core.kind-switch.error" : "web.host.kind.changed", args: isErr ? [after.notice!.text] : [], source: "host-web" } });
    });
  });
  openSheet("kind");
}

// ---- constrained value sheet (Mode::SchemaEnum) ----
// Mirrors openKindSheet's structure: a bottom-sheet grid of `.kind-opt` cells
// in the shared `sheets.kind` element. Core enters Mode::SchemaEnum via
// begin_inline_edit when an enum/const-constrained field is tapped (Task 6),
// and for any `bool` scalar (`fromSchema: false` — the schema-independent
// true/false picker, which is touch's only bool-toggle affordance: it has no
// keyboard `←/→` and no mouse wheel to Nudge with); this only renders that
// mode. Selection moves the cursor (SchemaEnumMove) then
// commits (SchemaEnumCommit) — commit uses whatever cursor index is current.
function openSchemaEnumSheet(path: Path, options: string[], fromSchema: boolean) {
  if (!session) return;
  if ((snap?.clipboard_count ?? 0) > 0) {
    send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
    return;
  }
  const cells = options
    .map(
      (label, i) =>
        `<button class="add-cell kind-opt" data-idx="${i}"><span class="dotc" style="background:var(--warn)"></span>${esc(label)}</button>`,
    )
    .join("");
  sheets.kind.innerHTML =
    '<div class="grab"></div>' +
    `<div class="sheet-head"><h3>${fromSchema ? "Schema value" : "Value"}</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
    `<div class="sheet-body"><div class="addgrid">${cells}</div></div>`;
  sheets.kind.querySelectorAll<HTMLElement>(".kind-opt").forEach((b) => {
    b.addEventListener("click", () => {
      const idx = Number(b.dataset.idx);
      const current =
        modeTag(snap!.mode) === "SchemaEnum"
          ? (snap!.mode as { SchemaEnum: { options: string[]; cursor: number } }).SchemaEnum.cursor
          : 0;
      send({ SchemaEnumMove: idx - current });
      closeSheets();
      const after = sendR("SchemaEnumCommit");
      const isErr = after.notice?.severity === "error";
      send({ SetHostNotice: { key: isErr ? "core.error.generic" : "web.host.value.changed", args: isErr ? [after.notice!.text] : [], source: "host-web" } });
    });
  });
  openSheet("kind");
}

// ---- Add-type picker sheet (Mode::AddPicker) ----
// Mirrors openSchemaEnumSheet's structure: a bottom-sheet grid of `.kind-opt`
// cells in the shared `sheets.kind` element. Core enters Mode::AddPicker via
// AddNode/AddChild/AddSibling (the FAB → Action menu → "Add child"/"Append
// sibling", desktop `a`); tapping a cell applies it directly (AddPickerPick),
// unlike SchemaEnum's move-then-commit (AddPicker has a direct pick intent).
function openAddPickerSheet(options: AddOptionView[], cursor: number) {
  if (!session) return;
  const cells = options
    .map(
      (o, i) =>
        `<button class="add-cell kind-opt${i === cursor ? " sel" : ""}" data-idx="${i}"><span class="dotc" style="background:var(--accent)"></span>${esc(o.label)}</button>`,
    )
    .join("");
  sheets.kind.innerHTML =
    '<div class="grab"></div>' +
    `<div class="sheet-head"><h3>${esc(t("core.add.picker.title"))}</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
    `<div class="sheet-body"><div class="addgrid">${cells}</div></div>`;
  sheets.kind.querySelectorAll<HTMLElement>(".kind-opt").forEach((b) => {
    b.addEventListener("click", () => {
      const idx = Number(b.dataset.idx);
      closeSheets();
      send({ AddPickerPick: idx });
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
// tracks the responsive breakpoints instead of hardcoding a fixed set. `key` is
// the `[data-act="…"]` selector `isFolded` checks (shared `ToolbarEntry` type
// with the desktop UI's `TOOLBAR_ENTRIES`); a button added to `.edit-grp`/
// `.nav-grp`/`.viewtabs` (marked `data-foldable`) without a matching entry here
// is caught by `web/toolbar-fold.spec.mjs`.
const MENU_CANDIDATES: ToolbarEntry[] = [
  { key: '[data-act="undo"]', icon: IC.undo, labelKey: "web.menu.undo", run: () => send("Undo") },
  { key: '[data-act="redo"]', icon: IC.redo, labelKey: "web.menu.redo", run: () => send("Redo") },
  { key: '[data-act="theme"]', icon: IC.sun, labelKey: "web.menu.toggleTheme", run: toggleTheme },
  {
    key: '[data-act="lang"]',
    icon: '<span class="ic" aria-hidden="true">\u{1F310}</span>',
    labelKey: "web.toolbar.lang.title",
    run: openLangSheet,
  },
  { key: '[data-act="info"]', icon: IC.help, labelKey: "web.menu.helpAbout", run: () => send("EnterHelp") },
  { key: '[data-act="expandall"]', icon: IC.expand, labelKey: "web.menu.expandAll", run: () => send("ExpandAll") },
  {
    key: '[data-act="collapseall"]',
    icon: IC.collapse,
    labelKey: "web.menu.collapseAll",
    run: () => send("CollapseAll"),
  },
  {
    key: '[data-act="toggleview"]',
    icon: IC.open,
    labelKey: "web.menu.toggleView",
    run: () => setRawView(!rawView),
  },
];
// A toolbar control is "folded" (→ belongs in the menu) when it's not laid out
// (its group is display:none, so offsetParent is null).
function isFolded(sel: string): boolean {
  const el = app.querySelector<HTMLElement>(sel);
  return !!el && el.offsetParent === null;
}
// Applies the chosen language, syncs core, re-renders.
function chooseLang(lang: Lang) {
  setLang(lang);
  if (session) send({ SetLang: getLang() });
  applyStaticI18n(app);
  render();
}

// The language picker as its own bottom sheet (same anatomy as the kind-switch
// sheet: a list of choice buttons in `.sheet-body`), opened from the ⋯ menu's
// language row instead of cycling on tap. Scales to any number of languages
// (`availableLangs()`); the active one is marked `.sel` with a check icon.
function openLangSheet() {
  if ((snap?.clipboard_count ?? 0) > 0) {
    send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
    return;
  }
  const cur = getLang();
  const cells = availableLangs()
    .map((lang) => {
      const sel = lang === cur;
      return `<button class="menu-item${sel ? " sel" : ""}" data-lang="${esc(lang)}"><span class="ic">${sel ? "✓" : ""}</span>${esc(LANG_DISPLAY_NAMES[lang])}</button>`;
    })
    .join("");
  sheets.lang.innerHTML =
    '<div class="grab"></div>' +
    `<div class="sheet-head"><h3>${t("web.toolbar.lang.title")}</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
    `<div class="sheet-body">${cells}</div>`;
  sheets.lang.querySelectorAll<HTMLElement>("[data-lang]").forEach((b) => {
    const lang = b.dataset.lang as Lang;
    b.addEventListener("click", () => {
      closeSheets();
      chooseLang(lang);
    });
  });
  openSheet("lang");
}

// Tapping the toolbar Save button always opens this action-choice sheet — no
// one-tap quick-save on touch (unlike desktop's ⌘S), since a merged Save/
// Save-As split-button pill turned out to render as two stacked buttons on
// at least one real device with no obvious CSS cause; a plain sheet sidesteps
// that whole class of layout bug.
function openSaveSheet() {
  if ((snap?.clipboard_count ?? 0) > 0) {
    send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
    return;
  }
  sheets.save.innerHTML =
    '<div class="grab"></div>' +
    `<div class="sheet-head"><h3>${t("web.toolbar.save.label")}</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
    "<div class=\"sheet-body\">" +
    mi(TIC.save, t("web.toolbar.save.label"), "", "save") +
    mi(TIC.chevron, t("web.toolbar.saveAs.title"), "", "saveas") +
    "</div>";
  sheets.save.querySelectorAll<HTMLElement>(".menu-item").forEach((it) => {
    it.addEventListener("click", () => {
      const id = it.dataset.mi!;
      closeSheets();
      if (id === "save") void doQuickSave(io);
      else openSaveConvert(io);
    });
  });
  openSheet("save");
}

function openMenuSheet() {
  const items = foldedEntries(MENU_CANDIDATES, isFolded);
  sheets.menu.innerHTML =
    '<div class="grab"></div>' +
    `<div class="sheet-head"><h3>${t("web.toolbar.more.title")}</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
    '<div class="sheet-body">' +
    items.map((c, i) => mi(c.icon ?? "", t(c.labelKey), "", String(i))).join("") +
    "</div>";
  sheets.menu.querySelectorAll<HTMLElement>(".menu-item").forEach((it) => {
    it.addEventListener("click", () => {
      const id = it.dataset.mi!;
      const c = items[Number(id)];
      if ((snap?.clipboard_count ?? 0) > 0 && c.run !== toggleTheme) {
        closeSheets();
        send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
        return;
      }
      if (c.run !== toggleTheme) closeSheets();
      c.run();
    });
  });
  openSheet("menu");
}

// ---- Action menu sheet (Mode::ActionMenu — centralized node operations,
// design doc `docs/superpowers/specs/2026-08-30-action-menu-design.md` §7,
// ADR 0009). Mirrors `openMenuSheet`'s shape but is driven by `snap.mode`
// like the other mode-driven sheets (TypeFilter/Convert/Prompt/SchemaEnum)
// rather than a fire-once host-local list.
function openActionMenuSheet(am: { items: ActionItemView[]; target_label: string }) {
  sheets.actions.innerHTML =
    '<div class="grab"></div>' +
    `<div class="sheet-head"><h3>${esc(am.target_label)}</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
    '<div class="sheet-body">' +
    am.items.map((it, i) => actionItemHTML(it, i)).join("") +
    "</div>";
  sheets.actions.querySelectorAll<HTMLElement>(".menu-item:not([disabled])").forEach((b) => {
    const i = Number(b.dataset.i);
    b.addEventListener("click", () => {
      closeSheets();
      const id = am.items[i].id;
      // Detail has no core-mode-driven rendering on touch (mirrors the
      // `i`/Enter ToggleDetail key path, see `toggleDetailSheet`) — exit the
      // Action menu without letting core enter Mode::Detail (which touch
      // never renders), then open the host-local detail sheet directly.
      if (id === "Detail") {
        send("ExitActionMenu");
        toggleDetailSheet();
        return;
      }
      send({ ActionMenuPick: id });
    });
  });
  openSheet("actions");
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
    `<div class="sheet-head"><h3>${t("web.typefilter.label")}</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
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
  if ((snap?.clipboard_count ?? 0) > 0) {
    send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
    return;
  }
  if (sheets.ext.classList.contains("open")) return;
  const kind = ext.kind as { Value?: { path: Path }; Comment?: { path: Path } };
  const isComment = !!kind.Comment;
  const path = (kind.Value ?? kind.Comment)!.path;
  const title = isComment
    ? t("web.editModal.editComment")
    : esc(tArgs("web.editModal.editValue", [lastKey(path) || t("web.editModal.value")]));
  sheets.ext.innerHTML =
    '<div class="grab"></div>' +
    `<div class="sheet-head"><h3>${title}</h3><button class="close" data-act="extcancel">${IC.close}</button></div>` +
    '<div class="sheet-body">' +
    '<textarea class="ext-text" spellcheck="false" autocomplete="off" autocapitalize="off"></textarea>' +
    `<div class="row-btns"><button class="btn" data-act="extcancel">${t("web.common.cancel")}</button>` +
    `<button class="btn primary ext-apply">${t("web.common.apply")}</button></div>` +
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
  // preventScroll: `.app` is position:absolute (scrolls with the page), and
  // autofocus-triggered scrollIntoView shifts the whole app shell out from
  // under its bottom-anchored sheets, uncovering the next sheet underneath.
  txt.focus({ preventScroll: true });
}

// Help/About bottom sheet (header info button). Mirrors `renderFilterSheet`'s
// "read from `snap.mode`, re-render every snapshot" pattern (rather than a
// fire-once `open*Sheet`) since the tab flips via `send("ToggleHelpTab")` and
// must re-render live.
function renderHelpSheet() {
  const tag = modeTag(snap!.mode);
  if (tag !== "Help") {
    if (sheets.help.classList.contains("open")) closeSheets();
    return;
  }
  const activeTab = (snap!.mode as { Help: { tab: "Help" | "About" } }).Help.tab;
  // helpBodyHTML output is pre-escaped HTML (key spans) — insert raw.
  const body = helpBodyHTML(activeTab, snap!.doc_format, session!.aboutText());
  sheets.help.innerHTML =
    '<div class="grab"></div>' +
    `<div class="sheet-head"><h3>${t("web.help.tab.help")} / ${t("web.help.tab.about")}</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
    '<div class="sheet-body">' +
    '<div class="help-tabs">' +
    `<button class="btn tab-btn${activeTab === "Help" ? " primary" : ""}" data-tab="Help">${t("web.help.tab.help")}</button>` +
    `<button class="btn tab-btn${activeTab === "About" ? " primary" : ""}" data-tab="About">${t("web.help.tab.about")}</button>` +
    "</div>" +
    `<pre class="help-body">${body}</pre>` +
    "</div>";
  sheets.help.querySelectorAll<HTMLElement>("[data-tab]").forEach((btn) => {
    btn.onclick = () => {
      if (btn.dataset.tab !== activeTab) send("ToggleHelpTab");
    };
  });
  if (!sheets.help.classList.contains("open")) openSheet("help");
}

// "Open" bottom sheet (header Open button) — local-file browse or fetch a
// remote config by URL. A URL open has no on-disk handle, so a later Save
// falls back to download (like the file path).
function openOpenSheet() {
  if ((snap?.clipboard_count ?? 0) > 0) {
    send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
    return;
  }
  if (sheets.url.classList.contains("open")) return;
  sheets.url.innerHTML =
    '<div class="grab"></div>' +
    `<div class="sheet-head"><h3>${t("web.open.title")}</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
    '<div class="sheet-body">' +
    '<button class="btn browse-local">' +
    `<span class="bl-ic">${TIC.open}</span>` +
    `<span class="bl-text"><strong>${t("web.open.browseLocal.title")}</strong><small>${t("web.open.browseLocal.subtitle")}</small></span>` +
    '<svg class="bl-chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M9 6l6 6-6 6"/></svg>' +
    "</button>" +
    `<div class="sheet-divider">${t("web.open.orUrl")}</div>` +
    `<input class="url-input" type="url" inputmode="url" spellcheck="false" autocomplete="off" autocapitalize="off" placeholder="${t("web.open.urlPlaceholder")}" />` +
    `<div class="row-btns"><button class="btn url-cancel" data-act="closesheet">${t("web.common.cancel")}</button>` +
    `<button class="btn primary url-open">${t("web.open.confirm")}</button></div>` +
    "</div>";
  const inp = sheets.url.querySelector<HTMLInputElement>(".url-input")!;
  const go = () => {
    const url = inp.value.trim();
    closeSheets();
    if (url) void openFromUrl(io, openText, url);
  };
  sheets.url.querySelector<HTMLElement>(".browse-local")!.onclick = () => {
    closeSheets();
    void doOpen();
  };
  // Open is wired directly (no data-act) so shell delegation never double-fires.
  sheets.url.querySelector<HTMLElement>(".url-open")!.onclick = go;
  inp.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      go();
    }
  });
  openSheet("url");
  // Explicit focus on Cancel (not the URL input): some browsers (e.g. iOS
  // Firefox) auto-focus the first form field in a freshly shown sheet on
  // their own, popping the keyboard uninvited. Claiming focus ourselves
  // preempts that without landing the user in a text field either.
  sheets.url.querySelector<HTMLElement>(".url-cancel")!.focus({ preventScroll: true });
}

// ---- grip reorder + tap (pointer flow; horizontal swipe removed) ----
// Tap-vs-scroll tracking on a row (grip-drag reorder is handled separately).
let sx = 0,
  sy = 0,
  dragRow: HTMLElement | null = null,
  dragging = false,
  moved = false;
// Live target preview during a body-drag while the clipboard is armed (§6b):
// pointerdown latches `pasteDragActive` (armed && not a `.caret` press);
// pointermove repaints the paste-target cue via `onPasteDragMove` past the
// dead zone; pointerup commits the classified slot via `finishPasteDrag` —
// a set, never a `Paste` (the FAB alone dispatches that).
let pasteDragActive = false,
  pasteDragStartY = 0,
  pasteDragMoved = false,
  pasteDragRow: HTMLElement | null = null;
// Edge auto-scroll while dragging (§6b follow-up, ADR 0005 §6, unified with
// grip reorder below): a live rAF loop, distinct from the pointermove-driven
// previews below it — the finger can sit still at `treePane`'s edge while
// content keeps scrolling under it, which no pointermove event would ever
// drive. Shared by both drag gestures that read pixel Y against the live
// tree (armed-paste body-drag and grip reorder), since `reordering` and
// `pasteDragActive` are mutually exclusive (a grip press never arms
// `pasteDragActive`, see installTreeGestures) - so desktop's native-DnD
// auto-scroll (a browser feature `web/dnd.ts` gets for free) and touch's
// two custom drags now behave identically. Self-terminates the frame once
// neither drag is active anymore (pointerup/pointercancel already clear
// both), so no explicit cancelAnimationFrame bookkeeping is needed. Safe
// against the render()-triggered scrollTop-restore latch (`web/touch/app.ts`
// render(), ~line 429) because neither `onPasteDragMove` nor `onReorderMove`
// dispatches mid-drag — only release does, by which point this loop has
// already stopped.
let edgeScrollRAF: number | null = null;
let edgeScrollY = 0;
const EDGE_SCROLL_ZONE = 44;
const EDGE_SCROLL_MAX_SPEED = 16;
function edgeAutoScrollStep() {
  if (!pasteDragActive && !reordering) {
    edgeScrollRAF = null;
    return;
  }
  const rect = treePane.getBoundingClientRect();
  const distTop = edgeScrollY - rect.top;
  const distBottom = rect.bottom - edgeScrollY;
  let dy = 0;
  if (distTop < EDGE_SCROLL_ZONE && treePane.scrollTop > 0) {
    dy = -EDGE_SCROLL_MAX_SPEED * (1 - Math.max(distTop, 0) / EDGE_SCROLL_ZONE);
  } else if (distBottom < EDGE_SCROLL_ZONE && treePane.scrollTop < treePane.scrollHeight - treePane.clientHeight) {
    dy = EDGE_SCROLL_MAX_SPEED * (1 - Math.max(distBottom, 0) / EDGE_SCROLL_ZONE);
  }
  if (dy !== 0) {
    treePane.scrollTop += dy;
    // The finger is stationary while rows shift under it — refresh whichever
    // drag's live preview is active against the same y (past its own dead
    // zone, since a drag is already in progress).
    if (pasteDragActive) onPasteDragMove(edgeScrollY);
    else onReorderMove(edgeScrollY);
  }
  edgeScrollRAF = requestAnimationFrame(edgeAutoScrollStep);
}
function kickEdgeAutoScroll(y: number) {
  edgeScrollY = y;
  if (edgeScrollRAF === null) edgeScrollRAF = requestAnimationFrame(edgeAutoScrollStep);
}
// Double-tap detection (item 6): same path within DOUBLE_TAP_MS opens the panel.
let lastTapKey: string | null = null;
let lastTapTime = 0;
const DOUBLE_TAP_MS = 300;

// Swipe-to-delete: a horizontal left-swipe on a row's `.row-main` slides it open
// to reveal a single Delete action (`.row-del`). One row is open at a time.
let swiping = false;
let swipeMain: HTMLElement | null = null;
let swipeHasDel = false;
let swipeHasRemark = false;
let swipeBase = 0;
let swipeOff = 0;
let openSwipeMain: HTMLElement | null = null;
let openSwipeOff = 0;
const SWIPE_W = 96;

// Reveal / hide the red Delete behind a row (`.row.swiping` — CSS keeps
// `.row-del` visibility:hidden at rest so scroll repaints can't flash red
// slivers at the rounded corners). Hiding waits out the close animation so the
// button slides behind the row instead of vanishing mid-slide.
function setSwipeRevealed(main: HTMLElement | null, on: boolean) {
  const row = main?.closest<HTMLElement>(".row");
  if (!row) return;
  if (on) {
    row.classList.add("swiping");
    return;
  }
  window.setTimeout(() => {
    // Re-opened (or mid-swipe again) in the meantime — keep it revealed.
    if (main === openSwipeMain || (swiping && main === swipeMain)) return;
    row.classList.remove("swiping");
  }, 260);
}

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
  kickEdgeAutoScroll(e.clientY);
}
function onReorderMove(y: number) {
  edgeScrollY = y;
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
    const rel = (y - hr.top) / (hr.height || 1);
    const slot = session?.pointerSlot(pathOf(hit)!, rel);
    if (slot && "Into" in slot) {
      reMode = "into";
    } else {
      reMode = rel < 0.5 ? "before" : "after";
    }
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
  // Restore the armed-paste cue this wipe may have collaterally stripped
  // (see renderPasteSlotCue's doc comment above).
  if (snap) renderPasteSlotCue(snap);
  if (reRow) reRow.classList.remove("dragging");
  if (reMoved && reTarget && reSrcPath) {
    const tgtPath = pathOf(reTarget);
    if (tgtPath && !pathEq(tgtPath, reSrcPath)) {
      const sources = [reSrcPath];
      if (reMode === "into") {
        const idx = rowFor(tgtPath)?.child_count ?? 0;
        send({ MoveSelectionTo: { sources, target: tgtPath, index: idx } });
      } else {
        const sib = siblingIndex(snap!.rows, tgtPath);
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

// Live per-pixel preview of the armed-paste target during a body-drag, once
// past the same 6px dead zone `onReorderMove` uses — mirrors its hit-test
// loop (prefer a row whose rect contains `y`, else nearest by edge
// distance), but skips the source-subtree exclusion (armed clipboard rows
// are ordinary rows, not a row mid-drag; trust `session.pointerSlot` the
// same way desktop's `onArmedPasteHover` does, no client-side filtering).
// Repaints via `renderPasteSlotCue`'s existing cue elements, client-only —
// same fallback-to-committed behavior as `onArmedPasteHover` when the hit
// resolves to nothing classifiable.
function onPasteDragMove(y: number) {
  edgeScrollY = y;
  if (Math.abs(y - pasteDragStartY) < 6 && !pasteDragMoved) return;
  pasteDragMoved = true;
  const rows = Array.prototype.filter.call(
    treeEl.querySelectorAll<HTMLElement>(".row"),
    (r: HTMLElement) => r.offsetHeight > 0,
  ) as HTMLElement[];
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
  hit = hit ?? nearest;
  if (!hit || !snap || !session) return;
  pasteDragRow = hit;
  const path = pathOf(hit);
  if (!path) return;
  const r = hit.getBoundingClientRect();
  const relY = (y - r.top) / (r.height || 1);
  const slot = session.pointerSlot(path, relY);
  renderPasteSlotCue(snap, slot ?? snap.paste_slot ?? undefined);
}

// Commits the drag's last classified target on release — a set, never a
// paste (the FAB alone dispatches `Paste`); mirrors `armedTarget()` in
// `handleTap` below, for the stationary-tap case.
function finishPasteDrag(y: number) {
  if (!pasteDragRow || !session) return;
  const path = pathOf(pasteDragRow);
  if (!path) return;
  const r = pasteDragRow.getBoundingClientRect();
  const relY = (y - r.top) / (r.height || 1);
  const slot = session.pointerSlot(path, relY);
  send(slot ? { SetPasteSlot: slot } : { SetCursor: path });
}

function installTreeGestures() {
  treeEl.addEventListener("pointerdown", (e) => {
    const grip = (e.target as HTMLElement).closest<HTMLElement>(".drag-handle");
    if (grip) {
      // Armed clipboard hides the grip via CSS (`.app.paste-mode .drag-handle`)
      // so it can't be tapped in the first place — no runtime guard needed here.
      const row = grip.closest<HTMLElement>(".row");
      if (row) startReorder(e, row);
      return;
    }
    const armed = (snap?.clipboard_count ?? 0) > 0;
    pasteDragActive = armed && !(e.target as HTMLElement).closest(".caret");
    pasteDragStartY = e.clientY;
    pasteDragMoved = false;
    pasteDragRow = null;
    if (pasteDragActive) kickEdgeAutoScroll(e.clientY);
    const tgt = e.target as HTMLElement;
    // A tap can land on the visible row-main OR (when already swiped open) on the
    // revealed `.row-del`/`.row-remark` behind it — all map to the same row.
    const main =
      tgt.closest<HTMLElement>(".row-main") ??
      tgt.closest<HTMLElement>(".row-del") ??
      tgt.closest<HTMLElement>(".row-remark");
    const rowEl = main?.closest<HTMLElement>(".row");
    if (!rowEl) return;
    dragRow = rowEl;
    // Swipe only when the row carries a Delete action (read-only rows don't) and clipboard is not armed.
    const clipboardArmed = (snap?.clipboard_count ?? 0) > 0;
    swipeHasDel = !clipboardArmed && !!rowEl.querySelector<HTMLElement>(".row-del");
    swipeHasRemark = !clipboardArmed && !!rowEl.querySelector<HTMLElement>(".row-remark");
    swipeMain = swipeHasDel || swipeHasRemark ? rowEl.querySelector<HTMLElement>(".row-main") : null;
    swipeBase = swipeMain && openSwipeMain === swipeMain ? openSwipeOff : 0;
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
    if (pasteDragActive && dragging) {
      e.preventDefault();
      onPasteDragMove(e.clientY);
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
        setSwipeRevealed(swipeMain, true);
        if (openSwipeMain && openSwipeMain !== swipeMain) {
          const other = openSwipeMain;
          other.style.transform = "";
          openSwipeMain = null;
          setSwipeRevealed(other, false);
        }
        if (swipeMain) swipeMain.style.transition = "none";
      } else if (Math.abs(dy) > 8) {
        moved = true;
      }
    }
    if (swiping && swipeMain) {
      e.preventDefault();
      const lo = swipeHasDel ? -SWIPE_W : 0;
      const hi = swipeHasRemark ? SWIPE_W : 0;
      swipeOff = Math.max(lo, Math.min(hi, swipeBase + dx));
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
      const settleOff = swipeOff < -SWIPE_W / 2 ? -SWIPE_W : swipeOff > SWIPE_W / 2 ? SWIPE_W : 0;
      swipeMain.style.transform = settleOff !== 0 ? `translateX(${settleOff}px)` : "";
      openSwipeMain = settleOff !== 0 ? swipeMain : null;
      openSwipeOff = settleOff;
      const main = swipeMain;
      swiping = false;
      setSwipeRevealed(main, settleOff !== 0);
    } else if (pasteDragActive && pasteDragMoved) {
      finishPasteDrag(e.clientY);
    } else if (dragging && dragRow && !moved) {
      handleTap(e.target as HTMLElement, dragRow, e.clientY, e);
    }
    dragging = false;
    dragRow = null;
    swiping = false;
    swipeMain = null;
    pasteDragActive = false;
    pasteDragMoved = false;
    pasteDragRow = null;
  });
  treeEl.addEventListener("pointercancel", () => {
    if (reordering) {
      endReorder();
      return;
    }
    if (swiping && swipeMain) {
      swipeMain.style.transition = "";
      const open = openSwipeMain === swipeMain;
      swipeMain.style.transform = open ? `translateX(${openSwipeOff}px)` : "";
      const main = swipeMain;
      swiping = false;
      setSwipeRevealed(main, open);
    }
    if (pasteDragMoved && snap) renderPasteSlotCue(snap);
    pasteDragActive = false;
    pasteDragMoved = false;
    pasteDragRow = null;
    dragging = false;
    dragRow = null;
    swiping = false;
    swipeMain = null;
  });
  // Tap on empty tree space (the `.tree-pane` padding below the last row, or
  // any gap not covered by a `.row`) clears the multi-select + error banner —
  // matches desktop `onTreeClick`'s empty-area branch. A plain `click` (not
  // the pointer flow above) is enough since nothing here needs drag/swipe
  // tracking, and a tap that hits a `.row` never reaches this listener target.
  treePane.addEventListener("click", (e) => {
    if ((e.target as HTMLElement).closest(".row")) return;
    if (snap?.rows.some((r) => r.selected)) send({ SetSelection: { paths: [] } });
    if (snap?.notice?.severity === "error") statusEl.textContent = (session ? schemaHintText(session.schemaHint(snap.cursor)) : "") || t("web.status.ready");
  });
}

// Single tap = select only (⇧ ranges, ⌘/Ctrl toggles, via the shared
// `resolveClick` gesture resolution — mirrors desktop `onTreeClick`); double
// tap (same row, no modifiers, within DOUBLE_TAP_MS) opens the panel. The
// caret toggles expand; the kind badge now behaves like a normal tap (kind
// switching lives inside the edit panel).
function handleTap(target: HTMLElement, row: HTMLElement, clientY: number, mods: Mods) {
  const path = pathOf(row);
  if (!path) return;
  if (!snap) return;
  const armedTarget = (): Intent => {
    if (session) {
      const r = row.getBoundingClientRect();
      const relY = (clientY - r.top) / (r.height || 1);
      const slot = session.pointerSlot(path, relY);
      if (slot) return { SetPasteSlot: slot };
    }
    return { SetCursor: path };
  };
  const actBtn = target.closest<HTMLElement>("[data-act]");
  if (actBtn) {
    const act = actBtn.dataset.act;
    if (act === "grip") return;
    // Revealed Delete (swipe-to-delete): remove this row, then re-render closes it.
    if (act === "rowdel") {
      if ((snap?.clipboard_count ?? 0) > 0) {
        send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
        return;
      }
      openSwipeMain = null;
      openSwipeOff = 0;
      send({ SetCursor: path });
      send({ SetSelection: { paths: [path] } });
      const after = sendR("DeleteSelected");
      const isErr = after.notice?.severity === "error";
      send({ SetHostNotice: { key: isErr ? "core.delete.error" : "web.host.delete.ok", args: isErr ? [after.notice!.text] : [], source: "host-web" } });
      return;
    }
    if (act === "rowremark") {
      if ((snap?.clipboard_count ?? 0) > 0) {
        send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
        return;
      }
      openSwipeMain = null;
      openSwipeOff = 0;
      send({ SetCursor: path });
      send({ SetSelection: { paths: [path] } });
      send("Remark");
      return;
    }
    if (act === "caret") {
      // Paste mode freezes the selection (core's SetSelection is a no-op
      // there), so it falls back to positioning the paste target instead —
      // same guard as the plain-tap fallback below (ADR 0004 §1). That
      // never moves the cursor, though, and `ToggleExpand` is cursor-based —
      // without an explicit `SetCursor` here it kept toggling the frozen
      // clipboard source instead of whichever branch was actually tapped;
      // `SetCursor` doesn't disturb the just-armed paste slot (separate
      // field), so it's safe to send unconditionally.
      if ((snap?.clipboard_count ?? 0) > 0) send(armedTarget());
      else selectOnly(path);
      send({ SetCursor: path });
      return send("ToggleExpand");
    }
  }
  // A tap while a row is swiped open just closes it (no selection change).
  if (openSwipeMain) {
    const wasOpen = openSwipeMain;
    openSwipeMain.style.transform = "";
    openSwipeMain = null;
    setSwipeRevealed(wasOpen, false);
    if (wasOpen === row.querySelector(".row-main")) return;
  }
  const key = JSON.stringify(path);
  const now = Date.now();
  const plain = !mods.shiftKey && !mods.ctrlKey && !mods.metaKey;
  const isDouble = plain && key === lastTapKey && now - lastTapTime < DOUBLE_TAP_MS;
  lastTapKey = plain ? key : null;
  lastTapTime = now;
  if (isDouble) openPanel(path);
  // In paste mode the clipboard freezes the selection, so a tap positions the
  // paste target (`Into`/`After`) instead; the green `.drop-into`/
  // `.reorder-line` cue is the only highlight it gets (ADR 0004 §1) — the
  // cursor's own row style is suppressed while armed, see `web/touch/style.css`.
  else if ((snap?.clipboard_count ?? 0) > 0) send(armedTarget());
  else send({ SetSelection: { paths: resolveClick(snap, path, mods) } });
}

// ---- file I/O (host-owned, via fs.ts; the shared flows live in host-io.ts) ----
// Opener the shared sample helpers (samples.ts) call back into.
function openSample(text: string, format: SampleFormat) {
  openText(text, format, null, "sample", true);
}

function openText(
  text: string,
  format: "toml" | "json" | "yaml" | "yml",
  handle: FsHandle | null,
  name: string | null,
  asSample = false,
) {
  const next = replaceSession(session, text, format, io.err);
  if (!next) return;
  session = next;
  fileHandle = handle;
  fileName = name;
  setSampleMode(asSample);
  resetAnchor(); // a stale shift-range anchor must not survive the document swap
  // `strict_json` drives the per-row comment-advisory decoration — only the
  // host knows the real extension; the wasm core treats .json/.jsonc
  // identically. Set before the first dispatch so the initial snapshot's
  // rows already reflect it. Sample docs have no real filename (`name` is
  // the literal "sample"), so a `.json` sample is detected via `format`.
  const isPlainJson =
    format === "json" && (asSample || (!!name && /\.json$/i.test(name)));
  if (isPlainJson) session.setStrictJson(true);
  // A fresh Session always boots at core's default lang (`en`) — sync it to
  // the selector's persisted choice so status/error/About text match.
  snap = session.dispatch({ SetLang: getLang() });
  // One-shot advisory when the file already had comments at open (a JSONC
  // upgrade the user didn't ask for) — mirrors web/ui.ts's openText.
  if (isPlainJson && session.hadCommentsAtOpen()) {
    snap = session.dispatch({
      SetHostNotice: { key: "web.host.json-comments-detected", args: [], source: "host-web" },
    });
  }
  rawView = false;
  render();
}
// A file the OS opened us with (mobile file-association "Open with"), cold
// or warm — read via the same path `openTauriPath`'s Open Recent flow uses;
// the granted `content://`/`file://` URI reads no differently than an
// already-known Tauri path. The Rust side delivers a cold-start URL through
// BOTH `tauriOpenedUrls()` (drained at boot) and a possibly-already-live
// `"opened"` listener — dedupe here so a cold start never opens the same URL
// twice (which double-frees the previous `Session` and crashes the wasm).
const openedUrlsHandled = new Set<string>();
async function openOpenedUrl(url: string): Promise<void> {
  if (openedUrlsHandled.has(url)) return;
  openedUrlsHandled.add(url);
  const opened = await openTauriPath(url);
  if (!opened) {
    io.err(t("web.menu.recentGone"));
    return;
  }
  openText(opened.text, formatFromName(opened.name), opened.handle, opened.name);
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

// ---- keyboard shortcuts (external/Bluetooth keyboard on a touch device) ----
// Reuses the same pure `resolveKeyIntent` desktop's `onKey` (`web/ui.ts`) is
// built on, so the key→Intent mapping can't drift between surfaces. Most
// resolved intents are safe to `send()` as-is because touch already renders
// every core sub-mode they can produce (TypeFilter/Convert/Prompt/SchemaEnum/
// Help all reactively open/close their sheet in `render()`, proven by the
// existing toolbar/menu buttons that already dispatch them). Three intents
// are special-cased below because touch's own editing surfaces bypass the
// core modes those intents drive on desktop (`Mode::Edit`, `Mode::KindSwitch`)
// — touch has no rendering for those modes, so sending them raw would leave
// the UI silently stuck. `ToggleDetail` isn't core-mode-driven on touch at
// all (the detail sheet is host-local, see `openPanel`), so it's replaced
// with an equivalent local toggle.

// Mirrors desktop `ui.ts`'s `navSelect`: plain cursor navigation collapses
// the selection onto the new cursor row (skipped in paste mode, where arrows
// move the insertion slot instead).
function touchNavSelect(i: Intent) {
  send(i);
  if (snap && (snap.clipboard_count ?? 0) === 0) {
    send({ SetSelection: { paths: [snap.cursor] } });
  }
}

// Mirrors desktop `ui.ts`'s `toggleSelectedBranches`: Space on a single/zero
// selection is a plain expand/collapse toggle; on a multi-branch selection it
// expand/collapse-toggles every selected branch while keeping the selection.
function toggleSelectedBranches() {
  const branches = snap?.rows.filter((r) => r.selected && r.is_branch) ?? [];
  if (branches.length <= 1) return send("ToggleExpand");
  const keep = snap!.rows.filter((r) => r.selected).map((r) => r.path);
  for (const r of branches) {
    send({ SetCursor: r.path });
    send("ToggleExpand");
  }
  send({ SetSelection: { paths: keep } });
}

// `i`/`Enter` (ToggleDetail): the detail sheet is host-local state (no core
// mode backs it on touch, unlike desktop's `Mode::Detail`), so this toggles
// it directly instead of dispatching the core intent. No-op in wide layout,
// where the side pane is always visible.
function toggleDetailSheet() {
  if (isWide()) return;
  if (sheets.detail.classList.contains("open")) {
    dismissSheets();
    return;
  }
  const cur = cursorRow();
  if (cur) openPanel(cur.path);
}

// PageUp/PageDown step for the TypeFilter sheet, in nav-row units. Mirrors
// desktop `ui.ts`'s `typeFilterPageStep` (scroll-ratio, not pixel row
// heights), reading the touch filter sheet's scrollable body instead of
// `#tfPop`.
function touchTypeFilterPageStep(grid: TypeFilterView): number {
  const total = navRowCount(grid);
  const body = sheets.filter.querySelector<HTMLElement>(".sheet-body");
  if (!body || total === 0) return 1;
  const ratio = body.scrollHeight > 0 ? body.clientHeight / body.scrollHeight : 1;
  return Math.max(1, Math.min(total, Math.round(ratio * total)));
}

function onKey(ev: KeyboardEvent) {
  if (!session || !snap) return;
  // A focused text field (search, save-as path, external-edit textarea, URL
  // sheet) owns its own keys; the ext/url sheets are checked explicitly too
  // since their fields aren't always the very first thing focused.
  if (sheets.ext.classList.contains("open")) return;
  if (sheets.url.classList.contains("open")) return;
  const tag = (document.activeElement as HTMLElement)?.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;

  // `vshost: true` also suppresses `q`/QuitRequested — a web/touch surface has
  // no "quit the app" concept to bind it to.
  const result = resolveKeyIntent(snap.mode, ev.key, { ctrl: ev.ctrlKey || ev.metaKey, shift: ev.shiftKey }, rawView, true);
  if (!result) return;
  switch (result.kind) {
    case "intent":
      if (result.preventDefault) ev.preventDefault();
      if (result.intent === "ToggleDetail") return toggleDetailSheet();
      if (result.intent === "BeginEdit") {
        const cur = cursorRow();
        return cur ? openPanel(cur.path) : undefined;
      }
      if (result.intent === "OpenKindSwitch") {
        const cur = cursorRow();
        return cur ? openKindSheet(cur.path) : undefined;
      }
      if (result.intent === "Escape" && sheets.detail.classList.contains("open") && !isWide()) {
        closeSheets();
        return;
      }
      return send(result.intent);
    case "nav":
      if (result.preventDefault) ev.preventDefault();
      return touchNavSelect(result.intent);
    case "typefilter-page": {
      ev.preventDefault();
      const mode = snap.mode;
      if (typeof mode !== "object" || !("TypeFilter" in mode)) return;
      return send({ TypeFilterMove: [result.dir * touchTypeFilterPageStep(mode.TypeFilter), 0] });
    }
    case "native":
      if (result.preventDefault) ev.preventDefault();
      switch (result.action) {
        case "focus-search":
          return void searchInput.focus();
        case "undo":
          if ((snap?.clipboard_count ?? 0) > 0) {
            send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
            return;
          }
          return send("Undo");
        case "redo":
          if ((snap?.clipboard_count ?? 0) > 0) {
            send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
            return;
          }
          return send("Redo");
        case "save":
          if ((snap?.clipboard_count ?? 0) > 0) {
            send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
            return;
          }
          return openSaveSheet();
        case "open":
          if ((snap?.clipboard_count ?? 0) > 0) {
            send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
            return;
          }
          return openOpenSheet();
        case "toggle-branches":
          return toggleSelectedBranches();
        case "save-convert":
          return void runSaveConvertShared(snap!, { send, doSaveAsCopy: (path: string) => doSaveAsCopy(io, path) });
      }
  }
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
        if ((snap?.clipboard_count ?? 0) > 0) {
          send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
          return;
        }
        send("EnterTypeFilter");
        break;
      case "open":
        if ((snap?.clipboard_count ?? 0) > 0) {
          send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
          return;
        }
        openOpenSheet();
        break;
      case "actions":
        // Paste-armed (after Copy/Cut) → the FAB pastes at the cursor; otherwise
        // it opens the centralized Action menu (design doc §7, ADR 0009).
        if ((snap?.clipboard_count ?? 0) > 0) send("Paste");
        else send("OpenActionMenu");
        break;
      case "cyclefmt":
        if ((snap?.clipboard_count ?? 0) > 0) {
          send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
          return;
        }
        cycleSampleFormat(openSample); // no-op unless in sample mode
        break;
      case "save":
        if ((snap?.clipboard_count ?? 0) > 0) {
          send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
          return;
        }
        openSaveSheet();
        break;
      case "undo":
        if ((snap?.clipboard_count ?? 0) > 0) {
          send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
          return;
        }
        send("Undo");
        break;
      case "redo":
        if ((snap?.clipboard_count ?? 0) > 0) {
          send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
          return;
        }
        send("Redo");
        break;
      case "theme":
        toggleTheme();
        break;
      case "lang":
        if ((snap?.clipboard_count ?? 0) > 0) {
          send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
          return;
        }
        openLangSheet();
        break;
      case "info":
        if ((snap?.clipboard_count ?? 0) > 0) {
          send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
          return;
        }
        send("EnterHelp");
        break;
      case "expandall":
        if ((snap?.clipboard_count ?? 0) > 0) {
          send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
          return;
        }
        send("ExpandAll");
        break;
      case "collapseall":
        if ((snap?.clipboard_count ?? 0) > 0) {
          send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
          return;
        }
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
        if ((snap?.clipboard_count ?? 0) > 0) {
          send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
          return;
        }
        setRawView(!rawView);
        break;
      case "searchclear":
        if ((snap?.clipboard_count ?? 0) > 0) {
          send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
          return;
        }
        searchInput.value = "";
        searchInput.parentElement!.classList.remove("has-val");
        send({ SetFilter: "" });
        break;
    }
  });

  // Search → debounced SetFilter.
  searchInput.addEventListener("input", () => {
    if ((snap?.clipboard_count ?? 0) > 0) {
      searchInput.value = "";
      send({ SetHostNotice: { key: "core.clipboard.action-locked", args: [], source: "host-web" } });
      return;
    }
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
// exits, Help exits, and an open external-edit sheet sends Escape (clears
// `external_edit`).
function dismissSheets() {
  const tag = snap ? modeTag(snap.mode) : "Normal";
  if (sheets.ext.classList.contains("open")) {
    closeSheets();
    return send("Escape");
  }
  if (tag === "TypeFilter") return send("CommitTypeFilter");
  if (tag === "ActionMenu") {
    closeSheets();
    return send("ExitActionMenu");
  }
  if (tag === "Convert") return send("ExitConvert");
  // A prompt must be *answered*, not hidden — scrim/grab dismissal = "no"
  // (peel-on-dismiss; otherwise core stays stuck in Mode::Prompt).
  if (tag === "Prompt") return send({ PromptKey: "n" });
  // Same peel-on-dismiss requirement: Escape → `Session::escape()` →
  // `schema_enum_cancel()`, which also removes a freshly-added placeholder
  // (`created_on_add`) — mirrors desktop's `focusSchemaEnumSelect` Escape wiring.
  if (tag === "SchemaEnum") {
    closeSheets();
    return send("Escape");
  }
  // Same peel-on-dismiss requirement: swipe/scrim/grab dismissal of the
  // Add-type picker must cancel via Escape (nothing is inserted yet — no
  // placeholder to remove until AddPickerCommit/AddPickerPick), not just
  // hide the sheet.
  if (tag === "AddPicker") {
    closeSheets();
    return send("Escape");
  }
  // Same peel-on-dismiss requirement as Prompt/Convert/TypeFilter above: without
  // this, dismissing the Help sheet only removed its `.open` CSS class while
  // core stayed in `Mode::Help`, so the very next unrelated render() (e.g. a tap
  // selecting a different node) saw `tag === "Help"` again and reopened it.
  if (tag === "Help") return send("ExitHelp");
  closeSheets();
}

// ---- boot ----
async function main() {
  initTheme();
  const root = document.getElementById("root")!;
  root.innerHTML = appHTML();
  app = root.querySelector(".app")!;
  applyStaticI18n(app);
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
  clipBadge.title = t("web.badge.clipboard.clearTitle");
  clipBadge.addEventListener("click", () => {
    if ((snap?.clipboard_count ?? 0) > 0) send("Escape");
  });
  sheets.detail = app.querySelector(".detail-sheet")!;
  sheets.menu = app.querySelector(".menu-sheet")!;
  sheets.actions = app.querySelector(".actions-sheet")!;
  sheets.filter = app.querySelector(".filter-sheet")!;
  sheets.kind = app.querySelector(".kind-sheet")!;
  sheets.lang = app.querySelector(".lang-sheet")!;
  sheets.save = app.querySelector(".save-sheet")!;
  sheets.convert = app.querySelector(".convert-sheet")!;
  sheets.ext = app.querySelector(".ext-sheet")!;
  sheets.help = app.querySelector(".help-sheet")!;
  sheets.url = app.querySelector(".url-sheet")!;
  sheets.prompt = app.querySelector(".prompt-sheet")!;
  // Prompt answer buttons (incl. the header ×, data-pk="n") → PromptKey.
  bindPromptClicks(sheets.prompt, sendR);

  restoreDetailWidth();
  installTreeGestures();
  installShellHandlers();
  document.body.addEventListener("keydown", onKey);
  installSplitter();
  wireConvertDialog(convRefs(), {
    send,
    fileStem: () => fileStem(io),
    doSaveAsCopy: (path: string) => doSaveAsCopy(io, path),
    getSnap: () => snap,
  });

  const wasmUrl = new URL("../pkg/confy_ffi_bg.wasm", import.meta.url);
  await load(wasmUrl);
  // A warm-running app receiving a new "Open with" file-association intent.
  void onTauriOpened((url) => {
    void openOpenedUrl(url);
  });
  // Cold-start "Open with" file association; else ?url= deep-link; else sample.
  const openedUrls = await tauriOpenedUrls();
  const urlParam = new URLSearchParams(location.search).get("url");
  if (openedUrls.length > 0) {
    await openOpenedUrl(openedUrls[0]);
  } else if (urlParam) {
    await openFromUrl(io, openText, urlParam);
  } else {
    loadSample("toml", openSample);
  }
}

void main();
