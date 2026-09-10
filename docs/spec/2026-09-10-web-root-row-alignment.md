# Web UI draws the document Root row (TUI alignment) — Design evaluation

Status: Shipped (2026-09-10)

**Classification:** Planning (design evaluation). Implemented in four commits — `01622af`
(core: Root cursor-only + dimmed set-operations), `d945721` (root visibility becomes core
state + the document-level Action item), `50ea816` (`SetFilename`/`SetRootVisible` intents),
`0bc6df7` (hosts: the drawn desktop Root row, all seven stand-ins deleted). See
[ADR 0013](../adr/0013-root-visibility-is-core-state.md).

Evaluates one change: **the web hosts render the document Root row**, as the TUI already does
(`crates/confy-tui/src/tui/ui.rs`), instead of dropping it from the row list
(`web/render.ts`, `web/touch/render.ts`). Today's divergence and its three consequences are
indexed in [`HOST_PARITY.md`](../reference/HOST_PARITY.md) §2, which calls it "the single
biggest source of host-specific code".

Terminology per [`glossary.md`](../reference/glossary.md): **Root** is the single top-of-tree
Node whose key is the filename, path `[]`, depth 0.

---

## 1. Evidence

All measurements below were taken through the real wasm command channel — the same
`Intent → SessionSnapshot` path both web hosts use — with
`docs/tmp/claude-scratch/root-row-evidence.mjs` (scratch, gitignored; rebuild the pkg with
`cd crates/confy-ffi && wasm-pack build --target web` first). "Rows a web host would draw" =
`rows.filter(r => r.path.length > 0).length`, i.e. `web/render.ts`'s own filter.

| # | Measured | Result |
|---|---|---|
| E1 | Boot state of a fresh Session | `cursor = []`, `rows[0] = { path: [], depth: 0 }` — the cursor starts on the Root and no web host moves it (`web/ui.ts::openText` dispatches `SetLang` only) |
| E2 | `rows[0].key` in a web host | `""` — **the web never calls `set_filename`**: no ffi method (`crates/confy-ffi/src/lib.rs`) and no `Intent` for it. The TUI sets it in `confy_tui::load_document` |
| E3 | `rows[0].badge_label` / `badge_note` | `""` / `""` — `badge_label_note` returns empty for Root (`session/status_fmt.rs`); the TUI draws `[G]` from its own `type_tag` |
| E4 | `ToggleExpand` at boot (web: `Space` → `toggleExpandSelected`) | core returns **1 row** (the Root) → **0 rows a web host would draw**: a blank tree, no cursor, recoverable only via `9` (`ExpandAll`) |
| E5 | Type filter with only the `[G] root` facet on (pointer-reachable: `f`, click one cell) | core returns **1 row** → **0 drawn**: the same blank tree, no keyboard involved |
| E6 | `BeginEditExternal` at boot (cursor on Root) | `kind = Value{path: []}`, `initial === the whole document`; the follow-up `ApplyReplace{path: [], …}` rewrote the whole TOML file with **no error** (all three backends implement whole-document `Replace`) |
| E7 | `OpenActionMenu` with the Root as target | `Edit`, `AddChild`, `Copy`, `Cut`, `Remark`, `Detail`, `Delete` **enabled**; `AddSibling` disabled |
| E8 | Those enabled items, actually dispatched on the Root | `DeleteSelected` → *"delete error: operation not supported here"*; `Remark` → *"remark error: path not found"*; `CutSelected` → *"cut 1 node(s)"* — it **arms the clipboard with the Root**, which can never paste (self-subtree reject), so the modal lock is a dead end until `Esc` |
| E9 | `CursorHome` while the clipboard is armed | `paste_slot = Into([])` — slot index 0 of the stepping order resolves to an append at the document *end* (the reason `overshotUndrawnRootSlot` exists) |

E6 is the motivation, confirmed: **core already supports editing the entire file as text** —
the entry point is what the web lacks. E4/E5 are two reachable blank-tree states caused by the
same root cause. E7/E8 are pre-existing **core** legality defects (equally reachable from the
TUI today) that drawing the Root row would make far more discoverable, since the row acquires a
hover `⋮`.

## 2. Motivation

1. **No whole-document entry point (the stated motive).** Every root-scoped capability core
   offers is either unreachable or accidental in the web hosts:
   - *Edit the entire file as text* (`E` / action `Edit` on the Root → the external-edit modal,
     E6) is reachable **only before the first navigation**, because `drawnCursorFallback`
     (`web/path-utils.ts`) re-targets the cursor off the Root after every keyboard nav. Nothing
     in the UI hints it exists.
   - *Convert the document* is root-only in core (`Session::open_convert`), so the host fakes
     the target: `web/host-io.ts::runSaveConvertShared` dispatches `SetCursor: []` before
     `OpenConvert`.
   - *Append at the document end* (`AddChild` on the Root) has no row to hang a `＋` on.
   - *Fold the whole file to one row* produces a blank tree instead (E4).
2. **A MUST-principle violation today.** `wens-dev-principles ui 5` — the focus cursor is always
   visible. The web breadcrumb's `⌂` segment (`web/breadcrumb.ts`) Reveals the Root, and
   `Home`/`g` target it; both land the cursor on a row nobody draws. The fix in place is a
   host-side re-target, i.e. compensating in the renderer for a state the host chose to hide —
   also counter to `ui 16` (render is a pure function of state) and `ui 1` (single source of
   truth).
3. **The patch count is growing, not shrinking.** Three stand-ins exist purely because the row
   is undrawn, two of them added in the last week as bug fixes (`1323319` "draw the undrawn root
   row's two paste slots", `0ab5a1a` "stop paste-mode's up-step landing on the undrawn root
   slot"). E4 and E5 are the next two bugs in the same family. Drawing the row removes the
   cause; every alternative adds a fourth patch.

## 3. What exists today

Core emits the Root unconditionally (`NodeTree::flatten` → `Session::visible_rows`); there is no
"hide the root" mode and no `is_root` flag — hosts key on `path.length === 0` / `depth === 0`.

| Host-side stand-in | Where | Exists because |
|---|---|---|
| Row filter + `depth - 1` indent | `web/render.ts` (`treeHTML`, `renderRow`), `web/touch/render.ts` | the row is dropped, so every real depth shifts one step left |
| `drawnCursorFallback` | `web/path-utils.ts`; called in `web/ui.ts::navSelect`, `web/touch/app.ts::touchNavSelect` | an invisible cursor after `Home`/`g`/`k`-at-top |
| `rootSlotLine` | `web/slot-line.ts`; 5 call sites (`web/dnd.ts`, `web/ui.ts` ×2, `web/touch/app.ts` ×2) | the Root's two paste slots have no row to paint, so they borrow the first/last row's edge |
| `overshotUndrawnRootSlot` | `web/path-utils.ts`; `web/ui.ts::navSelect`, `web/touch/app.ts::touchNavSelect` | upward nav landing on slot 0 (`Into([])`) threw the insertion point to the document end (E9) |
| `visiblePaths` root filter | `web/select.ts` | shift-range/marquee must not include an undrawn row |
| `drawnAsLine` root case | `web/touch/app.ts::scrollFocusIntoView` | `Into([])` is drawn as a line, not an outlined row |
| Detail-panel suppression | `web/touch/app.ts::renderDetailPanel`, header badge `web.badge.none` | a cursor on the Root has nothing to show |
| `SetCursor: []` before `OpenConvert` | `web/host-io.ts` | Convert is root-only in core |

Root-related web code that is **not** a stand-in and stays either way: `web/breadcrumb.ts`'s
`⌂` crumb and synthetic mini-tree root row, `web/panel.ts::humanPath`'s `"(root)"`,
`parentIsInline`'s `path.length === 0` guard (the Root has no parent).

## 4. Behavior deltas if the Root row is drawn

Per the decisions in §5, the Root row is drawn in **root-visible mode** (desktop web) and absent
in **root-hidden mode** (touch, VS Code) — a mode core itself honors (D5), not a renderer flag.
The user-named areas are §4.2–§4.4.

### 4.1 Rendering

| Area | Today | After |
|---|---|---|
| Row list | root dropped; top-level nodes flush-left | root at indent level 0; **every other row shifts one `--indent` step right** (22 px desktop) — matches the TUI's `"  ".repeat(depth)` |
| Root NAME cell | n/a | the filename — **requires new plumbing** (E2): a `set_filename` ffi method or `Intent`, called by `openText`/save-as (and by the VS Code host, whose title still needs it even root-less). Until then the row would be nameless |
| Root KIND badge | n/a | `badge_label_note` returns `("","")` for Root (E3) → **D3: core gives Root a badge label** (one source; the TUI keeps its `[G]` tag, web renders the pill) |
| Root value / grip | n/a | value cell `—`; **no drag grip** (Root `Move` is `Unsupported`) |
| Empty document | nothing drawn at all | one row (the file), i.e. a real drop target |

### 4.2 Selection and the movable range (incl. drag / cut-copy-paste)

- **Cursor:** `drawnCursorFallback` is **deleted**. In root-visible mode `Home`/`g`/`k`-at-top
  land visibly on the Root, as in the TUI; in root-hidden mode core never puts the cursor
  there in the first place (D5), so `web/ui.ts`'s `jump()` / `openActionMenuFromKeyboard`
  (both `tree.querySelector(".row.cursor")`) can no longer silently fail in either mode.
- **Selection — D1: the Root is cursor-only, never selected.** Core grows the guard:
  `toggle_select` / `set_selection` / `extend_select_*` drop `[]` (core has no `select_all`;
  these four are the whole selection surface), so `web/select.ts`'s `visiblePaths` root filter
  is **deleted** rather than kept as a stand-in. Consequences: a ⇧-range or marquee spanning
  the top stops at the first real row; a plain click on the Root row sets the cursor and clears
  the selection; `selection::normalize`'s whole-document fold becomes unreachable from any UI
  (it is the sole mechanism behind E8's successful `Cut`). Per **D6** the guard is not silent:
  it sets an Info `Notice` ("the Root cannot be selected"), because the same guard is what makes
  a ⇧-range refuse to grow past the top. This is a **TUI behavior change** — `s` on the file row
  used to select it — so `KEYMAP.md`'s `s` row and `TUI.md` gain a line, and it needs one new
  catalog key.
- **Paste slots:** `rootSlotLine` and `overshotUndrawnRootSlot` are both **deleted**. Root-visible
  mode gets the real row's outline/line from the same `pointer_slot` every other row uses;
  root-hidden mode has no `Into([])`/`After([])` slots to paint at all (D5), so the up-step can
  no longer overshoot. E9's quirk — stepping order slot 0 appending at the document *end* — is
  inherited from the TUI unchanged in root-visible mode.
- **Drag:** the Root is a **drop target** only (`Into` = append at the end, like any collapsed
  branch); as a source it must be ungrippable, and `web/dnd.ts`'s self-subtree reject already
  covers the rest.

### 4.3 Expand / collapse

- Root-visible mode: `Space` / caret on the Root collapses the file to a **single visible row**
  (TUI parity, `crates/confy-tui/src/tui/tests.rs` pins exactly this) instead of blanking the
  tree — **E4 fixed**.
- Root-hidden mode: the Root is not a row and not a cursor target, so nothing can collapse it —
  **E4 fixed there too**, structurally, with no host guard and no placeholder row.
- `0` (`CollapseAll`) re-inserts `[]` in core, so it is unaffected; `2` (`CollapseLevel`) already
  guards the Root; `1`/`9` unchanged.
- **E5** likewise splits by mode: root-visible renders the `[G] root` facet's one row;
  root-hidden **omits the `root` facet from the type-filter layout** (`session/type_filter.rs`),
  so the blank-tree state is unreachable instead of guarded.

### 4.4 Panel, chrome, actions

- Detail/edit panel: in root-visible mode the Root gets a read-only panel (Key = filename,
  **not** renamable — core returns `Illegal("cannot rename root")`; Value `—`; Kind;
  Path `(root)`; Children = top-level count). Root-hidden mode never has the Root as cursor, so
  touch's suppression branch and its `web.badge.none` header badge are **deleted**, not kept.
- Row actions (root-visible): hover `＋` = `AddChild` = append at the document end — the
  discoverable "add at the end" affordance the web lacks; `⋮` = the action menu, which **must
  not** offer Cut/Remark/Delete on the Root (E7/E8 — a **core** fix in `action_menu_items`,
  benefiting the TUI too).
- **D7 — root-hidden hosts get *one* document-level action without a row:** the Action menu
  (`session/action_menu.rs`, `ModeView::ActionMenu`, ADR 0009) grows a bottom section, always
  present and separator-led, with a single item — *edit the whole file as text*
  (`core.action.edit-document`). Touch reaches it from its header/FAB; the TUI shows the same
  section (one item list, ADR 0009); VS Code suppresses it, since its native editor owns
  whole-file text editing, the way it already suppresses `q`/`Ctrl+O`. So "root-hidden" stays a
  **rendering** decision, never a capability one.
  **No "append at the document end" item** (considered and rejected 2026-09-10): the existing
  `AddSibling` on the last top-level Node already *is* an append-at-end, so the item would be a
  second spelling of a shipped operation. The one case it does not cover — an **empty**
  document in root-hidden mode, where there is no row to be a sibling of — is served by D11's
  empty-state button, which dispatches `AddChild([])` directly.
- `E` on the Root = edit the whole file in the external-edit modal (E6) — the motive, delivered.
- `C` Convert can drop `host-io.ts`'s `SetCursor: []` workaround: `open_convert` stops requiring
  the cursor to be on the Root (it is document-scoped, not row-scoped).
- Breadcrumb: the `⌂` crumb Reveals the Root in root-visible mode (a visible landing row at
  last); in root-hidden mode it targets the first top-level Node instead.

### 4.5 Out of scope / unchanged

Core's slot order, mutation legality, filter semantics, undo/redo, schema validation, page-step
math, the KIND-vocabulary divergence (`HOST_PARITY.md` §5). One new piece of `Session` state is
added — `root_visible` (D5) — and nothing else.

## 5. Options considered, and the decisions taken

| Option | Content | Cost | Benefit |
|---|---|---|---|
| **A — draw it everywhere** | desktop + touch + VS Code all draw the Root | one render path; touch loses 18 px of indent budget per level; VS Code shows a row whose text-edit action it should defer | full TUI parity; every stand-in deleted |
| **B — host flag `drawRoot`** | `treeHTML`/`renderRow` take a flag off `VSHOST`/entry; core unchanged | **all four stand-ins stay**, now conditionally exercised; E4/E5 need explicit host guards; §2.3's growing-patch-count motive unaddressed for touch/VS Code | smallest core diff |
| **B′ — core owns root visibility** (**chosen**) | `Session` gains `root_visible`, set by the host at open; core's `visible_rows`, paste slots, navigation, `reveal_path` and type-filter layout all agree with it. Desktop web = visible; touch + VS Code = hidden | one new piece of `Session` state; `visible_rows` becomes mode-dependent; core tests double for the hidden mode | **every** stand-in deleted, in **both** modes; E4/E5 become unreachable rather than guarded; each mode is a self-consistent state instead of a hidden one plus compensations (`ui 1`, `ui 16`) |
| **C — keep it undrawn, patch the defects** | fix E4/E5 only | a fifth and sixth stand-in; the motive (§2.1) is unaddressed | smallest diff overall |

**Decisions (2026-09-10):**

- **D1 — the Root is cursor-only, never selected** (all hosts, both modes). See §4.2.
- **D2 — VS Code is root-hidden**, keeping the native editor as the whole-file text editor
  there. It suppresses the document-level *text edit* action only (D7).
- **D3 — core supplies the Root's web badge label: `⌂`, note = the document format**
  (e.g. `⌂·toml`). Same glyph as the breadcrumb's `⌂`, needs no catalog key, and collides with
  no container glyph (`{}` / `[]`). **The TUI keeps its own `[G]` tag** — its bracketed KIND
  vocabulary is an already-documented deliberate divergence (`HOST_PARITY.md` §5), so D3 is
  scoped to "the one Root label the web badge needs", *not* "one label for every host".
- **D4 — only desktop web is root-visible.** Touch keeps its indent budget; with D7 it loses no
  capability.
- **D5 — root visibility lives in core, not in the renderer** (Option B′). Root-hidden mode:
  core omits the Root row, omits its two paste slots, never seats the cursor on it, retargets
  `reveal_path`/`⌂`, and drops the `root` type-filter facet.
- **D6 — the D1 guard reports** an Info `Notice` instead of no-oping silently (`ui 5`); new
  catalog key; `KEYMAP.md` + `TUI.md` record the TUI `s` change.
- **D7 — one document-level action (whole-file text edit) rides the Action menu**, always
  visible, in a bottom section; no append-at-end item (§4.4).
- **D8 — the Root row costs one indent level on desktop** (every other row shifts 22 px right),
  matching the TUI. Rejected: a non-indenting "file header" row — it would render the Root as a
  row that is visibly not the parent of its children, and `slotLineIndentPx` could not stay
  self-consistent for an `Into(root)` drop line.
- **D9 — the filename arrives as `Intent::SetFilename`**, dispatched by **every** host (E2), not
  as a side-channel ffi method: the filename is `Session` state (the TUI sets the same field in
  `load_document`), and the wire contract is one command channel, so a side channel would punch
  a hole in the `?diag=1` trace. Root-hidden hosts dispatch it too — Convert and save-as default
  names read it.
- **D10 — the mode arrives as `Intent::SetRootVisible(bool)`**, dispatched by the host at open,
  structurally identical to D9. Rejected: a `from_text` constructor parameter (forks Session
  construction and makes two-mode headless tests awkward) and a **user preference** (a View-menu
  toggle / persisted setting would turn a host-environment fact into user-maintained state and
  add an unknown to every bug report; it can grow out of D10 later at no cost).
- **D11 — an empty document in root-hidden mode is the host's empty state.** Core keeps its
  contract clean (root-hidden means no Root, no exception for zero children), so zero rows is a
  legal, reachable state there; touch and VS Code **must** render an empty-state panel whose
  primary button dispatches `AddChild([])`. Root-visible hosts need none — they always have at
  least the file row. Rejected: re-emitting the Root row as a special case for empty documents.
- **D12 — the desktop Root row keeps its hover `⋮`** (row-targeted menu via `selected_paths()`'s
  cursor fallback; D7's section lives in that same menu, so nothing is duplicated), and its
  **kind badge is inert** — no click-to-kind-switch, no hover title suggesting one, because the
  Root has no kind to switch to. Closes §6.
- **D13 — this design gets ADR 0013** (`docs/adr/0013-root-visibility-is-core-state.md`),
  covering both faces of "what is the Root in the model": root visibility as core state
  (B vs B′) and D1's cursor-only rule. The ADR outlives this record, which gets archived.
- **D14 — the glossary is updated in the same breath** (done 2026-09-10): the **Root** entry
  carries the cursor-only rule, a new **Root-visible / root-hidden** entry replaces the
  host-framed "root-less mode" wording, and **Reveal** matches both.

Slices (each independently verifiable):

1. **Core, host-independent** — D1 guard + D6 notice, D3 Root badge label, E7/E8 action-flag
   legality (`Cut`/`Remark`/`Delete` disabled when the single target is the Root). Benefits the
   TUI immediately; no host change.
2. **Core, root visibility** — D5's `root_visible` (set via D10's `Intent::SetRootVisible`)
   threaded through `visible_rows`, `paste_slots`/`slot_target`, cursor seating/navigation,
   `reveal_path`, `type_filter` layout; D7's one-item document-level Action-menu section;
   `open_convert` stops requiring a Root cursor. Headless tests cover **both** modes.
3. **Plumbing** — D9 `Intent::SetFilename` + the ffi/`web` wrappers; each host dispatches its
   filename and its visibility mode at open.
4. **Hosts** — desktop renders the Root row (anatomy per §4.1/§4.4, incl. D12's `⋮` and inert
   badge) and **deletes** all four stand-ins plus the `SetCursor: []` workaround; touch +
   VS Code wire D7's action (VS Code suppressing it), add D11's empty state, and drop their root
   special cases; the three web specs and `HOST_PARITY.md` §2 are rewritten around the two modes.
5. **ADR 0013 + doc sweep** — D13/D14 and the §7 doc list.

## 6. Open questions

None. §6's `⋮`/badge question is settled by D12; the empty-document hole found in round 2 is
settled by D11. Every branch of the design tree has been walked.

## 7. Verification plan (when implemented)

- **Core:** `cargo test -p confy-core` + new cases in `tests/session_headless.rs` for the Root
  action flags, the D1 selection guard + D6 notice, and **both** visibility modes (root-hidden:
  no Root row, no `Into([])`/`After([])` slots, cursor never seats on `[]`, no `root` facet);
  `crates/confy-tui/src/tui/tests.rs` root-collapse tests must stay green (the TUI is
  root-visible and is the parity oracle).
- **Web:** the three specs that pin the stand-ins — `web/touch-key-scroll.spec.mjs`,
  `web/armed-paste.spec.mjs`, `web/paste-hover.spec.mjs` — are rewritten against the two modes:
  the touch/VS Code cases assert core simply never emits the root row/slots, the desktop cases
  assert the drawn Root row instead of a borrowed row edge; `npm test` + `npm run typecheck`.
- **Real artifact:** rebuild the wasm, then confirm in a browser that on desktop E4/E5 no longer
  blank the tree, `E` on the Root opens the whole file, and a drag onto the Root row appends at
  the end; on touch confirm the Action menu's document-level *edit whole file* / *append at end*
  work and that no root row/slot appears; in VS Code confirm the text-edit action is suppressed;
  and confirm the Tauri desktop build still drags (`dragDropEnabled: false` regression watch).
- **Docs:** `HOST_PARITY.md` §2 is rewritten as *root-visible (desktop web, TUI) vs root-hidden
  (touch, VS Code)*, with the stand-in list deleted rather than moved; `WEBUI.md`,
  `ROW_STATE_MODEL.md` §6a, `TUI.md` §Rendering, `KEYMAP.md` (the `s` change), `CHROME.md`
  (the document-level Action-menu section), `glossary.md` (Root, Reveal, and the new
  root-visible/root-hidden pair), `CLAUDE.md`'s module map (`slot-line.ts`, `path-utils.ts`,
  `select.ts` all shrink) and `CHANGELOG.md`.

## 8. Recommendation

Implement **Option B′** in the four slices above, with slice 1 (D1 guard + D6 notice, D3 badge
label, E7/E8 legality) landing first: it is host-independent, benefits the TUI immediately, and
is what makes a Root row safe to expose. The §2.1 motive is then satisfied on **every** host —
desktop web by a row the user can see, put the cursor on, and act on with the same keys and menu
as any other Node; touch and VS Code by D7's document-level Action-menu section — and the four
host stand-ins are deleted rather than made conditional, because root visibility is a core state
both modes agree with instead of a hidden state the renderer compensates for.

Independently of this decision, E4, E5 and E7/E8 are verified live defects and are filed in
[`../plan/2026-09-09-open-follow-ups.md`](../plan/2026-09-09-open-follow-ups.md).
