// confy Web UI — a pure render of the `SessionSnapshot` + a stream of `Intent`s
// back. No editor logic lives here (PORTING §8.4; WEBUI.md). Drives the wasm
// `Session` via the typed `web/confy.ts` wrapper.
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
import type {
  Intent,
  ModeView,
  SessionSnapshot,
  TypeFilterCellView,
  TypeFilterRow,
  ViewRow,
} from "./types.js";

const SAMPLE = `[server]
host = "localhost"
port = 8080
enabled = true

[plugins]
names = ["auth", "metrics"]

# A multiline note
notes = """
multi
line
"""
`;

let session: Session | null = null;
let snap: SessionSnapshot | null = null;

// Host-owned file state. `fileHandle` is non-null only when the doc is backed
// by a real on-disk file opened/saved through the File System Access API.
let fileHandle: FsHandle | null = null;
let fileName: string | null = null;

// ---- DOM ----
function $<T extends HTMLElement = HTMLElement>(id: string): T {
  return document.getElementById(id) as T;
}
const tree = $<HTMLPreElement>("tree");
const overlay = $("overlay");
const statusEl = $("status");
const errorEl = $("error");
const formatEl = $("format");
const dirtyEl = $("dirty");
const titleEl = $("title");
const selectionEl = $("selection");
const clipboardEl = $("clipboard");
const themeBtn = $<HTMLButtonElement>("theme-btn");
const openBtn = $<HTMLButtonElement>("open-btn");
const saveBtn = $<HTMLButtonElement>("save-btn");
const FS_AVAILABLE = fsAccessAvailable();

// ---- bootstrap ----
async function main() {
  initTheme();
  const wasmUrl = new URL("./pkg/confy_ffi_bg.wasm", import.meta.url);
  await load(wasmUrl);
  // Hide the FS-only "Open file…" button on browsers without the API.
  if (!FS_AVAILABLE) openBtn.classList.add("hidden");
  updateSaveLabel();
  openText(SAMPLE, "toml");
  bindGlobal();
}

function openText(
  text: string,
  format: "toml" | "json" | "yaml" | "yml",
  handle: FsHandle | null = null,
  name: string | null = null,
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
  snap = session.snapshot();
  render();
}

// ---- theme ----
type Theme = "dark" | "light";
function initTheme() {
  const stored = localStorage.getItem("confy-theme");
  const theme: Theme = stored === "light" ? "light" : "dark";
  applyTheme(theme);
}
function applyTheme(theme: Theme) {
  document.documentElement.dataset.theme = theme;
  themeBtn.textContent = theme === "dark" ? "☀" : "☾";
}
function toggleTheme() {
  const cur: Theme = document.documentElement.dataset.theme === "light" ? "light" : "dark";
  const next: Theme = cur === "dark" ? "light" : "dark";
  localStorage.setItem("confy-theme", next);
  applyTheme(next);
}

// ---- render ----
function render() {
  if (!snap || !session) return;
  formatEl.textContent = snap.doc_format;
  dirtyEl.classList.toggle("hidden", !snap.is_dirty);
  titleEl.textContent = fileName ?? "confy";
  setStatus(snap.status, snap.error ?? "");

  renderTree();
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
  // The button advertises the actual save path: in-place / save-as / download.
  const hasHandle = fileHandle !== null;
  saveBtn.textContent = hasHandle
    ? "Save"
    : FS_AVAILABLE
      ? "Save…"
      : "Save (download)";
}

function renderTree() {
  const rows = snap!.rows;
  const edit = getEdit();

  const lines: string[] = [];
  for (const r of rows) {
    const indent = "  ".repeat(r.depth);
    const marker = r.is_cursor ? "▶ " : r.selected ? "◉ " : "  ";
    const branch = r.is_branch ? (isExpanded(r) ? "▾ " : "▸ ") : "";
    const cls = `row${r.is_cursor ? " cursor" : ""}${r.selected ? " selected" : ""}${r.read_only ? " readonly" : ""}`;
    let line = `<div class="${cls}">`;
    line += `<span class="marker">${marker}</span>`;
    line += `<span class="depth">${escapeHtml(indent)}</span>`;
    line += `<span class="branch-mark">${branch}</span>`;
    if (r.key) {
      line += `<span class="key">${escapeHtml(r.key)}</span>`;
      if (!r.is_branch) line += `<span class="val"> = ${renderValue(r, edit)}</span>`;
    } else if (!r.is_branch) {
      line += `<span class="val">${renderValue(r, edit)}</span>`;
    }
    if (r.trailing_comment) {
      line += ` <span class="comment">${escapeHtml(r.trailing_comment)}</span>`;
    }
    line += `</div>`;
    lines.push(line);
  }
  tree.innerHTML = lines.join("\n");
  const cur = tree.querySelector(".row.cursor") as HTMLElement | null;
  cur?.scrollIntoView({ block: "nearest" });
}

function valueTypeClass(r: ViewRow): string {
  switch (r.scalar_type) {
    case "String":
      return "v-str";
    case "Integer":
      return "v-int";
    case "Float":
      return "v-float";
    case "Boolean":
      return "v-bool";
    case "Null":
      return "v-null";
    case "Datetime":
      return "v-date";
    default:
      return "";
  }
}

function renderValue(r: ViewRow, edit: ReturnType<typeof getEdit>): string {
  if (edit && r.is_cursor && edit.field === "Value") {
    return `<span class="edit-cell">${escapeHtml(edit.buffer)}</span>`;
  }
  const vcls = valueTypeClass(r);
  const raw = escapeHtml(r.value ?? "");
  return vcls ? `<span class="${vcls}">${raw}</span>` : raw;
}

function getEdit() {
  return typeof snap!.mode === "object" && "Edit" in snap!.mode
    ? snap!.mode.Edit
    : null;
}

// Expand state is inferred from rendered rows: a branch is expanded iff a
// later row sits at depth+1 under it.
function isExpanded(r: ViewRow): boolean {
  const rows = snap!.rows;
  const idx = rows.indexOf(r);
  for (let i = idx + 1; i < rows.length; i++) {
    if (rows[i].depth <= r.depth) break;
    return true;
  }
  return false;
}

function modeTag(m: ModeView): string {
  return typeof m === "string" ? m : Object.keys(m)[0];
}

function renderOverlay() {
  const m = snap!.mode;
  const tag = modeTag(m);
  if (tag === "Normal" || tag === "FilterResults" || tag === "Edit") {
    overlay.classList.add("hidden");
    return;
  }
  overlay.classList.remove("hidden");
  if (tag === "Detail") {
    overlay.innerHTML = `<h3>Detail</h3><pre>${escapeHtml(snap!.detail_text ?? "")}</pre>`;
  } else if (tag === "Help") {
    overlay.innerHTML = `<h3>Help</h3><pre>${escapeHtml(HELP_TEXT)}</pre>`;
  } else if (tag === "Prompt") {
    overlay.innerHTML = `<h3>${escapeHtml(snap!.status ?? "confirm")}</h3>
        <div class="opt">y / Enter = yes</div><div class="opt">n / Esc = no</div>`;
  } else if (tag === "Filter") {
    const f = (m as { Filter: { text: string; cursor: number } }).Filter;
    overlay.innerHTML = `<h3>Filter</h3>
        <div class="edit-cell">${escapeHtml(f.text.slice(0, f.cursor))}|${escapeHtml(f.text.slice(f.cursor))}</div>
        <div>Enter to commit · Esc to clear</div>`;
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
  } else if (tag === "Convert") {
    const cv = (m as { Convert: { step: string; path: string; warnings: string[] } })
      .Convert;
    overlay.innerHTML = `<h3>Convert (${escapeHtml(cv.step)})</h3>
        <pre>path: ${escapeHtml(cv.path)}</pre>
        ${cv.warnings.length ? `<pre>${escapeHtml(cv.warnings.join("\n"))}</pre>` : ""}
        <div>↑/↓ pick · Enter next · Esc cancel</div>`;
  } else if (tag === "TypeFilter") {
    renderTypeFilter((m as { TypeFilter: import("./types.js").TypeFilterView }).TypeFilter);
  } else {
    overlay.classList.add("hidden");
  }
}

function checkGlyph(state: import("./types.js").CheckState): string {
  return state === "On" ? "[✓]" : state === "Partial" ? "[~]" : "[ ]";
}

function renderTypeFilter(grid: import("./types.js").TypeFilterView) {
  const body = grid.rows
    .map((row) => {
      if (isHeader(row)) {
        return `<div class="tf-header">${escapeHtml(row.Header)}</div>`;
      }
      const cells = row.Cells.map((c) =>
        renderTypeCell(c),
      ).join("");
      return `<div class="tf-row">${cells}</div>`;
    })
    .join("");
  overlay.innerHTML =
    `<h3>Type filter${grid.active ? " <span class='tf-active'>active</span>" : ""}</h3>` +
    `<div class="tf-grid">${body}</div>` +
    `<div class="tf-hint">↑↓←→ move · Space toggle · Enter apply · Esc cancel</div>`;
}

function renderTypeCell(c: TypeFilterCellView): string {
  const cls = `tf-cell${c.is_cursor ? " cursor" : ""} tf-${c.state.toLowerCase()}`;
  return `<span class="${cls}">${checkGlyph(c.state)} ${escapeHtml(c.label)}</span>`;
}

function isHeader(row: TypeFilterRow): row is { Header: string } {
  return "Header" in row;
}

function renderFooter() {
  const n = snap!.rows.filter((r) => r.selected).length;
  selectionEl.textContent = n > 0 ? `${n} selected` : "";
  selectionEl.classList.toggle("hidden", n === 0);
  const cc = snap!.clipboard_count;
  clipboardEl.textContent = cc ? `clipboard: ${cc}` : "";
  clipboardEl.classList.toggle("hidden", !cc);
}

// ---- keyboard → Intent (mirrors tui/keys.rs) ----
function onKey(ev: KeyboardEvent) {
  if (!session || !snap) return;
  if (!document.getElementById("ext-modal")!.classList.contains("hidden")) return;
  if (!document.getElementById("load-modal")!.classList.contains("hidden")) return;

  const m = snap.mode;
  if (typeof m === "object" && "Filter" in m) {
    if (ev.key === "Enter") return send("CommitFilter");
    if (ev.key === "Escape") return send("Escape");
    if (ev.key === "Backspace") return send("FilterBackspace");
    if (ev.key.length === 1) return send({ FilterChar: ev.key });
    return;
  }
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
      if (ev.key === "Enter") return send("ConvertRun");
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
    return void doOpenFs();
  }
  switch (ev.key) {
    case "j": case "ArrowDown": return send("CursorDown");
    case "k": case "ArrowUp": return send("CursorUp");
    case "g": case "Home": return send("CursorHome");
    case "G": case "End": return send("CursorEnd");
    case "Enter": case " ": return send("ToggleExpand");
    case "e": return send("BeginEdit");
    case "a": return send("AddNode");
    case "d": return send("DeleteSelected");
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
    case "/": return send("EnterFilter");
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
  };
  confirm.onclick = () => {
    close();
    if (kind.Value) send({ ApplyReplace: { path: path as never, text: txt.value } });
    else send({ ApplyEditComment: { path: path as never, text: txt.value } });
  };
  cancel.onclick = () => {
    close();
    send("Escape"); // peel the pending edit
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

async function doOpenFs() {
  if (!FS_AVAILABLE) return;
  const opened = await pickOpenFile();
  if (!opened) return;
  const fmt = formatFromName(opened.name);
  openText(opened.text, fmt, opened.handle, opened.name);
  tree.focus();
}

function formatFromName(name: string): "toml" | "json" | "yaml" {
  return name.endsWith(".json") || name.endsWith(".jsonc")
    ? "json"
    : name.endsWith(".yaml") || name.endsWith(".yml")
      ? "yaml"
      : "toml";
}

function bindGlobal() {
  tree.addEventListener("keydown", onKey);
  tree.focus();
  document.body.addEventListener("keydown", (ev) => {
    if (document.activeElement !== tree && noModalOpen()) {
      if ((document.activeElement as HTMLElement)?.tagName === "BUTTON") return;
      onKey(ev);
    }
  });

  openBtn.addEventListener("click", () => void doOpenFs());
  saveBtn.addEventListener("click", () => void doSave());
  themeBtn.addEventListener("click", toggleTheme);

  $("load-btn").addEventListener("click", () => {
    $("load-modal").classList.remove("hidden");
  });
  $("load-confirm").addEventListener("click", () => {
    const fmt = ($("load-format") as HTMLSelectElement).value as
      | "toml" | "json" | "yaml";
    const text = ($("load-text") as HTMLTextAreaElement).value;
    $("load-modal").classList.add("hidden");
    openText(text, fmt);
    tree.focus();
  });
  $("load-cancel").addEventListener("click", () => {
    $("load-modal").classList.add("hidden");
  });
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
  errorEl.textContent = error ?? "";
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
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

Open (Ctrl-o) and in-place Save need the File System Access API
(Chrome/Edge). Other browsers fall back to the Load/Save-download buttons.`;

main().catch((e) => setStatus("", String(e)));
