# Plan — Root-hidden alignment (the document Root is never a row)
Status: Approved (2026-09-14)

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
| T2 | D7 selection guard — `[]` cannot enter the selection | S1 | Open |
| T3 | D3 cursor seeding in core — first top-level Node | S1 | Open |
| T4 | D2 Root unconditionally expanded | S1 | Open |
| T5 | D1 root-hidden flatten + D4 depth rebase | S2 | Open |
| T6 | D5 paste-slot screen order | S2 | Open |
| T7 | D10 Convert drops its root-cursor precondition | S2 | Open |
| T8 | D11 `[G] root` facet / `TypeToken::Root` retired | S2 | Open |
| T9 | D12 zero-row document target + D13 `RevealPath([])` retarget | S2 | Open |
| — | D8/D9 document-scoped action | S3 | **Done** — shipped by the Raw-write record (ADR 0014); placement verified by E6 |
| T10 | D15 title bar + D6 document-edge insertion lines + D12 TUI empty state + the `9`/`0`/`1`/`2`/`e`/`i` root special cases and ~65 row-index test assumptions | S4 | Open |
| T11 | Web/touch/VS Code stand-in deletions + D6 rename + D12 empty state + the `*.spec.mjs` suites | S5 | Open |
| T12 | Remaining reference docs (D16, `HOST_PARITY.md` §2 deletion, `ROW_STATE_MODEL.md` §6a, `MESSAGES.md` keys) + backlog/retrospective/record status flips | S6 | Open |

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

## Ordering constraint

T2–T4 are direction-neutral: the TUI still draws the Root row and stays the parity oracle, so
each is independently revertible. From T5 on, core and both host legs must land in the same
series — do not stop between T5 and T11.
