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
import { IC, esc, treeHTML, isComment, isPositional, valueTypeClass, pathEq } from "./render.js";

type FsHandle = OpenedFile["handle"];

const SAMPLE = `# confy — sample configuration
[server]
host = "0.0.0.0"  # listen on all interfaces
port = 8080
workers = 4

[server.tls]
enabled = true
cert = "/etc/confy/cert.pem"

[database]
url = "postgres://localhost/confy"
pool_size = 16
timeout_ms = 3000

[logging]
level = "info"
format = "json"
targets = ["stdout", "file"]

[features]
beta_ui = false
rate_limit = true
`;

const FS_AVAILABLE = fsAccessAvailable();

// ---- module state ----
let session: Session | null = null;
let snap: SessionSnapshot | null = null;
let fileHandle: FsHandle | null = null;
let fileName: string | null = "config.toml";
let rawView = false;
let openSwipeRow: HTMLElement | null = null;
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
    '<span class="meta"><span class="fmt-pill"></span><span class="dirty-dot"></span></span></span></div>' +
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
  closeSwipe();
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

  // Persistent side-pane detail (≥600px). Narrow uses the detail sheet on tap.
  if (isWide() && !rawView) {
    if (cur && cur.path.length) {
      dpBody.innerHTML = detailHTML(cur);
      wireDetail(dpBody, cur);
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

// ---- detail (key / value / comment / kind / actions) ----
function detailHTML(r: ViewRow): string {
  const branch = r.is_branch;
  const comment = isComment(r);
  const elem = isPositional(r);
  let h = '<div class="detail">';

  if (comment) {
    h += '<div class="field-label">Comment</div>';
    h += `<input class="c-edit" data-field="comment-node" value="${esc(r.value ?? "")}" autocomplete="off" spellcheck="false" />`;
    h += `<dl><dt>Path</dt><dd>${esc(JSON.stringify(r.path))}</dd></dl>`;
    h += '<div class="row-btns"><button class="btn danger" data-act="del">Delete</button></div></div>';
    return h;
  }

  // Key (array-element index is positional, not renamable).
  h += '<div class="field-label">Key</div>';
  if (elem) {
    h += `<input class="v-edit" value="${esc(r.key)}" disabled />`;
    h += '<div class="hint-line">Array-element index is positional — drag the grip to reorder.</div>';
  } else if (!r.read_only) {
    h += `<input class="k-edit" data-field="name" value="${esc(r.key)}" autocomplete="off" spellcheck="false" />`;
  } else {
    h += `<input class="v-edit" value="${esc(r.key)}" disabled />`;
  }

  // Value (scalars only).
  if (!branch) {
    h += `<div class="field-label">Value (${esc(r.type_label)})</div>`;
    h += `<input class="v-edit${r.read_only ? "" : ""}" data-field="value" value="${esc(r.value ?? "")}"${r.read_only ? " disabled" : ""} />`;
  }

  // Trailing comment.
  if (!r.read_only) {
    h += '<div class="field-label">Trailing comment</div>';
    h += `<input class="c-edit" data-field="trailing" value="${esc(r.trailing_comment ?? "")}" placeholder="add a comment…" autocomplete="off" spellcheck="false" />`;
  }

  // Kind switch.
  if (!r.read_only) {
    const hue = branch ? "branch" : valueTypeClass(r).replace("t-", "") || "branch";
    h += '<div class="field-label">Kind</div>';
    h += `<button class="btn kindbtn" data-act="kindswitch"><span class="dotc" style="background:var(--t-${hue})"></span>${esc(r.type_label)} · switch notation</button>`;
  }

  // Meta + actions.
  h += `<dl><dt>Path</dt><dd>${esc(JSON.stringify(r.path))}</dd>`;
  if (branch) h += `<dt>Children</dt><dd>${r.child_count}</dd>`;
  h += "</dl>";
  if (!r.read_only) {
    h +=
      '<div class="row-btns">' +
      '<button class="btn" data-act="dup">Duplicate</button>' +
      '<button class="btn danger" data-act="del">Delete</button></div>';
  }
  h += "</div>";
  return h;
}

function wireDetail(container: HTMLElement, r: ViewRow) {
  const path = r.path;
  const ke = container.querySelector<HTMLInputElement>('[data-field="name"]');
  const ve = container.querySelector<HTMLInputElement>('[data-field="value"]');
  const te = container.querySelector<HTMLInputElement>('[data-field="trailing"]');
  const cn = container.querySelector<HTMLInputElement>('[data-field="comment-node"]');
  const kb = container.querySelector<HTMLElement>("[data-act=kindswitch]");
  const commit = (el: HTMLInputElement, fn: () => void) => {
    el.addEventListener("change", fn);
    el.addEventListener("keydown", (e) => {
      if ((e as KeyboardEvent).key === "Enter") el.blur();
    });
  };
  if (ke)
    commit(ke, () => {
      send({ SetCursor: path });
      send({ CommitEdit: { value: null, name: ke.value } });
    });
  if (ve)
    commit(ve, () => {
      send({ SetCursor: path });
      send({ CommitEdit: { value: ve.value, name: null } });
    });
  if (te)
    commit(te, () => {
      send({ SetTrailing: { path, comment: te.value || null } });
    });
  if (cn)
    commit(cn, () => {
      send({ ApplyEditComment: { path, text: cn.value } });
    });
  if (kb) kb.addEventListener("click", () => openKindSheet(path));
}

function selectNode(path: Path) {
  send({ SetCursor: path });
  send({ SetSelection: { paths: [path] } });
  const r = rowFor(path);
  if (!r) return;
  if (!isWide()) {
    sheets.detail.innerHTML =
      '<div class="grab"></div>' +
      `<div class="sheet-head"><h3>${esc(r.key || lastKey(path))}</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
      `<div class="sheet-body detail-wrap">${detailHTML(r)}</div>`;
    wireDetail(sheets.detail, r);
    openSheet("detail");
  }
  // Wide mode: render() already refreshed the side pane.
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
  let cellRow = -1;
  let chips = "";
  for (const row of grid.rows) {
    if ("Header" in row) {
      chips += `<div class="field-label">${esc(row.Header)}</div><div class="chips">`;
      // close any prior open chips wrapper handled below by structure
      continue;
    }
    cellRow++;
    chips += row.Cells.map((c, col) => {
      const on = c.state === "On" ? "1" : c.state === "Partial" ? "partial" : "0";
      return `<button class="chip" data-on="${on}" data-r="${cellRow}" data-c="${col}"><span class="box">${IC.check}</span>${esc(c.label)}</button>`;
    }).join("");
    chips += "</div>";
  }
  sheets.filter.innerHTML =
    '<div class="grab"></div>' +
    `<div class="sheet-head"><h3>Type filter</h3><button class="close" data-act="closesheet">${IC.close}</button></div>` +
    `<div class="sheet-body">${chips}` +
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

// ---- swipe-to-reveal + grip reorder + tap (ported pointer flow) ----
function closeSwipe() {
  if (openSwipeRow) {
    openSwipeRow.classList.remove("swiped");
    openSwipeRow = null;
  }
}

let sx = 0,
  sy = 0,
  dragRow: HTMLElement | null = null,
  dragging = false,
  decided = false,
  horiz = false;

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
  closeSwipe();
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
    decided = false;
    horiz = false;
  });
  treeEl.addEventListener(
    "pointermove",
    (e) => {
      if (reordering) {
        e.preventDefault();
        onReorderMove(e.clientY);
        return;
      }
      if (!dragging || !dragRow) return;
      const dx = e.clientX - sx,
        dy = e.clientY - sy;
      if (!decided) {
        if (Math.abs(dx) < 8 && Math.abs(dy) < 8) return;
        decided = true;
        horiz = Math.abs(dx) > Math.abs(dy);
        if (horiz && openSwipeRow && openSwipeRow !== dragRow) closeSwipe();
      }
      if (!horiz) return;
      e.preventDefault();
      const m = dragRow.querySelector<HTMLElement>(".row-main")!;
      m.style.transition = "none";
      const base = dragRow.classList.contains("swiped") ? -186 : 0;
      const tx = Math.max(-186, Math.min(0, base + dx));
      m.style.transform = `translateX(${tx}px)`;
    },
    { passive: false },
  );
  treeEl.addEventListener("pointerup", (e) => {
    if (reordering) {
      endReorder();
      return;
    }
    if (!dragging || !dragRow) {
      dragging = false;
      return;
    }
    const main = dragRow.querySelector<HTMLElement>(".row-main")!;
    const dx = e.clientX - sx;
    main.style.transition = "";
    main.style.transform = "";
    if (horiz) {
      const willOpen = dragRow.classList.contains("swiped") ? dx > -60 : dx < -60;
      if (willOpen) {
        dragRow.classList.add("swiped");
        openSwipeRow = dragRow;
      } else {
        dragRow.classList.remove("swiped");
        if (openSwipeRow === dragRow) openSwipeRow = null;
      }
    } else if (!decided) {
      handleTap(e.target as HTMLElement, dragRow);
    }
    dragging = false;
    dragRow = null;
  });
  treeEl.addEventListener("pointercancel", () => {
    if (reordering) {
      endReorder();
      return;
    }
    if (dragRow) {
      const mm = dragRow.querySelector<HTMLElement>(".row-main");
      if (mm) {
        mm.style.transition = "";
        mm.style.transform = "";
      }
    }
    dragging = false;
    dragRow = null;
  });

  // Left-swipe action buttons (outside row-main; handled by click).
  treeEl.addEventListener("click", (e) => {
    const b = (e.target as HTMLElement).closest<HTMLElement>(".row-actions [data-act]");
    if (!b) return;
    const row = b.closest<HTMLElement>(".row")!;
    const path = pathOf(row);
    if (!path) return;
    const act = b.dataset.act;
    closeSwipe();
    if (act === "edit") selectNode(path);
    else if (act === "dup") dupNode(path);
    else if (act === "del") delNode(path);
  });

  treePane.addEventListener(
    "scroll",
    () => {
      if (openSwipeRow) closeSwipe();
    },
    { passive: true },
  );
}

function handleTap(target: HTMLElement, row: HTMLElement) {
  const path = pathOf(row);
  if (!path) return;
  if (target.closest(".kind")) {
    openKindSheet(path);
    return;
  }
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
  if (openSwipeRow) {
    closeSwipe();
    return;
  }
  selectNode(path);
}

// ---- node actions ----
function dupNode(path: Path) {
  send({ SetCursor: path });
  send({ SetSelection: { paths: [path] } });
  send("CopySelected");
  send("Paste");
  toast("Duplicated");
}
function delNode(path: Path) {
  send({ SetCursor: path });
  send({ SetSelection: { paths: [path] } });
  send("DeleteSelected");
  if (!isWide()) closeSheets();
  toast("Deleted");
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
        send("AddNode");
        toast("Node added");
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
      closeSwipe();
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

  installTreeGestures();
  installShellHandlers();

  const wasmUrl = new URL("../pkg/confy_ffi_bg.wasm", import.meta.url);
  await load(wasmUrl);
  session = Session.fromText(SAMPLE, "toml");
  snap = session.snapshot();
  render();
}

void main();
