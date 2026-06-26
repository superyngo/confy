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
  DocFormat,
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

type FsHandle = OpenedFile["handle"];

// Workspace version stamped by esbuild (see build.mjs `define`); falls back to
// "dev" when bundled without the define.
declare const __APP_VERSION__: string;
const APP_VERSION =
  typeof __APP_VERSION__ !== "undefined" ? __APP_VERSION__ : "dev";

// Built-in welcome sample — identical content to the desktop UI (`web/ui.ts`
// SAMPLES.toml), so both surfaces boot the same tree.
const SAMPLE = `# 👋 Welcome to confy — a lossless editor for TOML · JSON · YAML
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
`;

const FS_AVAILABLE = fsAccessAvailable();

// ---- module state ----
let session: Session | null = null;
let snap: SessionSnapshot | null = null;
let fileHandle: FsHandle | null = null;
let fileName: string | null = "config.toml";
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
const sheets: Record<string, HTMLElement> = {};

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
function appHTML(): string {
  return (
    '<div class="app">' +
    '<div class="appbar">' +
    '<div class="brand"><span class="logo">cy</span>' +
    '<span class="doc"><span class="name"></span>' +
    '<span class="meta"><button class="fmt-pill" data-act="convert" aria-label="save / convert format"></button><span class="dirty-dot"></span></span></span></div>' +
    '<span class="grow"></span>' +
    `<button class="tapbtn" data-act="undo" aria-label="undo">${IC.undo}</button>` +
    `<button class="tapbtn primary" data-act="save" aria-label="save">${IC.save}</button>` +
    `<button class="tapbtn" data-act="menu" aria-label="more">${IC.more}</button>` +
    "</div>" +
    '<div class="searchbar">' +
    `<div class="search"><span class="ic">${IC.search}</span>` +
    '<input type="search" placeholder="search keys or values…" autocomplete="off" spellcheck="false" />' +
    `<button class="clear" data-act="searchclear" aria-label="clear">${IC.close}</button></div>` +
    `<button class="filter-btn" data-act="filter" aria-label="type filter">${IC.filter}<span class="dot"></span></button>` +
    "</div>" +
    '<div class="tabs"><button class="tab active" data-tab="tree">Tree</button><button class="tab" data-tab="raw">Raw</button></div>' +
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
    '<div class="toast"></div>' +
    '<div class="scrim" data-act="scrim"></div>' +
    '<div class="sheet detail-sheet"></div>' +
    '<div class="sheet menu-sheet"></div>' +
    '<div class="sheet filter-sheet"></div>' +
    '<div class="sheet kind-sheet"></div>' +
    '<div class="sheet convert-sheet"></div>' +
    "</div>" +
    // external-edit modal (multi-line value / comment; reuses the sheet styling)
    '<div class="scrim ext-scrim" data-act="extcancel"></div>' +
    '<div class="sheet ext-sheet">' +
    '<div class="grab"></div>' +
    '<div class="sheet-head"><h3>Edit</h3><button class="close" data-act="extcancel">' +
    IC.close +
    "</button></div>" +
    '<div class="sheet-body"><textarea class="v-edit ext-text" rows="10" style="height:auto;min-height:160px;resize:vertical"></textarea>' +
    '<div class="row-btns"><button class="btn" data-act="extcancel">Cancel</button>' +
    '<button class="btn primary" data-act="extapply">Apply</button></div></div>' +
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

// ---- render ----
function render() {
  if (!snap || !session) return;
  fmtPill.textContent = snap.doc_format.toUpperCase();
  docNameEl.textContent = fileName ?? "config";
  dirtyDot.style.opacity = snap.is_dirty ? "1" : "0";
  statusEl.textContent = snap.error ? snap.error : snap.status ?? "ready";
  const cur = cursorRow();
  selBadge.textContent = cur && cur.path.length ? lastKey(cur.path) : "none";
  clipBadge.textContent = `clipboard ${snap.clipboard_count ?? 0}`;

  if (rawView) {
    rawEl.textContent = session.serialize();
    app.classList.add("raw");
  } else {
    treeEl.innerHTML = treeHTML(snap);
    app.classList.remove("raw");
  }

  // Persistent side-pane detail (≥600px). Narrow uses the detail sheet on
  // double-tap. The shared panel.ts renders/wires the body identically to desktop.
  if (isWide() && !rawView) {
    if (cur && cur.path.length) {
      dpBody.innerHTML = panelHTML(cur);
      wirePanel(dpBody, cur, sendR, openKindRow, toast);
    } else {
      dpBody.innerHTML = '<div class="dp-empty">Tap any node<br>to edit its value and metadata here</div>';
    }
  }

  // Mode-driven sheets.
  const tag = modeTag(snap.mode);
  if (tag === "TypeFilter") renderFilterSheet((snap.mode as { TypeFilter: TypeFilterView }).TypeFilter);
  else sheets.filter.classList.remove("open");
  if (tag === "Convert") renderConvertSheet((snap.mode as { Convert: ConvertView }).Convert);
  else sheets.convert.classList.remove("open");
  if (tag !== "TypeFilter" && tag !== "Convert" && !anySheetOpen()) scrim.classList.remove("show");

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
// Double-tap (narrow) opens the bottom-sheet panel. Wide mode keeps the
// persistent side pane (render() refreshed it), so no sheet is needed.
function openPanel(path: Path) {
  selectOnly(path);
  const r = rowFor(path);
  if (!r) return;
  if (!isWide()) {
    sheets.detail.innerHTML =
      '<div class="grab"></div>' +
      `<div class="sheet-head"><h3>${esc(r.key || lastKey(path))}</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
      `<div class="sheet-body detail-wrap">${panelHTML(r)}</div>`;
    wirePanel(sheets.detail, r, sendR, openKindRow, toast);
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
function openMenuSheet() {
  sheets.menu.innerHTML =
    '<div class="grab"></div>' +
    `<div class="sheet-head"><h3>More actions</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
    '<div class="sheet-body">' +
    mi(IC.open, "Open file", "", "open") +
    mi(IC.convert, "Save / Convert format", "", "convert") +
    mi(IC.redo, "Redo", "", "redo") +
    '<div class="menu-sep"></div>' +
    mi(IC.expand, "Expand all", "", "expandall") +
    mi(IC.collapse, "Collapse all", "", "collapseall") +
    '<div class="menu-sep"></div>' +
    mi(IC.sun, "Toggle light / dark", "", "theme") +
    "</div>";
  sheets.menu.querySelectorAll<HTMLElement>(".menu-item").forEach((it) => {
    it.addEventListener("click", () => menuAction(it.dataset.mi!));
  });
  openSheet("menu");
}
function menuAction(id: string) {
  switch (id) {
    case "theme":
      toggleTheme();
      return; // keep menu open? close for parity
    case "expandall":
      closeSheets();
      return send("ExpandAll");
    case "collapseall":
      closeSheets();
      return send("CollapseAll");
    case "redo":
      closeSheets();
      return send("Redo");
    case "open":
      closeSheets();
      return void doOpen();
    case "convert":
      closeSheets();
      return send("OpenConvert");
  }
}

// ---- type-filter sheet (driven by snapshot.mode TypeFilterView) ----
function renderFilterSheet(grid: TypeFilterView) {
  // Well-formed grouped markup: each Header opens a group + its `.chips` row;
  // each Cells row fills it. `cellRow` indexes Cells rows (matches cursor_row).
  let cellRow = -1;
  let chips = "";
  let groupOpen = false;
  for (const row of grid.rows) {
    if ("Header" in row) {
      if (groupOpen) chips += "</div>"; // close prior .chips
      chips += `<div class="field-label">${esc(row.Header)}</div><div class="chips">`;
      groupOpen = true;
      continue;
    }
    if (!groupOpen) {
      chips += '<div class="chips">';
      groupOpen = true;
    }
    cellRow++;
    chips += row.Cells.map((c, col) => {
      const on = c.state === "On" ? "1" : c.state === "Partial" ? "partial" : "0";
      return `<button class="chip" data-on="${on}" data-r="${cellRow}" data-c="${col}"><span class="box">${IC.check}</span>${esc(c.label)}</button>`;
    }).join("");
  }
  if (groupOpen) chips += "</div>"; // close last .chips
  sheets.filter.innerHTML =
    '<div class="grab"></div>' +
    `<div class="sheet-head"><h3>Type filter</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
    `<div class="sheet-body tf-body">${chips}` +
    '<div class="row-btns"><button class="btn primary" data-act="closesheet">Done</button></div></div>';
  sheets.filter.querySelectorAll<HTMLElement>("[data-r]").forEach((b) => {
    b.addEventListener("click", () => {
      const dr = Number(b.dataset.r) - grid.cursor_row;
      const dc = Number(b.dataset.c) - grid.cursor_col;
      if (dr || dc) send({ TypeFilterMove: [dr, dc] });
      send("TypeFilterToggle");
    });
  });
  if (!sheets.filter.classList.contains("open")) openSheet("filter");
}

// ---- convert sheet (driven by snapshot.mode ConvertView) ----
function renderConvertSheet(cv: ConvertView) {
  const all: DocFormat[] = [snap!.doc_format, ...cv.options.filter((f) => f !== snap!.doc_format)];
  const crossFmt = cv.target !== snap!.doc_format;
  const hasWarn = crossFmt && cv.warnings.length > 0;
  const fmtBtns = all
    .map(
      (f) =>
        `<button class="add-cell conv-fmt${f === cv.target ? " on" : ""}" data-fmt="${f}"><span class="dotc" style="background:var(--accent)"></span>${f.toUpperCase()}${f === snap!.doc_format ? " (same)" : ""}</button>`,
    )
    .join("");
  const runLabel = !crossFmt ? "Save copy" : cv.step === "Confirm" ? "Confirm & save" : "Convert & save";
  sheets.convert.innerHTML =
    '<div class="grab"></div>' +
    `<div class="sheet-head"><h3>Save / Convert</h3><button class="close" data-act="convcancel">${IC.close}</button></div>` +
    '<div class="sheet-body">' +
    `<div class="field-label">Format</div><div class="addgrid">${fmtBtns}</div>` +
    `<div class="field-label">Output path</div><input class="v-edit conv-path" value="${esc(cv.path)}" autocomplete="off" spellcheck="false" />` +
    (hasWarn
      ? `<div class="field-label">Lossy conversion</div><div class="preview">${cv.warnings.map((w) => "• " + esc(w)).join("\n")}</div>`
      : "") +
    `<div class="row-btns"><button class="btn" data-act="convcancel">Cancel</button>` +
    `<button class="btn primary" data-act="convrun">${runLabel}</button></div></div>`;
  sheets.convert.querySelectorAll<HTMLElement>(".conv-fmt").forEach((b) => {
    b.addEventListener("click", () => send({ SetConvertFormat: b.dataset.fmt as DocFormat }));
  });
  const pathInput = sheets.convert.querySelector<HTMLInputElement>(".conv-path");
  if (pathInput)
    pathInput.addEventListener("change", () => send({ SetConvertPath: pathInput.value }));
  const run = sheets.convert.querySelector<HTMLElement>("[data-act=convrun]");
  if (run)
    run.addEventListener("click", () => send(cv.step === "Confirm" ? "ConvertConfirm" : "ConvertRun"));
  if (!sheets.convert.classList.contains("open")) openSheet("convert");
}

// ---- external edit (multi-line value/comment) ----
function openExternalEdit(ext: { initial: string; kind: unknown }) {
  const sheet = sheets.ext;
  const txt = sheet.querySelector<HTMLTextAreaElement>(".ext-text")!;
  txt.value = ext.initial;
  const kind = ext.kind as { Value?: { path: Path }; Comment?: { path: Path } };
  const path = (kind.Value ?? kind.Comment)!.path;
  openSheet("ext");
  txt.focus();
  const apply = sheet.querySelector<HTMLElement>("[data-act=extapply]")!;
  apply.onclick = () => {
    closeSheets();
    if (kind.Value) send({ ApplyReplace: { path, text: txt.value } });
    else send({ ApplyEditComment: { path, text: txt.value } });
  };
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
    const main = (e.target as HTMLElement).closest<HTMLElement>(".row-main");
    if (!main) return;
    dragRow = main.parentElement as HTMLElement;
    sx = e.clientX;
    sy = e.clientY;
    dragging = true;
    moved = false;
  });
  treeEl.addEventListener("pointermove", (e) => {
    if (reordering) {
      e.preventDefault();
      onReorderMove(e.clientY);
      return;
    }
    if (!dragging || !dragRow) return;
    // Any meaningful drag (scroll) cancels the pending tap.
    if (Math.abs(e.clientX - sx) > 8 || Math.abs(e.clientY - sy) > 8) moved = true;
  });
  treeEl.addEventListener("pointerup", (e) => {
    if (reordering) {
      endReorder();
      return;
    }
    if (dragging && dragRow && !moved) handleTap(e.target as HTMLElement, dragRow);
    dragging = false;
    dragRow = null;
  });
  treeEl.addEventListener("pointercancel", () => {
    if (reordering) {
      endReorder();
      return;
    }
    dragging = false;
    dragRow = null;
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
    if (act === "caret") {
      send({ SetCursor: path });
      send("ToggleExpand");
      return;
    }
  }
  const key = JSON.stringify(path);
  const now = Date.now();
  const isDouble = key === lastTapKey && now - lastTapTime < DOUBLE_TAP_MS;
  lastTapKey = key;
  lastTapTime = now;
  if (isDouble) openPanel(path);
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

// ---- file I/O (host-owned, via fs.ts) ----
function openText(text: string, format: "toml" | "json" | "yaml" | "yml", handle: FsHandle | null, name: string | null) {
  session?.free();
  try {
    session = Session.fromText(text, format);
  } catch (e) {
    statusEl.textContent = String((e as Error).message ?? e);
    return;
  }
  fileHandle = handle;
  fileName = name;
  snap = session.snapshot();
  rawView = false;
  app.querySelectorAll(".tab").forEach((t) => t.classList.toggle("active", (t as HTMLElement).dataset.tab === "tree"));
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
async function doSave() {
  if (!session || !snap) return;
  const text = session.serialize();
  if (fileHandle) {
    try {
      await writeFile(fileHandle, text);
      send("Save");
      toast("Saved");
    } catch (e) {
      statusEl.textContent = `save failed: ${String((e as Error).message ?? e)}`;
    }
    return;
  }
  if (FS_AVAILABLE) {
    const fmt = snap.doc_format;
    const handle = await pickSaveFile(fmt, (fileName ?? "confy-export") + extFor(fmt));
    if (!handle) return;
    try {
      await writeFile(handle, text);
      fileHandle = handle;
      fileName = (await handle.getFile()).name;
      send("Save");
      toast("Saved");
      render();
    } catch (e) {
      statusEl.textContent = `save failed: ${String((e as Error).message ?? e)}`;
    }
    return;
  }
  downloadText((fileName ?? "confy-export") + extFor(snap.doc_format), text);
  send("Save");
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

// ---- shell-level click delegation (appbar / footer / scrim / sheets) ----
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
      case "add":
        addContextual();
        break;
      case "convert":
        send("OpenConvert");
        break;
      case "save":
        void doSave();
        break;
      case "undo":
        send("Undo");
        break;
      case "scrim":
      case "closesheet":
        dismissSheets();
        break;
      case "convcancel":
        send("ExitConvert");
        break;
      case "extcancel":
        closeSheets();
        send("Escape");
        break;
      case "extapply":
        break; // wired per-open in openExternalEdit
      case "searchclear":
        searchInput.value = "";
        searchInput.parentElement!.classList.remove("has-val");
        send({ SetFilter: "" });
        break;
    }
  });

  // Tabs (Tree / Raw) — a view toggle, not a mutation.
  app.querySelectorAll<HTMLElement>(".tab").forEach((t) => {
    t.addEventListener("click", () => {
      app.querySelectorAll(".tab").forEach((x) => x.classList.remove("active"));
      t.classList.add("active");
      rawView = t.dataset.tab === "raw";
      render();
    });
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

// Dismiss whatever sheet is open, committing mode-driven ones so their state
// persists (type-filter) or unwinds cleanly (convert / external edit).
function dismissSheets() {
  const tag = snap ? modeTag(snap.mode) : "Normal";
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
  scrim = app.querySelector(".scrim:not(.ext-scrim)")!;
  dpBody = app.querySelector(".dp-body")!;
  statusEl = app.querySelector(".status")!;
  selBadge = app.querySelector(".sel-badge")!;
  clipBadge = app.querySelector(".clip-badge")!;
  searchInput = app.querySelector(".search input")!;
  fmtPill = app.querySelector(".fmt-pill")!;
  docNameEl = app.querySelector(".brand .name")!;
  dirtyDot = app.querySelector(".dirty-dot")!;
  filterBtn = app.querySelector(".filter-btn")!;
  toastEl = app.querySelector(".toast")!;
  sheets.detail = app.querySelector(".detail-sheet")!;
  sheets.menu = app.querySelector(".menu-sheet")!;
  sheets.filter = app.querySelector(".filter-sheet")!;
  sheets.kind = app.querySelector(".kind-sheet")!;
  sheets.convert = app.querySelector(".convert-sheet")!;
  sheets.ext = root.querySelector(".ext-sheet")!;

  restoreDetailWidth();
  installTreeGestures();
  installShellHandlers();
  installSplitter();

  const wasmUrl = new URL("../pkg/confy_ffi_bg.wasm", import.meta.url);
  await load(wasmUrl);
  session = Session.fromText(SAMPLE, "toml");
  snap = session.snapshot();
  render();
}

void main();
