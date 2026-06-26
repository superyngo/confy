// confy Web UI — a pure render of the `SessionSnapshot` + a stream of `Intent`s
// back. No editor logic lives here (PORTING §8.4; WEBUI.md). Drives the wasm
// `Session` via the typed `web/confy.ts` wrapper. This module is the
// orchestrator: boot/load, the new web-native chrome (toolbar / filter row /
// footer), `send(Intent)`, and the external-edit handshake. Tree rendering lives
// in `web/render.ts`; mode overlays not yet ported to dedicated chrome keep the
// keyboard-driven `#overlay` fallback.
import { load, Session } from "./confy.js";
import {
  downloadText,
  extFor,
  fsAccessAvailable,
  pickOpenFile,
  pickSaveFile,
  writeFile,
  type FsHandle,
} from "./fs.js";
import { currentKindLabel, escapeHtml, IC_CARET, renderTree } from "./render.js";
import { resolveClick, rowsInRect, setAnchor } from "./select.js";
import { installDnd } from "./dnd.js";
import type {
  ConvertView,
  DocFormat,
  Intent,
  ModeView,
  Path,
  SessionSnapshot,
  TypeFilterRow,
  TypeFilterView,
} from "./types.js";

// Workspace version stamped in at build time (see `build.mjs` `define`); falls
// back to "dev" when the bundle is loaded without that define (e.g. raw serve).
declare const __APP_VERSION__: string;
const APP_VERSION =
  typeof __APP_VERSION__ !== "undefined" ? __APP_VERSION__ : "dev";

// Built-in demo doc — a self-describing intro to confy. All three carry the
// *same* tree (identical keys/values/comments); only the dialect's notation and
// comment marker differ, so cycling the header pill shows one config wearing
// three outfits. The pill cycles these while the doc is the unsaved sample (see
// `sampleMode`); opening or saving a real file leaves sample mode and freezes it.
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
// True while the open doc is the built-in sample (no backing file) — enables the
// format pill toggle.
let sampleMode = false;
let sampleFormat: "toml" | "json" | "yaml" = "toml";

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
  return document.getElementById(id) as T;
}
const tree = $<HTMLDivElement>("tree");
const overlay = $("overlay");
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
const openBtn = $<HTMLButtonElement>("btnOpen");
const saveBtn = $<HTMLButtonElement>("btnSave");
const FS_AVAILABLE = fsAccessAvailable();

// ---- bootstrap ----
async function main() {
  initTheme();
  initTouchScaffolding();
  const wasmUrl = new URL("./pkg/confy_ffi_bg.wasm", import.meta.url);
  await load(wasmUrl);
  updateSaveLabel();
  loadSample("toml");
  bindGlobal();
}

// Load the built-in sample in `format`, entering sample mode (pill toggle on).
function loadSample(format: "toml" | "json" | "yaml") {
  sampleFormat = format;
  openText(SAMPLES[format], format, null, "sample", true);
}

// Cycle the sample doc to the next backend (pill click while in sample mode).
function cycleSampleFormat() {
  if (!sampleMode) return;
  const next = SAMPLE_ORDER[(SAMPLE_ORDER.indexOf(sampleFormat) + 1) % SAMPLE_ORDER.length];
  loadSample(next);
}

function openText(
  text: string,
  format: "toml" | "json" | "yaml" | "yml",
  handle: FsHandle | null = null,
  name: string | null = null,
  asSample = false,
) {
  session?.free();
  try {
    session = Session.fromText(text, format);
  } catch (e) {
    setStatus("", String((e as Error).message ?? e));
    return;
  }
  fileHandle = handle;
  fileName = name;
  sampleMode = asSample;
  snap = session.snapshot();
  render();
}

// ---- theme ----
type Theme = "dark" | "light";
function initTheme() {
  const stored = localStorage.getItem("confy-theme");
  applyTheme(stored === "light" ? "light" : "dark");
}
function applyTheme(theme: Theme) {
  document.documentElement.dataset.theme = theme;
}
function toggleTheme() {
  const cur: Theme = document.documentElement.dataset.theme === "light" ? "light" : "dark";
  const next: Theme = cur === "dark" ? "light" : "dark";
  localStorage.setItem("confy-theme", next);
  applyTheme(next);
}

// Switch between the interactive tree and the read-only serialized text. Raw is
// a *view* of the same document — no editing — so it just re-renders.
function setView(raw: boolean) {
  rawView = raw;
  $("btnViewTree").classList.toggle("active", !raw);
  $("btnViewRaw").classList.toggle("active", raw);
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
  fmtPill.classList.toggle("toggleable", sampleMode);
  fmtPill.title = sampleMode ? "Sample — click to switch format" : "document format";
  document.body.classList.toggle("dirty", snap.is_dirty);
  document.body.classList.toggle("paste-mode", (snap.clipboard_count ?? 0) > 0);
  titleEl.textContent = fileName ?? "confy";
  titleEl.title = fileName ?? ""; // full name on hover when the chip truncates
  setStatus(snap.status, snap.error ?? "");

  renderRawOrTree();
  focusInlineEdit();
  renderDetailPanel();
  renderTypeFilterPop();
  renderConvertDialog();
  renderOverlay();
  renderFooter();
  updateSaveLabel();
  if (snap.external_edit) openExternalEdit(snap.external_edit);
  if (snap.convert_write) void doConvertWrite(snap.convert_write[0], snap.convert_write[1]);
  if (snap.quit) {
    setStatus("", "quit (reload to reopen)");
  }
}

function updateSaveLabel() {
  // The button opens the Save / Convert panel; ⌘S is the instant in-place path,
  // so the tooltip advertises what that fast key actually does.
  const inPlace = fileHandle
    ? "⌘S saves in place"
    : FS_AVAILABLE
      ? "⌘S = Save as…"
      : "⌘S = download";
  saveBtn.title = `Save / Convert…  (${inPlace})`;
}

function getEdit() {
  return typeof snap!.mode === "object" && "Edit" in snap!.mode
    ? snap!.mode.Edit
    : null;
}

function modeTag(m: ModeView): string {
  return typeof m === "string" ? m : Object.keys(m)[0];
}

// The detail aside (design slide-in panel) mirrors the `Detail` mode: it shows
// `detail_text` and slides in via `.detail.open`, replacing the keyboard
// fallback overlay for this mode.
function renderDetailPanel() {
  const panel = $("detail");
  if (modeTag(snap!.mode) === "Detail") {
    $("detailBody").innerHTML = `<pre class="mono">${escapeHtml(snap!.detail_text ?? "")}</pre>`;
    panel.classList.add("open");
  } else {
    panel.classList.remove("open");
  }
}

// The `#overlay` keyboard fallback now serves only Help, Prompt, and the `K`
// kind-switch mode. Filter → native search box; TypeFilter → `#tfPop` popover;
// Convert → `#convDlg` dialog (all rendered by their own functions below).
function renderOverlay() {
  const m = snap!.mode;
  const tag = modeTag(m);
  if (tag === "Help" || tag === "Prompt" || tag === "KindSwitch") {
    overlay.classList.remove("hidden");
  } else {
    overlay.classList.add("hidden");
    return;
  }
  if (tag === "Help") {
    // The KIND legend differs per backend (mirrors the TUI's per-format help in
    // crates/confy-tui/src/tui/keys.rs): TOML has dotted/AoT/radix rows, JSON
    // drops them and adds null/exponent, YAML adds block/flow + opaque + styles.
    const legend = KIND_LEGEND[snap!.doc_format] ?? "";
    overlay.innerHTML = `<h3>Help</h3><pre>${escapeHtml(HELP_TEXT + "\n" + legend)}</pre>`;
  } else if (tag === "Prompt") {
    overlay.innerHTML = `<h3>${escapeHtml(snap!.status ?? "confirm")}</h3>
        <div class="opt">y / Enter = yes</div><div class="opt">n / Esc = no</div>`;
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

// The check glyph inside a facet cell's `.box` (design markup; CSS reveals it
// only for `data-state="On"`).
const TF_CHECK = `<span class="box"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3"><path d="M5 12l5 5 9-11"/></svg></span>`;

function isHeader(row: TypeFilterRow): row is { Header: string } {
  return "Header" in row;
}

// The `f` type-filter facet grid as a native popover (`#tfPop`/`#tfInner`). Each
// cell is a button: clicking it moves the core cursor to that cell
// (`TypeFilterMove` delta) and toggles it. Apply commits, Cancel exits. The
// popover stays open across re-renders while `Mode::TypeFilter` is active; the
// keyboard path (onKey) still drives the same mode for accessibility.
function renderTypeFilterPop() {
  const pop = $("tfPop");
  if (modeTag(snap!.mode) !== "TypeFilter") {
    pop.classList.remove("open");
    return;
  }
  const grid = (snap!.mode as { TypeFilter: TypeFilterView }).TypeFilter;
  const inner = $("tfInner");
  let cellRow = -1;
  // Header carries the live-active hint and a `×` clear button (no Apply/Cancel —
  // toggles filter live and persist when the popup closes).
  let html =
    `<div class="tf-head"><span class="menu-label">Type filter${grid.active ? " <span class='tf-active'>· active</span>" : ""}</span>` +
    `<button class="tf-clear" data-tf="clear" title="clear type filter">✕</button></div>`;
  for (const row of grid.rows) {
    if (isHeader(row)) {
      html += `<div class="menu-label">${escapeHtml(row.Header)}</div>`;
      continue;
    }
    cellRow++;
    html +=
      `<div class="tf-grid">` +
      row.Cells.map(
        (c, col) =>
          `<button class="tf-cell${c.is_cursor ? " cursor" : ""}" data-state="${c.state}" data-r="${cellRow}" data-c="${col}">` +
          `${TF_CHECK}${escapeHtml(c.label)}</button>`,
      ).join("") +
      `</div>`;
  }
  inner.innerHTML = html;
  inner.querySelectorAll<HTMLElement>("[data-r]").forEach((b) => {
    b.onclick = () => {
      const dr = Number(b.dataset.r) - grid.cursor_row;
      const dc = Number(b.dataset.c) - grid.cursor_col;
      if (dr || dc) send({ TypeFilterMove: [dr, dc] });
      send("TypeFilterToggle");
    };
  });
  // × clears the filter *and* closes the popup; clicking outside closes it
  // keeping the filter (wired in bindGlobal).
  (inner.querySelector('[data-tf="clear"]') as HTMLElement).onclick = () =>
    send("ExitTypeFilter");
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
  const dlg = $<HTMLDialogElement>("convDlg");
  if (modeTag(snap!.mode) !== "Convert") {
    if (dlg.open) dlg.close();
    return;
  }
  const cv = (snap!.mode as { Convert: ConvertView }).Convert;
  const sel = $<HTMLSelectElement>("convFmt");
  const path = $<HTMLInputElement>("convPath");
  const warns = $("convWarns");
  const run = $("convRun");
  // Unified "Save / Convert" panel: the current format leads the list (default)
  // so picking it is a plain save-as; the other two are cross-format converts.
  const all = [snap!.doc_format, ...cv.options];
  if (!dlg.open) {
    sel.innerHTML = all
      .map((f) => `<option value="${f}">${f.toUpperCase()}</option>`)
      .join("");
    sel.value = cv.target;
    path.value = cv.path;
    dlg.showModal();
  } else {
    if (sel.value !== cv.target) sel.value = cv.target;
    // Don't clobber the box while the user is typing the path.
    if (document.activeElement !== path) path.value = cv.path;
  }
  // Same format → faithful save (no loss); only a cross-format convert warns.
  const crossFmt = cv.target !== snap!.doc_format;
  const hasWarn = crossFmt && cv.warnings.length > 0;
  warns.innerHTML = hasWarn
    ? `<strong>Lossy conversion</strong><div class="warns-note">These styles will be normalized; the output is still valid ${cv.target.toUpperCase()}. Review and confirm to save.</div>` +
      `<ul>${cv.warnings.map((w) => `<li>${escapeHtml(w)}</li>`).join("")}</ul>`
    : "";
  warns.classList.toggle("hide", !hasWarn);
  run.textContent = !crossFmt
    ? "Save copy"
    : cv.step === "Confirm"
      ? "Confirm & save"
      : "Convert & save";
}

function renderFooter() {
  // Badges stay visible (design resting state); only text + `on` accent change.
  const n = snap!.rows.filter((r) => r.selected).length;
  selBadge.textContent = n === 0 ? "none selected" : `${n} selected`;
  selBadge.classList.toggle("on", n > 0);
  const cc = snap!.clipboard_count ?? 0;
  clipBadge.textContent = `clipboard ${cc}`;
  clipBadge.classList.toggle("on", cc > 0);
}

// ---- keyboard → Intent (mirrors tui/keys.rs) ----
function onKey(ev: KeyboardEvent) {
  if (!session || !snap) return;
  if (!document.getElementById("ext-modal")!.classList.contains("hidden")) return;
  if (!document.getElementById("load-modal")!.classList.contains("hidden")) return;

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
      if (ev.key === "Enter") return runSaveConvert();
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

  const ctrl = ev.ctrlKey || ev.metaKey;
  if (ctrl && ev.key === "s") {
    ev.preventDefault();
    return void doSave();
  }
  if (ctrl && ev.key === "o") {
    ev.preventDefault();
    return void doOpen();
  }
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
    case "z": return send("Undo");
    case "y": return send("Redo");
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
    case "q": return send("QuitRequested");
  }
}

function send(i: Intent) {
  if (!session) return;
  snap = session.dispatch(i);
  render();
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
async function doSave() {
  if (!session) return;
  const text = session.serialize();
  // 1. In-place write to an open file handle.
  if (fileHandle) {
    try {
      await writeFile(fileHandle, text);
      send("Save");
      setStatus("Saved", "");
      return;
    } catch (e) {
      setStatus("", `save failed: ${String((e as Error).message ?? e)}`);
      return;
    }
  }
  // 2. Save As via the FS Access API (stores the handle for subsequent saves).
  if (FS_AVAILABLE) {
    const fmt = snap!.doc_format;
    const handle = await pickSaveFile(
      fmt,
      (fileName ?? "confy-export") + extFor(fmt),
    );
    if (handle) {
      try {
        await writeFile(handle, text);
        fileHandle = handle;
        fileName = await deriveName(handle, fmt);
        sampleMode = false; // now backed by a real file → freeze the format pill
        send("Save");
        setStatus("Saved", "");
        render();
        return;
      } catch (e) {
        setStatus("", `save failed: ${String((e as Error).message ?? e)}`);
        return;
      }
    }
    // User cancelled the picker — fall through to nothing (don't surprise them
    // with a download after they explicitly cancelled Save As).
    return;
  }
  // 3. Download fallback.
  downloadText((fileName ?? "confy-export") + extFor(snap!.doc_format), text);
  send("Save");
}

async function deriveName(handle: FsHandle, fmt: string): Promise<string> {
  try {
    return (await handle.getFile()).name;
  } catch {
    return fileName ?? "confy-export" + extFor(fmt);
  }
}

async function doConvertWrite(path: string, text: string) {
  // The convert flow always produces a new file. Prefer Save As when the host
  // supports it, else download.
  const baseName = path.split("/").pop() ?? "confy-converted";
  if (FS_AVAILABLE) {
    const fmt = snap?.doc_format ?? "Toml";
    const outExt = extFor(path.endsWith(".json") ? "Json" : path.endsWith(".yaml") || path.endsWith(".yml") ? "Yaml" : "Toml");
    const handle = await pickSaveFile(fmt, baseName.endsWith(outExt) ? baseName : baseName + outExt);
    if (handle) {
      try {
        await writeFile(handle, text);
        setStatus(`Converted → ${(await handle.getFile()).name}`, "");
        return;
      } catch (e) {
        setStatus("", `convert write failed: ${String((e as Error).message ?? e)}`);
        return;
      }
    }
    return; // cancelled
  }
  downloadText(baseName, text);
}

// Open: the FS Access API picker where available (keeps a handle for in-place
// save), else a native `<input type=file>` — file reading works in every
// browser, so the paste modal is no longer needed.
async function doOpen() {
  if (FS_AVAILABLE) {
    const opened = await pickOpenFile();
    if (!opened) return;
    const fmt = formatFromName(opened.name);
    openText(opened.text, fmt, opened.handle, opened.name);
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

function formatFromName(name: string): "toml" | "json" | "yaml" {
  return name.endsWith(".json") || name.endsWith(".jsonc")
    ? "json"
    : name.endsWith(".yaml") || name.endsWith(".yml")
      ? "yaml"
      : "toml";
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
    // Click on empty tree space clears the multi-select (cursor stays put).
    if (snap.rows.some((r) => r.selected)) send({ SetSelection: { paths: [] } });
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
  // empty area within 350ms toggles the row (branch expand / boolean value).
  // Native `dblclick` is unreliable here (the first click re-renders/scrolls the
  // row), so we time it ourselves — and only empty-space clicks reach this far.
  const key = JSON.stringify(path);
  const plain = !ev.shiftKey && !ev.ctrlKey && !ev.metaKey;
  if (plain && lastBodyClick && lastBodyClick.key === key && Date.now() - lastBodyClick.t < 350) {
    lastBodyClick = null;
    return toggleRow(path);
  }
  lastBodyClick = plain ? { key, t: Date.now() } : null;
  send({ SetSelection: { paths: resolveClick(snap, path, ev) } });
}
// Last plain empty-area body click (path + time) for manual double-click detect.
let lastBodyClick: { key: string; t: number } | null = null;

// Double-click on a row's *empty* area (detected manually in `onTreeClick`, see
// `lastBodyClick`): a branch toggles expand/collapse, a boolean leaf toggles its
// value true↔false. Only plain empty-space body clicks reach this — key/value/
// caret/etc. are handled (and return) earlier in `onTreeClick`.
function toggleRow(path: Path) {
  const key = JSON.stringify(path);
  const row = snap?.rows.find((r) => JSON.stringify(r.path) === key);
  if (!row) return;
  if (row.is_branch) {
    send({ SetCursor: path });
    return send("ToggleExpand");
  }
  if (row.scalar_type === "Bool" && !row.read_only) {
    const next = (row.value ?? "").trim().toLowerCase() === "true" ? "false" : "true";
    const tc = row.trailing_comment;
    send({ SetCursor: path });
    send({ CommitEdit: { value: tc ? `${next}  ${tc}` : next, name: null } });
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
  return [$("kindMenu"), $("ctxMenu"), $("moreMenu")];
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

function buildCtxMenu(path: Path): HTMLElement {
  const key = JSON.stringify(path);
  const row = snap!.rows.find((r) => JSON.stringify(r.path) === key);
  const cc = snap!.clipboard_count ?? 0;
  const isComment = row?.type_label === "comment";
  // "Append comment" attaches a *new* trailing comment; once a row has one you
  // edit it by clicking it (so the item is offered only when there is none).
  const canAppend =
    !!row && !isComment && !row.read_only && !row.trailing_comment;
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

// The "⋯ More" overflow menu (shown only under the narrow breakpoint): the same
// secondary actions the CSS hides from the toolbar / filter row, as a popup.
function buildMoreMenu(): HTMLElement {
  const items: Array<[string, () => void]> = [
    ["Save / Convert… (Ctrl-s)", openSaveConvert],
    ["Undo (z)", () => send("Undo")],
    ["Redo (y)", () => send("Redo")],
    ["Toggle theme", toggleTheme],
    ["Expand all (9)", () => send("ExpandAll")],
    ["Collapse all (0)", () => send("CollapseAll")],
    ["Tree view", () => setView(false)],
    ["Raw view", () => setView(true)],
  ];
  const menu = $("moreMenu");
  menu.innerHTML = items
    .map(
      ([label], i) =>
        `<button class="menu-item" data-i="${i}">${escapeHtml(label)}</button>`,
    )
    .join("");
  menu.querySelectorAll<HTMLElement>("[data-i]").forEach((b) => {
    const i = Number(b.dataset.i);
    b.onclick = () => {
      closePops();
      items[i][1]();
    };
  });
  return menu;
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

// The open file's stem (no directory, no extension) — the suggested output name.
function fileStem(): string {
  const base = (fileName ?? "config").split("/").pop()!;
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(0, dot) : base;
}
function extForTag(tag: string): string {
  return tag === "Json" ? ".json" : tag === "Yaml" ? ".yaml" : ".toml";
}

// Open the unified "Save / Convert" panel from the root node. `open_convert`
// leaves `target` = the current format (the panel's default), so the dialog
// opens on "save in the current format"; seed the output name from the open
// file's stem (core would otherwise default to "out.<ext>").
function openSaveConvert() {
  send({ SetCursor: [] });
  send("OpenConvert");
  send({ SetConvertPath: fileStem() + extForTag(snap?.doc_format ?? "Toml") });
}

// Run the panel's action: a same-format pick is a faithful save-as of the live
// document; a cross-format pick drives core's convert (warnings → confirm).
function runSaveConvert() {
  const m = snap!.mode;
  if (typeof m !== "object" || !("Convert" in m)) return;
  const cv = m.Convert;
  if (cv.target === snap!.doc_format) return void doSaveAsCopy(cv.path);
  send(cv.step === "Confirm" ? "ConvertConfirm" : "ConvertRun");
}

// Faithful "save a copy" of the live document (byte-for-byte `serialize()`),
// used when the panel's format equals the open format. Like convert, it writes
// an export copy and does not adopt the handle (the toolbar Save owns in-place).
async function doSaveAsCopy(path: string) {
  if (!session) return;
  const text = session.serialize();
  const fmt = snap!.doc_format;
  const baseName = path.split("/").pop() || "confy-export" + extFor(fmt);
  send("ExitConvert");
  if (FS_AVAILABLE) {
    const handle = await pickSaveFile(fmt, baseName);
    if (!handle) return;
    try {
      await writeFile(handle, text);
      setStatus(`Saved copy → ${(await handle.getFile()).name}`, "");
    } catch (e) {
      setStatus("", `save failed: ${String((e as Error).message ?? e)}`);
    }
    return;
  }
  downloadText(baseName, text);
}

function bindConvertDialog() {
  $<HTMLSelectElement>("convFmt").addEventListener("change", (e) => {
    const tag = (e.target as HTMLSelectElement).value as DocFormat;
    send({ SetConvertFormat: tag });
    // SetConvertFormat reseeds the path to "out.<ext>"; restore the real stem.
    send({ SetConvertPath: fileStem() + extForTag(tag) });
  });
  $<HTMLInputElement>("convPath").addEventListener("input", (e) =>
    send({ SetConvertPath: (e.target as HTMLInputElement).value }),
  );
  $("convRun").addEventListener("click", runSaveConvert);
  $("convCancel").addEventListener("click", () => send("ExitConvert"));
  // Native dialog Esc → leave Convert mode (render then closes the dialog).
  $<HTMLDialogElement>("convDlg").addEventListener("cancel", (e) => {
    e.preventDefault();
    send("ExitConvert");
  });
}

function bindGlobal() {
  tree.addEventListener("keydown", onKey);
  tree.addEventListener("click", onTreeClick);
  tree.addEventListener("contextmenu", onTreeContext);
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
  openBtn.addEventListener("click", () => void doOpen());
  saveBtn.addEventListener("click", openSaveConvert);
  fmtPill.addEventListener("click", cycleSampleFormat); // no-op unless in sample mode
  themeBtn.addEventListener("click", toggleTheme);
  $("btnMore").addEventListener("click", (e) => {
    // Toggle: a second click on ⋯ while its menu is open closes it.
    if ($("moreMenu").classList.contains("open")) return closePops();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    placePopAt(buildMoreMenu(), r.right - 200, r.bottom + 4);
  });
  $("btnUndo").addEventListener("click", () => send("Undo"));
  $("btnRedo").addEventListener("click", () => send("Redo"));
  $("btnExpandAll").addEventListener("click", () => send("ExpandAll"));
  $("btnCollapseAll").addEventListener("click", () => send("CollapseAll"));
  $("btnTypeFilter").addEventListener("click", () =>
    // Toggle: open the popup, or close it keeping the filter applied.
    send(snap && modeTag(snap.mode) === "TypeFilter" ? "CommitTypeFilter" : "EnterTypeFilter"),
  );
  $("btnViewTree").addEventListener("click", () => setView(false));
  $("btnViewRaw").addEventListener("click", () => setView(true));

  $("load-confirm").addEventListener("click", () => {
    const fmt = ($("load-format") as HTMLSelectElement).value as
      | "toml" | "json" | "yaml";
    const text = ($("load-text") as HTMLTextAreaElement).value;
    $("load-modal").classList.add("hidden");
    openText(text, fmt);
    tree.focus();
  });
  const closeLoadModal = () => {
    $("load-modal").classList.add("hidden");
    tree.focus();
  };
  $("load-cancel").addEventListener("click", closeLoadModal);
  // Esc closes the load modal (onKey early-returns while it's open, so it needs
  // its own handler — mirrors the external-edit modal's Esc-to-cancel).
  $("load-modal").addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    e.preventDefault();
    e.stopPropagation();
    closeLoadModal();
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
    document.getElementById("load-modal")!.classList.contains("hidden")
  );
}

// ---- utils ----
function setStatus(status: string | undefined, error: string | undefined) {
  statusEl.textContent = status ?? "";
  const err = error ?? "";
  errorEl.textContent = err;
  errorEl.classList.toggle("hidden", err === "");
}

const HELP_TEXT = `confy web — keys
j/k or ↑/↓     move cursor
Enter/Space    toggle branch / edit leaf / activate
e              edit (inline or multiline modal)
a              add node · d delete · c copy · x cut · v paste
r              remark (toggle node ↔ comment)
+/- or ←/→     nudge numeric value
z / y          undo / redo
s              toggle select · 0 collapse-all · 9 expand-all
1 / 2          expand / collapse one level
/              filter · f type-filter · K kind-switch · C convert
i              detail popup · ? this help · Ctrl-s save · Ctrl-o open
q              quit (prompts if dirty)

── pointer ──────────────────────────────────────
click          select          ⇧click   range-select
⌘click         multi-select    drag     marquee / move
right-click    context menu

Open (Ctrl-o) and in-place Save need the File System Access API
(Chrome/Edge). Other browsers fall back to the paste-load / download path.`;

// Per-format KIND legend appended to the Help overlay, keyed by `doc_format`
// (ported from the TUI's TOML_HELP/JSON_HELP/YAML_HELP KIND column). The kind
// badge shows the friendly label + notation suffix; this explains what each
// notation means for the open file's backend.
// ---- touch scaffolding stubs (capability-gated, INERT this phase) ----
// Groundwork for a future touch-first interaction layer: swipe gestures,
// a bottom-sheet overlay, and a FAB for primary actions. All stubs early-return
// so no live UI or behavior change occurs for any current user. Wired only on
// pointer:coarse devices (phones/tablets) to keep the mouse path untouched.
function initTouchScaffolding() {
  if (!window.matchMedia("(pointer:coarse)").matches) return;
  // DISABLED this phase — stubs register no listeners and perform no DOM changes.
  // Future phases will wire swipe → bottom-sheet open, FAB → AddNode, etc.
  _touchSwipeStub();
  _touchSheetStub();
  _touchFabStub();
}
// swipe-action handler stub (future: left-swipe → delete, right-swipe → copy)
function _touchSwipeStub() { return; }
// bottom-sheet stub (future: slides up to surface context actions on touch)
function _touchSheetStub() { return; }
// FAB stub (future: floating button for AddNode as primary touch action)
function _touchFabStub() { return; }

const KIND_LEGEND: Record<string, string> = {
  Toml: `── KIND badge (TOML) ──────────────────────────────
Containers (label·notation):
  table·scope    standard [header] table
  table·dotted   dotted-key table (a.b.c = …)
  inline         inline table { … }
  array·inline   inline array        array·multi  multiline array
  AoT            array-of-tables  [[…]]

Scalars (label·notation):
  str            basic string        str·"…"  (quoted)
  str·'…'        literal string
  str·"""        multiline basic     str·'''  multiline literal
  int            decimal integer
  int·0x int·0o int·0b   hex / octal / binary
  float / float·dec      float        float·1e  exponent
  float·inf float·nan    infinity / NaN
  bool · date · time · null`,
  Json: `── KIND badge (JSON / JSONC) ──────────────────────
Containers (label·notation):
  table          object { … }        table·multi  multiline object
  inline         inline object
  array·inline   inline array        array·multi  multiline array

Scalars (label·notation):
  str            string              null
  int            integer
  float          float               float·1e  exponent
  bool`,
  Yaml: `── KIND badge (YAML) ──────────────────────────────
Containers (label·notation):
  table·block    block mapping       table·flow  flow mapping { … }
  array·block    block sequence      array·flow  flow sequence [ … ]
  (opaque nodes — anchors/aliases/merge/tags — are read-only)

Scalars (label·notation):
  str            plain string        str·'…'  single-quoted
  str·"…"        double-quoted       str·|    literal block
  str·>          folded block
  int            decimal integer     int·0x int·0o  hex / octal
  float          float               float·1e  exponent
  float·inf float·nan    infinity / NaN
  bool · null`,
};

main().catch((e) => setStatus("", String(e)));
