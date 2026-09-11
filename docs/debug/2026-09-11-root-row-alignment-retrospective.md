# Root-row alignment — retrospective and handoff
Status: In progress

The root-row-alignment work (ADR 0013, design record
`2026-09-10-web-root-row-alignment.md`) was **reversed on 2026-09-11 before any release**:
instead of teaching every host to draw the document Root, **every host — the TUI included —
goes root-hidden**. The TUI's root row was the origin of the asymmetry; aligning web to it
was the wrong direction, and the reverse alignment is cheaper everywhere.

This record is the handoff to the redesign. The full implementation is preserved on branch
**`root-row-alignment`** (six commits, `e8e8d5b`..`f2bfef3`); `main` was rewound to `b0f40e1`
without rewriting anything. Findings below are re-verified against **main's current tree**,
not the branch.

## What survives the reversal (cherry-pick candidates)

| Branch commit | Why it survives |
|---|---|
| `01622af` — the Root takes the cursor but is never selected; Cut/Copy/Remark/Delete refused at the selection boundary | Direction-neutral. Fixes P1 below under *any* rendering decision. |
| `50ea816` — `Intent::SetFilename` | The web never received the open file's name at all (`rows[0].key` is `""` on main today). Needed in every design. |
| `d945721` — `Session.root_visible` + mode-aware flatten, `Intent::SetRootVisible`, the document-level Action item | The mechanism is reusable as-is if any visibility choice remains; the document-level item (whole-file text edit) is a requirement, not a rendering choice. See Q1/Q4. |
| slice 4's touch/VS Code leg | Those hosts are root-hidden in the new direction too — the stand-in deletions there are the reference implementation. |
| slice 4's desktop Root-row drawing + TUI changes | **Discarded.** Do not resurrect. |

## Problems that must be handled (all live on main today, all re-verified)

**P1 — the clipboard can be armed with the Root, dead-ending the modal lock.** The cursor
starts on `[]`; `m` (Action menu) on it lets **Cut succeed** — the clipboard now holds a node
that can never paste (self-subtree reject), and the armed clipboard holds the modal lock
(ADR 0005 §5) until `Esc`. `Delete` reports "operation not supported here"; `Remark` reports
the misleading "path not found". Sites: `session/clipboard.rs` (`cut_selected`,
`move_selection_to`), `session/action_menu.rs`. Measured through the wasm channel on the
branch (E7/E8) and confirmed by reading main's guard-free selection path. →
open-follow-ups row.

**P2 — two web-only blank trees.** `Space` with the cursor on the Root collapses the file;
core returns one row, both web renderers filter it out — a blank tree with no cursor,
recoverable only via `9`. The `f` → `[G] root` type-filter facet blanks the tree the same way
by pointer alone. → open-follow-ups row; the new direction makes both structurally
unreachable (core emits no root row to any host, and the facet disappears with it).

**P3 — the web still has no entry point for whole-document operations.** This was the
*original trigger* of the whole effort and it survives the reversal: whole-file `Replace`
exists in all three backends and `BeginEditExternal` on the Root hands back the entire file,
but no web host can reach either. The document-level Action item (branch `d945721`) is the
carried-forward answer. → open-follow-ups row.

## Unresolved observation — do not carry forward unexamined

The branch CHANGELOG observed that `Into([])` seemed to resolve to root **index 0 (prepend)**
rather than the `children.len()` append. That observation **did not reproduce on main**:
`slot_target` (`session/session.rs`, `PasteSlot::Into(p) => index: children.len()`) was never
touched by the branch and reads as append for every path, `[]` included. Either it was a
branch-mode artifact or a misread. Filed under *Watching*; one clean re-measure on whichever
tree the new design builds on settles it. Lesson: an observation made under a new mechanism
does not automatically transfer back after a revert — re-verify on the tree you are about to
change.

## Process lessons

1. **The mechanism was built configurable before the product decision stabilized.** Two
   "real modes" (root-visible desktop, root-hidden touch) never existed as requirements; when
   the direction collapsed to *uniform hidden*, the per-host configurability became the first
   thing to question. Build the toggle only when a second real mode exists.
2. **The alignment direction itself was never on the option table.** The design record's
   *Considered options* evaluated where the Root row should be drawn, but always took
   "web aligns to TUI" as the fixed axis; "TUI aligns to web" — the eventual answer — was
   never enumerated. For any cross-host symmetry question, both alignment directions are
   first-class options.
3. **Slicing contained the cost of the reversal.** The mechanism slices (selection guard,
   core state, intents) were direction-neutral and survive; only the presentation slice is
   discarded. Keep the mechanism/presentation boundary when planning the redo.
4. **Evidence-first made the reversal cheap and honest.** E1–E9 were measured through the
   real wasm channel and the real TUI binary, so "which parts still hold?" was answerable by
   re-reading evidence, not re-deriving history. Keep that discipline.
5. **"Unreachable by current UI paths" is the fragile invariant that created the seven
   stand-ins in the first place.** The new direction will make Root-targeted defects
   unreachable too — that is not the same as fixed. See Q3.

## Questions the new design must settle

- **Q1 — does `root_visible` state exist at all?** If no host ever shows the Root, core can
  simply never emit the row/cursor/slots, and the flag + `SetRootVisible` intent die. Simpler;
  but check whether any future host (a hex-editor-like raw view?) wants the row before
  deleting the seam.
- **Q2 — cursor seeding.** The `Session` seats the cursor at `[]` at open. With no root row
  anywhere, that seating must move to the first top-level Node **inside core** — not as a
  host-side fallback. (`drawnCursorFallback` existed precisely because a host papered over
  core's seating; do not recreate it as a TUI patch.)
- **Q3 — keep the Root-never-selected boundary guard?** Under uniform root-hidden it becomes
  unreachable from every current UI path — exactly the class of "dead by unreachability" that
  rotted before. The guard is a few lines at the selection boundary; keep it as insurance.
- **Q4 — the document-level Action item** (whole-file text edit) must land; it is the
  requirement that started this, independent of rendering.
- **Q5 — glossary.** The branch updated `docs/reference/glossary.md` for the two-mode
  vocabulary; the new design needs its own vocabulary pass ("root-hidden everywhere" makes
  *Root-visible/root-hidden* as host-facing terms obsolete).
