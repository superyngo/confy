# Root-hidden alignment — the document Root is never a row, on any host
Status: Shipped (2026-09-14)

S0–S6 landed (`dc69a47`…`4328778`); plan: [`../plan/2026-09-11-root-hidden-alignment.md`](../plan/2026-09-11-root-hidden-alignment.md)

Predecessor: [`../debug/2026-09-11-root-row-alignment-retrospective.md`](../debug/2026-09-11-root-row-alignment-retrospective.md)
(the reversed *web-aligns-to-TUI* attempt; branch `root-row-alignment`, `e8e8d5b`..`f2bfef3`).
ADR to record with slice 1: `0013-the-root-is-never-a-row.md` (0013 is unused on `main` —
the reversed branch's ADR 0013 was rewound and never shipped).

Sibling record: [`2026-09-11-raw-write-mode-design.md`](2026-09-11-raw-write-mode-design.md) —
it takes over **P3** (the whole-document entry point) and **ships first**. D8's Action item
becomes that feature's quick entry rather than a modal opener; D9's placement is unchanged.

Every file:line below was read on **main at `db3f700`**, not carried over from the branch
(retrospective lesson 5: an observation made under a reverted mechanism does not transfer).

## Problem

`confy-core` projects the document Root as a real row (`NodeTree::flatten` walks from
`self.root` at depth 0, `crates/confy-core/src/model/node.rs:255-272`). One host draws that
row (the TUI), two filter it out and then compensate for the hole (`web/render.ts:130`,
`web/touch/render.ts:74`). The compensation is now the single largest host divergence in the
repo — [`HOST_PARITY.md`](../reference/HOST_PARITY.md) §2 exists for it alone — and it has
produced four live defects:

- **P1 — the clipboard can be armed with the Root.** The cursor starts on `[]`
  (`session.rs:110`); nothing at the selection boundary excludes it, so `m` → Cut succeeds
  with a node that can never paste (self-subtree reject) while the armed clipboard holds the
  modal lock (ADR 0005 §5). `Delete` answers "operation not supported here", `Remark` the
  misleading "path not found". Sites: `session/clipboard.rs` (`cut_selected`,
  `move_selection_to`), `session/action_menu.rs:41`.
- **P2 — two web-only blank trees.** `Space` on the Root collapses the file; core returns one
  row, both web renderers drop it — a tree with no rows and no cursor, recoverable only via
  `9`. `f` → the `[G] root` facet (`session/type_filter.rs:191,250,276,316`) reaches the same
  state by pointer alone.
- **P3 — the web has no entry point for whole-document operations.** Whole-file `Replace`
  exists in all three backends and `begin_external_edit` on `[]` hands back the entire file
  (`session.rs:430-460`, `external_edit_path` returns `([], false)` for the empty path,
  `session.rs:1738-1755`), but the only route to it is *putting the cursor on the Root row*,
  which no web host draws. This was the original trigger of the reversed work and survives it.
- **P4 (new) — the paste-slot order contradicts the screen order.** `paste_slots()`
  (`session.rs:670-680`) emits each row's `Into` before its `After`, so `Into([])` is index 0
  while `slot_target` resolves it to `children.len()` — an append at the document's **end**
  (`session.rs:710`+). The TUI's drawn Root row hides the contradiction; the web pays for it
  with `overshotUndrawnRootSlot` (`web/path-utils.ts:40`) plus a stand-in edge renderer.

The reversed attempt asked "where should each host draw the Root row?". This record takes the
direction that was never on that table: **no host draws it.** The TUI's Root row was the origin
of the asymmetry, and it is the cheaper side to change — it is one visible row, a title-bar
read, four expand/collapse guards and a Convert precondition, against ~200 lines of web
stand-ins across seven modules.

## Decision

**The Root is a model node, never a view row.** Core stops emitting it to every host; nothing
is host-configurable (no `root_visible`, no `SetRootVisible` — retrospective Q1 answered *no*:
build the toggle when a second real mode exists). Whole-document operations stop being
"operations on the Root row" and become **document-scoped commands** reachable identically
everywhere.

### Decisions taken

| # | Decision | Consequence / retrospective link |
|---|---|---|
| D1 | `NodeTree::flatten`'s consumer in `Session` drops the Root row unconditionally; the Root keeps its place in the model, `node_at(&[])`, `Target { parent: [] }` and every whole-file mutation | Q1 = no flag. `visible_nodes`/`visible_rows` are the only two sites (`session.rs:165,190`) |
| D2 | The Root is **unconditionally expanded**: `flatten(&|p| p.is_empty() \|\| expanded.contains(p))` | Kills `collapse_all`'s `insert(Vec::new())` (`session.rs:541`), `collapse_level`'s two root guards (`session.rs:596-605`), and P2's collapse route structurally |
| D3 | Cursor seeding moves **into core**: `from_tree` seats the cursor on the first top-level Node; `cursor_home`/`page_up`/nav already clamp to `visible_nodes()`, which no longer contains `[]` | Q2 answered in core, not as a host patch. `drawnCursorFallback` is **deleted**, not ported to the TUI |
| D4 | Depth is rebased in core: a top-level Node is depth 0 | Deletes `Math.max(0, r.depth - 1)` in both web renderers; the TUI's indent arithmetic loses one level |
| D5 | `paste_slots()` is re-emitted in **screen order**: `After([])` first (document top, root index 0), then per-row `Into`/`After`, then `Into([])` last (append at `children.len()`) | Fixes P4 in core for every host; deletes `overshotUndrawnRootSlot`. The two document-edge slots have no row to paint on **any** host now, so their rendering becomes a shared, uniform rule rather than a web-only stand-in (D6) |
| D6 | Both document-edge slots render as **insertion lines on a borrowed row edge**, identically everywhere: top slot at the first row's top edge, end slot at the last row's bottom edge. `web/slot-line.ts`'s `rootSlotLine` survives (renamed `documentEdgeLine`), and the TUI gains the same two cases in its existing standalone-insertion-line path (`tui/ui.rs:483-510`) | The TUI currently green-fills the `Into` *row*; with no Root row it must draw a line, which is what both web hosts already do |
| D7 | The selection boundary keeps the **Root-never-selected** guard (branch `01622af`, direction-neutral): all four selection entry points drop `[]`; Cut/Copy/Remark/Delete refuse with `core.selection.root-excluded` | Q3 = keep as insurance. Fixes P1 even though D1/D3 make the pointer/keyboard routes unreachable |
| D8 | Whole-document text edit becomes an **Action-menu item** (`ActionId::EditDocument`, `core.action.edit-document`, always enabled, dispatching `begin_external_edit_document`) — reusing branch `d945721`'s core mechanism. **Amended:** the surface it opens is the Raw pane in write mode on the web hosts and `$EDITOR` on the TUI — see the raw-write record's R1/R3/R9; the core mechanism is identical either way | Fixes P3 and replaces the TUI's "`e` on the Root row" route on every host, TUI included |
| D9 | **Item placement (settled 2026-09-11):** `EditDocument` sits **immediately above `Delete`, in the same section** — the section-leading separator moves onto `EditDocument` (`separator_before: true`, `danger: false`) and `Delete` loses it (`separator_before: false`, still `danger: true`, still the list's last row) | Keyboard `Enter` and a mis-tap still cannot land on `Delete` by overshooting past the list end, and the separator keeps meaning "document/destructive section". Deviates from branch `d945721`, which gave the item its own trailing section |
| D10 | Convert (`C`) drops its root-cursor precondition in core (`session.rs:1283-1300`); `core.convert.root-only` is retired | Deletes `web/host-io.ts:218`'s faked `SetCursor: []`. Convert becomes document-scoped like D8 |
| D11 | The `[G] root` type-filter facet is deleted from all three format layouts; `TypeToken::Root` is retired from the facet vocabulary (classification of the Root node itself is unreachable once it is not a row) | Fixes P2's pointer route structurally. Filter ancestor-keeping still keeps `[]` in `filtered_paths` (`tui/tests.rs:54`) — unchanged |
| D12 | Zero-row document: core resolves an add target to `Target { parent: [], index: 0 }` when there is no cursor row; every host draws an empty-state affordance (web `.tree-empty` + Add, branch D11; a TUI hint line) | With the Root row gone, an empty file is a genuinely empty tree on every host — today the TUI always had one row to stand on |
| D13 | `RevealPath([])` (breadcrumb `⌂`, `web/breadcrumb.ts:180`) retargets to the first row **in core** | Branch `d945721`'s retarget, kept; not a host patch |
| D14 | No `Intent::SetFilename` (branch `50ea816` is **not** carried) | Its only consumer was the Root row's key. Both web orchestrators already show the filename in their own chrome; the TUI title bar switches to `session.tree.root.key` (D15) |
| D15 | `draw_title` stops reading `app.rows.first()` (`tui/ui.rs:270`) and reads the Root node's key directly | Otherwise removing the row blanks the TUI title bar — found while reading main, not present in the retrospective |
| D16 | Vocabulary: `glossary.md`'s **Root** entry states it is never a view row; *root-visible / root-hidden* are **not** introduced as host-facing terms (they described two modes that will never both exist); `HOST_PARITY.md` §2 is **deleted**, not rewritten | Q5 |

### Section shape after D9

```
Edit / Add child / Add sibling / Copy / Cut / Remark / Detail
──────────────────────────────────────────────────────────── (separator)
Edit whole file            ← EditDocument, document-scoped, always enabled
Delete                     ← danger, last row
```

`action_menu_move` skips disabled items, so the cursor never rests between the two; the
destructive row keeps its position as the list's last, which is the position muscle memory
already has.

### Open item that must be re-measured before D5 lands

The living backlog's *Watching* entry (`../plan/BACKLOG.md:60-66`) —
`Into([])` observed resolving to root index 0 on the branch, but reading as
`children.len()` on main. D5 rewrites exactly this ordering, so slice 0 re-measures it once
on this tree through the real wasm channel and records the answer. Retrospective lesson: do
not trust either reading until re-measured on the tree being changed.

## Slices

Each slice is one commit (code + `CHANGELOG.md` + the docs it invalidates), and each states
its own acceptance. The mechanism/presentation boundary that made the last reversal cheap is
kept: slices 1 and 2 are direction-neutral core work, slice 3 is the requirement that started
this, slices 4-5 are the host legs.

**Documentation first (settled 2026-09-11).** `SD` lands before any product code: the ADR, this
record's approval, and the task plan. Reference docs under `docs/reference/` are **not** part of
`SD` — they describe current behavior, so each one moves with the slice that changes that
behavior (the remainder in `S6`).

**SD — documentation (no product code).** `docs/adr/0013-the-root-is-never-a-row.md` (the
decision, its two rejected alternatives — per-host `root_visible`, and the reversed
*web-aligns-to-TUI* direction — and the D9 placement); this record promoted `Draft` →
`Approved`; `docs/plan/2026-09-11-root-hidden-alignment.md` as the task-by-task plan derived
from it; the retrospective cross-linked to both.
*Acceptance:* ADR 0013 exists and is listed in `docs/adr/README.md`; the plan enumerates every
slice below as tasks with the same acceptance criteria; `rg -n '^Status: (Draft|Approved|In
progress)' docs/{spec,plan}/*.md` shows this record `Approved` and the plan `Approved`.

**S0 — baseline evidence (no product code).**
Re-measure `Into([])`/`After([])` resolution through `crates/confy-ffi/functional_smoke.mjs`
and reproduce P1/P2 on the real `confy` binary and the real wasm channel. Record as E1..En in
this record's *Evidence* section.
*Acceptance:* every P above is either reproduced with a transcript or struck from the record.

**S1 — core mechanism (direction-neutral).** D7 selection guard, D3 cursor seeding, D2
root-always-expanded. No row-shape change yet, so the TUI still draws the Root row and stays
the parity oracle.
*Acceptance:* `cargo test -p confy-core`; new headless tests pin that `[]` cannot enter the
selection, that a fresh `Session` seats the cursor on the first top-level Node, and that
`collapse_all` + `Space` can no longer produce a zero-row tree.

**S2 — core presentation contract.** D1 root-hidden flatten, D4 depth rebase, D5 slot order,
D10 Convert, D11 facet removal, D12 empty-document target, D13 reveal retarget.
*Acceptance:* `cargo test` (workspace) green after the TUI/test churn below; `paste_slots()`
order asserted against screen order; a zero-child document yields zero rows and a legal add
target.

**S3 — the document-scoped action (D8/D9).** `ActionId::EditDocument` + i18n keys in
`i18n/en.json` and `i18n/zh-TW.json` + all four renderers
(`tui/overlay_action_menu.rs`, `web/action-menu-items.ts` — shared by the desktop popup and
the touch sheet — and the VS Code host's suppression decision, which must be re-taken now
that this is the only whole-file route: VS Code owns the buffer, so the item likely stays
**enabled** there and routes through the extension's `TextDocument`).
*Acceptance:* the item appears **directly above `Delete`** and carries the section separator in
all four surfaces, is always enabled, and opens the whole file in `$EDITOR` (TUI) / the
external-edit modal (web); `functional_smoke.mjs` drives it end-to-end.

**S4 — TUI host leg.** D15 title bar, D6 insertion lines at both document edges, D12 empty
state, and the simplification fallout: `9`/`0`/`1`/`2` lose their root special cases, `e`/`i`
on the Root row disappear as routes (S3 replaces the first; Detail-on-Root is dropped), and
the ~55 row-index assumptions in `crates/confy-tui/src/tui/tests.rs` plus 10 in `ui.rs`'s
tests are rewritten.
*Acceptance:* `cargo test -p confy-tui`; `cargo clippy -- -D warnings`; and a real-binary pass
against a fixture confirming the title bar still names the file, the first row is a top-level
Node at the leftmost indent, paste mode cues both document edges, and `C` works from any row.

**S5 — web/touch/VS Code leg.** Deletions: both render row filters and depth shifts,
`drawnCursorFallback`, `overshotUndrawnRootSlot` (`web/path-utils.ts`), `select.ts:11`'s root
filter, `host-io.ts:218`'s faked cursor, and touch's root cases in `drawnAsLine`/badge/panel.
Kept and renamed: `slot-line.ts`'s document-edge helper (D6). Plus the empty state (D12) and
the six `*.spec.mjs` suites that encode the stand-ins.
*Acceptance:* `npm test`, `npm run typecheck`, `node functional_smoke.mjs`; the deleted
symbols return no hits under `rg`.

**S6 — remaining docs.** `glossary.md` (D16); `HOST_PARITY.md` §2 deleted; `ROW_STATE_MODEL.md`
§6a rewritten as one uniform rule; `WEBUI.md`, `TUI.md`, `KEYMAP.md`, `MESSAGES.md`
(`core.convert.root-only` retired, `core.selection.root-excluded` added), `CLAUDE.md`'s module
map; the three open-follow-up rows → *Done*, the *Watching* row settled; the retrospective →
`Status: Resolved (YYYY-MM-DD)`; this record → `Shipped`.
*Acceptance:* the two-pass documentation audit (`wens-dev-principles docs 19`); no reference
doc still claims a host draws the Root row.

## Evidence

Measured on **main at `21cbea9`** (2026-09-14) by S0: a throwaway `confy-core` integration
test for the core readings, the real wasm channel (`crates/confy-ffi/pkg`) for the host-facing
ones, and the real `target/debug/confy` binary under `tmux` for the TUI. Fixture:
`title = "t"` + `[a] x = 1` (+ `[b] y = 2` where a third row matters).

**E1 — the Root is row 0 at depth 0, and the cursor starts on it.** Core rows:
`0: path=[] depth=0 key=""`, `1: [title] depth=1`, `2: [a] depth=1`, `3: [b] depth=1`;
`cursor = []`. Confirms D1/D3/D4's premise (the depth rebase is exactly one level).

**E2 — the TUI really draws it.** Real binary, `--lang en`:
```
confy — s0.toml ──────────────────────────────────────────────── v1.2.0
  NAME                                   KIND     VALUE
 ▾   s0.toml                             [G]
       title                             [S:str ] "t"
   ▸   a                                 [T/S]
```
Row 0 is the file; the title bar names the file too — so removing the row costs nothing in
the title bar *provided* D15 lands (`draw_title` reads `app.rows.first()` today).

**E3 — P4 reproduced, and the *Watching* row is settled: main appends.**
`paste_slots()` on the 3-row fixture, each resolved through `slot_target`:
```
0: Into([])      -> Target { parent: [], index: 3 }   <- document END
1: After([])     -> Target { parent: [], index: 0 }   <- document TOP
2: After([title])-> Target { parent: [], index: 1 }
3: Into([a])     -> Target { parent: [a], index: 1 }
4: After([a])    -> Target { parent: [], index: 2 }
5: Into([b])     -> Target { parent: [b], index: 1 }
6: After([b])    -> Target { parent: [], index: 3 }
```
`Into([])` is slot **index 0** on screen but resolves to the document's **end** — the exact
contradiction D5 re-orders away. The branch's "`Into([])` prepends" observation does **not**
reproduce: it is `children.len()` here, an append. The backlog's *Watching* row is answered.

**E4 — P1 reproduced, on the wasm channel.** A fresh session (cursor on `[]`) →
`CutSelected` → `clipboard_count = 1`: the clipboard is armed with the Root. In core the same
call reports `cut 1 node(s)`. `OpenActionMenu` immediately afterwards returns **zero items**
(the armed clipboard holds the modal lock, ADR 0005 §5), so the state is reachable and its
only exit is `Esc`. D7's guard is required.

**E5 — P2 reproduced, on the wasm channel.** `ToggleExpand` with the cursor on `[]` →
core returns **1 row**, of which the web renderers draw **0** (`rows.filter(path.length > 0)`).
A blank tree with no cursor. D2 removes the route structurally.

**E6 — P3 is closed, and so is S3.** Its fix shipped as the sibling Raw-write record
(`ActionId::EditDocument` + `Intent::BeginEditDocument`, ADR 0014, `5190d7a`…`d54539a`).
D9's placement is already satisfied: `OpenActionMenu` over the wasm channel returns
`Edit / Add child / Append sibling / Copy / Cut / Toggle comment / Detail / **Edit whole
file** / Delete` — the document item sits immediately above `Delete`, always enabled.
**S3 is therefore done**; the remaining work is S1, S2, S4, S5, S6.

No P is struck: P1, P2 and P4 are reproduced above, and P3 is reproduced-then-fixed.
