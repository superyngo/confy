// Grip drag-reparent / reorder → `MoveSelectionTo` intent (WEBUI.md, mirrors
// `design_index_model.html`'s drag model). Dragging a row's grip moves it (or,
// if it is part of the selection, the whole selection):
//   - over a **branch**'s middle band (rel 0.25–0.75) → drop **into** it (append
//     as a child); the branch row gets a `.drag-over-into` outline.
//   - otherwise → drop **before/after** the hovered row as a sibling; a
//     horizontal `#dropLine` shows the insertion point.
// Index/legality stay core's job: the move routes through `do_paste` (a real
// `Mutation::Move`), which adjusts the original-sequence index for removed
// earlier siblings and rejects collision / illegal / self-subtree drops with the
// document untouched. The sibling index is read from the snapshot (when a parent
// is expanded all its direct children — comments included — are visible rows in
// document order, so their position equals core's full-child-sequence index).
import type { Intent, Path, PasteSlot, SessionSnapshot, ViewRow } from "./types.js";
import { parentOf, pathEq as eq, siblingIndex } from "./path-utils.js";

type DropTarget =
  | { mode: "into"; path: Path }
  | { mode: "before" | "after"; path: Path };

export function installDnd(
  treeEl: HTMLElement,
  getSnap: () => SessionSnapshot | null,
  send: (i: Intent) => void,
  pointerSlot: (path: Path, relY: number) => PasteSlot | undefined,
  // Runs at the end of every `endDrag()` — after `clearOver()` — so the owner
  // can redraw what that wipes unconditionally: the armed-paste cue (ADR
  // 0004 §1), whose `.drag-over-into` row class and `#dropLine` double as
  // the drag feedback. Kept optional and last so callers can pass
  // `pointerSlot` without it.
  onDragEnd?: () => void,
): void {
  const wrap = document.getElementById("treeWrap") as HTMLElement;
  const dropLine = document.getElementById("dropLine") as HTMLElement;
  let sources: Path[] | null = null;
  let target: DropTarget | null = null;

  const rowOf = (t: EventTarget | null): HTMLElement | null =>
    (t as HTMLElement | null)?.closest?.(".row") ?? null;
  const pathOf = (row: HTMLElement | null): Path | null =>
    row?.dataset.path ? (JSON.parse(row.dataset.path) as Path) : null;
  const rowFor = (snap: SessionSnapshot, p: Path): ViewRow | undefined =>
    snap.rows.find((r) => eq(r.path, p));

  const clearOver = () => {
    treeEl.querySelectorAll(".drag-over-into").forEach((el) => el.classList.remove("drag-over-into"));
    dropLine.style.display = "none";
  };
  const endDrag = () => {
    sources = null;
    target = null;
    clearOver();
    treeEl.querySelectorAll(".drag-src").forEach((el) => el.classList.remove("drag-src"));
    // Last, and only after the wipe above: restore the armed-paste cue
    // `clearOver()` may have collaterally hidden while a clipboard is armed.
    onDragEnd?.();
  };

  treeEl.addEventListener("dragstart", (ev) => {
    if (document.body?.classList?.contains("paste-mode")) {
      ev.preventDefault();
      return;
    }
    const handle = (ev.target as HTMLElement).closest?.("[data-grip]");
    const row = rowOf(ev.target);
    const path = pathOf(row);
    const snap = getSnap();
    if (!handle || !path || !snap) {
      ev.preventDefault();
      return;
    }
    const selected = snap.rows.filter((r) => r.selected).map((r) => r.path);
    sources = selected.some((p) => eq(p, path)) ? selected : [path];
    // Dim the dragged rows (design's `.drag-src`); don't re-render mid-drag.
    for (const src of sources) {
      const el = treeEl.querySelector(`.row[data-path='${CSS.escape(JSON.stringify(src))}']`);
      el?.classList.add("drag-src");
    }
    ev.dataTransfer?.setData("text/plain", "confy-move");
    if (ev.dataTransfer) ev.dataTransfer.effectAllowed = "copyMove";
  });

  treeEl.addEventListener("dragover", (ev) => {
    if (!sources) return;
    ev.preventDefault(); // allow drop
    const copy = ev.altKey || ev.ctrlKey;
    if (ev.dataTransfer) ev.dataTransfer.dropEffect = copy ? "copy" : "move";
    const row = rowOf(ev.target);
    const path = pathOf(row);
    const snap = getSnap();
    if (!row || !path || !snap || sources.some((s) => eq(s, path))) return;
    clearOver();
    const r = row.getBoundingClientRect();
    const rel = (ev.clientY - r.top) / r.height;
    const isInto = (slot: PasteSlot | undefined): slot is { Into: Path } =>
      !!slot && "Into" in slot;
    // Into-eligibility (branch, non-`Format::Inline`) now comes from core's
    // `pointer_slot` (ADR 0004 §1) instead of a hand-rolled copy of the same
    // check — `touch/app.ts`'s own copy had already drifted (different
    // thresholds, no `Format::Inline` guard at all), which is exactly the
    // per-surface drift this unification eliminates. The before/after
    // sibling-index math below is unchanged — only the into/not-into decision
    // is now core's call.
    if (isInto(pointerSlot(path, rel))) {
      row.classList.add("drag-over-into");
      target = { mode: "into", path };
    } else {
      const before = rel < 0.5;
      const wr = wrap.getBoundingClientRect();
      const indentW = (row.querySelector(".indent") as HTMLElement | null)?.offsetWidth ?? 0;
      dropLine.style.top = `${(before ? r.top : r.bottom) - wr.top + wrap.scrollTop}px`;
      dropLine.style.left = `${indentW + 8}px`;
      dropLine.style.display = "block";
      target = { mode: before ? "before" : "after", path };
    }
  });

  treeEl.addEventListener("drop", (ev) => {
    if (!sources || !target) return endDrag();
    ev.preventDefault();
    const snap = getSnap();
    const src = sources;
    const tgt = target;
    const cut = !(ev.altKey || ev.ctrlKey);
    endDrag();
    if (!snap) return;
    if (tgt.mode === "into") {
      // Append as the last child (design pushes onto `children`).
      const idx = rowFor(snap, tgt.path)?.child_count ?? 0;
      send({ MoveSelectionTo: { sources: src, target: tgt.path, index: idx, cut } });
    } else {
      const sib = siblingIndex(snap.rows, tgt.path);
      send({
        MoveSelectionTo: {
          sources: src,
          target: parentOf(tgt.path),
          index: tgt.mode === "after" ? sib + 1 : sib,
          cut,
        },
      });
    }
  });

  treeEl.addEventListener("dragend", endDrag);
}
