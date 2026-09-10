// Grip drag-reparent / reorder → `MoveSelectionTo` intent (WEBUI.md, mirrors
// `design_index_model.html`'s drag model). Dragging a row's grip moves it (or,
// if it is part of the selection, the whole selection) to wherever the pointer
// classifies as a `PasteSlot` — `Into` a branch (green `.drag-over-into`
// outline) or `After` a row (horizontal `#dropLine`).
//
// The destination is core's call end to end (ADR 0010): `pointerSlot` (=
// `Session::pointer_slot`) turns "this row, this relative Y" into the slot, and
// the drop hands that slot to `MoveSelectionTo`, which resolves it with the
// same `slot_target` an armed keyboard paste uses. This file used to ask core
// only whether the hover was `Into`, then hand-roll the rest as "before/after
// this row ⇒ a sibling in `parentOf(path)` at `siblingIndex(...) ± 1`" with a
// 0.5 split. Both halves of that were wrong against core: `After` an
// *expanded* branch means that branch's **first child** (`resolve_target`), not
// a sibling one level up — so dragging into the gap under an expanded `[table]`
// aimed a level too shallow (in TOML usually straight into "a key here would be
// captured by the table above it", document untouched) while an armed paste
// released at the very same pixel landed inside; and core's leaf before/after
// boundary is 0.75, so the 0.5–0.75 band classified the two gestures opposite
// ways. Index/legality stay core's job: the move routes through `do_paste` (a
// real `Mutation::Move`), which rejects collision / illegal / self-subtree
// drops with the document untouched.
import type { Intent, Path, PasteSlot, SessionSnapshot } from "./types.js";
import { pathEq as eq } from "./path-utils.js";
import { slotLineIndentPx } from "./slot-line.js";

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
  // The hovered `PasteSlot`, verbatim from core — the only drop-target state.
  let slot: PasteSlot | null = null;

  const rowOf = (t: EventTarget | null): HTMLElement | null =>
    (t as HTMLElement | null)?.closest?.(".row") ?? null;
  const pathOf = (row: HTMLElement | null): Path | null =>
    row?.dataset.path ? (JSON.parse(row.dataset.path) as Path) : null;

  const clearOver = () => {
    treeEl.querySelectorAll(".drag-over-into").forEach((el) => el.classList.remove("drag-over-into"));
    dropLine.style.display = "none";
  };
  const endDrag = () => {
    sources = null;
    slot = null;
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
    // One classification, core's own (ADR 0010): no local band thresholds, no
    // before/after fork. "Above this row" already arrives as `After(<the
    // preceding row's slot>)` from `paste_slots()`'s flattened order, which is
    // why there is no `before` case left to draw.
    slot = pointerSlot(path, (ev.clientY - r.top) / r.height) ?? null;
    if (!slot) return;
    if ("Into" in slot) {
      row.classList.add("drag-over-into");
      return;
    }
    // `After(p)`: the line sits under `p`'s row — which may be a different row
    // than the hovered one — and one indent level deeper when `p` is an
    // expanded branch, because that is where core will actually insert.
    const lineRow =
      treeEl.querySelector<HTMLElement>(`.row[data-path='${CSS.escape(JSON.stringify(slot.After))}']`) ??
      row;
    const lr = lineRow.getBoundingClientRect();
    const wr = wrap.getBoundingClientRect();
    const indentW = (lineRow.querySelector(".indent") as HTMLElement | null)?.offsetWidth ?? 0;
    dropLine.style.top = `${lr.bottom - wr.top + wrap.scrollTop}px`;
    dropLine.style.left = `${slotLineIndentPx(lineRow, indentW) + 8}px`;
    dropLine.style.display = "block";
  });

  treeEl.addEventListener("drop", (ev) => {
    if (!sources || !slot) return endDrag();
    ev.preventDefault();
    const src = sources;
    const dest = slot;
    const cut = !(ev.altKey || ev.ctrlKey);
    endDrag();
    // Slot in, nothing derived: core resolves it through `slot_target`, the
    // same call an armed `Paste` makes, so the two gestures can't disagree.
    send({ MoveSelectionTo: { sources: src, slot: dest, cut } });
  });

  treeEl.addEventListener("dragend", endDrag);
}
