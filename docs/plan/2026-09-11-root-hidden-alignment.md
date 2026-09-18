# Plan — Root-hidden alignment (the document Root is never a row)
Status: Shipped (2026-09-14)

Design record: [`../spec/2026-09-11-root-hidden-alignment-design.md`](../spec/2026-09-11-root-hidden-alignment-design.md)
(decisions D1–D16, slices SD–S6, Evidence E1–E6).
ADR: [`../adr/0013-the-root-is-never-a-row.md`](../adr/0013-the-root-is-never-a-row.md).
Retrospective: [`../debug/2026-09-11-root-row-alignment-retrospective.md`](../debug/2026-09-11-root-row-alignment-retrospective.md).

One task = one commit (code + `CHANGELOG.md` + the docs that task invalidates). Acceptance is
copied from the design record's slices; nothing here adds scope to it.

| # | Task | Slice | Status |
|---|---|---|---|
| T0 | ADR 0013 + design record `Approved` + this plan | SD | **Done** (ADR `2026-09-11`; record/plan `2026-09-14`) |
| T1 | Baseline evidence E1–E6 | S0 | **Done** (2026-09-14) |
| T2 | D7 selection guard — `[]` cannot enter the selection | S1 | **Done** (2026-09-14) |
| T3 | D3 cursor seeding in core — first top-level Node | S1 | **Done** (2026-09-14) |
| T4 | D2 Root unconditionally expanded | S1 | **Done** (2026-09-14) |
| T5 | D1 root-hidden flatten + D4 depth rebase | S2 | **Done** (2026-09-14) |
| T6 | D5 paste-slot screen order | S2 | **Done** (2026-09-14) |
| T7 | D10 Convert drops its root-cursor precondition | S2 | **Done** (2026-09-14) |
| T8 | D11 `[G] root` facet / `TypeToken::Root` retired | S2 | **Done** (2026-09-14) |
| T9 | D12 zero-row document target + D13 `RevealPath([])` retarget | S2 | **Done** (2026-09-14) |
| — | D8/D9 document-scoped action | S3 | **Done** — shipped by the Raw-write record (ADR 0014); placement verified by E6 |
| T10 | D15 title bar + D6 document-edge insertion lines + D12 TUI empty state + the `9`/`0`/`1`/`2`/`e`/`i` root special cases and ~65 row-index test assumptions | S4 | **Done** (2026-09-14) |
| T11 | Web/touch/VS Code stand-in deletions + D6 rename + D12 empty state + the `*.spec.mjs` suites | S5 | **Done** (2026-09-14) |
| T12 | Remaining reference docs (D16, `HOST_PARITY.md` §2 deletion, `ROW_STATE_MODEL.md` §6a, `MESSAGES.md` keys) + backlog/retrospective/record status flips | S6 | **Done** (2026-09-14) |

## Task detail

**T2 (D7).** All four selection entry points drop `[]`; `cut_selected` / `copy_selected` /
Remark / Delete refuse with a new `core.selection.root-excluded` message (both catalogs).
*Acceptance:* `cargo test -p confy-core`; a headless test pins that `CutSelected` with the
cursor on `[]` leaves `clipboard` `None` (today: E4 measures `clipboard_count = 1`).

**T3 (D3).** `Session::new`/`from_tree` seats the cursor on the first top-level Node;
`cursor_home`/`page_up`/nav already clamp to `visible_nodes()`.
*Acceptance:* a headless test pins the seeded cursor; `drawnCursorFallback` is **not** ported
to the TUI (it is deleted in T11).

**T4 (D2).** `flatten(&|p| p.is_empty() || expanded.contains(p))`; drop `collapse_all`'s
`insert(Vec::new())` and `collapse_level`'s two root guards.
*Acceptance:* a headless test pins that `CollapseAll` + `ToggleExpand` can no longer yield a
one-row (web: zero-row) tree — today E5 measures exactly that.

**T5–T9 (S2).** Direction-committing core presentation work. `cargo test` (workspace) must be
green, which means the TUI's row-index test churn lands with T5, not deferred to T10.
*Acceptance:* `paste_slots()` asserted in screen order (`After([])` first, `Into([])` last, E3
is the before-picture); a zero-child document yields zero rows and a legal add target.

**T10 (S4)/T11 (S5)/T12 (S6).** Acceptance verbatim from the record's S4/S5/S6, including the
real-binary TUI pass (title bar still names the file, first row is a top-level Node at the
leftmost indent, both document edges cue in paste mode, `C` works from any row) and
`rg` returning no hits for the deleted web symbols.

## Deviations recorded while implementing

- **T4 kept `collapse_level`'s two root guards.** The design record's D2 says they die with
  the re-insertion. They cannot yet: `path.is_empty()` guards a `path.len() - 1` underflow, and
  `target.is_empty()` is what stops a top-level row's collapse from parking the cursor on the
  Root while the TUI still draws that row. Both become removable in T10, once no cursor can
  reach the Root at all.
- **T4 also had to fix two Root-expanded readers the record does not name:** `is_path_visible`
  (an ancestor-prefix walk that required `[]` in the expand set, so every single-row lookup —
  `cursor_row`, hence the whole Action menu — went blind) and `is_expanded` (the TUI drew a
  collapsed `▾`→`▸` caret on a file whose children were all on screen). Both now mirror
  `visible_nodes`' predicate.
- **T2 additionally dims the Action menu's node-scoped items on the Root** (the backlog row's
  own acceptance wording), leaving only document-scoped `Edit whole file`. The refusal notice
  stays as the keyboard backstop.

### S2 (T5–T9)

- **T5 forced the TUI's root row out in the same commit**, as the ordering constraint predicted:
  `compute_rows` feeds `app.rows`, so dropping the Root there removes the drawn row. D15 (the
  title bar reading the new `Session::root_key` instead of `rows.first()`) therefore landed here
  rather than in T10 — without it the title bar goes blank.
- **Three core lookups had to learn the document edges.** `slot_target` resolved a slot by
  *finding its row*, so both `[]` slots returned `None` (silently no-op moves) until they were
  resolved from the tree instead. `paste_slots` now emits them explicitly (D5) instead of
  inheriting them from row 0, and `move_paste_slot` no longer drags the cursor onto `[]`.
- **D7's guard needed a second case.** `selected_paths()` returns *nothing* for a Root cursor
  now, so `paths.is_empty()` short-circuited before the guard; the guard also treats "no operand
  while the cursor is on the Root" as the Root case.
- **`Home` in paste mode changed meaning** (a real, intended D5 consequence): the first slot is
  the document *top* (`After([])`), where it used to be `Into([])`, the document *end*.
- **T8 removed the `[G] root` facet from all three layouts but kept `TypeToken::Root`** as a
  classification value: `classify` still maps `NodeKind::Root`, and filter ancestor-keeping
  still puts `[]` in `filtered_paths`. Deleting the variant is vocabulary cleanup for S6.
- **T9's host empty states are still open** (S4/S5); the core half — an add target of
  `Target { parent: [], index: children.len() }` with no cursor row, and `RevealPath([])`
  retargeting to the first row — is done.
- **Web/touch renderers lost `Math.max(0, r.depth - 1)`** here too, since D4 rebased depth in
  core; the remaining web stand-in deletions are still T11.

### S4 (T10)

- **D6's two edge lines are drawn by `draw_tree` itself, not by `paste_line_row`** — they hang off
  the viewport edges (`start == 0` / `end == total`), not off a `RowSnapshot`, so they share only
  the green `─` styling. Verified on the real binary: `Home` puts the line above the first row,
  `End` below the last.
- **D12's empty state is a `Paragraph`, drawn instead of the `Table`** when `app.rows` is empty
  (new `tui.tree.empty` in both catalogs). Verified on an empty `.toml`.
- **Dead root guards removed** now that no host can put the cursor on the Root:
  `toggle_expand`'s `path.is_empty()` early-return, `collapse_level`'s first one (its
  `target.is_empty()` check *stays* — a top-level Node's parent IS the Root), both
  `extend_select_*` anchor guards plus the `rows[idx - 1].path.is_empty()` row test, and
  `edit_target_kind`'s. `toggle_select`'s guard stays: an empty document leaves the cursor at
  `[]` with no row to move to.

### S5 (T11)

- **A real bug fell out of the browser check, not the test suites**: `compute_rows` dropped any
  paste slot whose path wasn't a visible row, so both document-edge slots were wiped the instant
  `Home`/`End` set one (the snapshot then fell back to `After(cursor)`). Every unit test passed;
  only stepping the slots in a real browser showed it. `compute_rows` now exempts the empty path.
- **Six web stand-ins deleted**: `drawnCursorFallback` and `overshotUndrawnRootSlot`
  (`path-utils.ts`, plus their two call sites in `navSelect`/`touchNavSelect`), `select.ts`'s
  root filter, `host-io.ts`'s faked `SetCursor: []` before `OpenConvert` (dead after D10), and
  the two `r.path.length === 0` row skips in `render.ts`/`touch/render.ts`.
  `rootSlotLine`/`RootSlotLine` → `documentEdgeLine`/`DocumentEdgeLine`.
- **D12's web empty state** is a `.tree-empty` div (new `web.tree.empty` key, one CSS rule per
  sheet), returned early by both renderers. Verified in a real headless Chromium by deleting
  every top-level node.
- **`touch-key-scroll.spec.mjs` lost its sections 4 and 5** and now asserts the *absence* of both
  corrections in the two nav functions; `touch-render.spec.mjs` gained the empty-state case (its
  zero-row `treeHTML` check had to move to a one-row snapshot).
- **`HOST_PARITY.md` §2 is deleted** ("the undrawn root row (web only)") — the divergence it
  documented no longer exists. Sections were not renumbered: §3 onward keep their anchors.

### S6 (T12)

- **D16** — the glossary's **Root** entry now states the contract (model node, empty path,
  never a view row, its two slots are the document-edge slots) and adds "root row" to its
  _Avoid_ list.
- **`ROW_STATE_MODEL.md` §6a** rewritten: the two document-edge bullets replace the
  root-row/stand-in pair, and the `compute_rows` staleness caveat is recorded there.
- **`TypeToken::Root` retired** (vocabulary cleanup, D11's second half): `classify` now returns
  `Option<TypeToken>` — `None` for the Root, the one kind with no facet — and the TUI's
  `type_tag` returns an empty tag for it. The `[G]|root/file node` row is deleted from the Help
  KIND legend in all three formats × both catalogs, with the remaining `containers.N` keys
  renumbered (the TUI's legend reader walks `N` from 1 until a key is missing, so a gap would
  truncate the list).
- **Backlog/record statuses flipped**: the three Open rows in
  `docs/plan/BACKLOG.md` moved to Done, the retrospective is
  `Resolved (2026-09-14)` with each of P1/P2/P4 mapped to the decision that closed it, the
  design record is `Shipped`, and ADR 0013's status records the implementation commits.

## Ordering constraint

T2–T4 are direction-neutral: the TUI still draws the Root row and stays the parity oracle, so
each is independently revertible. From T5 on, core and both host legs must land in the same
series — do not stop between T5 and T11.
