// confy Web UI — a pure render of the `SessionSnapshot` + a stream of `Intent`s
// back. No editor logic lives here (PORTING §8.4; WEBUI.md). Drives the wasm
// `Session` via the typed `web/confy.ts` wrapper. This module is the
// orchestrator: boot/load, the new web-native chrome (toolbar / filter row /
// footer), `send(Intent)`, and the external-edit handshake. Tree rendering lives
// in `web/render.ts`; mode overlays not yet ported to dedicated chrome keep the
// keyboard-driven `#overlay` fallback.
import { load, Session } from "./confy.js";
import {
  canSaveAs,
  fsAccessAvailable,
  openTauriPath,
  pickOpenFile,
  tauriStartupFile,
  type FsHandle,
} from "./fs.js";
import { isVsCode, onHostMessage, post, trackVsCodeTheme } from "./vscode.js";
import type { ConfigFormat, HostToWebview } from "./vscode-protocol.js";
import { recentAdd, recentRemove, rebuildMenu, setupAppMenu } from "./menu.js";
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
  toggleTheme,
  type HostIo,
} from "./host-io.js";
import {
  cycleSampleFormat,
  inSampleMode,
  loadSample,
  setSampleMode,
  type SampleFormat,
} from "./samples.js";
import { currentKindLabel, editWidthCh, escapeHtml, IC_CARET, renderTree } from "./render.js";
import { helpBodyHTML } from "./help-content.js";
import { applyStaticI18n, availableLangs, getLang, LANG_DISPLAY_NAMES, setLang, t, tArgs } from "./i18n.js";
import type { Lang } from "./i18n.js";
import { resolveClick, resetAnchor, rowsInRect, setAnchor } from "./select.js";
import { foldedEntries, type ToolbarEntry } from "./toolbar-fold.js";
import { installDnd } from "./dnd.js";
import { panelHTML, wirePanel } from "./panel.js";
import { bindPromptClicks, promptButtonsHTML, promptQuestion } from "./prompt.js";
import { typeFilterHTML, wireTypeFilter } from "./typefilter.js";
import {
  type ConvertRefs,
  type ConvertSurface,
  extForTag,
  renderConvertDialog as renderConvertDialogShared,
  runSaveConvert as runSaveConvertShared,
  wireConvertDialog,
} from "./convert-dialog.js";
import type {
  ConvertView,
  ExternalEdit,
  Intent,
  ModeView,
  Path,
  PromptView,
  SessionSnapshot,
  TypeFilterView,
  ViewRow,
} from "./types.js";


let session: Session | null = null;
let snap: SessionSnapshot | null = null;
// Set by a completed marquee drag so the trailing `click` doesn't also fire a
// single-row selection (mouseup → click ordering).
let suppressClick = false;

// Host-owned file state. `fileHandle` is non-null only when the doc is backed
// by a real on-disk file opened/saved through the File System Access API.
let fileHandle: FsHandle | null = null;
let fileName: string | null = null;

// ---- DOM ----
function $<T extends HTMLElement = HTMLElement>(id: string): T {
  const el = document.getElementById(id);
  if (!el) throw new Error(`missing element #${id}`);
  return el as T;
}
const tree = $<HTMLDivElement>("tree");
const overlay = $("overlay");
const overlayScrim = $("overlayScrim");
// Tree vs read-only Raw text view (#12, read-only first). The Session stays the
// single source of truth; Raw is just `session.serialize()` rendered live.
let rawView = false;
const statusEl = $("status");
const errorEl = $("error");
const fmtPill = $("fmtPill");
const titleEl = $("title");
const selBadge = $("selBadge");
const clipBadge = $("clipBadge");
const themeBtn = $<HTMLButtonElement>("btnTheme");
const langBtn = $<HTMLButtonElement>("btnLang");
const langLabel = $("langLabel");
const openBtn = $<HTMLButtonElement>("btnOpen");
const saveBtn = $<HTMLButtonElement>("btnSave");
const saveAsBtn = $<HTMLButtonElement>("btnSaveAs");
const FS_AVAILABLE = fsAccessAvailable();

// VS Code webview host (third shell): the extension host owns file I/O and
// the undo entry point — see web/vscode.ts + editors/vscode/. All VS Code
// behavior differences below are gated on this flag so the browser and Tauri
// hosts are untouched when acquireVsCodeApi is absent.
const VSHOST = isVsCode();

// VS Code host-bridge state. Declared up here (not inside the bridge block
// further down) because `render()` — which reads hostDirty — sits above that
// block; module-scope order makes the closure resolve at boot regardless, but
// keeping them here reads cleanly. See the "VS Code host bridge" block below.
let hostDirty = false; // tab dirty mirror; authoritative over snap.is_dirty here
// Set when a text-changed failed to parse: the visible tree is stale, so
// posting edits from it would clobber newer raw text. Cleared by the next
// text-changed that parses.
let staleTree = false;

// The host surface the shared I/O flows (host-io.ts) are parameterized on.
const io: HostIo = {
  fsAvailable: FS_AVAILABLE && !VSHOST,
  canSaveAs: canSaveAs() && !VSHOST,
  getSnap: () => snap,
  send,
  batch,
  serialize: () => session?.serialize() ?? null,
  getFileName: () => fileName,
  getHandle: () => fileHandle,
  setHandle: (h) => {
    fileHandle = h;
  },
  ok: (msg) => setStatus(msg, ""),
  err: (msg) => setStatus("", msg),
  afterSaveAs: (handle, name) => {
    if (handle.path) {
      recentAdd(handle.path, name);
      void rebuildMenu();
    }
  },
  adoptFile: (text, format, handle, name) => openText(text, format, handle, name),
};

// ---- bootstrap ----
// ---- language ----
function updateLangUI() {
  langLabel.textContent = getLang() === "zh-TW" ? "繁" : "EN";
  applyStaticI18n();
}

function chooseLang(lang: Lang) {
  setLang(lang);
  if (session) send({ SetLang: getLang() });
  updateLangUI();
  render();
  void rebuildMenu();
}

// Menu > File > New: same as reloading the page with no startup file/URL —
// discard the current doc and load the default (toml) built-in sample. No
// confirmation, matching a browser refresh's unconditional discard.
function doNew(): void {
  loadSample("toml", openSample);
}

// Menu > File > Open Recent handler: reopen a previously-known Tauri path.
// Missing file (deleted/moved on disk) drops it from the list with an error.
async function openRecentPath(path: string): Promise<void> {
  const opened = await openTauriPath(path);
  if (!opened) {
    recentRemove(path);
    void rebuildMenu();
    setStatus("", t("web.menu.recentGone"));
    return;
  }
  openText(opened.text, formatFromName(opened.name), opened.handle, opened.name);
  recentAdd(path, opened.name);
  void rebuildMenu();
}

async function main() {
  initTheme();
  if (VSHOST) {
    document.body.classList.add("host-vscode");
    trackVsCodeTheme();
  }
  applyStaticI18n();
  updateLangUI();
  // Not awaited: menu build is several async IPC round-trips; don't delay wasm load on it.
  void setupAppMenu({
    doNew,
    doOpen,
    doSave,
    openSaveConvert: () => openSaveConvert(io),
    send,
    toggleTheme,
    chooseLang,
    openRecentPath,
    err: (msg) => setStatus("", msg),
  });
  const wasmUrl = new URL("./pkg/confy_ffi_bg.wasm", import.meta.url);
  await load(wasmUrl);
  updateSaveLabel();
  if (VSHOST) {
    onHostMessage(handleHostMsg);
    post({ type: "ready" });
    bindGlobal();
    return;
  }
  // Desktop (Tauri): open a file passed on the command line; else ?url= deep-link; else sample.
  const startup = await tauriStartupFile();
  const urlParam = new URLSearchParams(location.search).get("url");
  if (startup) {
    openText(startup.text, formatFromName(startup.name), startup.handle, startup.name);
    if (startup.path) {
      recentAdd(startup.path, startup.name);
      void rebuildMenu();
    }
  } else if (urlParam) {
    await openFromUrl(io, openText, urlParam);
  } else {
    loadSample("toml", openSample);
  }
  bindGlobal();
}

// Opener the shared sample helpers (samples.ts) call back into.
function openSample(text: string, format: SampleFormat) {
  openText(text, format, null, "sample", true);
}

function openText(
  text: string,
  format: "toml" | "json" | "yaml" | "yml",
  handle: FsHandle | null = null,
  name: string | null = null,
  asSample = false,
) {
  const next = replaceSession(session, text, format, (msg) => setStatus("", msg));
  if (!next) return;
  session = next;
  fileHandle = handle;
  fileName = name;
  setSampleMode(asSample);
  resetAnchor(); // a stale shift-range anchor must not survive the document swap
  // A fresh Session always boots at core's default lang (`en`) — sync it to the
  // selector's persisted choice so status/error/About text match immediately.
  snap = session.dispatch({ SetLang: getLang() });
  render();
}

// Switch between the interactive tree and the read-only serialized text. Raw is
// a *view* of the same document — no editing — so it just re-renders. The
// single toggle button's label is the view tapping/clicking switches TO
// (mirrors touch/app.ts's `setRawView`); `active` while in Raw.
function setRawView(raw: boolean) {
  rawView = raw;
  const vt = $("btnViewToggle");
  vt.textContent = raw ? t("web.toolbar.viewToggle.tree") : t("web.toolbar.viewToggle.raw");
  vt.classList.toggle("active", raw);
  render();
}

// Render whichever view is active. Raw shows `session.serialize()` (the live
// document, including unsaved edits) read-only; the tree is hidden but kept so
// toggling back is instant.
function renderRawOrTree() {
  const rawEl = $("raw");
  if (rawView) {
    rawEl.textContent = session!.serialize();
    rawEl.classList.remove("hidden");
    tree.classList.add("hidden");
  } else {
    rawEl.classList.add("hidden");
    tree.classList.remove("hidden");
    renderTree(tree, snap!, getEdit());
  }
}

// ---- render ----
function render() {
  if (!snap || !session) return;
  fmtPill.textContent = snap.doc_format.toUpperCase();
  fmtPill.classList.toggle("toggleable", inSampleMode());
  fmtPill.title = inSampleMode() ? "Sample — click to switch format" : "document format";
  document.body.classList.toggle("dirty", VSHOST ? hostDirty : snap.is_dirty);
  document.body.classList.toggle("paste-mode", (snap.clipboard_count ?? 0) > 0);
  titleEl.textContent = fileName ?? "confy";
  titleEl.title = fileName ?? ""; // full name on hover when the chip truncates
  setStatus(snap.status, snap.error ?? "");

  // Active type-filter indicator on the funnel button (same `.on` + dot
  // mechanism as the touch UI, driven by the shared snapshot flag).
  $("btnTypeFilter").classList.toggle("on", snap.type_filter_active);
  renderRawOrTree();
  focusInlineEdit();
  renderDetailPanel();
  renderTypeFilterPop();
  renderConvertDialog();
  renderOverlay();
  renderFooter();
  updateSaveLabel();
  if (snap.external_edit) openExternalEdit(snap.external_edit);
  if (snap.convert_write) {
    if (VSHOST) {
      const [outPath, outText] = snap.convert_write;
      post({
        type: "convert-save",
        suggestedName: outPath.split("/").pop() || outPath,
        text: outText,
      });
    } else {
      void doConvertWrite(io, snap.convert_write[0], snap.convert_write[1]);
    }
  }
  if (snap.quit) {
    setStatus("", "quit (reload to reopen)");
  }
}

function updateSaveLabel() {
  // ⌘S is the instant in-place path; the tooltip advertises what that fast
  // key actually does (the button itself opens the Save / Save As menu).
  const inPlace = fileHandle
    ? "⌘S saves in place"
    : FS_AVAILABLE
      ? "⌘S = Save as…"
      : "⌘S = download";
  saveBtn.title = `Save  (${inPlace})`;
}

function getEdit() {
  return typeof snap!.mode === "object" && "Edit" in snap!.mode
    ? snap!.mode.Edit
    : null;
}

function modeTag(m: ModeView): string {
  return typeof m === "string" ? m : Object.keys(m)[0];
}

// The detail aside (design slide-in panel) mirrors the `Detail` mode. While open
// it shows the shared editable node panel (`web/panel.ts`, identical to the touch
// UI's edit sheet) for the current cursor row, re-rendered from each snapshot so
// it always tracks the selection. Falls back to the static `detail_text` only
// when there is no cursor row (e.g. an empty document).
function renderDetailPanel() {
  const panel = $("detail");
  const tag = modeTag(snap!.mode);
  // A confirmation prompt (e.g. a panel value edit changing the type) floats
  // above whatever is open — leave the panel exactly as it is; the session
  // returns to Detail when the prompt resolves.
  if (tag === "Prompt") return;
  if (tag !== "Detail") {
    panel.classList.remove("open");
    return;
  }
  const body = $("detailBody");
  const cursorRow = snap!.rows.find((r) => r.is_cursor);
  if (!cursorRow) {
    body.innerHTML = `<pre class="mono">${escapeHtml(snap!.detail_text ?? "")}</pre>`;
  } else {
    body.innerHTML = panelHTML(cursorRow, parentIsInline(cursorRow.path));
    wirePanel(body, cursorRow, panelSend, openKindForRow, (msg) => setStatus("", msg), (msg) => {
      setStatus("", msg);
      send("ExitDetail");
    });
  }
  panel.classList.add("open");
}

// `send` variant for the shared panel: dispatch, re-render, and return the new
// snapshot so `wirePanel` can read `snapshot.error`. The global `send` returns
// void (other call sites depend on that), so this is a thin local adapter.
function panelSend(i: Intent): SessionSnapshot {
  snap = session!.dispatch(i);
  render();
  return snap;
}

// Open the desktop kind-switch popover for the panel's row, anchored at its Kind
// button (captured before `SetCursor` re-renders the panel and detaches it).
function openKindForRow(row: ViewRow) {
  const btn = $("detailBody").querySelector<HTMLElement>("[data-act=kindswitch]");
  const r = btn?.getBoundingClientRect();
  panelSend({ SetCursor: row.path });
  openKindMenuAt(
    row.path,
    r ? r.left : window.innerWidth - 240,
    r ? r.bottom + 4 : 120,
  );
}

// The `#overlay` keyboard fallback now serves only Help, Prompt, and the `K`
// kind-switch mode. Filter → native search box; TypeFilter → `#tfPop` popover;
// Convert → `#convDlg` dialog (all rendered by their own functions below).
function renderOverlay() {
  const m = snap!.mode;
  const tag = modeTag(m);
  if (tag === "Help" || tag === "Prompt" || tag === "KindSwitch") {
    overlay.classList.remove("hidden");
    overlayScrim.classList.remove("hidden");
  } else {
    overlay.classList.add("hidden");
    overlayScrim.classList.add("hidden");
    return;
  }
  if (tag === "Help") {
    const activeTab = (m as { Help: { tab: "Help" | "About" } }).Help.tab;
    // helpBodyHTML output is pre-escaped HTML (key spans) — insert raw.
    const body = helpBodyHTML(activeTab, snap!.doc_format, session!.aboutText());
    overlay.innerHTML =
      `<div class="overlay-head"><div class="help-tabs">` +
      `<button class="opt tab-btn${activeTab === "Help" ? " sel" : ""}" data-tab="Help">${t("web.help.tab.help")}</button>` +
      `<button class="opt tab-btn${activeTab === "About" ? " sel" : ""}" data-tab="About">${t("web.help.tab.about")}</button>` +
      `</div><button class="overlay-close" title="${t("web.common.close")}"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M6 6l12 12M18 6 6 18"/></svg></button></div>` +
      `<pre>${body}</pre>`;
    overlay.querySelectorAll<HTMLElement>("[data-tab]").forEach((btn) => {
      btn.addEventListener("click", () => {
        if (btn.dataset.tab !== activeTab) send("ToggleHelpTab");
      });
    });
    overlay.querySelector<HTMLElement>(".overlay-close")!.addEventListener("click", () => send("Escape"));
    // Move focus onto the panel so native scroll/text-select/copy (arrows,
    // shift-select, Ctrl/Cmd-C) act on the Help/About body instead of the
    // tree behind it; onKey's Help branch also stops list shortcuts firing.
    overlay.focus();
  } else if (tag === "Prompt") {
    const kind = (m as { Prompt: { kind: PromptView } }).Prompt.kind;
    overlay.innerHTML =
      `<h3>${escapeHtml(promptQuestion(kind, snap!.status ?? snap!.error ?? undefined))}</h3>` +
      promptButtonsHTML(kind);
  } else if (tag === "KindSwitch") {
    const ks = (m as { KindSwitch: { cursor: number; options: { label: string }[] } })
      .KindSwitch;
    overlay.innerHTML =
      `<h3>Kind</h3>` +
      ks.options
        .map(
          (o, i) =>
            `<div class="opt${i === ks.cursor ? " sel" : ""}">${escapeHtml(o.label)}</div>`,
        )
        .join("");
  }
}

// The `f` type-filter facet grid as a native popover (`#tfPop`/`#tfInner`). The
// grid HTML + per-cell wiring is shared with the touch UI (`typefilter.ts`);
// this keeps the popover open/placement logic. The popover stays open across
// re-renders while `Mode::TypeFilter` is active; the keyboard path (onKey) still
// drives the same mode for accessibility.
function renderTypeFilterPop() {
  const pop = $("tfPop");
  if (modeTag(snap!.mode) !== "TypeFilter") {
    pop.classList.remove("open");
    return;
  }
  const grid = (snap!.mode as { TypeFilter: TypeFilterView }).TypeFilter;
  const inner = $("tfInner");
  inner.innerHTML = typeFilterHTML(grid);
  wireTypeFilter(inner, grid, { send });
  if (!pop.classList.contains("open")) {
    const r = $("btnTypeFilter").getBoundingClientRect();
    pop.style.left = `${Math.max(6, Math.min(r.left, window.innerWidth - 260))}px`;
    pop.style.top = `${r.bottom + 4}px`;
    pop.classList.add("open");
  }
}

// The convert flow as a native `<dialog>`: a format `<select>`, an output-path
// `<input>`, and a warnings list. Open while `Mode::Convert`, closed otherwise.
// Format/path edits dispatch `SetConvertFormat`/`SetConvertPath`; the action
// button runs `ConvertRun` (or `ConvertConfirm` once warnings are shown).
function renderConvertDialog() {
  const surface = convSurface();
  if (modeTag(snap!.mode) !== "Convert") {
    if (surface.isOpen()) surface.close();
    return;
  }
  const cv = (snap!.mode as { Convert: ConvertView }).Convert;
  renderConvertDialogShared(convRefs(), cv, snap!);
}

// The native convert `<dialog>` wrapped as a `ConvertSurface` (desktop hosts the
// shared form in a native modal dialog; touch uses a bottom sheet instead).
function convSurface(): ConvertSurface {
  const dlg = $<HTMLDialogElement>("convDlg");
  return {
    isOpen: () => dlg.open,
    open: () => {
      dlg.showModal();
      // showModal() auto-focuses the first focusable control (the Format
      // select) by default; move focus to Cancel instead so opening the
      // dialog doesn't yank focus into a form field.
      $("convCancel").focus();
    },
    close: () => dlg.close(),
    onCancel: (cb) =>
      dlg.addEventListener("cancel", (e) => {
        e.preventDefault();
        cb();
      }),
  };
}

// The convert form's five children plus the dialog-backed surface, bundled as
// the refs the shared convert-dialog module operates on.
function convRefs(): ConvertRefs {
  return {
    surface: convSurface(),
    fmt: $<HTMLSelectElement>("convFmt"),
    path: $<HTMLInputElement>("convPath"),
    warns: $("convWarns"),
    run: $("convRun"),
    cancel: $("convCancel"),
  };
}

function renderFooter() {
  // Badges stay visible (design resting state); only text + `on` accent change.
  const n = snap!.rows.filter((r) => r.selected).length;
  selBadge.textContent = n === 0 ? t("web.badge.noneSelected") : tArgs("web.badge.nSelected", [String(n)]);
  selBadge.classList.toggle("on", n > 0);
  const cc = snap!.clipboard_count ?? 0;
  clipBadge.textContent = tArgs("web.badge.clipboard", [String(cc)]);
  clipBadge.classList.toggle("on", cc > 0);
}

// ---- keyboard → Intent (mirrors tui/keys.rs) ----
function onKey(ev: KeyboardEvent) {
  if (!session || !snap) return;
  if (!document.getElementById("ext-modal")!.classList.contains("hidden")) return;
  if (!document.getElementById("url-modal")!.classList.contains("hidden")) return;

  const m = snap.mode;
  if (typeof m === "object" && "Edit" in m) {
    if (ev.key === "Enter") return send("EditCommit");
    if (ev.key === "Escape") return send("EditCancel");
    if (ev.key === "Tab") {
      ev.preventDefault();
      return send("EditToggleField");
    }
    if (ev.key === "Backspace") return send("EditBackspace");
    if (ev.key.length === 1) return send({ EditChar: ev.key });
    return;
  }
  if (typeof m === "object" && "Prompt" in m) {
    if (ev.key === "y" || ev.key === "Y" || ev.key === "Enter")
      return send({ PromptKey: "y" });
    if (ev.key === "n" || ev.key === "N" || ev.key === "Escape")
      return send({ PromptKey: "n" });
    // Collision offers Overwrite (o) / Rename (r) besides cancel.
    if (ev.key === "o" || ev.key === "r") return send({ PromptKey: ev.key });
    return;
  }
  if (typeof m === "object" && "Convert" in m) {
    const step = (m as { Convert: { step: string } }).Convert.step;
    if (ev.key === "Escape") return send("Escape");
    if (step === "Format") {
      if (ev.key === "ArrowUp") return send({ ConvertMove: -1 });
      if (ev.key === "ArrowDown") return send({ ConvertMove: 1 });
      if (ev.key === "Enter") return send("ConvertPickFormat");
    } else if (step === "Path") {
      if (ev.key === "Enter")
        return runSaveConvertShared(snap!, {
          send,
          doSaveAsCopy: saveCopy,
        });
      if (ev.key === "Backspace") return send("ConvertPathBackspace");
      if (ev.key.length === 1) return send({ ConvertPathChar: ev.key });
    } else if (step === "Confirm") {
      if (ev.key === "y" || ev.key === "Y" || ev.key === "Enter")
        return send("ConvertConfirm");
      return send("Escape");
    }
    return;
  }
  if (modeTag(m) === "TypeFilter") {
    if (ev.key === "ArrowUp") return send({ TypeFilterMove: [-1, 0] });
    if (ev.key === "ArrowDown") return send({ TypeFilterMove: [1, 0] });
    if (ev.key === "ArrowLeft") return send({ TypeFilterMove: [0, -1] });
    if (ev.key === "ArrowRight") return send({ TypeFilterMove: [0, 1] });
    if (ev.key === " ") {
      ev.preventDefault();
      return send("TypeFilterToggle");
    }
    if (ev.key === "Enter") return send("CommitTypeFilter");
    if (ev.key === "Escape") return send("ExitTypeFilter");
    return;
  }
  if (modeTag(m) === "KindSwitch") {
    if (ev.key === "ArrowUp") return send({ KindSwitchMove: -1 });
    if (ev.key === "ArrowDown") return send({ KindSwitchMove: 1 });
    if (ev.key === "Enter") return send("KindSwitchCommit");
    if (ev.key === "Escape") return send("ExitKindSwitch");
    return;
  }
  // Help/About panel: pause every tree shortcut (j/k/e/a/d/c/x/v/… would
  // otherwise still reach the list underneath). Only close/tab-switch are
  // handled here; every other key is left alone so the browser's native
  // scroll/text-select/copy on the focused #overlay (see renderOverlay) works.
  if (modeTag(m) === "Help") {
    if (ev.key === "Escape" || ev.key === "?") return send("Escape");
    if (ev.key === "Tab") {
      ev.preventDefault();
      return send("ToggleHelpTab");
    }
    return;
  }

  const ctrl = ev.ctrlKey || ev.metaKey;
  if (ctrl && ev.key === "s") {
    ev.preventDefault();
    return void doSave();
  }
  if (ctrl && ev.key === "o") {
    ev.preventDefault();
    return void doOpen();
  }
  // Every other Ctrl/Cmd-modified key (Select All, native Edit-menu accelerators,
  // OS shortcuts, …) is left to the browser/OS — otherwise it falls through to
  // the modifier-free single-letter tree shortcuts below (e.g. Cmd+A becoming
  // the plain "a" → AddNode shortcut).
  if (ctrl) return;
  // Raw view is read-only serialized text, not the tree — every other key
  // (arrows/Home/End scrolling, Ctrl+A select-all, Ctrl+C copy, …) is left to
  // native `<pre>` behavior instead of the tree's single-letter shortcuts
  // (which would otherwise hijack e.g. Ctrl+A as "a" → AddNode).
  if (rawView) return;
  // ⇧+↑/↓ extends the multi-select range (cursor and selection move together).
  if (ev.shiftKey && (ev.key === "ArrowUp" || ev.key === "ArrowDown")) {
    ev.preventDefault();
    return send(ev.key === "ArrowUp" ? "ExtendSelectUp" : "ExtendSelectDown");
  }
  // Arrows / Home / End / Space natively scroll the focused container; we own
  // them as navigation (cursor scroll-into-view keeps the row visible). Without
  // this, ←/→ horizontally scroll the off-canvas detail panel into view.
  if (["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End", " "].includes(ev.key)) {
    ev.preventDefault();
  }
  switch (ev.key) {
    case "j": case "ArrowDown": return navSelect("CursorDown");
    case "k": case "ArrowUp": return navSelect("CursorUp");
    case "g": case "Home": return navSelect("CursorHome");
    case "G": case "End": return navSelect("CursorEnd");
    case "Enter": return toggleSelectedBranches();
    case " ": return send("ToggleDetail");
    // preventDefault: these open a text editor synchronously (inline input or the
    // external modal); without it the triggering keystroke leaks into the field.
    case "e": ev.preventDefault(); return send("BeginEdit");
    case "a": ev.preventDefault(); return send("AddNode");
    case "d": case "Delete": return send("DeleteSelected");
    case "c": return send("CopySelected");
    case "x": return send("CutSelected");
    case "v": return send("Paste");
    case "r": return send("Remark");
    case "z": return uiUndo();
    case "y": return uiRedo();
    case "s": return send("ToggleSelect");
    case "1": return send("ExpandLevel");
    case "2": return send("CollapseLevel");
    case "0": return send("CollapseAll");
    case "9": return send("ExpandAll");
    case "+": case "ArrowRight": return send({ Nudge: 1 });
    case "-": case "ArrowLeft": return send({ Nudge: -1 });
    case "/": ev.preventDefault(); return $("search").focus();
    case "f": return send("EnterTypeFilter");
    case "K": return send("OpenKindSwitch");
    case "C": return send("OpenConvert");
    case "i": return send("ToggleDetail");
    case "?": return send("EnterHelp");
    case "Escape": return send("Escape");
    case "q": if (VSHOST) return; return send("QuitRequested");
  }
}

function send(i: Intent) {
  if (!session) return;
  snap = session.dispatch(i);
  if (!batching) {
    render();
    notifyHost();
  }
}

// Dispatch every `send` inside `fn` with a single render at the end, so a
// multi-intent gesture (nav+select, per-branch toggles, save/convert seeding)
// rebuilds the tree DOM once instead of once per intent.
let batching = false;
function batch(fn: () => void) {
  if (batching) return fn(); // nested batches render at the outermost level
  batching = true;
  try {
    fn();
  } finally {
    batching = false;
    render();
    notifyHost();
  }
}

// ---- VS Code host bridge (no-op unless VSHOST) ----
// M1.5: VS Code's TextDocument is the single source of truth. The Session is
// a view: user mutations post `edit {serialize()}`; every document change the
// webview doesn't already hold comes back as `text-changed` and reloads the
// Session (expansion + cursor restored by path).
let hostInitiated = false; // reloading from host text — don't echo it back
let lastNotifyText: string | null = null;
// (hostDirty / staleTree are declared up near `const VSHOST` — render() reads
// hostDirty and sits above this block in module-scope order.)

function hostDispatch(i: Intent) {
  hostInitiated = true;
  try {
    send(i);
  } finally {
    hostInitiated = false;
  }
}

// Called after every render outside a batch (and once per batch): posts `edit`
// whenever the serialized text actually moved. Navigation-only intents change
// nothing and post nothing.
function notifyHost() {
  if (!VSHOST || !session || !snap) return;
  // Edit-mode gating (plan Decisions #2): defer while an inline edit is in
  // flight — BEFORE lastNotifyText moves, so an add→Esc rollback that lands
  // back on lastNotifyText posts nothing at all, and a commit posts one edit.
  if (typeof snap.mode === "object" && "Edit" in snap.mode) return;
  const text = session.serialize();
  if (text === lastNotifyText) return;
  lastNotifyText = text;
  if (hostInitiated || staleTree) return;
  hostDirty = true;
  document.body.classList.toggle("dirty", true);
  post({ type: "edit", text });
}

// Expansion + cursor survive a text-changed reload by path. A branch row is
// expanded iff its successor row is deeper (ViewRow carries no expanded flag).
function isExpandedRow(rows: ViewRow[], i: number): boolean {
  return rows[i].is_branch && i + 1 < rows.length && rows[i + 1].depth > rows[i].depth;
}

function captureTreeState(): { expanded: Path[]; cursor: Path } | null {
  if (!snap) return null;
  const expanded = snap.rows.filter((_, i) => isExpandedRow(snap!.rows, i)).map((r) => r.path);
  return { expanded, cursor: snap.cursor };
}

function restoreTreeState(saved: { expanded: Path[]; cursor: Path } | null) {
  if (!saved || !session || !snap) return;
  // Parents precede children in row order, so expanding in order always finds
  // the child row once its parent is open. dispatch() directly (not send) —
  // no notifyHost churn for view-only changes.
  for (const p of saved.expanded) {
    const rows = snap.rows;
    const key = JSON.stringify(p);
    const i = rows.findIndex((r) => JSON.stringify(r.path) === key);
    if (i >= 0 && !isExpandedRow(rows, i)) {
      snap = session.dispatch({ SetCursor: p });
      snap = session.dispatch("ToggleExpand");
    }
  }
  if (rowAt(saved.cursor)) snap = session.dispatch({ SetCursor: saved.cursor });
  render();
}

// Reload the Session from host-provided text (init dirty carry / text-changed).
function reloadFromHost(text: string, format: ConfigFormat, name: string | null) {
  const saved = captureTreeState();
  const before = session;
  hostInitiated = true;
  try {
    openText(text, format, null, name);
    // openText dispatches directly (not via send), so no notification fires on
    // its own — run notifyHost explicitly to prime lastNotifyText with this text.
    notifyHost();
  } finally {
    hostInitiated = false;
  }
  if (session !== before) {
    staleTree = false;
    restoreTreeState(saved);
  } else {
    // Parse failed: replaceSession left the old session in place. Freeze it —
    // see staleTree above. Status carries the reason (replaceSession already
    // wrote the parse error via its err callback; append the pause notice).
    staleTree = true;
    setStatus("", t("web.vscode.staleTree"));
  }
  // Visual cue while stale (grilling Q3): the tree dims but stays browsable.
  document.body.classList.toggle("stale-tree", staleTree);
}

function handleHostMsg(msg: HostToWebview) {
  switch (msg.type) {
    case "init": {
      // VS Code's display language is authoritative in this host (same
      // principle as theme). Apply before openText so its internal
      // SetLang(getLang()) picks it up.
      setLang(msg.lang === "zh-TW" ? "zh-TW" : "en");
      hostDirty = msg.dirty;
      hostInitiated = true;
      try {
        openText(msg.text, msg.format, null, msg.name);
        notifyHost();
      } finally {
        hostInitiated = false;
      }
      // openText leaves `session` untouched on a parse failure — surface it to
      // the host so it can offer the plain text editor instead.
      if (!session) {
        post({ type: "parse-error", message: errorEl.textContent || "parse failed" });
      }
      break;
    }
    case "text-changed":
      hostDirty = msg.dirty;
      reloadFromHost(msg.text, formatFromName(fileName ?? "config.toml"), fileName);
      break;
    case "saved":
      hostDirty = false;
      // Session may be stale (see staleTree) — the class toggle below still
      // runs via render(); marking the session clean is safe either way.
      hostDispatch("Save");
      break;
  }
}

// Path-keyed lookup into the current snapshot's rows, rebuilt lazily when the
// snapshot changes (replaces the O(rows) `find` + JSON.stringify per candidate
// that several hot paths — wheel, bool toggle, menus — used to run).
let rowMap: Map<string, ViewRow> | null = null;
let rowMapSnap: SessionSnapshot | null = null;
function rowAt(path: Path): ViewRow | undefined {
  if (!snap) return undefined;
  if (rowMapSnap !== snap || !rowMap) {
    rowMap = new Map(snap.rows.map((r) => [JSON.stringify(r.path), r]));
    rowMapSnap = snap;
  }
  return rowMap.get(JSON.stringify(path));
}

// With a multi-selection, Enter toggles every selected branch (each independently,
// like the per-row caret); a single/zero selection keeps the plain cursor toggle.
function toggleSelectedBranches() {
  const branches = snap?.rows.filter((r) => r.selected && r.is_branch) ?? [];
  if (branches.length <= 1) return send("ToggleExpand");
  const keep = snap!.rows.filter((r) => r.selected).map((r) => r.path);
  for (const r of branches) {
    send({ SetCursor: r.path });
    send("ToggleExpand");
  }
  send({ SetSelection: { paths: keep } }); // restore the multi-selection
}

// Plain cursor navigation that also collapses the selection onto the new cursor
// row, so the selected highlight (and what d/c/x act on) never decouples from
// the cursor bar. Skipped in paste mode, where arrows move the insertion slot
// and the selection is frozen.
function navSelect(i: Intent) {
  send(i);
  if (snap && (snap.clipboard_count ?? 0) === 0) {
    send({ SetSelection: { paths: [snap.cursor] } });
  }
}

// ---- external-edit async handshake (§8.2) ----
function openExternalEdit(ext: { initial: string; kind: unknown }) {
  const modal = $("ext-modal");
  const txt = $("ext-text") as HTMLTextAreaElement;
  txt.value = ext.initial;
  modal.classList.remove("hidden");
  txt.focus();
  const kind = ext.kind as { Value?: { path: unknown }; Comment?: { path: unknown } };
  const path = (kind.Value ?? kind.Comment)!.path as unknown;
  const confirm = $("ext-confirm");
  const cancel = $("ext-cancel");
  const close = () => {
    modal.classList.add("hidden");
    confirm.onclick = null;
    cancel.onclick = null;
    txt.onkeydown = null;
  };
  confirm.onclick = () => {
    close();
    if (kind.Value) send({ ApplyReplace: { path: path as never, text: txt.value } });
    else send({ ApplyEditComment: { path: path as never, text: txt.value } });
  };
  const doCancel = () => {
    close();
    send("Escape"); // peel the pending edit
  };
  cancel.onclick = doCancel;
  // Esc cancels/closes the modal (Enter stays free for newlines in the editor).
  txt.onkeydown = (e) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      doCancel();
    }
  };
}

// ---- save / open / download ----
// Instant Save: in-place write to an open handle, or a first Save-As if none
// yet. Shared with touch/app.ts (host-io.ts's `doQuickSave`) so the two hosts
// can't drift; the toolbar Save button and ⌘S both call this — the separate
// "Save / Convert…" panel (`openSaveConvert`) is the explicit destination/
// format-picking flow.
function doSave(): Promise<void> {
  // VS Code host: saving is the workbench's job (dirty tracking + save-ok
  // ack); the webview only requests it.
  if (VSHOST) {
    post({ type: "request-save" });
    return Promise.resolve();
  }
  return doQuickSave(io);
}

// Undo/redo single-owner rule (spec §Undo): in the VS Code host these forward
// to the workbench so its edit stack stays the sole entry point; the Session
// executes them only via the host's undo/redo callback messages.
function uiUndo() {
  if (VSHOST) post({ type: "request-undo" });
  else send("Undo");
}
function uiRedo() {
  if (VSHOST) post({ type: "request-redo" });
  else send("Redo");
}

// Same-format "save a copy" out of the Save/Convert panel. VS Code host: the
// destination pick is the host's save dialog, same as a convert output.
function saveCopy(path: string) {
  if (VSHOST) {
    const text = session?.serialize();
    if (text == null) return;
    send("ExitConvert");
    post({ type: "convert-save", suggestedName: path.split("/").pop() || path, text });
    return;
  }
  void doSaveAsCopy(io, path);
}

// Open: the FS Access API picker where available (keeps a handle for in-place
// save), else a native `<input type=file>` — file reading works in every
// browser, so the paste modal is no longer needed.
async function doOpen() {
  if (VSHOST) return; // tab-bound document — opening is VS Code's job
  if (FS_AVAILABLE) {
    const opened = await pickOpenFile();
    if (!opened) return;
    const fmt = formatFromName(opened.name);
    openText(opened.text, fmt, opened.handle, opened.name);
    if (opened.path) {
      recentAdd(opened.path, opened.name);
      void rebuildMenu();
    }
    tree.focus();
    return;
  }
  openViaFileInput();
}

// Native file-picker fallback (no FS Access API): read the chosen file's text
// and name. No on-disk handle, so a later Save falls back to Save As / download.
function openViaFileInput() {
  const input = $<HTMLInputElement>("fileInput");
  input.value = ""; // allow re-opening the same file
  input.onchange = async () => {
    const file = input.files?.[0];
    if (!file) return;
    const text = await file.text();
    openText(text, formatFromName(file.name), null, file.name);
    tree.focus();
  };
  input.click();
}

// Show the combined Open modal (local file browse + open from URL). Confirm/Esc
// are wired in bindGlobal, mirroring the paste-source load modal.
function openOpenModal() {
  const input = $<HTMLInputElement>("url-input");
  input.value = "";
  $("url-modal").classList.remove("hidden");
  // Focus the browse button, not the URL input/select — keeps focus inside the
  // modal (so Esc/Enter, wired on the modal element, still bubble correctly)
  // without landing the user in a text field ready to type.
  $("url-browse").focus();
}

// ---- pointer: click routing for every row affordance ----
function onTreeClick(ev: MouseEvent) {
  if (!session || !snap) return;
  if (suppressClick) {
    suppressClick = false;
    return;
  }
  const target = ev.target as HTMLElement;
  // Clicks inside the live edit input are handled by the input itself.
  if (target.closest("[data-editing]")) return;
  const rowEl = target.closest(".row") as HTMLElement | null;
  if (!rowEl) {
    // Click on empty tree space (including the wrap's blank area below the
    // last row) clears the multi-select (cursor stays put) and any error banner.
    if (snap.rows.some((r) => r.selected)) send({ SetSelection: { paths: [] } });
    if (snap.error) setStatus(snap.status, "");
    return;
  }
  const raw = rowEl.dataset.path;
  if (raw === undefined) return;
  const path = JSON.parse(raw) as Path;

  // Hover action buttons.
  if (target.closest('[data-act="add"]')) {
    // The `＋` is branch-only and always adds a *child* (unlike the TUI `a`,
    // which appends a sibling when the branch is collapsed).
    send({ SetCursor: path });
    return send("AddChild");
  }
  if (target.closest('[data-act="menu"]')) {
    // Toggle: a second click on the same row's ⋮ closes the menu.
    const pathKey = JSON.stringify(path);
    if ($("ctxMenu").classList.contains("open") && ctxMenuPath === pathKey) {
      return closePops();
    }
    // Read the anchor rect BEFORE SetCursor re-renders the tree — `render()`
    // rebuilds `tree.innerHTML`, detaching this button, and a detached node's
    // `getBoundingClientRect()` is all-zeros (popup would jump to 0,0).
    const r = (target.closest("button") as HTMLElement).getBoundingClientRect();
    selectForMenu(path);
    openCtxMenuAt(path, r.left, r.bottom + 4); // calls closePops(), clearing ctxMenuPath
    ctxMenuPath = pathKey;
    return;
  }
  // Kind badge → toggle the kind-conversion popover (second click on the same
  // badge closes it).
  const kindEl = target.closest("[data-kind]") as HTMLElement | null;
  if (kindEl) {
    const pathKey = JSON.stringify(path);
    if ($("kindMenu").classList.contains("open") && kindMenuPath === pathKey) {
      return closePops();
    }
    const r = kindEl.getBoundingClientRect(); // capture before the re-render
    send({ SetCursor: path });
    return openKindMenuAt(path, r.left, r.bottom + 4);
  }
  // Caret → toggle expand without editing.
  if (target.closest("[data-caret]")) {
    send({ SetCursor: path });
    return send("ToggleExpand");
  }
  // Editable cells: key → rename, value/comment-node → inline edit, trailing
  // comment → its own separate editor (web-native; the TUI bundles it with the
  // value, but the web edits it independently).
  const editEl = target.closest("[data-edit]") as HTMLElement | null;
  if (editEl) {
    if (editEl.dataset.edit === "note") return beginTrailingEdit(rowEl, path);
    send({ SetCursor: path });
    return send(editEl.dataset.edit === "key" ? "BeginRename" : "BeginEdit");
  }
  // In paste mode the clipboard freezes the selection, so a click can't reselect
  // — it moves the cursor, which is the paste destination (`After(cursor)`), and
  // `body.paste-mode` styling makes that target visible.
  if ((snap.clipboard_count ?? 0) > 0) {
    return send({ SetCursor: path });
  }
  // Plain row-body click → selection gesture (plain / ⇧range / ⌘toggle). Core
  // moves the cursor to the focal path; expand stays on the caret (design).
  // Manual double-click detection: a second *plain* click on the same path's
  // empty area within 350ms opens the Detail panel for that row.
  // Native `dblclick` is unreliable here (the first click re-renders/scrolls the
  // row), so we time it ourselves — and only empty-space clicks reach this far.
  const key = JSON.stringify(path);
  const plain = !ev.shiftKey && !ev.ctrlKey && !ev.metaKey;
  if (plain && lastBodyClick && lastBodyClick.key === key && Date.now() - lastBodyClick.t < 350) {
    lastBodyClick = null;
    send({ SetCursor: path });
    // Double-click toggles the Detail panel (open if closed, close if open).
    send("ToggleDetail");
    return;
  }
  lastBodyClick = plain ? { key, t: Date.now() } : null;
  send({ SetSelection: { paths: resolveClick(snap, path, ev) } });
}
// Last plain empty-area body click (path + time) for manual double-click detect.
let lastBodyClick: { key: string; t: number } | null = null;

// Toggle a boolean leaf's value true↔false, preserving its trailing comment.
// Shared by the wheel-over-value handler (and reusable elsewhere).
function toggleBool(path: Path) {
  const key = JSON.stringify(path);
  const row = snap?.rows.find((r) => JSON.stringify(r.path) === key);
  if (!row || row.scalar_type !== "Bool" || row.read_only) return;
  const next = (row.value ?? "").trim().toLowerCase() === "true" ? "false" : "true";
  const tc = row.trailing_comment;
  send({ SetCursor: path });
  send({ CommitEdit: { value: tc ? `${next}  ${tc}` : next, name: null } });
}

// Mouse-wheel over a row's *value* cell adjusts it (desktop affordance): a bool
// toggles, a number nudges ±1 (wheel up = +1). Anywhere else (no value cell, a
// read-only/other-typed value) falls through to normal page scrolling.
function onTreeWheel(ev: WheelEvent) {
  const valEl = (ev.target as HTMLElement).closest('[data-edit="val"]');
  if (!valEl) return;
  const rowEl = valEl.closest(".row") as HTMLElement | null;
  if (!rowEl?.dataset.path) return;
  const path = JSON.parse(rowEl.dataset.path) as Path;
  const row = snap?.rows.find((r) => JSON.stringify(r.path) === JSON.stringify(path));
  if (!row || row.read_only) return;
  if (row.scalar_type === "Bool") {
    ev.preventDefault();
    return toggleBool(path);
  }
  if (row.scalar_type === "Integer" || row.scalar_type === "Float") {
    ev.preventDefault();
    send({ SetCursor: path });
    send({ Nudge: ev.deltaY < 0 ? 1 : -1 });
  }
}

// Focus the live edit `<input>` (rendered by render.ts in Edit mode) and commit
// on Enter/blur via `CommitEdit`, cancel on Escape. Native text entry — the
// modal `EditChar` keyboard path is bypassed for the pointer UI.
function focusInlineEdit() {
  const edit = getEdit();
  if (!edit) return;
  const input = tree.querySelector("input[data-editing]") as HTMLInputElement | null;
  if (!input || document.activeElement === input) return;
  input.focus();
  const n = input.value.length;
  input.setSelectionRange(n, n);
  // Grow with the typed text (render.ts seeded the initial width the same way).
  input.addEventListener("input", () => {
    input.style.width = editWidthCh(input.value);
  });
  let done = false;
  const finish = (commit: boolean) => {
    if (done) return;
    done = true;
    if (!commit) return send("EditCancel");
    if (edit.field === "Value") {
      // The value `<input>` holds the value only (the trailing comment is edited
      // in its own cell). Re-attach the unchanged comment so a value edit never
      // drops it — core bundles `value␠␠# comment` and re-splits on commit.
      const r = snap?.rows.find(
        (row) => JSON.stringify(row.path) === JSON.stringify(snap!.cursor),
      );
      const tc = edit.is_comment ? undefined : r?.trailing_comment;
      const value = tc ? `${input.value}  ${tc}` : input.value;
      send({ CommitEdit: { value, name: null } });
    } else send({ CommitEdit: { value: null, name: input.value } });
  };
  input.onkeydown = (e) => {
    e.stopPropagation(); // native typing; don't leak to the global key handler
    if (e.key === "Enter") {
      e.preventDefault();
      finish(true);
    } else if (e.key === "Escape") {
      e.preventDefault();
      finish(false);
    }
  };
  input.onblur = () => finish(true);
}

// The rendered `.row` element for a path (the menu reopens against a still-live
// DOM after `closePops`, so we look it up rather than threading the element).
function rowElByPath(path: Path): HTMLElement | null {
  const key = JSON.stringify(path);
  return (
    Array.from(tree.querySelectorAll<HTMLElement>(".row")).find(
      (el) => el.dataset.path === key,
    ) ?? null
  );
}

// Line-comment leader for the current document (TOML/YAML `#`, JSON/JSONC `//`).
function commentPrefix(): string {
  return snap?.doc_format === "Json" ? "//" : "#";
}

// Edit (or append, when the row has none yet) a node's **trailing** inline
// comment in its own small `<input>`, separate from the value cell. A transient
// web-local affordance: it lives in the DOM only until commit/cancel (no core
// edit-mode), so opening it issues no `send()` and survives until the user is
// done. Enter/blur → `SetTrailing` (empty clears); Escape restores via re-render.
function beginTrailingEdit(rowEl: HTMLElement, path: Path) {
  if (!snap) return;
  const row = snap.rows.find((r) => JSON.stringify(r.path) === JSON.stringify(path));
  const input = document.createElement("input");
  input.className = "cell-input mono comment-input";
  input.dataset.editing = "trailing";
  input.value = row?.trailing_comment ?? "";
  input.placeholder = `${commentPrefix()} comment`;
  // Open at the existing comment's own width and grow while typing (same
  // formula as render.ts's seeded editors; CSS min/max-width clamp).
  input.style.width = editWidthCh(input.value);
  input.addEventListener("input", () => {
    input.style.width = editWidthCh(input.value);
  });
  const note = rowEl.querySelector('[data-edit="note"]');
  const actions = rowEl.querySelector(".row-actions");
  if (note) note.replaceWith(input);
  else if (actions) rowEl.insertBefore(input, actions);
  else rowEl.appendChild(input);
  input.focus();
  const n = input.value.length;
  input.setSelectionRange(n, n);
  let done = false;
  const finish = (commit: boolean) => {
    if (done) return;
    done = true;
    if (!commit) return render(); // restore the original row DOM
    const text = input.value.trim();
    let comment: string | null;
    if (text === "") comment = null;
    else {
      const pfx = commentPrefix();
      comment = text.startsWith(pfx) ? text : `${pfx} ${text}`;
    }
    send({ SetTrailing: { path, comment } });
  };
  input.onkeydown = (e) => {
    e.stopPropagation(); // native typing; don't leak to the global key handler
    if (e.key === "Enter") {
      e.preventDefault();
      finish(true);
    } else if (e.key === "Escape") {
      e.preventDefault();
      finish(false);
    }
  };
  input.onblur = () => finish(true);
}

// ---- popovers: kind switch + context menu ----
// Only the click-driven menus live here; the type-filter `#tfPop` is mode-driven
// and managed solely by `renderTypeFilterPop`, so these never touch it (else the
// two would open/close together).
function clickMenus(): HTMLElement[] {
  return [$("kindMenu"), $("ctxMenu"), $("moreMenu"), $("langMenu"), $("saveMenu")];
}
// One shared outside-click closer; closing always removes it so listeners never
// accumulate (a stale one fires on the reopening click and flashes the menu shut
// — the "must click elsewhere first to reopen" bug).
let popCloser: ((e: MouseEvent) => void) | null = null;
// The path the kind menu is currently open for, so a second click on the same
// badge toggles it shut (rather than reopening).
let kindMenuPath: string | null = null;
// Same idea for the per-row ⋮ context menu: a second click on the same row's ⋮
// closes it.
let ctxMenuPath: string | null = null;
function closePops() {
  for (const m of clickMenus()) m.classList.remove("open");
  kindMenuPath = null;
  ctxMenuPath = null;
  if (popCloser) {
    document.removeEventListener("click", popCloser);
    popCloser = null;
  }
}
function anyClickMenuOpen(): boolean {
  return clickMenus().some((m) => m.classList.contains("open"));
}
function placePopAt(pop: HTMLElement, x: number, y: number) {
  // Synchronously close any other click-menu and drop its stale closer *before*
  // opening, so the reopening click can't immediately re-close the new menu.
  closePops();
  pop.style.left = `${Math.max(6, Math.min(x, window.innerWidth - 220))}px`;
  // Reveal first so the menu has a measurable height, then clamp `top` so a menu
  // opened near the bottom of the viewport slides up to stay fully visible
  // rather than spilling below the fold.
  pop.style.top = "0px";
  pop.classList.add("open");
  const h = pop.offsetHeight;
  pop.style.top = `${Math.max(8, Math.min(y, window.innerHeight - h - 8))}px`;
  // Defer registering the outside-click closer so this very click doesn't trip it.
  setTimeout(() => {
    popCloser = (e: MouseEvent) => {
      if (!(e.target as HTMLElement).closest(".pop")) closePops();
    };
    document.addEventListener("click", popCloser);
  }, 0);
}

function openKindMenuAt(path: Path, x: number, y: number) {
  const opts = session!.kindOptions(path);
  if (!opts.length) {
    setStatus("no kind conversions for this node", "");
    return;
  }
  const menu = $("kindMenu");
  // Disabled "Current: …" header (design's `目前：…` row) so the popup shows the
  // node's present kind/notation before listing the alternatives.
  const key = JSON.stringify(path);
  const row = snap!.rows.find((r) => JSON.stringify(r.path) === key);
  const cur = row
    ? `<button class="menu-item" disabled><span class="ic">${IC_CARET}</span>Current: ${escapeHtml(currentKindLabel(row))}</button><div class="menu-sep"></div>`
    : "";
  menu.innerHTML =
    `<div class="menu-label">Convert kind</div>` +
    cur +
    opts
      .map(
        (o, i) =>
          `<button class="menu-item" data-i="${i}"><span class="ic">${IC_CARET}</span>${escapeHtml(o.label)}</button>`,
      )
      .join("");
  placePopAt(menu, x, y); // calls closePops() first, which clears kindMenuPath
  kindMenuPath = key;
  menu.querySelectorAll<HTMLElement>("[data-i]").forEach((b) => {
    const i = Number(b.dataset.i);
    b.onclick = () => {
      closePops();
      send({ CommitKind: { path, target: opts[i].target } });
    };
  });
}

// A menu row's action: a plain Intent to `send`, or a custom callback (for the
// web-local trailing-comment editor, which isn't a one-shot Intent).
type CtxAction = Intent | (() => void);

// Whether `path`'s immediate parent is a single-line container (TOML inline
// table, JSON single-line object/array, YAML flow map/seq — all projected as
// `Format::Inline` by core). Such containers can't hold comments — core
// rejects `SetTrailingComment`/`InsertComment` into them — so this gates
// "Append comment" and the Detail panel's trailing-comment input up front
// instead of round-tripping to a failed mutation + error banner.
function parentIsInline(path: Path): boolean {
  if (!snap || path.length === 0) return false;
  const parentKey = JSON.stringify(path.slice(0, -1));
  const parent = snap.rows.find((r) => JSON.stringify(r.path) === parentKey);
  return parent?.format === "Inline";
}

function buildCtxMenu(path: Path): HTMLElement {
  const key = JSON.stringify(path);
  const row = snap!.rows.find((r) => JSON.stringify(r.path) === key);
  const cc = snap!.clipboard_count ?? 0;
  const isComment = row?.type_label === "comment";
  // "Append comment" attaches a *new* trailing comment; once a row has one you
  // edit it by clicking it (so the item is offered only when there is none).
  // Also unavailable on a member of an inline/flow container (see `parentIsInline`).
  const canAppend =
    !!row && !isComment && !row.read_only && !row.trailing_comment && !parentIsInline(path);
  const appendComment = () => {
    const el = rowElByPath(path);
    if (el) beginTrailingEdit(el, path);
  };
  const items: Array<[string, CtxAction, boolean]> = [
    ["Edit", "BeginEdit", true],
    ["Add child", "AddChild", !!row?.is_branch],
    ["Append sibling", "AddSibling", path.length > 0],
    ["Copy", "CopySelected", true],
    ["Cut", "CutSelected", true],
    ["Paste", "Paste", cc > 0],
    ["Delete", "DeleteSelected", true],
    ["Toggle comment", "Remark", true],
    ["Append comment", appendComment, canAppend],
    ["Detail", "ToggleDetail", true],
  ];
  const menu = $("ctxMenu");
  menu.innerHTML = items
    .map(
      ([label, , enabled], i) =>
        `<button class="menu-item" data-i="${i}"${enabled ? "" : " disabled"}>${escapeHtml(label)}</button>`,
    )
    .join("");
  menu.querySelectorAll<HTMLElement>("[data-i]:not([disabled])").forEach((b) => {
    const i = Number(b.dataset.i);
    b.onclick = () => {
      closePops();
      const action = items[i][1];
      if (typeof action === "function") action();
      else send(action);
    };
  });
  return menu;
}
function openCtxMenuAt(path: Path, x: number, y: number) {
  placePopAt(buildCtxMenu(path), x, y);
}

// A toolbar button is "folded" (→ belongs in the ⋯ menu) when its group is
// CSS `display:none`'d at the current width, so it isn't laid out (mirrors the
// touch UI's `isFolded`). Without this check the menu listed every candidate
// unconditionally — a duplicate of buttons that were still visible in the header.
function isToolbarFolded(id: string): boolean {
  const el = document.getElementById(id);
  return !!el && el.offsetParent === null;
}

// The language popup opener, shared by the toolbar `#btnLang` click and the
// folded ⋯-menu row so both paths open the identical popup (never diverge).
function openLangMenuNear(el: HTMLElement) {
  const r = el.getBoundingClientRect();
  placePopAt(buildLangMenu(), r.left, r.bottom + 4);
}

// The single source of truth for every toolbar control that can fold into the
// "⋯ More" menu, in toolbar display order. `key` is the element id
// `isToolbarFolded` checks; the ⋯ menu is derived from this list via
// `foldedEntries` rather than a hand-maintained parallel array, so a button
// added to `#editGroup`/`#navGroup`/`#viewTabs` (marked `data-foldable`) can't
// silently disappear when its group folds without also getting a menu entry
// (enforced by `web/toolbar-fold.spec.mjs`).
const TOOLBAR_ENTRIES: ToolbarEntry[] = [
  { key: "btnUndo", labelKey: "web.toolbar.undo.title", run: () => uiUndo() },
  { key: "btnRedo", labelKey: "web.toolbar.redo.title", run: () => uiRedo() },
  { key: "btnTheme", labelKey: "web.toolbar.theme.title", run: toggleTheme },
  { key: "btnLang", labelKey: "web.toolbar.lang.title", run: () => openLangMenuNear($("btnMore")) },
  { key: "btnInfo", labelKey: "web.toolbar.info.title", run: () => send("EnterHelp") },
  { key: "btnExpandAll", labelKey: "web.toolbar.expandAll.title", run: () => send("ExpandAll") },
  { key: "btnCollapseAll", labelKey: "web.toolbar.collapseAll.title", run: () => send("CollapseAll") },
  { key: "btnViewToggle", labelKey: "web.toolbar.viewToggle.title", run: () => setRawView(!rawView) },
];

// The "⋯ More" overflow menu (shown only under the narrow breakpoint): only the
// secondary actions the CSS actually hides from the toolbar / filter row at the
// current width, as a popup.
function buildMoreMenu(): HTMLElement {
  const items = foldedEntries(TOOLBAR_ENTRIES, isToolbarFolded);
  const menu = $("moreMenu");
  menu.innerHTML = items
    .map((e, i) => `<button class="menu-item" data-i="${i}">${escapeHtml(t(e.labelKey))}</button>`)
    .join("");
  menu.querySelectorAll<HTMLElement>("[data-i]").forEach((b) => {
    const i = Number(b.dataset.i);
    b.onclick = () => {
      closePops();
      items[i].run();
    };
  });
  return menu;
}

// The language-picker popup (`#langMenu`): same `.pop`/`.menu-item` anatomy as
// `#moreMenu`/`#kindMenu` — a list of choices, the active one marked `.sel`
// with a check icon, click applies and closes. Scales to any number of
// languages (`availableLangs()`), not hardcoded to 2.
function buildLangMenu(): HTMLElement {
  const cur = getLang();
  const menu = $("langMenu");
  menu.innerHTML =
    `<div class="menu-label">${escapeHtml(t("web.toolbar.lang.title"))}</div>` +
    availableLangs()
      .map((lang) => {
        const sel = lang === cur;
        return `<button class="menu-item${sel ? " sel" : ""}" data-lang="${escapeHtml(lang)}"><span class="ic">${sel ? "✓" : ""}</span>${escapeHtml(LANG_DISPLAY_NAMES[lang])}</button>`;
      })
      .join("");
  menu.querySelectorAll<HTMLElement>("[data-lang]").forEach((b) => {
    const lang = b.dataset.lang as Lang;
    b.onclick = () => {
      closePops();
      chooseLang(lang);
    };
  });
  return menu;
}

// The split-button chevron next to Save opens this — Save itself (the pill's
// main tap target) always saves in place; this menu only offers the other
// destination/format-picking path.
function buildSaveMenu(): HTMLElement {
  const menu = $("saveMenu");
  menu.innerHTML = `<button class="menu-item" data-act="saveas">${escapeHtml(t("web.toolbar.saveAs.title"))}</button>`;
  menu.querySelectorAll<HTMLElement>("[data-act]").forEach((b) => {
    b.onclick = () => {
      closePops();
      openSaveConvert(io);
    };
  });
  return menu;
}
function openSaveMenuNear(el: HTMLElement) {
  const r = el.getBoundingClientRect();
  placePopAt(buildSaveMenu(), r.left, r.bottom + 4);
}

// Live search: the always-visible box owns the filter text and dispatches
// `SetFilter` (debounced) on every keystroke. No `Mode::Filter` is entered.
function bindSearch() {
  const box = $<HTMLInputElement>("search");
  const wrap = $("searchWrap");
  box.disabled = false;
  let timer = 0;
  // The design's `.search.has-val .clear` reveals the × only when there's text.
  const syncClear = () => wrap.classList.toggle("has-val", box.value !== "");
  const clear = () => {
    box.value = "";
    syncClear();
    send({ SetFilter: "" });
  };
  box.addEventListener("input", () => {
    syncClear();
    clearTimeout(timer);
    timer = window.setTimeout(() => send({ SetFilter: box.value }), 80);
  });
  // Esc clears the query when there's text; when already empty it drops focus
  // back to the tree.
  box.addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    e.stopPropagation();
    if (box.value !== "") clear();
    else {
      box.blur();
      tree.focus();
    }
  });
  $("searchClear").addEventListener("click", () => {
    clear();
    box.focus();
  });
}

function bindConvertDialog() {
  wireConvertDialog(convRefs(), {
    send,
    fileStem: () => fileStem(io),
    doSaveAsCopy: saveCopy,
    getSnap: () => snap,
  });
}

function bindGlobal() {
  tree.addEventListener("keydown", onKey);
  // Prompt overlay Yes/No/… buttons (renderOverlay rewrites the innerHTML per
  // render; the delegated listener on the stable #overlay survives).
  bindPromptClicks(overlay, (i) => send(i));
  // Bound to the wrap (not `#tree`, which is only as tall as its rows) so a
  // click on the blank space below the last row also reaches `onTreeClick`'s
  // "empty area" branch — same wrap `installMarquee`'s mousedown uses.
  $("treeWrap").addEventListener("click", onTreeClick);
  tree.addEventListener("contextmenu", onTreeContext);
  tree.addEventListener("wheel", onTreeWheel, { passive: false });
  installMarquee();
  installDnd(tree, () => snap, send);
  $("detailClose").addEventListener("click", () => send("ExitDetail"));
  // Escape closes an open click-menu before anything else handles it (the
  // mode-driven #tfPop is closed by its own ExitTypeFilter path instead).
  document.addEventListener("keydown", (ev) => {
    if (ev.key === "Escape" && anyClickMenuOpen()) {
      ev.stopPropagation();
      closePops();
    }
  });
  // A press outside the type-filter popup (and not on its toolbar button) closes
  // it, keeping the filter applied (CommitTypeFilter). Uses `mousedown`, not
  // `click`: toggling a facet cell re-renders `#tfInner` inside the cell's own
  // click handler, detaching `ev.target` — a later `click` would then see an
  // orphaned node (`closest("#tfPop")` → null) and wrongly close the popup.
  // `mousedown` fires before that re-render, so the target is still attached.
  document.addEventListener("mousedown", (ev) => {
    if (!snap || modeTag(snap.mode) !== "TypeFilter") return;
    const t = ev.target as HTMLElement;
    if (t.closest("#tfPop") || t.closest("#btnTypeFilter")) return;
    send("CommitTypeFilter");
  });
  tree.focus();
  document.body.addEventListener("keydown", (ev) => {
    if (document.activeElement !== tree && noModalOpen()) {
      // Don't hijack text entry / native form widgets (search box, convert
      // dialog inputs) — they own their own keys. A focused BUTTON must NOT be
      // guarded, or every shortcut dies after clicking a toolbar/row button.
      const tag = (document.activeElement as HTMLElement)?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
      onKey(ev);
    }
  });

  bindSearch();
  bindConvertDialog();
  openBtn.addEventListener("click", openOpenModal);
  saveBtn.addEventListener("click", () => void doSave());
  saveAsBtn.addEventListener("click", () => openSaveMenuNear(saveAsBtn));
  fmtPill.addEventListener("click", () => cycleSampleFormat(openSample)); // no-op unless in sample mode
  themeBtn.addEventListener("click", toggleTheme);
  langBtn.addEventListener("click", (e) => {
    // Toggle: a second click on the language button while its menu is open closes it.
    if ($("langMenu").classList.contains("open")) return closePops();
    openLangMenuNear(e.currentTarget as HTMLElement);
  });
  $("btnInfo").addEventListener("click", () => send("EnterHelp"));
  $("btnMore").addEventListener("click", (e) => {
    // Toggle: a second click on ⋯ while its menu is open closes it.
    if ($("moreMenu").classList.contains("open")) return closePops();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    placePopAt(buildMoreMenu(), r.right - 200, r.bottom + 4);
  });
  $("btnUndo").addEventListener("click", () => uiUndo());
  $("btnRedo").addEventListener("click", () => uiRedo());
  $("btnExpandAll").addEventListener("click", () => send("ExpandAll"));
  $("btnCollapseAll").addEventListener("click", () => send("CollapseAll"));
  $("btnTypeFilter").addEventListener("click", () =>
    // Toggle: open the popup, or close it keeping the filter applied.
    send(snap && modeTag(snap.mode) === "TypeFilter" ? "CommitTypeFilter" : "EnterTypeFilter"),
  );
  $("btnViewToggle").addEventListener("click", () => setRawView(!rawView));

  const closeUrlModal = () => {
    $("url-modal").classList.add("hidden");
    tree.focus();
  };
  $("url-confirm").addEventListener("click", () => {
    const url = $<HTMLInputElement>("url-input").value.trim();
    $("url-modal").classList.add("hidden");
    if (url) void openFromUrl(io, openText, url);
  });
  $("url-cancel").addEventListener("click", closeUrlModal);
  $("url-browse").addEventListener("click", () => {
    $("url-modal").classList.add("hidden");
    void doOpen();
  });
  // Enter confirms, Esc cancels (onKey early-returns while the modal is open).
  $("url-modal").addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      $<HTMLButtonElement>("url-confirm").click();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      closeUrlModal();
    }
  });
}

// Marquee (rubber-band) selection: drag over empty tree space (or a row body)
// to rubber-band a rectangle and select the rows it touches. ⇧/⌘/Ctrl unions
// with the current selection; a plain drag replaces it. A non-moving press is
// left to `onTreeClick` (single-row select); a real drag suppresses that click.
function installMarquee() {
  const wrap = $("treeWrap");
  const box = $("marquee");
  let sx = 0,
    sy = 0,
    active = false,
    moved = false,
    additive = false;
  wrap.addEventListener("mousedown", (ev) => {
    if (ev.button !== 0) return;
    // Raw view is read-only serialized text (`.raw-view` sets user-select:text) —
    // leave mouse drags entirely to native text selection, not row rubber-banding.
    if (rawView) return;
    // Don't hijack grips (native drag), buttons, inputs, or open popovers.
    if ((ev.target as HTMLElement).closest("[data-grip],button,input,.pop")) return;
    sx = ev.clientX;
    sy = ev.clientY;
    active = true;
    moved = false;
    additive = ev.ctrlKey || ev.metaKey || ev.shiftKey;
  });
  window.addEventListener("mousemove", (ev) => {
    if (!active) return;
    const dx = ev.clientX - sx;
    const dy = ev.clientY - sy;
    if (!moved && Math.hypot(dx, dy) < 4) return; // tolerance: a click, not a drag
    moved = true;
    const wr = wrap.getBoundingClientRect();
    box.style.left = `${Math.min(sx, ev.clientX) - wr.left + wrap.scrollLeft}px`;
    box.style.top = `${Math.min(sy, ev.clientY) - wr.top + wrap.scrollTop}px`;
    box.style.width = `${Math.abs(dx)}px`;
    box.style.height = `${Math.abs(dy)}px`;
    box.style.display = "block";
  });
  window.addEventListener("mouseup", (ev) => {
    if (!active) return;
    active = false;
    if (!moved || !snap) return;
    box.style.display = "none";
    const rect = new DOMRect(
      Math.min(sx, ev.clientX),
      Math.min(sy, ev.clientY),
      Math.abs(ev.clientX - sx),
      Math.abs(ev.clientY - sy),
    );
    const hit = rowsInRect(tree, rect);
    let paths = hit;
    if (additive) {
      const byKey = new Map<string, Path>();
      for (const r of snap.rows.filter((r) => r.selected)) byKey.set(JSON.stringify(r.path), r.path);
      for (const p of hit) byKey.set(JSON.stringify(p), p);
      paths = [...byKey.values()];
    }
    suppressClick = true;
    // Safety net: if the trailing `click` doesn't reach `onTreeClick` (mouseup
    // over the wrap, not a row), clear the flag so the next real click works.
    setTimeout(() => (suppressClick = false), 0);
    // The marquee result becomes the base a following shift-range unions onto.
    setAnchor(hit.length ? hit[hit.length - 1] : null, paths);
    send({ SetSelection: { paths } });
  });
}

function onTreeContext(ev: MouseEvent) {
  if (!session || !snap) return;
  const rowEl = (ev.target as HTMLElement).closest(".row") as HTMLElement | null;
  if (!rowEl || rowEl.dataset.path === undefined) return;
  ev.preventDefault();
  const path = JSON.parse(rowEl.dataset.path) as Path;
  selectForMenu(path);
  openCtxMenuAt(path, ev.clientX, ev.clientY);
}

// Align the selection with the row a menu is opening for, so the menu acts on
// what was clicked — Copy/Cut/Delete operate on the *selection*, so a bare
// SetCursor would silently target a different (still-selected) node. Standard
// desktop rule: opening the menu on a row inside the current multi-selection
// keeps it (act on all); on a row outside it, select just that row.
function selectForMenu(path: Path) {
  const key = JSON.stringify(path);
  const inSel = snap?.rows.some((r) => r.selected && JSON.stringify(r.path) === key);
  if (inSel) send({ SetCursor: path });
  else send({ SetSelection: { paths: [path] } });
}

function noModalOpen(): boolean {
  return (
    document.getElementById("ext-modal")!.classList.contains("hidden") &&
    document.getElementById("url-modal")!.classList.contains("hidden")
  );
}

// ---- utils ----
function setStatus(status: string | undefined, error: string | undefined) {
  statusEl.textContent = status ?? "";
  const err = error ?? "";
  errorEl.textContent = err;
  errorEl.classList.toggle("hidden", err === "");
}

main().catch((e) => setStatus("", String(e)));
