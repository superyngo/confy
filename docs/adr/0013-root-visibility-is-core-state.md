# Root visibility is core state, and the Root is never selected

Status: Superseded (2026-09-11) — direction reversed before any release; see the note below
Design record: [`../spec/2026-09-10-web-root-row-alignment.md`](../spec/2026-09-10-web-root-row-alignment.md)
(evidence E1–E9, decisions D1–D14)

> **Superseded 2026-09-11, before any release.** The direction was reversed: every host —
> the TUI included — goes **root-hidden**, instead of teaching hosts to draw the Root. The
> TUI's root row was the asymmetry's origin, so "web aligns to TUI" was the wrong axis; the
> design record's *Considered options* never enumerated the reverse alignment. The
> implementation this ADR describes is preserved on this branch (`e8e8d5b`..`f2bfef3`);
> `main` was rewound to `b0f40e1` with nothing rewritten. The salvage list, the defects that
> remain live on `main`, and the redesign questions are in
> `docs/debug/2026-09-11-root-row-alignment-retrospective.md` on `main`; a replacement ADR
> will take the 0013 number there.

## Context

`NodeTree::flatten` → `Session::visible_rows` emits the **Root** unconditionally; there has never
been a "hide the root" mode and there is no `is_root` flag on `ViewRow`. The TUI draws that row.
Both web hosts drop it (`web/render.ts`, `web/touch/render.ts` filter `path.length === 0` and
render every real Node at `depth - 1`), which leaves core holding a row-shaped state no web
renderer will draw. Seven host-side mechanisms exist only to compensate:

| Stand-in | Compensating for |
|---|---|
| row filter + `depth - 1` (`render.ts`, `touch/render.ts`) | the dropped row shifting every depth |
| `drawnCursorFallback` (`path-utils.ts`) | an invisible cursor after `Home`/`g`/`k`-at-top |
| `rootSlotLine` (`slot-line.ts`, 5 call sites) | the Root's two paste slots having no row to paint |
| `overshotUndrawnRootSlot` (`path-utils.ts`) | upward nav landing on slot 0 (`Into([])`) |
| `visiblePaths` root filter (`select.ts`) | a range/marquee including an undrawn row |
| detail-panel + header-badge suppression (`touch/app.ts`) | a cursor on a row with nothing to show |
| `SetCursor: []` before `OpenConvert` (`host-io.ts`) | Convert being root-only in core |

Two of them shipped in the last week as bug fixes (`1323319`, `0ab5a1a`), and two more defects of
the same family were measured through the real wasm channel while evaluating this change: with the
cursor on the Root (where it starts), `ToggleExpand` returns one row that both web hosts filter out
— a blank tree with no cursor, recoverable only via `9`; the `[G] root` type-filter facet does the
same with the pointer alone. The patch count was growing, not shrinking.

Separately, the Root is **selectable** in core: `toggle_select` / `set_selection` /
`extend_select_*` have no guard, and `selection::normalize` folds every other path into an ancestor
— so selecting the Root means "the whole document", and the action menu (whose target falls back to
the cursor row) then offers `Cut`, `Remark` and `Delete` on it. Measured: `Delete` reports
"operation not supported here", `Remark` reports the misleading "path not found", and `Cut`
**succeeds**, arming the clipboard with a node that can never paste (self-subtree reject) — a modal
dead end. This is reachable in the TUI today. The glossary already described the Root as having
"no selectable row"; core did not agree with the glossary.

The trigger for revisiting all of this: the web UI has **no entry point for whole-document
operations**, even though core supports them. Whole-document `Replace` is implemented in all three
backends, and `BeginEditExternal` on the Root hands back the entire file — but the only way to
reach it in a web host is before the first keyboard navigation, because `drawnCursorFallback` then
moves the cursor away, and nothing in the UI hints it exists.

## Decision

### 1. Root visibility is a mode of the `Session`, not of the renderer

`Session` gains `root_visible: bool`, set by the host at open via `Intent::SetRootVisible(bool)`
(the same channel `Intent::SetFilename` uses — the filename was likewise never plumbed to the web,
so `rows[0].key` was `""`). Two self-consistent modes, both honored **inside core**:

- **Root-visible** (TUI, desktop web): the Root is a row, a cursor target, a drop target, and
  contributes its own indent level.
- **Root-hidden** (touch, VS Code): `visible_rows` omits the Root row; `paste_slots` omits
  `Into([])`/`After([])`; the cursor is never seated on `[]`; `reveal_path` and the breadcrumb's
  `⌂` retarget to the first top-level Node; the `root` type-filter facet is not offered.

All seven stand-ins are **deleted**, not made conditional. A host never compensates for a state
core exposes; it tells core which shape it has and renders the rows it is given
(`wens-dev-principles ui 1`, `ui 16`).

### 2. The Root takes the cursor but is never selected

Every selection entry point (`toggle_select`, `set_selection`, `extend_select_up`,
`extend_select_down` — core has no `select_all`) drops `[]`. A ⇧-range or marquee reaching the top
stops at the first top-level Node; `selection::normalize`'s whole-document fold becomes unreachable
from any UI. The guard is not silent: it sets an Info `Notice`, because the same guard is what makes
a range refuse to grow past the top. `action_menu_items` additionally disables `Cut`/`Remark`/
`Delete` when the single target is the Root, since the action menu still reaches it through the
cursor fallback.

This is a behavior change in the **TUI** (`s` on the file row used to select it) and is recorded in
`KEYMAP.md` and `TUI.md`.

### 3. Root-scoped operations are reachable in every mode

Root-visible hosts reach them from the row: `E`/`Edit` = edit the whole file as text, hover `＋` =
`AddChild` = append at the document end, `⋮` = the row-targeted action menu (its kind badge is
**inert** — the Root has no kind to switch). Root-hidden hosts get one always-visible,
separator-led document-level item at the bottom of the same Action menu (ADR 0009's single item
list, so the TUI shows it too): *edit the whole file as text*. VS Code suppresses that one item,
because its native editor owns whole-file text editing, exactly as it already suppresses `q` and
`Ctrl+O`.

No "append at the document end" item: `AddSibling` on the last top-level Node already *is* an
append-at-end. The one case it cannot express — an **empty** document in root-hidden mode, where
there is no row to be a sibling of — is a legal, reachable state there, and root-hidden hosts must
render an empty state whose primary button dispatches `AddChild([])`. Root-visible hosts need none;
they always have at least the file row.

So "root-hidden" is a **rendering** decision. It never removes a capability.

## Consequences

- Both blank-tree defects become unreachable rather than guarded: root-visible mode collapses the
  file to the single file row (TUI parity, pinned by `crates/confy-tui/src/tui/tests.rs`);
  root-hidden mode has no Root row to collapse and no `root` facet to filter by.
- `visible_rows`, `paste_slots`, navigation, `reveal_path` and the type-filter layout become
  **mode-dependent**. Every headless test touching them needs a root-hidden counterpart; that
  doubling is the price of the decision and is the acceptance criterion for the core slices.
- The desktop tree shifts one `--indent` step (22 px) right at every level. Accepted over a
  non-indenting "file header" row, which would draw the Root as a row visibly *not* the parent of
  its children and would leave `slotLineIndentPx` unable to stay self-consistent for an
  `Into(root)` drop line.
- Touch keeps its 18 px-per-level indent budget by being root-hidden, and loses nothing: §3 gives
  it the whole-file text edit it never had.
- Wire additions: `Intent::SetRootVisible`, `Intent::SetFilename`, one `ActionId` for the
  document-level item, two catalog keys (the selection notice, the action label). Additive — no
  existing intent changes shape.
- The glossary is the durable statement of §2 and §1's vocabulary: the **Root** entry carries the
  cursor-only rule, and a **Root-visible / root-hidden** entry replaces the host-framed
  "root-less mode" phrasing (`_Avoid_: "drawRoot flag"`).

## Considered options

- **Draw the Root in every host** (and let VS Code cede only the text edit). One render path, every
  stand-in deleted, full TUI parity — the original recommendation. Rejected: it spends touch's
  tightest resource (indent budget per level on a phone) for the least benefit, and puts a row in
  the VS Code webview whose primary action that host should decline.
- **A host-side `drawRoot` flag** through `treeHTML`/`renderRow`, core untouched. Smallest core
  diff. Rejected: it keeps all seven stand-ins alive and merely makes them conditional, leaves the
  two blank-tree defects needing explicit host guards, and preserves the exact condition — a core
  state the renderer hides and then compensates for — that produced four bugs in one week. Two
  modes exist either way under this decision; the question was only whether core knows about them.
- **Keep the Root undrawn and patch the two defects.** Rejected: two more stand-ins, and the
  whole-document entry point (the reason for the change) stays missing.
- **Make root visibility a user preference** (View-menu toggle, persisted per host). Rejected for
  now: it turns a host-environment fact into user-maintained state and adds an unknown to every bug
  report. It can grow out of `Intent::SetRootVisible` later at no cost.
- **Keep the Root selectable and fix only the three action flags.** Rejected: "select the Root"
  means "the whole document" through `normalize`'s ancestor fold, which is a *document* operation
  wearing a Node-selection costume; the flags would have to be re-checked at every future
  selection-driven operation instead of once at the selection boundary.
