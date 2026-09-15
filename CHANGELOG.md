# Changelog

All notable changes to this project will be documented in this file. This file carries
`[Unreleased]` plus the **current version series**; completed series are archived verbatim
under [`docs/reference/changelog/`](docs/reference/changelog/README.md).

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


## [Unreleased]

### 2026-09-15

**Added**

- The web Raw pane's breadcrumb mapping is **two-way** now (Q4). Moving the caret in the Raw
  pane moves the tree cursor and breadcrumb onto the Node the caret sits in — the inverse of
  the breadcrumb pick, which already selected a Node's source span. Core answers it with
  `Session::node_at_offset` (also on the wasm surface), the **innermost** Node whose span
  contains a byte offset; the Block editor's own `path_at_offset` could not be reused because
  it resolves the first Node at or *after* a splice anchor and so skips the Node a caret is
  inside. Three guards keep a two-way binding from oscillating: a one-shot latch armed by the
  jump, a 50 ms debounce so a drag-select or held arrow key collapses to one dispatch, and an
  identity short-circuit when the resolved Node is already the cursor. Both directions stay
  gated on a clean Raw-write buffer, since `text_range`s describe the last commit. TUI, touch
  and VS Code are unaffected: no Raw pane, no breadcrumb, write mode suppressed.

**Fixed**

- Cross-format convert's lossy-normalization warnings are **translated** now. All thirteen
  of them (three schema-hint drops, eight style normalizations, two semantic-loss
  conversions) were raw English in every surface — CLI stderr, the TUI convert overlay, the
  web convert dialog — the last user-facing text in the app that bypassed the catalog. They
  are a structured `ConvertWarning` enum in `model/` (which has no `Lang` and must not gain
  one) carrying `catalog_key()`, resolved at the same edge as every other string: the
  `Session` convert projection and the CLI. `ConvertView.warnings` stays `Vec<String>`, so
  the wasm wire contract, `web/types.ts` and all three host renderers are untouched.

- The Block editor's cursor re-anchor made a **key rename** quadratic: it asked the backend
  for every node's spans, and each such query serializes and projects the whole document. A
  rename in a 37 KB / 3,000-node TOML file took **20.7 s** (500 sections: 4.5 s; 200: 0.65 s),
  while the same edit that keeps its key took 26 ms — the re-anchor only runs when the
  pre-edit path stops resolving, which is exactly a rename. It now reads the projected
  `Node::text_range` that `project()` already filled: **25 ms** for that same rename, flat
  against the same-key cost. (Introduced in this same unreleased series, so no shipped
  release is affected.)
- Reference-doc drift left by the Block switchover: `MUTATIONS.md` said two
  `replace_table_spans` guards were retired (they are still enforced on the keyed-path
  `Mutation::Replace` the Block route no longer uses), `TUI.md` still credited
  `serialize_fragment`, `WEBUI.md` still said a Comment Apply closes unconditionally,
  `BEHAVIOR_MATRIX.md` and `ConfigDocument::array_elements_addressable`'s doc still cited the
  deleted `wrap_element`, the `ARCHITECTURE.md` module map listed two deleted `blank_lines`
  functions and none of the four new modules, and the glossary called the editor's buffer a
  "fragment" — the word its own **Block** entry tells you to avoid.
- A latent backwards-range slice in the TOML span walk: `end_of(Target::AotGroup)` returned
  `0`, which the `Unsupported`-extent fallback would have handed to `&full[start..0]`. That
  fallback is for a node sharing a line with a flow collection, which an `[[array-of-tables]]`
  group never is, so it was unreachable — `end_of` now returns `Option` and the group declines
  instead of trusting that. Block-edit span coverage closed with it: TOML's **scattered** AoT
  group (spec §3's third multi-span shape), the "a preceding standalone Comment is excluded"
  rule for JSON and YAML (previously TOML-only), and eight parity-matrix rows for the shapes
  JSON/YAML had none — branch, array element / sequence item, flow member, comment block.
- YAML: a multi-line `#` comment block's **trailing blank count** was measured after its
  *first* line, not after the block, so the Detail popup's `Blank after:` read 0 for a 2-line
  block followed by two blank lines (and `SetTrailingBlankLines` anchored at the wrong
  offset). The projection recorded only the run's first token — the lexer does not merge a
  `#` run, contrary to what the extent code's own comment claimed. The Comment node's range
  now spans first-token start to last-token end, and the extent walk continues through the
  block's remaining lines; the two local workarounds this had grown in the span walk are gone
  with it, so the block's end has one algorithm.

**Removed**

- Eleven orphaned i18n keys, from both catalogs: `tui.prompt.{collision,confirm-quit,
  type-change,array-upgrade}` (prompt text renders core-side; the TUI uses the `.legend`
  siblings), `web.prompt.q.{arrayUpgrade,confirmQuit}` (the web reads
  `ModeView::Prompt.question`), `core.action.title`, and `web.host.{add.node,add.child,
  add.sibling,kind.no-options}` — the last four also dropped from `severity_of`, whose
  host-key count goes 23 → 19.

**Changed**

- The multi-line editor (`e` / `$EDITOR`) now edits a Node's **Block** — the text of the byte
  spans it owns — instead of a fragment bound 1:1 to that Node. A buffer may **rename the
  Node's key** or emit **several sibling Nodes**;
  it is accepted if and only if it parses as a legal Node sequence at the Node's container
  level. Otherwise the whole commit is rejected, the document is untouched, and `$EDITOR`
  reopens **holding your text** — the error arrives as a `Warn` notice, never inside the
  buffer. An empty buffer is refused rather than read as a deletion (`d` deletes). A scattered
  `[T/S]`/`[T/D]` table hands over all of its spans joined and consolidates at the first.
  All three hosts: the TUI's `$EDITOR`, the desktop pop-up and the touch sheet — the two web
  surfaces keep the pop-up/sheet **open holding your text** instead of re-spawning an editor.
  A **Comment** row takes the same route, so deleting the `#` in the buffer **un-comments** the
  Node (and adding one comments it out) — a conversion the old per-Node route could not express
  at all. ([spec](docs/spec/2026-09-15-block-edit-whole-file-reparse-design.md))
- YAML **opaque** nodes (`&anchor`, `*alias`, `<<:` merge, `!tag`) are now **text**-editable
  through `e`: that route reparses the whole file and recomputes opaque fencing from scratch,
  which the whole-file `E` route already relied on. They stay read-only *structurally* — no
  rename, kind switch, remark, or paste-into. `read_only` accordingly means "not structurally
  editable", restated in the glossary and BEHAVIOR_MATRIX.
- Docs: the **Block** ↔ **Remark** relationship is now stated — Remark comments the lines of
  the Node's Block minus that Block's trailing blank run, which is where the own-line rule
  comes from (both `glossary.md` entries and `MUTATIONS.md`'s Remark row).

**Removed**

- The per-Node fragment plumbing the Block editor replaced: `Session::external_edit_path` (the
  array-element `__elem__ = …` wrap and the "which path can this fragment address" redirection),
  `apply_external_replace`'s `wrap_element` parameter and the buffer's packaged-blank split, and
  `ExternalEditKind::Comment`. `Intent::ApplyEditComment` survives as the desktop panel's
  **one-line** comment field only — a field with neither the `#` markers nor the indent a Block
  carries — and no longer splits blank lines off the text it is given.
- Three more leftovers the audit found: `web/types.ts`'s `ExternalEditKind.Comment` member
  (the Rust enum has only `Value`, so it could never arrive), `multiline_edit_initial`'s
  non-empty-path branch (dead since the switchover — it is now the whole-document seed only,
  which is its one live caller), and with it `blank_lines::with_trailing_run` /
  `trim_trailing_blank_lines`, whose last consumer that branch was.

**Docs**

- The Block-edit implementation audit is a frozen record now
  (`docs/audit/2026-09-15-block-edit-implementation-audit.md`, indexed in that folder's README
  and cross-linked from both the design record and the plan): one blocking performance defect,
  nine smaller findings, each with the commit that fixed it, plus the two scout claims that
  were wrong. It also carries the same-day backlog sweep, which emptied the Open section of
  `docs/plan/2026-09-09-open-follow-ups.md`.
- Nine commit citations across `CHANGELOG.md` and four plan docs pointed at **unreachable**
  objects: each was written as a placeholder, `sed`-ed to the real hash, then `git commit
  --amend`-ed — which rewrote the commit the citation had just named. All nine now name their
  reachable commit, and `CLAUDE.md` gained the rule plus the one-line check that finds them.
  (The five remaining unreachable hashes are deliberate: the `root-row-alignment` record cites
  the abandoned branch it documents.)
- `ARCHITECTURE.md` corrections: `text-offset.ts` now documents both conversion directions,
  `session.rs` names the two span queries the Raw panes bind through, `block_edit_parity.rs`
  joins the integration-suite list, and two stale counts (19 → 21 core suites, 37 → 39 web
  spec suites) are current. The Block-edit plan gained the `Status:` line every `docs/plan/`
  document is required to carry — it was the only one missing.

**Fixed**

- The **web** external editor seeded its buffer from the old fragment producer while committing
  a Block, which is a seed/commit mismatch: handing the buffer back untouched was itself an
  edit. Measured across `tests/block_edit_identity.rs`'s corpus, **193 of 779** Nodes were
  affected — every JSON/TOML block-formatted member (its separating comma), every array element
  (the `__elem__ = …` carrier) and every node with a packaged trailing blank run. Both web hosts
  now seed from `block_text`, the same producer the TUI uses; verified over the real wasm
  boundary. (Introduced in this same unreleased series, so no shipped release is affected.)
- TOML: a value `Replace` whose fragment spells a **different key** is now rejected as
  `Fragment` instead of applying the value and dropping the key silently — renaming a key in
  the `$EDITOR`/pop-up buffer used to report success and change nothing. The synthetic carriers
  `__k__`/`__elem__` mean "no key" and stay exempt, so the inline editor, kind switch and
  array-element edits are unaffected. `docs/reference/MUTATIONS.md`'s `Replace` row promised
  this rejection already; it now describes what the code does, including the surplus-*nodes*
  gap that remains until the block editor moves to the whole-file route
  ([spec](docs/spec/2026-09-15-block-edit-whole-file-reparse-design.md)).

**Docs — second documentation audit: `CLAUDE.md` splits, 42 drift defects fixed**

Record: [`docs/audit/2026-09-15-documentation-audit.md`](docs/audit/2026-09-15-documentation-audit.md).
Every living document re-verified against the code at `b5b9647`; frozen records untouched
except where a `Status:` line broke the value set.

- **New [`docs/reference/ARCHITECTURE.md`](docs/reference/ARCHITECTURE.md)** owns the workspace
  shape, the full module map, the host file-I/O boundary, per-host build/packaging, and the
  `taplo` dependency surface. `CLAUDE.md` drops to conduct only — commands, release process,
  doc-pointer table, the two review-gate invariants, the `taplo` decision plus its `cargo audit`
  trigger, terminology — 587 → 121 lines (`wens-dev-principles docs 3`). The map was corrected
  while moving: `benches/perf.rs`, `confy-tauri`'s `build.rs`/`msix/`/`play/`, the picker
  crate's `src/*.rs`/`guest-js/`/`permissions/`, `web/privacy.html`/`manifest.webmanifest`/
  `icons/`/`run-tests.mjs` + 37 spec suites, and `editors/vscode/src/`/`test-integration/`.
- **Corrected against the code**: the `a`-add inserts as the **first** child, not the last
  (`TUI.md`, `BEHAVIOR_MATRIX.md` Table B); `[G]` root and the `format_kind_tag` name are gone
  from `glossary.md` and its padding rule now distinguishes inside- from outside-bracket
  padding; Escape peels clipboard before selection (`HOST_PARITY.md`); fuzzy marks are
  yellow+bold+underline, not reverse/bold; desktop `Space` is `native:toggle-branches`;
  `BEHAVIOR_MATRIX.md §7` gains the `value_kind` and `trailing_blank_anchor` facets and a
  `[T/D]` offers both flow and block; `WEBUI.md`'s breadcrumb/Save-split-button/file-input/
  `.toast`/`add-picker-items` claims and its dead `PORTING.md` citation; `README.md` no longer
  ties `C` to a Root node; `VSCODE.md` now records the ADR 0014 R10 suppression.
- **Counts re-derived**: `functional_smoke.mjs` 129 → **176**, `confy.ts` 16 → **18** methods,
  `SessionSnapshot` 21 → **22** fields (`doc_revision`), `MESSAGES.md` 68/45 → **70/47** keys,
  `en.json` 102 → **105** `core.*`. Unchanged and re-confirmed: taplo 49/28/2, 19 core suites,
  1,241-LOC vendoring scope, 427-key catalog parity.
- **Fixed a drifted invariant test**: `severity_of_covers_the_full_catalog_table` listed 45
  `core.*` keys and asserted 45 while `severity_of` had 47 — ADR 0014's
  `core.document.apply-failed`/`core.document.edit-locked` were never added, so the stale doc
  and the stale test agreed with each other.
- **Reference stops carrying history and roadmap**: `ROW_STATE_MODEL.md`'s implementation-history
  and resolved-bug sections become one *Boundaries* section; `WEBUI.md`'s structured-diff
  roadmap moves to the living backlog's *Watching*.
- **Records**: three unindexed 2026-09-11 documents added, two stale *In progress* rows moved to
  *Landed*, four status lines normalized to the legal value set, ADR 0013/0014 marked
  `Implemented (2026-09-14)`, `HOST_PARITY.md` renumbered §1–§5 (ADR 0013 had deleted §2), and
  one new backlog row for four groups of orphaned i18n keys.

**Changed — Raw write mode's crumbs-row band: two symmetric controls**

- The band is now **Edit → Apply** plus **Cancel** (was View⇄Edit toggle / Apply / Cancel).
  The first control is an **action** whose label is the effect of pressing it: `Edit` in Raw
  view, accent-filled as the one thing to do there; `Apply` in Raw write, with the fill
  dropped so it sits level with `Cancel`.
- **Both controls now leave write mode**: Apply commits the buffer *and* exits, Cancel
  discards back to the last applied text *and* exits (no confirm — the press is the answer).
  A *failed* Apply is the one case that stays put, keeping the buffer and its notice.
- Neither is gated on a dirty buffer any more: they are the mode's two exits, so a clean
  buffer still has a way out. `#btnRawApply` and the orphaned `web.raw.controls.view` key are
  removed; the pane's `input` listener goes with them.
- `⌘↩` keeps its **apply-and-stay** meaning (the checkpoint you press while still typing);
  `Esc` keeps R7's dirty-buffer confirm.
- Docs: `CHROME.md` (band section + table), `WEBUI.md`, `KEYMAP.md`, `HOST_PARITY.md`, and
  `docs/spec/2026-09-11-raw-write-mode-design.md` R4/R13 amended.

### Update - 2026-09-14 (22)

**Changed — root-hidden alignment S6 (T12): the vocabulary and the records**

Closes `docs/plan/2026-09-11-root-hidden-alignment.md` (T0–T12) and with it ADR 0013.

- **D16 — the glossary's `Root` entry states the contract**: a model node at the empty path
  (the parent of every whole-document `Target`) that is **never a view row** — no host draws
  it, no cursor sits on it, it is never a row-operation operand, and its two paste slots are
  the *document-edge* slots. "root row" joins its _Avoid_ list.
- **`TypeToken::Root` retired** (D11's second half): `classify()` now returns
  `Option<TypeToken>`, `None` for the Root — the one kind with no facet — and the TUI's
  `type_tag` returns an empty tag for it. The `[G] root/file node` row is deleted from the
  Help KIND legend in all three formats × both catalogs, with the remaining `containers.N`
  keys renumbered (the legend reader walks `N` from 1 until a key is missing, so a gap would
  truncate the list). Verified on the real binary: the legend still ends at `[T/E]`.
- **`ROW_STATE_MODEL.md` §6a rewritten** around the document-edge slots (including the
  `compute_rows` staleness caveat), and its §-index line updated.
- **Records flipped**: the three Open rows in `docs/plan/2026-09-09-open-follow-ups.md` move to
  Done, the retrospective is `Resolved (2026-09-14)` with P1/P2/P4 each mapped to the decision
  that closed it, the design record is `Shipped`, and ADR 0013's status names the commits.

### Update - 2026-09-14 (21)

**Fixed — a document-edge paste slot was wiped the instant it was set**

`compute_rows` drops a paste slot whose path is no longer a visible row (stale after a
structural change) — and the two document-edge slots (`After([])`/`Into([])`) name the Root,
which has no row on purpose, so `Home`/`End` set one and the very next rebuild threw it away,
silently falling back to `After(cursor)`. Every unit test passed; only stepping the slots in a
real browser surfaced it. The check now exempts the empty path.

**Removed — root-hidden alignment S5 (T11): the web stand-ins are gone**

With core no longer emitting a Root row (and emitting the paste slots in screen order), the
host-side corrections that hid it have nothing left to correct:

- **`drawnCursorFallback` and `overshotUndrawnRootSlot` deleted** (`web/path-utils.ts`) along
  with their call sites in desktop `navSelect` and touch `touchNavSelect`, `select.ts`'s
  root filter, `host-io.ts`'s faked `SetCursor: []` before `OpenConvert` (dead since D10), and
  the `r.path.length === 0` row skips in `render.ts` / `touch/render.ts`.
- **`slot-line.ts`'s `rootSlotLine` → `documentEdgeLine`** (`RootSlotLine` →
  `DocumentEdgeLine`): the two slots it draws are a document-edge rule on every host now, not a
  web-only stand-in.
- **D12 — a zero-row document draws a `.tree-empty` hint** on both web hosts (new
  `web.tree.empty`: "Empty document — use ＋ to add the first node").
- **`HOST_PARITY.md` §2 ("the undrawn root row") is deleted** — the divergence is gone.
- **Verified in a real headless Chromium** against the built `web/dist`: `Home` puts the
  insertion line at the first row's top edge (6px) and `End` at the last row's bottom edge
  (306px), and deleting every top-level node shows the empty-document hint. Plus `npm run
  typecheck`, `npm test`, `functional_smoke.mjs`, `cargo test --workspace`, `clippy
  --workspace --all-targets -D warnings`, and a re-run of the TUI tmux check.

### Update - 2026-09-14 (20)

**Changed — root-hidden alignment S4 (T10): the TUI's replacements for the Root row**

- **D6 — the two document-edge paste slots now have cues.** `After([])` (paste at the document
  top, the slot `Home` reaches) draws a green insertion line above the first row; `Into([])`
  (append at the document end) draws one below the last. They hang off the viewport edges rather
  than a row, since neither has one any more — this is what the Root row's own `Into`/`After`
  cues used to provide. Verified on the real binary.
- **D12 — a document with no top-level Node draws a hint line** (new `tui.tree.empty` in both
  catalogs: "(empty document) — press `a` to add the first node") instead of an empty pane.
- **Removed the now-dead root special cases** in `confy-core`, since no host can put the cursor
  on the Root: `toggle_expand`'s early-return, `collapse_level`'s first guard, both
  `extend_select_up`/`down` anchor guards and their `rows[idx - 1]` root test, and
  `edit_target_kind`'s. `collapse_level`'s `target.is_empty()` check stays — a top-level Node's
  parent *is* the Root — as does `toggle_select`'s, for the empty-document cursor.

### Update - 2026-09-14 (19)

**Changed — root-hidden alignment S2 (T5–T9): the Root stops being a row**

The direction-committing half of `docs/plan/2026-09-11-root-hidden-alignment.md` (ADR 0013
D1/D4/D5/D10/D11/D12/D13 + D15). The TUI's `document file` row is **gone** — every host now
agrees, which is what the whole project was for.

- **D1/D4 — the Root is dropped from both flattens and depth is rebased**, so a top-level Node
  is depth 0. The Root keeps its place in the *model* (`node_at(&[])`,
  `Target { parent: [] }`, every whole-document mutation); it is only never a view row.
  `view_row_at([])` returns `None`, and both web renderers lost their
  `Math.max(0, r.depth - 1)` correction.
- **D15 — the TUI title bar reads the new `Session::root_key`** instead of `app.rows.first()`,
  which would otherwise have blanked the moment the row disappeared.
- **D5 — `paste_slots()` is emitted in screen order:** the document-top slot (`After([])`,
  root index 0) first, each row's `Into`/`After` next, the document-end slot (`Into([])`,
  append at `children.len()`) last. They used to fall out of row 0 in the opposite order, so
  paste-mode `Home` meant "document end". `slot_target` resolves both edge slots from the tree
  (looking for their row returned `None`, i.e. a silently no-op move), and stepping onto them
  no longer drags the cursor to the empty path.
- **D10 — Convert (`C`) is document-scoped** and works from any row; its root-cursor
  precondition became unsatisfiable, and `core.convert.root-only` is retired from both
  catalogs (the web host's faked `SetCursor: []` is deleted in the web slice).
- **D11 — the `[G] root` type-filter facet is gone** from all three format layouts.
- **D12/D13 —** with no cursor row (a zero-row document) the add target is now
  `Target { parent: [], index: children.len() }`, and `RevealPath([])` (breadcrumb `⌂`)
  retargets to the first row in core.
- **D7's guard gained a second case:** `selected_paths()` returns nothing for a Root cursor
  now, so "no operand while the cursor is on the Root" gets the same refusal.
- **Verified on the real `confy` binary** (tmux, `--lang en`): the title bar still names the
  file, the first row is a top-level Node at the leftmost indent, and `C` opens the Convert
  popup from an ordinary node row. Plus `cargo test --workspace`, `clippy --workspace
  --all-targets -D warnings`, `functional_smoke.mjs`, `npm run typecheck`, `npm test`.
- **Test churn:** ~90 assertions across `session_headless.rs`, `modal_lock.rs`,
  `schema_headless.rs`, `tui/tests.rs`, `tui/ui.rs` and `functional_smoke.mjs` lost their
  row-0-is-the-Root assumption (row indices, `visible_keys()` prefixes, the `KEY_X` render
  column, and the two `△` summary counts). `convert_flow_opens_only_on_root` became
  `convert_flow_opens_from_any_row`.

### Update - 2026-09-14 (18)

**Fixed — root-hidden alignment S1 (T2–T4): the Root stops behaving like a row**

Direction-neutral core mechanism from `docs/plan/2026-09-11-root-hidden-alignment.md`
(ADR 0013 D7/D3/D2). The TUI still *draws* the Root row — that is S2/S4 — so it remains the
parity oracle; what changes is that no cursor is seeded there, nothing selects it, no row
operation accepts it, and it can no longer be collapsed.

- **D7 — the Root is never an operand of a row operation.** New `guard_root_operand`, called by
  `cut_selected`/`copy_selected`, `delete_selected` and `remark`: the refusal is the new
  `core.selection.root-excluded` (`Warn`, both catalogs). Previously `Cut` on the (then default)
  Root cursor **succeeded**, arming the clipboard with a node that can never paste while the
  armed clipboard held the modal lock (ADR 0005 §5) with `Esc` as its only exit — measured as
  E4 this morning. All four selection entry points now drop `[]` as well (`ToggleSelect`,
  `SetSelection`, and both ⇧-range rounds, which refuse to anchor or extend onto it), and the
  **Action menu dims every node-scoped item** on the Root, leaving only document-scoped
  `Edit whole file`.
- **D3 — cursor seeding moved into core.** `Session::from_tree` seats the cursor on the first
  top-level Node instead of on the Root (an empty document keeps the empty path). This is the
  core answer to what the web hosts patched per keystroke with `drawnCursorFallback` — that
  deletion is S5, not this slice.
- **D2 — the Root is unconditionally expanded.** `visible_nodes`/`visible_rows` flatten with
  `p.is_empty() || expanded.contains(p)`, `collapse_all` no longer re-inserts `[]`, and
  `ToggleExpand` on the Root is inert. `Space` there used to return one core row, which both
  web renderers draw as **zero** — a tree with no rows and no cursor (E5). Two readers the
  design record did not name had to follow: `is_path_visible` (its ancestor-prefix walk
  required `[]` in the set, so every single-row lookup — `cursor_row`, hence the whole Action
  menu — went blind) and `is_expanded` (the TUI drew a collapsed `▸` caret on a file whose
  children were all on screen).
- **Verified on the real `confy` binary** (tmux, `--lang en`): the caret reads `▾`, `Up` onto
  the Root followed by `Space` leaves all rows in place, `r` there reports the new refusal, and
  the Action menu shows `Edit whole file` as its only live item. Plus `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -D warnings`, `functional_smoke.mjs`,
  `npm run typecheck`, `npm test`.
- **Test churn:** 5 new headless tests pin D7/D3/D2 and the menu dimming; ~28 existing
  `session_headless.rs` tests plus 3 TUI tests lost the leading `CursorDown` that used to step
  off the Root row (`root_node_can_collapse_and_expand` became
  `root_node_cannot_be_collapsed`), and the whole-document buffer test now opens through
  `BeginEditDocument` rather than by parking the cursor on the Root.

### Update - 2026-09-14 (17)

**Docs — root-hidden alignment: SD + S0 (documentation and evidence only, no product code)**

- `docs/spec/2026-09-11-root-hidden-alignment-design.md` promoted `Draft` → **`Approved`**, and
  `docs/plan/2026-09-11-root-hidden-alignment.md` added as its task-by-task plan (T0–T12 against
  slices SD–S6, each with the record's own acceptance). ADR 0013 already existed and is listed.
- **S0 baseline evidence E1–E6** recorded in the design record, measured on main at `21cbea9`
  (throwaway core test + the real wasm channel + the real `confy` binary under tmux):
  - **E2** the TUI does still draw the Root (`▾ s0.toml [G]` as row 0) while the title bar names
    the same file — so D15 is the precondition for removing the row, as the record says.
  - **E3** settles the backlog's *Watching* row: `Into([])` is paste-slot **index 0** but
    resolves to `Target { parent: [], index: children.len() }` — the document's **end** — while
    `After([])` resolves to index 0, the top. The branch's "prepend" reading does not reproduce.
    P4 confirmed; D5 is the fix.
  - **E4** P1 reproduced: `CutSelected` with the seeded `[]` cursor arms the clipboard
    (`clipboard_count = 1`), and `OpenActionMenu` right afterwards returns **zero items** — the
    modal lock with `Esc` as its only exit.
  - **E5** P2 reproduced: `ToggleExpand` on `[]` returns 1 core row, of which the web renderers
    draw 0 — a blank tree with no cursor.
  - **E6** P3 and **slice S3 are already done**, shipped by the sibling Raw-write record
    (ADR 0014); D9's placement verified over the wasm channel — `Edit whole file` sits
    immediately above `Delete`, always enabled. Remaining work: S1, S2, S4, S5, S6.

### Update - 2026-09-14 (16)

**Changed — `core.action.edit` shortened to just "Edit"**

- `在編輯器中編輯` → **`編輯`** (`Edit in editor` → `Edit`). Which intent it dispatches
  (`BeginEditExternal`, the TUI's `E`) is unchanged; only the label is shorter. Verified over
  the rebuilt wasm channel — the full menu now reads `編輯 / 新增子節點 / 新增同層節點 / 複製 /
  剪下 / 切換註解 / 詳細資訊 / 編輯全檔 / 刪除`. `TUI.md`'s two item inventories follow.

**Fixed — the header Tree/Raw button now reaches Tree in one press from write mode**

- Pressing the header toggle while in Raw **write** mode exited to Raw *view*, so getting back
  to the tree took two presses even though the button read `Tree`. `exitRawWrite` now takes its
  landing state (`exitRawWrite(to: RawState = "view")`): `Esc` and the band's own toggle still
  land on Raw view, the header button passes `"off"`. The R7 dirty-buffer confirm gate is
  shared by both paths — the destination is the only difference.
- Verified in a real browser: in write mode (`body` = `raw-view raw-write`, button reads
  `Tree`) → one press → `body` = `""`, button reads `Raw`, 32 tree rows drawn. Dirty buffer:
  declining the confirm stays in write mode, accepting it lands on the tree directly.
  Two new `raw-write.spec.mjs` checks pin both (`exitRawWrite("off")` calls `setRawState`
  exactly once with `"off"`, and takes the same gate).

### Update - 2026-09-14 (15)

**Changed — the Raw toggle reads the state you are IN, and the Action item is shorter**

- `core.action.edit-document` shortened: `以文字編輯整份檔案` → **`編輯全檔`** (`Edit whole file
  as text` → `Edit whole file`). Verified through the wasm command channel on both catalogs
  (`OpenActionMenu` item labels: `["Edit whole file"]` / `["編輯全檔"]`).
- The band's toggle now labels the **current** state instead of the destination — `檢視` while
  viewing, `編輯` while editing — and its `.active` gets the header Tree/Raw tab's solid accent
  fill, so "blue = you are editing" reads the same in both places. The `#rawControls` prefix on
  that CSS rule is load-bearing: `.seg-btn.active`'s faint tint is declared later in
  `style.css` and would otherwise win on equal specificity (measured: the first attempt left
  the button at `oklch(0.34 0.06 250)` instead of the tab's `oklch(0.72 0.14 250)`).
- Verified in a real browser: view → label `View`, transparent; press → label `Edit`,
  background `oklch(0.72 0.14 250)` with `color: var(--bg)`, byte-identical to the header
  tab's active fill; press again → back to `View`, no fill. zh-TW: `檢視`/`編輯`/`套用`/`取消`.
- Docs: `CHROME.md` band table + toggle rule, `WEBUI.md`, `KEYMAP.md`, `HOST_PARITY.md`, and
  the raw-write design record's R13 amendment; `raw-jump.spec.mjs`'s two label checks flipped.

### Update - 2026-09-14 (14)

**Changed — the Raw band is a toggle plus an Apply/Cancel pair (web desktop)**

- The `檢視 | 編輯` pair collapses into **one toggle** (`#btnRawEdit`): its label/title is the
  state a press switches to (Edit while viewing, View while editing), with `aria-pressed` and
  `.active` carrying the current state — the header's Tree/Raw button already worked this way.
  Pressing it while editing takes the same confirm-gated exit `Esc` does.
- A **Cancel** control (`#btnRawCancel`, `web.raw.controls.cancel` in both catalogs) joins Apply
  under the **identical** enable rule (write mode + a dirty buffer), because they are the two
  halves of one decision: commit the changes or discard them. Cancel re-seeds the pane from the
  last-applied text with the scroll position preserved and **stays in write mode**; leaving the
  mode remains the toggle's and `Esc`'s job. Verified in a real browser: dirty → both enable,
  Cancel → text back to baseline, `scrollTop` unchanged, both back to disabled, still in write.
- The band is still three same-width (62px measured) always-present controls; the overflow-menu
  exclusion lists follow the new ids.

**Fixed — a breadcrumb jump could not target a comment node**

- Clicking a comment row in the breadcrumb mini-tree moved the tree cursor (the crumb bar read
  `⌂›#[0]›`) but did **nothing** in the Raw pane: no selection, no scroll, no message. Root
  cause was one layer — `Session::outline()` omits `NodeKind::Comment` on purpose (an editor
  Outline listing comments as symbols is noise), and `jumpSelectRawSpan` was the only consumer
  using it to look up a `text_range`, so `findOutlineByPath` returned `undefined` and the
  function returned silently. The comment's real span was in the projection the whole time.
- Fix: a narrow core query `Session::span_of(path) -> Option<(u32, u32)>` (comments included,
  via `node_at`), exported over wasm as `span_of` and wrapped as `Session.spanOf`. `outline()`
  is untouched, so the VS Code symbol tree does not grow comment entries. The jump also no
  longer serializes the whole outline across the wasm boundary on every pick.
- `findOutlineByPath` was orphaned by the fix and is removed (`web/text-offset.ts` keeps
  `byteToCodeUnit`).
- Verified: new `session_headless.rs` tests (`span_of_resolves_comments_and_live_nodes_alike`,
  `span_of_resolves_yaml_and_json_comments` — TOML top-level + nested, YAML, JSONC, plus the
  assertion that `outline()` still omits the comment, so the test fails loudly if that premise
  changes); real browser on a 156-line fixture: top-level comment selects
  `# long fixture for raw-pane scroll measurement`, and `[{"Key":"section_25"},{"Index":0}]`
  selects `# comment inside section 25` with `scrollTop` 0 → 2396.
- Verification: `cargo test -p confy-core` green; `wasm-pack build --target web` +
  `node functional_smoke.mjs` green; `npm run typecheck` clean; `npm test` green
  (`raw-jump.spec.mjs` rewritten for `spanOf`, the toggle, Cancel and `revertRawEdit`).
- Docs: `CHROME.md` band table/rules, `WEBUI.md`, `KEYMAP.md` (no key discards without exiting
  — that is Cancel only), `HOST_PARITY.md`, `CLAUDE.md`'s `text-offset.ts` entry, and the
  raw-write design record's R13/R14 amended again in place.

### Update - 2026-09-14 (13)

**Fixed / Changed — the Raw pane is one element (web desktop)**

- **View ↔ edit is no longer a swap.** The Raw pane was a read-only `<pre id="raw">` plus a
  separate `<textarea id="rawEdit">`; it is now **one `<textarea>`**, `readonly` in Raw view and
  writable in Raw write, absolutely positioned over the whole `.tree-wrap` box and owning the
  only scrollbar (`body.raw-view .tree-wrap` drops the wrap's padding/overflow). Measured in a
  real browser on a 306-line document, the old design: the `<pre>` never scrolled (the wrap did),
  so write mode got a *second*, 86px shorter scroll container (539px vs 625px — the wrap's 80px
  FAB padding reserve), the "copy scrollTop across the swap" rule copied a value that was always
  0 (so entering edit mode jumped back to the file head), and the text shifted ~10px. After: both
  states are the same box (`rect [130,0,1280,634]`), a scroll of 2000px survives the switch, and
  `.tree-wrap` no longer scrolls at all in Raw.
- **The crumbs-row band is static.** `檢視 | 編輯 | 套用`, three controls at the same width (62px
  measured), always present while Raw is active; the inapplicable one is `disabled`, not hidden,
  so nothing pops in or out under the pointer. **Apply is greyed** in Raw view and in write mode
  until the document buffer is actually dirty (it re-disables after a successful Apply). VS
  Code's R10 suppression is now a `disabled` Edit control rather than a hidden one.
- **The 存檔 button is removed** — it duplicated the header's Save and was never a requirement.
  `⌘S` keeps its apply-if-dirty-then-save meaning (`KEYMAP.md`); `web.raw.controls.save` is
  retired from both catalogs.
- **A breadcrumb jump now scrolls to the span.** `jumpSelectRawSpan` selected the right text in
  both Raw states but never scrolled to it: view mode nudged `#raw.scrollTop` on an element that
  was not a scroll container (measured: selection 4,929px below a 634px viewport, `scrollTop`
  stuck at 0), and `setSelectionRange` does not scroll either. One code path now — select, then
  `scrollRawToOffset` puts the span's line a third of the pane down (verified: `scrollTop` 0 →
  5,565 with `[target]\nneedle = "找到我"` selected).
- A **readonly** textarea no longer swallows shortcuts: `document.body`'s key delegation skips
  only a *writable* one, so the view-mode pane (which the jump focuses to show its selection)
  stays transparent to the global keymap.
- Verification: `npm run typecheck` clean; `npm test` green (`raw-write.spec.mjs` rewritten for
  the single-element pane incl. a committed-Apply scroll check, `raw-jump.spec.mjs` rewritten for
  the unified jump path + the static band, both suites pass); real-browser pass on a 306-line
  TOML covering every number quoted above, plus `⌘↩` Apply and `Esc` exit from the keyboard.
- Docs: `WEBUI.md` Tree | Raw view | Raw write rewritten; `CHROME.md`'s band section (4 → 3
  controls, disabled-not-hidden); `KEYMAP.md`'s `⌘S` row; `HOST_PARITY.md` §4/§6 rows;
  `CLAUDE.md` module map; `docs/spec/2026-09-11-raw-write-mode-design.md` R13/R14 and the
  *Switching* table amended in place with the measurements and the reason the shipped design
  failed. `docs/plan/2026-09-09-open-follow-ups.md` files a new **Open** row: ADR 0013 (no host
  draws the Root row) is accepted but unimplemented — the TUI still shows the `document file`
  row and the web hosts still carry the stand-ins.

### Update - 2026-09-14 (12)

**Documentation**

- **Raw write mode ships** (`docs/plan/2026-09-14-raw-write-mode.md` T11, RS4 — the plan's
  final task; both the plan and `docs/spec/2026-09-11-raw-write-mode-design.md` flip Status to
  `Shipped (2026-09-14)`; ADR 0014 flips to `implemented (2026-09-14)`): documentation caught up
  to the T1–T10 implementation, verified against the actual code rather than restated from the
  design record. `WEBUI.md`'s Tree | Raw view section is rewritten as **Tree | Raw view | Raw
  write** (the `rawState` tri-state, the `#rawEdit` textarea, `⌘↩`/`⌘S`/`Esc`, the crumbs-row
  control band, the breadcrumb jump, VS Code suppression) and its touch external-edit-sheet
  paragraph gains the R18/R19 whole-file routing rule. `CHROME.md` gains a new "crumbs-row Raw
  control band" section documenting the four `#rawControls` buttons and their overflow-menu
  exclusion rule. `KEYMAP.md` gains a "Raw write mode (desktop web only)" section for
  `⌘↩`/`⌘S`/`Esc`, outside the machine-checked normal-mode table since `rawState` is host-local,
  not a core `Mode`. `HOST_PARITY.md` gains a §4 row for the whole-document edit's per-host
  entry point (Raw write vs. touch's sheet), amends the breadcrumb row for R12's both-Raw-states
  visibility, and gains a §6 row for VS Code's R10 suppression. `CLAUDE.md`'s module map updates
  `ui.ts`'s Tree/Raw description (was "read-only"), adds the new `web/text-offset.ts`, and notes
  `apply_document_text` on `inline_edit.rs`. `docs/plan/2026-09-09-open-follow-ups.md` moves its
  **P3** row ("the web has no entry point for whole-document operations") to *Done*, and files a
  new **Q4** row (caret → cursor, the breadcrumb jump's un-implemented inverse — settled out of
  scope at ship time, `docs/spec/2026-09-11-raw-write-mode-design.md`'s own Q4). `MESSAGES.md`
  needed no changes: `core.document.apply-failed`/`core.document.edit-locked` were already
  covered by the existing severity table (still 45 `core.*` keys, `cargo test -p confy-core`'s
  `severity_of_covers_the_full_catalog_table` unchanged) and `web.raw.*` status strings already
  fall under the documented "chrome strings, not notices" bucket.
- `rg` confirms no reference doc still describes Raw as read-only-only outside the frozen
  `docs/reference/changelog/v0.x.md` archive (correctly historical).
- Verification: `cargo test -p confy-core` green; `npm run typecheck` clean; `npm test` (999
  checks, unchanged from T10 — docs-only task); `node functional_smoke.mjs` (13 checks) green.

### Update - 2026-09-14 (11)

**Added**

- **VS Code suppresses whole-document editing** (`docs/plan/2026-09-14-raw-write-mode.md` T10,
  RS4; R10): the workbench's own `TextDocument` is already the single source of truth for the
  open file's content/dirty/undo/save (`docs/reference/VSCODE.md`) — a second editable copy in
  the webview would be two owners of one document (ADR 0007), so under `VSHOST` the Raw pane's
  Edit control (the crumbs-row band's `btnRawEdit`) and the Action menu's *Edit whole file as
  text* item are both suppressed, on every reachable path: rendering (the button/menu item is
  never drawn, so it can't be clicked, and its `TOOLBAR_ENTRIES` entry is excluded from the "⋯
  More" overflow menu too), the click handlers, and — since core's own Action-menu cursor
  stepping doesn't know about the host and can still land keyboard `Enter` on the hidden item —
  the `onKey` commit dispatch itself, which now checks the cursor's item id before forwarding
  `ActionMenuCommit` to core. Raw *view* (read-only) is unaffected; only the entry point into
  write mode is gone. `web/raw-jump.spec.mjs` gains 2 checks for `renderRawControls`'s
  VSHOST-hidden case; `npm run typecheck` and `node build.mjs` both stay clean, confirming the
  VS Code extension (which embeds `web/dist` verbatim) still builds.

### Update - 2026-09-14 (10)

**Added**

- **Touch keeps its existing external-edit sheet for whole-file editing**
  (`docs/plan/2026-09-14-raw-write-mode.md` T9, RS3; R18/R19): unlike desktop's new Raw write
  mode (T7/T8), touch has no breadcrumb and its Raw pane is a read-only `<pre>` with no room
  for a control band, so the Action menu's *Edit whole file as text* item (already reachable —
  T4's `BeginEditDocument` surfaces it identically on both hosts) keeps routing to the same
  bottom sheet a per-node multi-line value/comment edit already uses (`web/touch/app.ts`'s
  `openExternalEdit`) — no new surface, no write mode. The one behavior change: a whole-file
  Apply (`path.length === 0`) that fails to commit now keeps the sheet open with the buffer
  text intact instead of closing unconditionally, detected the same way the desktop Raw pane
  does — `doc_revision` not moving — since closing on failure would otherwise silently discard
  the entire file's unsaved edit. A per-node value/comment Apply (empty path never applies
  there) keeps closing unconditionally, unchanged.
- New `web/touch-ext-apply.spec.mjs` (15 checks): extracts `openExternalEdit` verbatim from
  `touch/app.ts` and exercises it against a stubbed sheet/session — routing for both an empty
  and a per-node path, a failed whole-file Apply leaving the sheet open, a succeeding
  whole-file Apply closing it, and the unconditional-close control cases (per-node value,
  comment). Manually verified in the real touch host (forced via `?ui=touch`, since headless
  Chromium's fine pointer otherwise bounces `touch.html` back to the desktop entry): the Action
  menu's *Edit whole file as text* opens the sheet with the whole document, an unparsable Apply
  keeps it open with the invalid text and shows the failure notice inline, and Cancel discards
  it leaving the document untouched.

### Update - 2026-09-14 (9)

**Added**

- **The Raw pane's crumbs-row control band and breadcrumb jump**
  (`docs/plan/2026-09-14-raw-write-mode.md` T8, RS2c; R12–R17/R29): the breadcrumb bar now
  stays visible and live in **both** Raw states (R12 — `crumbsEl.classList.toggle("hidden", …)`
  removed; it is driven by the same `snap.cursor`/`children(path)` it already used in Tree).
  A new right-end control band (`#rawControls`, R13) renders only while Raw is active: a
  `檢視|編輯` (View|Edit) segmented pair (both Raw states — `Edit` dispatches
  `BeginEditDocument`, `View` calls the confirm-gated `exitRawWrite()`) plus **Apply**/**Save**
  buttons (write only, calling the same `applyRawEdit`/`rawEditSave` the keyboard shortcuts
  use). All four are registered in `TOOLBAR_ENTRIES` (`toolbar-fold.ts`'s overflow-menu
  mechanism), excluded from the "⋯ More" menu while their band/buttons are hidden for a
  business reason rather than a narrow width. The header's Tree/Raw toggle
  (`btnViewToggle`) now also routes through `exitRawWrite()`'s confirm-gate when leaving write
  mode, instead of bypassing it straight to `"off"`.
- **The breadcrumb jump additionally selects the node's source span in the Raw pane** (R14):
  clicking a segment or mini-tree row still dispatches `RevealPath`, and now also resolves the
  path's `text_range` from a new `session.outline()` wrapper (`web/confy.ts`, mirroring
  `crates/confy-ffi`'s existing `outline()` export) and selects it — a DOM `Range`/`Selection`
  in Raw **view**, `<textarea>.setSelectionRange` in Raw **write** — scrolling it into view.
  One-way only (R16): moving the caret in write mode never moves the tree cursor back. New
  `web/text-offset.ts`: `byteToCodeUnit` (R15 — `outline()`'s `text_range` is UTF-8 bytes, DOM
  selection APIs index UTF-16 code units; converts one offset, quantified against T2's
  CJK+emoji fixture, `drift === 8`) and `findOutlineByPath`. The selected span is the node's
  **whole member, key included** (R29/F5 narrowed this: `text_range` already spans
  `target = "needle"`, not just the value; no value-only range exists in the wire contract).
  Gated on a clean write buffer (R17): while the textarea differs from the last-applied
  baseline, a pick still moves the tree cursor but leaves the caret alone and reports
  `web.raw.jump-needs-apply` — `text_range`s come from the last commit, so a dirty buffer's
  offsets may point into text that is no longer there.
- New `web/raw-jump.spec.mjs` (26 checks): unit-tests `byteToCodeUnit`/`findOutlineByPath`
  directly (both pure, DOM-free) and extracts `jumpSelectRawSpan`/`renderRawControls`
  verbatim from `ui.ts`; manually verified in a real browser (breadcrumb pick selecting
  `[servers]` end-to-end in Raw view, the same pick selecting `[about]`'s whole member in Raw
  write, and a dirty-buffer pick moving the cursor but leaving the caret and reporting the
  status line).

### Update - 2026-09-14 (8)

**Added**

- **The Raw pane gains a write mode** (`docs/plan/2026-09-14-raw-write-mode.md` T7, RS2b;
  R1/R2/R4–R8): a pending document-level external edit (empty path — the Action menu's *Edit
  whole file*) now routes to a new `#rawEdit` textarea filling the Raw pane, in place of the
  per-node `#ext-modal` popup. `⌘/Ctrl+Enter` Applies (`ApplyReplace { path: [], text }`) and
  stays in write mode regardless of outcome (R4); commit/failure is read off `doc_revision`
  (R5) — never `history_len` or the notice, since a rejected buffer can surface at any
  severity. `⌘/Ctrl+S` applies only if the buffer is dirty, then saves; it never saves after a
  failed Apply (R6). `Esc` exits to Raw view, peeling core's pending edit via `Escape`, gated
  on a confirm (`web.raw.discard-confirm`) only when the buffer differs from the last-applied
  baseline (R7). `render()` never writes to `#rawEdit` while it is open (R8) — entry is
  guarded so the textarea is seeded exactly once per pending edit, not on every unrelated
  re-render. `#raw.raw-view`/`#rawEdit` now share one CSS metrics selector list, and scroll
  position is copied across the swap in both directions (no scroll jump), with the caret
  placed at offset 0 on entry. Two new cues: a footer hint line
  (`web.raw.write-hint`, shown only in write mode) and a `body.raw-write` accent
  border/tint/focus-ring on `#rawEdit`. New `web/raw-write.spec.mjs` (34 checks) extracts and
  exercises the real `enterRawWrite`/`maybeEnterRawWrite`/`applyRawEdit`/`rawEditSave`/
  `exitRawWrite`/`renderRawOrTree` bodies; manually verified in a real browser (entry, typing,
  Apply, the confirm-gated Escape both ways, scroll continuity). The crumbs-row control band
  (R13), the breadcrumb jump (R14–R17), and VS Code suppression (R10) are follow-up tasks
  (T8/T10) — until then the existing view-toggle button and breadcrumb-hiding behavior are
  unchanged.

### Update - 2026-09-14 (7)

**Changed**

- **`rawView: boolean` → `rawState: "off" | "view" | "write"`** in `web/ui.ts` and
  `web/touch/app.ts` (R27), a pure refactor with no behavior change — `"write"` is
  unreachable until T7 lands the textarea. `setRawView(raw)` is now `setRawState(next)`;
  `body.raw-write` is derived alongside `body.raw-view` in the same `setRawState`, so T7's
  diff only has to reach the state, not the class-toggle wiring. Every boolean call site
  (`resolveKeyIntent`, `scrollFocusIntoView`, the paste/drop cue guards) now reads
  `rawState !== "off"` inline; touch keeps the tri-state shape for parity even though it
  never reaches `"write"` (R18/R19: touch text edits always route to the sheet). Updated the
  matching extraction shims in `armed-paste.spec.mjs`, `paste-hover.spec.mjs`,
  `modal-lock.spec.mjs`, `touch-key-scroll.spec.mjs`, and `touch-modal-lock.spec.mjs` — each
  splices real source functions closing over the module-level flag by name, so the shim's
  stand-in variable had to be renamed too. `npm test` (915 checks) and `npm run typecheck`
  unchanged-green.

### Update - 2026-09-14 (6)

**Added**

- **The empty-path lock (R21/R24).** While a whole-document pending edit
  (`Intent::BeginEditDocument`) is open, every mutating intent and `Undo`/`Redo`
  are refused with a new `core.document.edit-locked` notice
  (`Severity::Warn`, mirrors `core.clipboard.action-locked`'s shape) and
  change nothing. The pending buffer stands for the entire document text, so
  a core mutation underneath it would be silently discarded the moment the
  host later applies that stale buffer, and `Undo`/`Redo` swap the whole
  document text out from under it — exactly the overwrite the lock exists to
  prevent. A pending edit at a **non**-empty path (a per-node external edit)
  is unaffected — navigation, delete, undo and redo all keep working.
  Implemented as `Session::guard_document_edit_locked`, called alongside the
  existing `guard_clipboard_locked` at every mutation entry point
  (`delete_selected`, `paste`, `move_selection_to`, `remark`,
  `add_node`/`add_child`/`add_sibling`, inline edit, `nudge`, `commit_kind`,
  `undo`, `redo`).

### Update - 2026-09-14 (5)

**Added**

- **One document-level Action-menu item: "Edit whole file as text"** (`ActionId::EditDocument`,
  `core.action.edit-document`). It is scoped to the file rather than the selection, so it is
  always enabled — including on a read-only node and in hosts that draw no Root row, which
  since ADR 0013 is every web host. It leads the menu's last section, taking over the
  separator `Delete` used to carry, and is never styled destructive.
- **`Intent::BeginEditDocument`** — opens the whole document in the host's own multi-line text
  surface, independent of the cursor; the document-scoped counterpart of `BeginEditExternal`.
  Refuses while the clipboard is armed, like every other modal-open path (ADR 0005 §5).
- **`Session::apply_document_text`** — the whole-document commit, resolving an `ApplyReplace` at
  the empty path. Deliberately *not* the per-node external-edit path: that one runs
  trailing-blank splitting, trailing-comment extraction and the keyless-element re-wrap, all
  node-shaped operations that merely happen to be inert on a whole file. A file's own final
  newline must never be reinterpreted as a node's trailing blank run.
- **`core.document.apply-failed`** (en + zh-TW), raised at `Severity::Error` for every cause.
  The backends notice a *parse* failure at `warn`, which is the "proceeded, with a caveat"
  tier — wrong for "none of your text was applied". The backend's own reason rides along as the
  message argument. A rejected buffer leaves the document byte-identical.

### Update - 2026-09-14 (4)

**Added**

- **`doc_revision` on `SessionSnapshot`** — a monotonic successful-commit counter, incremented
  exactly once per committed mutation (including an undo or redo, each of which changes the
  document text and so moves the revision *forward*, never back). A host can now ask "did the
  mutation I just dispatched commit?", which is what the coming Raw-pane whole-document Apply
  needs. Mirrored in `web/types.ts`.

**Documentation**

- **`history_len` is documented as what it is: a depth, not a commit count.** Both it and
  `History::depth()` now warn at the definition that a snapshot identical to the current one is
  deduped and that the undo caps evict from the front, so the value legitimately stays flat —
  or pinned at 200 — across a mutation that really did commit. Reading it as a commit counter is
  the trap that produced an earlier wrong design; new tests in `session_headless.rs` and
  `functional_smoke.mjs` pin both failure modes.

### Update - 2026-09-14 (3)

**Tests**

- **Raw write mode T2 — failure behavior, the failed-Apply detector, and byte offsets
  measured** (`crates/confy-ffi/functional_smoke.mjs`, *RS0/T2*, 18 checks). A broken
  whole-file Apply never commits on any backend (TOML/JSON unbalanced, TOML/JSON duplicate
  key, YAML multi-document). Two measurements produced design changes:
  - **Notice severity is not uniform** — a *parse* failure notices as `warn`, a *semantic*
    failure as `error`. So a rejected whole buffer could surface as a mere warning; the
    feature's `core.document.apply-failed` will be raised at `Severity::Error` regardless of
    the inner cause (design record R26, amended).
  - **`outline()`'s `text_range` spans a leaf's whole member** (`target = "needle"`), not just
    the value — there is no value-only range in the wire contract, so the planned Raw-pane
    jump selects the node's row text (new decision R29).
  - `history_len`'s unsuitability as a commit detector is now pinned by tests in both of its
    failure modes (dedup of a no-change Apply; pinned at 200 at the undo cap while a real
    commit lands), and the byte→code-unit drift has a quantified failing case (`drift === 8`
    on a CJK+emoji fixture) for the web helper to satisfy.

### Update - 2026-09-14 (2)

**Tests**

- **Raw write mode T1 — the identity-Apply measurement (Q1) is answered: byte-identical on
  every format.** `crates/confy-ffi/functional_smoke.mjs` gains an *RS0/T1* section (18
  checks) driving `ApplyReplace { path: [], text }` through the real wasm channel over six
  fixtures — TOML (comments + `[[tasks]]`), JSON, JSONC (leading/inner/trailing `//`), YAML,
  TOML with no final newline, YAML with CRLF. Each asserts `serialize()` equals the input
  byte-for-byte, that applying the **untouched** document buffer changes nothing, and — as a
  false-positive guard — that the same route commits a *modified* whole-file text with
  `history_len === 1`. No backend fix is needed before RS2 (the design record's R23 ship
  blocker is not triggered); results recorded in the record's Evidence section.

### Update - 2026-09-14 (1)

**Docs**

- **ADR 0014 — whole-document editing reuses each host's existing text surface** (new,
  `docs/adr/0014-whole-document-editing-reuses-each-hosts-text-surface.md`): core owns one
  operation (one Intent, one document-scoped Action item, one whole-file `Replace`) and each
  host routes it to the surface it already has — the desktop's Raw pane switching to write
  mode, touch's external-edit sheet, the TUI's `$EDITOR`, VS Code's own editor (suppressed).
  The rejected alternative, one uniform whole-file modal, is absurd on the TUI and redundant
  in VS Code. Indexed in `docs/adr/README.md`.
- **`docs/spec/2026-09-11-raw-write-mode-design.md`** — `Approved (2026-09-14)`; a grilling
  pass added R20–R28 and corrected two decisions that were provably wrong: the failed-Apply
  detector (`history_len` is deduped by `History::push` and evicted by the undo cap, so a
  successful no-change Apply read as a failure — replaced by a monotonic `doc_revision`), and
  the reuse of `apply_external_replace` for the whole file (its per-node blank/trailing-comment
  packaging would have turned the file's final newline into a mutation — replaced by a
  dedicated `apply_document_text`). Q1 narrowed from "does the route exist" (it does, in all
  three backends) to "is an identity Apply byte-identical, per format", with a non-identical
  format a ship blocker.
- **`docs/plan/2026-09-14-raw-write-mode.md`** (new) — 11 session-sized tasks, each one commit
  with its own verification command, **token estimate**, and a breakpoint state that survives a
  compaction or a cold start.
- **`docs/reference/glossary.md`** — three entries for the vocabulary this feature introduces:
  **Raw pane** (states *Raw view* / *Raw write*), **Document buffer** (the unapplied whole-file
  text, contrasted with core's *fragment*), and **Apply** (sending a document buffer into the
  `Session`, distinct from core's *commit* — one Apply can fail to commit).

### Update - 2026-09-11 (1)

**Docs**

- **The root-row-alignment direction was reversed before release, and the work preserved on
  branch `root-row-alignment`** (six commits, `e8e8d5b`..`f2bfef3`; `main` rewound to
  `090de73`, nothing rewritten). New direction: **every host — the TUI included — is
  root-hidden**; the TUI's root row was the asymmetry's origin, so aligning web to it was the
  wrong axis. Code on `main` is unchanged by this; the landing is documentation-only.
- **`docs/debug/2026-09-11-root-row-alignment-retrospective.md`** (new) — what survives the
  reversal and what is discarded, the three problems that remain live on `main` (Root-armed
  clipboard dead-end; the two web blank trees; no whole-document entry point on web), the
  `Into([])` observation that did not reproduce on `main`, five process lessons, and the five
  questions the redesign must settle. Listed in `docs/debug/README.md` §In progress.
- `docs/plan/2026-09-09-open-follow-ups.md` — the three problems filed as Open rows, plus a
  *Watching* entry for the `Into([])` two-trees discrepancy. No new documentation folder: the
  retrospective lives in the existing `docs/debug/` taxonomy (handoff notes from
  investigations); trust stays location-based.

### Update - 2026-09-10 (6)

**Fixed**

- **Blank lines added *inside* a comment block in the multiline editor were eaten (and the
  edit's trailing run lost).** Editing one Comment node and splitting it with blank lines
  (`# a` ⏎⏎ `# b` ⏎⏎ `# c`) saved as `# a` / `# b` / blank / `# c`: the **first** gap
  vanished, merging two groups, while the run the buffer ended with never landed.
  `Session::apply_edit_comment` splits the buffer's trailing blank lines off before
  `Mutation::EditComment` and re-applies them afterwards with
  `SetTrailingBlankLines { path, n }` — using the path the editor was opened on. That holds
  only while the block stays **one** node: a buffer with interior blank lines commits as one
  Comment node per blank-separated group (the projection rule), so `path` then named just the
  *first* group and the run was written into the first interior gap instead — set to `n`,
  i.e. **deleted** for the usual `n = 0`. The real trailing run was never set at all. The run
  is now applied to the **last** spliced group (`blank_lines::blank_separated_groups` counts
  them; for an unsplit block the target is `path` itself, unchanged behavior). The
  "`n` equals the current count ⇒ skip" short-circuit is why leaving one extra blank line at
  the buffer's end used to look correct — the mis-aimed write happened to be a no-op.
- **JSON/JSONC and YAML refused such a buffer outright, discarding the content edit with
  it.** Their `edit_comment` validators required *every* line to start with `//`/`#`, so one
  blank separator failed the whole mutation ("every line of a comment must start with //") and
  the document was left untouched — the text changes went with it. Both now exempt blank
  lines, matching TOML and their own `insert_comment`. JSON's multiline rebuild emits such a
  separator as a genuinely **empty** line rather than an indented one, so no trailing
  whitespace is written.
- Verified on the real binary (TUI + `$EDITOR`) for TOML and JSONC, not only in-process;
  pinned by `session_headless.rs::a_comment_buffer_split_by_blank_lines_keeps_every_run`
  (three backends, five TOML buffers) plus a `blank_lines` unit test.
- **Not fixed here** (pre-existing, unrelated to blank lines): TOML `EditComment` on a comment
  block **inside a multiline array** loses the continuation lines' indent — `arr = [` / `  # 1`
  / `  # 2` edited to two lines writes the second at column 0. The block fragment opens without
  indent and the splice re-emits it verbatim; the existing test only asserts `contains`.

### Update - 2026-09-10 (5)

**Documentation**

- **New `docs/reference/HOST_PARITY.md` — one index of every deliberate host divergence.**
  The TUI↔web differences were scattered across seven reference docs, five ADRs, `CLAUDE.md`
  and a handful of code comments, so "is this difference on purpose?" had no single place to
  ask; `KEYMAP.md` only ever covered the keyboard slice. The new file is an **index, not a
  spec**: ~45 divergences as one-liners in six sections (Input · the undrawn root row · row/
  cursor/clipboard state · editing · rendering · chrome, messages, capabilities), each row
  naming the doc that owns the detail, plus a "Not divergences" list of the core-owned
  behavior that must stay identical and a maintenance rule (a new host-specific behavior adds
  a row in the same commit). Indexed from `docs/reference/README.md` and `CLAUDE.md`'s
  contract table, and cross-linked from `KEYMAP.md`, `TUI.md`, `WEBUI.md` and
  `ROW_STATE_MODEL.md`.
- **Four divergences that only existed in code comments are now in the reference docs**: the
  web stripping/re-appending core's bundled `value␠␠# comment` inline-edit buffer so the
  comment gets its own cell (`WEBUI.md`, `TUI.md`); the fuzzy-match mark being a translucent
  background on the web but a foreground repaint in the TUI, because a terminal cell can't
  layer alpha (`WEBUI.md`); the TUI's underlined warn style as the stand-in for the web's
  wavy underline + hover tooltip on a non-standard-JSON comment (`TUI.md`); and `~/.config`
  on macOS being a deliberate terminal-tool convention rather than
  `~/Library/Application Support` (`TUI.md`).

### Update - 2026-09-10 (4)

**Fixed**

- **Web paste mode could still step the insertion point onto the undrawn root row.**
  Follow-up to (3), which drew both root slots but left the *stepping order* alone:
  `paste_slots()` lists each row's `Into` before its `After`, so `Into(root)` is index 0 —
  above everything — while `slot_target` resolves it to `children.len()`, an append at the
  document's **end**. So `↑`/`k`/PageUp/`Home` from the top of the tree threw the insertion
  point to the opposite end of the document and clamped there, reading as "the target moved
  somewhere invisible above the first node" in hosts that don't draw the root row. Fixed
  **web-side only**: `overshotUndrawnRootSlot()` (`web/path-utils.ts`, the paste-mode sibling
  of `drawnCursorFallback`) flags an upward nav that landed on `Into(root)`, and
  `navSelect`/`touchNavSelect` step one slot back down onto `After(root)` — the document's
  top. Downward navigation and `End` are untouched (reaching the append slot from below is
  correct). **Core and the TUI are deliberately unchanged**: the TUI draws the root row, so
  stepping onto it highlights a real, visible row there. Verified in Chromium — `↑`/`Home`
  now rest on a visible line at the first row's top edge, `End` on the append line at the
  last row's bottom, and a cut node pasted after clamping lands as the document's first node.

### Update - 2026-09-10 (3)

**Fixed**

- **The undrawn root row's two paste slots had no cue in either web host.** `treeHTML` never
  draws the root row, but core's `paste_slots()` emits `Into(root)`/`After(root)` first and
  `pointer_slot` resolves the *first drawn row's top band* to `After(root)` — the only route
  to "insert above everything" (root index 0, per `resolve_target`). Both surfaced wrong:
  - **Paste mode.** Stepping `↑` to the top of the list lost the insertion line for the last
    two steps (both slots' row lookup missed), so the document's top and end looked
    unreachable. Reproduced live in the browser: the cue went `display:none` on the third
    `↑` and stayed hidden.
  - **Drag.** `dnd.ts` fell back to the *hovered* row's bottom edge, which is exactly where
    `After(<first row>)` draws — the top 3/4 of the first row and its bottom quarter drew a
    pixel-identical line for two different destinations, so dragging to the very top looked
    like it stopped below the first node. Same fallback in touch's `onReorderMove`.
  - **Touch's `Into(root)`** was drawn at the tree's *top* on the assumption that `Into` means
    "first child"; `slot_target` resolves it to `children.len()`, i.e. an append at the
    document's **end**.
  New shared `rootSlotLine()` (`web/slot-line.ts`, next to the existing indent rule) resolves
  a root-anchored slot to its stand-in row edge — `After(root)` → first row's top,
  `Into(root)` → last row's bottom, both at the top-level indent — and all four cue sites
  (desktop confirmed + hover + drag, touch cue + reorder drag) use it. Two new
  `session_headless.rs` tests pin the core semantics both cues now mirror; the web specs gain
  root-slot coverage on the real extracted function bodies.

### Update - 2026-09-10 (2)

**Fixed**

- **Save As / Convert stacked a second extension onto a `.jsonc` or `.yml` name** — the
  reported `xxx.jsonc.json`. Three places treated `.json`/`.yaml` as the *only* spelling of
  their format, though `.jsonc` and `.yml` are the same `DocFormat` (glossary §*Comment
  advisory*): `host-io.ts`'s `ensureExt` appended `.json` to a name already ending in
  `.jsonc`; `targetTagFor` failed its `endsWith(".json")` test on `.jsonc` and fell through
  to `Toml`, so a TOML→JSONC convert offered `x.jsonc.toml`; and `fs.ts`'s save-picker
  accept list named one extension per format, which makes Chromium enforce it on the
  returned name (a `.jsonc` suggestion comes back `.jsonc.json`) even when the app asks for
  the right thing. `ensureExt` now keeps any extension that already names the target format,
  `targetTagFor` routes through `formatFromName`, and the picker accepts both spellings.
  Covered by five new `web/host-io.spec.mjs` checks (all three stacked names reproduced
  before the fix).

### Update - 2026-09-10

**Fixed**

- **The TOML table-capture paste error named the wrong side of the insertion point.** Moving
  or pasting a scope `[table]` above a scalar reported `paste error: a table here would
  capture the keys above it` — but the keys a header captures are the ones that *follow* it.
  The message now reads `a table here would capture the keys below it`, the wording the
  kind-switch path (`cst_edit/convert.rs`) already used for the same rule. The mirrored
  leaf-side message (`a key here would be captured by the table above it`) was already
  correct and is unchanged; `check_partition`'s doc comment carried the same inverted
  phrasing and was corrected with it.

## [v1.2.0] - 2026-09-09

### Update - 2026-09-09 (38)

**Documentation audit — every living doc re-verified against the code**

A full sweep of `docs/reference/`, the four working-record indexes, `CLAUDE.md`, `README.md` and
`CONTEXT.md`, checked against the tree as it stands after the F1–F15 remediation wave. Record:
[`docs/audit/2026-09-09-documentation-audit.md`](docs/audit/2026-09-09-documentation-audit.md).

The structure held up — zero broken links, zero unindexed documents, zero filename violations,
every `Status:` value legal, and the whole 49-row `KEYMAP.md` table plus all 33 README
keybindings verified against both implementations. What had rotted was accuracy:

- **`MUTATIONS.md` and `BEHAVIOR_MATRIX.md` disagreed on Remark.** `MUTATIONS.md` still called
  remarking an array element "YAML-only, by design"; F1 unified it on 2026-09-09 and
  `tests/format_parity.rs` pins all three formats. Rewritten to the own-line rule, deferring to
  §8 rather than restating it.
- **Ten dead `docs/superpowers/…` paths** in six reference docs, left by the 2026-09-09 folder
  move. They survived the link checker because they are inline code spans, not links (one also
  had the wrong filename). All corrected.
- **`MESSAGES.md` §8 was a 46-line resolved-bug backlog** living in the folder whose README says
  "current behavior only", and §4 pointed at it for a follow-up that had already closed. Section
  removed — its content is in the re-verification record, the backlog, and this changelog.
- **`glossary.md`'s "full vocabulary"** mis-stated the Detail `Sign:` line (`(B)`/`(Q)`/`(D)`/`(-)`
  are Type-filter glyphs; the line spells out `bare`/`quoted`/`dotted`/`none`), dropped the
  in-bracket padding from the integer tags (`[I:dec]` → `[I:dec ]`), omitted all four datetime
  tags, and filed JSON's `[T/M]` under TOML.
- **Recounted metrics in `CLAUDE.md`:** `taplo::parser::parse` is 49 call sites (not 48) and
  `crates/confy-core/tests/` holds 19 integration suites (not 18). `functional_smoke.mjs`'s 129
  checks and the taplo syntax/dom counts (28/2) re-measured as already correct.
- **Gaps filled:** the `C` convert overlay was undocumented in `TUI.md`, whose `~` overlay also
  still described pre-F3 behavior; `VSCODE.md`'s protocol table missed four `HostToWebview`
  schema-response variants; `TAURI.md`'s File menu missed Save As; `CHROME.md`'s inventory missed
  the format pill; `BEHAVIOR_MATRIX.md` §7 missed two `ConfigDocument` facets and credited
  `App::external_edit_path` to the wrong type; `WEBUI.md` had a 14-vs-16 method count, an
  impossible `docFormat` signature, and a `cf-build.sh` description two rewrites out of date;
  `README.md` never documented `--format`; and `CLAUDE.md`'s module map omitted ten real paths
  (`web/touch/`, `touch.html`, `path-utils.ts`, `vscode.ts`, `vscode-protocol.ts`, `sw.js`,
  `assemble-dist.mjs`, `schema/mod.rs`, `format_parity.rs`, `json/edit/tests.rs`).
- **Stale line-number citations** in `ROW_STATE_MODEL.md` replaced with symbol names, matching
  the "files and symbols, never line numbers" rule the re-verification record set.

Organization: the backlog contradicted both indexes — it read `Resolved` while `plan/README.md`
and `CONTEXT.md` called it the one living record. Resolved by filing the item that made the
contradiction real: `MESSAGES.md` §7.2 recorded that `ConvertResult.warnings` is raw untranslated
English on every convert surface, and it had never become a backlog row — exactly the "survives
only where nobody looks" case the backlog exists to prevent. `2026-08-29-code-audit.md` is frozen
`Resolved (2026-09-09)` now that all 21 findings are closed or parked, with its three sub-reports
linked instead of named; the re-verification record's `Status:` moved to line 2; the two unowned
prototype `.html` assets in `docs/spec/` gained an index table; and `docs/reference/README.md`
gained a fourth machine-checked row for `BEHAVIOR_MATRIX.md` §8.

No code changed. The four `keymap_doc_*` drift-guard tests still pass.

### Update - 2026-09-09 (37)

**F12 — `CHANGELOG.md` split by version series; the backlog is now empty**

The finding was the growth trend, not a size threshold: flagged at 341 KB / 2,042 lines in the
2026-08-29 audit, re-measured at 482 KB / 4,130 lines on 2026-09-09, and **534 KB / 4,796 lines
/ 38 release sections** when this task started — 57% growth in eleven days, almost all of it
`Unreleased Update` entries from the remediation work itself.

Split by series, moving nothing but whole sections:

| | lines | bytes | `## [` sections |
|---|---|---|---|
| `CHANGELOG.md` before | 4,796 | 534,061 | 38 |
| `CHANGELOG.md` after | 1,762 | **119,121** (−78%) | 4 (`[Unreleased]`, v1.1.0, v1.0.1, v1.0.0) |
| `docs/reference/changelog/v0.x.md` (new) | 3,048 | 415,364 | 35 (v0.32.0 … v0.2.0) |

The archive is **verbatim** — same format, same newest-first ordering, no edits on the way
over; only a short header and a link back were added. `docs/reference/changelog/README.md`
indexes it and states the archiving rule.

**The release contract is intact.** `.github/workflows/release.yml`'s `verify-versions` job
greps the *root* `CHANGELOG.md` for `## [v${tag}]`; every v1.x section stayed there, so the
next tag verifies exactly as before. The archive index and `CLAUDE.md`'s release-process list
both now say it in one line: never archive the series the next tag belongs to. Archiving
happens on the first release of a *new major* series.

Also updated: `CONTEXT.md`'s reading order and `docs/reference/README.md`'s cross-cutting index.

With F12 closed, `docs/plan/2026-09-09-open-follow-ups.md` has an empty `## Open` section and
its `Status:` is now `Resolved (2026-09-09)` — all 15 findings from the 2026-08-29 audit and
the 2026-09-09 re-verification are Done, and the record joins the frozen set.

### Update - 2026-09-09 (36)

**F10 — the live-index invariant had three edges, not one; `Move ×8` −93%**

The earlier P0 fix dropped the whole-document `CstIndex` before `Move`'s delete/insert phases
and took TOML `Move` down 48%. F10 filed the remainder as needing a structural `CstIndex`
rewrite (paths instead of live handles). Measured per phase first, at 7,001 nodes:

| phase | before | after |
|---|---|---|
| capture walk | 5.5 ms | 5.5 ms |
| delete (per source) | 45 ms | 8 ms |
| re-insert (per source) | 90 ms | 7 ms |

The cost was never spread out — it was two specific operations, each quadratic *on its own*
under a live index, because a `clone_for_update` parent tracks its live children in a scanned
list:

| under a live 7,001-node index | cost | without it |
|---|---|---|
| `tree.children().count()` over 5,000 ROOT children | 32 ms | **0.09 ms** |
| `tree.splice_children` at ROOT | ~95 ms | ~7 ms |

Three fixes follow directly, no rewrite:

- **`table_member_spans` asks the index instead of scanning the tree.** It wanted every header
  prefixed by the table's path; the index already holds exactly those, keyed by path.
- **`section_span_text` reads its span off `tree.green()`** — immutable data in the same child
  order, materializing no handles.
- **`insert_with` takes its `CstIndex` by value and drops it** once the insert position is
  resolved. Its resolution work totals ~52 µs; the 95 ms was entirely the final splice
  renumbering thousands of live handles it no longer needed.

| `--nodes 500` (7,001 nodes) | before | after | YAML |
|---|---|---|---|
| `Move ×1` | 296 ms | **33 ms** | 20 ms |
| `Move ×4` | 1.15 s | **88 ms** | 46 ms |
| `Move ×8` | 2.27 s | **158 ms** | 82 ms |

−93% at 8 sources, within 2× of the YAML backend — the acceptance criterion was "approaches
YAML's 79 ms". What is left is the per-source `walk` each re-insert still does (5.5 ms) plus
the shared `apply` envelope, so the filed `CstIndex` rewrite stays unnecessary.

Verified on the real binary, not just the bench: before and after builds, identical keystrokes
(`0j` `x` `jj` `v` `w`) moving `[alpha]` past `[beta]` in a file with an interior comment —
byte-identical output from both. Full workspace suite green (34 suites), including the
`roundtrip*` and `format_parity` sets that pin move/delete/insert layout.

`MUTATIONS.md`'s live-index invariant now states all three rules with the measurements.

### Update - 2026-09-09 (35)

**F9 — undo history gets a byte cap; the filed fix is refuted**

The finding said undo stores full text snapshots and proposed `rowan::GreenNode` snapshots for
structural sharing. Measured first, on a synthetic TOML document at three sizes, 200 mutations
deep:

| document | full 200-entry stack | one undo (re-parse) |
|---|---|---|
| 3.5 KB | 0.7 MB | 49 µs |
| 36 KB | 7.3 MB | 493 µs |
| 1.07 MB | **216 MB** | 15.5 ms |

Two things fall out. **Latency was never the problem** — 15 ms at 1 MB, on an operation nobody
performs at keystroke rate. And **a green tree is the expensive representation, not the cheap
one**: the same 1.07 MB document sits at ~94 MB resident once loaded (~70× its text), so 200
shared green snapshots would need near-perfect sharing just to match 200 plain `String`s. The
proposed rewrite would have touched the `ConfigDocument` trait and all three backends to make
the wrong axis faster.

The real axis is memory, and it is linear in document size because only the *entry* count was
capped. So `History` now enforces a second cap, `MAX_HISTORY_BYTES = 16 MiB`, alongside
`MAX_HISTORY = 200`, tighter one wins: every document up to ~80 KB keeps the full 200 steps,
larger ones lose *depth* instead of growing the footprint, and at least one undo step always
survives. ~20 lines in `session/state.rs`, no trait change.

RSS on the real binary turned out to be a **useless instrument** here and is recorded as such: a
1.28 MB file sits at ~95 MB resident before any edit, and 20 nudges and 200 nudges both land at
~257 MB, so parse-tree churn and allocator retention drown out the history entirely. The
decisive measurements are the direct ones above plus two new `History` unit tests. What the real
binary *did* verify is that the cap changes nothing a user sees: before and after binaries,
identical keystrokes (`9j` `→→→` `w` `zz` `w` `yy` `w`), file on disk read back each time —
`8083` / `8081` / `8083` from both.

ADR 0003 gains a dated amendment: it explicitly left "revisit compressed/diffed snapshots" open,
and that door is now closed with numbers.

### Update - 2026-09-09 (34)

**Build hygiene, and the last two dependency upgrades**

**F13 — `.cargo/config.toml`, `incremental = false`.** Measured on an M4 with a fully warm build:

| | size | files |
|---|---|---|
| `target/` | 49 GB | 484,567 |
| `target/debug/incremental` | **23 GB** | **120,313** |

Half the build directory, for a cache worth ~1.6 s on an edit-then-test rebuild (5.3 s with it
off vs 3.7 s on). The audit's diagnosis — "110 s wall for 0.09 s of tests, over 99.9% of it cargo
stat-ing fingerprints" — **only reproduces on a cold filesystem cache**: measured warm, a no-op
`cargo test -p confy-core --lib` is 0.32 s with incremental either way. So the reason to disable
it is the 23 GB and the cold-start tail, not steady-state speed, and the config file says so, in
case a future maintainer wants the 1.6 s back. CI never benefited (fresh checkout per job).

The existing `target/debug/incremental` is left alone — deleting a user's build cache is not this
change's business. `rm -rf target/debug/incremental` reclaims it whenever you like.

**F11 — `jsonschema 0.30 → 0.55`.** Not 0.44 as filed; 0.55.1 is current. `instance_path`,
`schema_path` and `kind` became accessor methods (were public fields), which is the entire
migration — 4 call sites in `schema/validate.rs` and `schema/hints_edit.rs`. Cost, measured:

| | raw | gzip |
|---|---|---|
| wasm before | 4,052,894 | 1,281,394 |
| wasm after (both upgrades) | 4,581,319 | 1,423,614 |

**+139 KB gzipped (+11%)**, and 5 new transitive crates (`jsonschema-regex`, `jsonschema-value`,
`data-encoding`, `micromap`, `unicode-general-category`). Recorded rather than glossed: this is
the price of the upgrade, and it is paid by every web visitor.

**F11 — `fuzzy-matcher 0.3 → nucleo-matcher 0.3.1.** The old crate is unmaintained. The surface
is two functions (`session/search.rs`), but the matcher is shared by the TUI's `highlight_spans`
and the web's `mark.fz` runs through one wasm export, so a scoring change shows up on both.
`Matcher` holds reusable scratch buffers and needs `&mut`, so the shared instance became a
`thread_local!` `RefCell` instead of a `LazyLock`; indices are sorted and deduplicated once here
rather than at each of the three call sites.

Two deliberate behavior changes, both verified on the real binary and suited to a haystack that
*is* `path + value + comment` joined by spaces:

- **A space now separates independent, order-free terms.** `8080 server` and `server 8080` both
  match `server.port 8080`; before, the space had to appear literally and in order, so neither did.
- **Unicode folding.** Typing `cafe` matches and highlights `"café"` — confirmed in the TUI, which
  marked `c a f é` and narrowed the tree to that row. Before, it did not match.

Everything else stays a literal fuzzy subsequence: `^`, `$`, `!`, `'` have no special meaning
(nucleo's anchor/negation syntax lives in `Pattern::parse`, which confy does not call). Verified
the web surface separately in a real browser: `prt` still produces exactly three `mark.fz` runs.

`TUI.md` §Filter documents both changes; `WEBUI.md` and the FFI doc comment no longer name
`SkimMatcherV2`.

### Update - 2026-09-09 (33)

**The `?diag=1` trace was desktop-only because there was one copy of it**

F15, opened while closing F4. `drainDiagIfEnabled` lived in `web/ui.ts`; `web/touch/app.ts` had
no equivalent, even though both orchestrators drive the same `ConfySession` and the same
256-event ring. Measured in a real browser on the touch entry (`touch.html?ui=touch&diag=1`,
420×820), three `j` keystrokes:

| entry | before | after |
|---|---|---|
| touch | **0** `[confy-diag]` lines | **12** — `dispatch CursorDown` / `mutation CursorDown ok` / `dispatch SetSelection` / … |
| desktop | 8 on two keystrokes | 8 — unchanged |

Extracted to a shared `web/diag.ts` (`drainDiagIfEnabled(session)` + `resetDiagCursor()`) rather
than pasting a second copy into the touch orchestrator. The reason is the bug itself: this went
unnoticed precisely because it was per-host code, and the cursor-reset rule that F4 added to
`ui.ts` would have needed adding twice, in two places nobody cross-checks. Both hosts now call
the drain from `render()` and the reset at their single `openText` session swap — verified in the
browser that a desktop format swap still logs the new session's events (8 → 19 across the swap)
rather than swallowing them.

`diag-export.spec.mjs` retargeted at the shared module and now asserts **both** hosts wire it up,
so a third entry point can't quietly ship without the trace. `WEBUI.md` §Diagnostics,
`MESSAGES.md` §4.1/§8 and `CLAUDE.md`'s module map updated; the backlog's last XS is closed.

### Update - 2026-09-09 (32)

**The TUI's diagnostic ring was empty because the taps were on the wrong function**

F7 + F8. Two host↔core contract defects, done together.

**F7 — the `~` overlay recorded nothing.** Measured on the real binary before touching anything:
16 navigation keystrokes (`9jjkjjkkjj0919jj`), then `~` → `Diagnostics — 0`, `(no events yet)`.

The filed cause did not survive re-measurement. It read "~15-20 sites in `tui/app.rs` and
`tui/mod.rs` mutate `Session` fields directly, bypassing `dispatch`". Counting only production
code — the rest were test setup — there were **five**, and the TUI already routed every keystroke
through `Session::apply(Intent)`. The real cause: **the diag taps sat on `dispatch()`**, which is
`apply()` plus a render snapshot, and the TUI deliberately calls the cheaper `apply()` to build
its own row list. So the taps traced the Web UI and nothing else.

Moved both taps into `apply()`. `dispatch()` delegates there, so each intent is still recorded
exactly once.

| same 16 keystrokes | ring | first lines |
|---|---|---|
| before | `Diagnostics — 0` | `(no events yet)` |
| after | `Diagnostics — last 20 of 32` | `[Debug] dispatch CursorUp` / `[Info ] mutation CursorUp ok` / `dispatch CollapseAll` / `dispatch ExpandAll` / `dispatch ExpandLevel` … |

Of the five direct field writes, four now go through the command channel: two new Intents
(`ConvertWriteDone` — back to rest but **keeping** the notice the host just set, the one thing
`ExitConvert` can't do; `SetStrictJson` — the load-time `.json` flag) and one that already
existed (`SetPasteSlot`, for the paste-mode re-assert after a rebuild). The fifth stays and now
says why in a comment: `apply()` performs the same `last_action_was_shift_select` reset, but
`~`, the language picker and `Noop` are handled entirely host-side and never reach `apply`.

**F8 — the failure variant is behavior, and two cases had the wrong one.** A host branches on
`MutateError`: `Fragment` keeps the inline editor open for a retype, `Collision` opens a prompt,
the rest report and stop. So the same situation must produce the same variant in every format.

| case | before | after |
|---|---|---|
| `d` on the Root row (real binary) | `NotFound` → *"delete error: path not found"* | `Unsupported` → *"delete error: operation not supported here"* |
| JSON `Replace` with `"unclosed` | `Illegal("expected R_BRACE, found None")` | `Fragment("unexpected token: …")` — matching TOML |
| JSON `Replace` with `[1, ` | `Fragment(…)` | unchanged — the record's claim that this one diverged was wrong |
| YAML, both fragments | `Ok` | unchanged — documented exception, now asserted |

The root is not *missing*, it is undeletable; all three backends now say so, and `Unsupported`'s
wording dropped "by this format" since the root and a read-only YAML span are about the *node*.
The JSON divergence was narrower than filed and had a concrete mechanism: the lexer emitted
`STRING` for a string that ran to EOF, so the splice passed and the *document-level* backstop
failed instead — producing `Illegal`. It now emits `ERROR` (still lossless), so the fragment is
rejected where it is parsed.

The taxonomy is written on the enum itself and pinned by two new `tests/format_parity.rs` cases
(12 behaviors now). YAML's leniency is asserted as the exception rather than left unstated, so
the day its lexer tightens, a test says so.

`MESSAGES.md` §4/§8 and `BEHAVIOR_MATRIX.md` §8 updated; three backlog entries closed (F7, F8,
and the *Watching* bullet that folded into F8).

### Update - 2026-09-09 (31)

**`json/edit.rs` was the last god object; it is now eight files**

F6. 2,892 lines — 1,802 production plus a 1,090-line inline `#[cfg(test)] mod tests`. The other
three had already been dealt with (`cst_edit/mod.rs` 295 production lines, `tui/app.rs` 1,009,
`yaml/edit/mod.rs` 147), each by extracting tests to a sibling `tests.rs` via
`#[path = "tests.rs"]`; JSON was the one genuinely monolithic *production* file left.

Split by construct, the same axis YAML uses — not by arbitrary size:

| file | lines | holds |
|---|---|---|
| `mod.rs` | 100 | atomic dispatch (`apply`) + `validate_semantics` + the module wiring |
| `resolve.rs` | 80 | path → `Target`, fragment serialization |
| `fragment.rs` | 224 | what the caller's text *means*: parse, adapt, trailing-comment extraction |
| `container.rs` | 335 | destination OBJECT/ARRAY lookup, item read-back, inline/multiline rebuild, indent detection |
| `replace_delete.rs` | 244 | `Replace`, `Delete` |
| `insert.rs` | 211 | `Insert`, `Move` |
| `mutations.rs` | 377 | Rename, Remark, EditComment, InsertComment, SetTrailingComment, SetTrailingBlankLines + extent helpers |
| `convert.rs` | 320 | `ConvertKind`: Inline↔Multiline, float Plain↔Exponent |
| `tests.rs` | 1,091 | the 67 tests, unchanged |

Largest production file: **377 lines, down from 1,774**. `resolve`/`fragment`/`container` are
the three layers every splice goes through, so they are files rather than sections; the rest is
one file per `Mutation` family.

**Pure code motion.** No behavior change, no new tests, no renamed public items — the six
`pub`/`pub(crate)` entry points (`resolve`, `serialize_fragment`, `apply`, `extent_end_offset`,
the two `fragment_*_trailing_comment`) are re-exported from `mod.rs`, so `doc.rs` is untouched.
Private `fn`s became `pub(super) fn`; one `const TRAILING_MARKER` followed. Verified the test
count is identical before and after (67 `#[test]` in both), all 34 suites pass, clippy is clean,
and the wasm functional smoke still passes.

`CLAUDE.md`'s module map and `BEHAVIOR_MATRIX.md`'s two `json/edit.rs` references now name the
directory.

### Update - 2026-09-09 (30)

**Touch severity toasts finally look like their severity**

F5, the last of the three original `MESSAGES.md` §8 items. `renderNotice` has applied
`sev-info`/`sev-success`/`sev-warn`/`sev-error` to the toast since it was written, and
`web/touch/style.css` styled **none** of them — so a `Warn` and a `Success` differed only by how
long they stayed up (3000 ms vs 1600 ms). Desktop tints its status line by severity, so the two
hosts disagreed on whether severity was visible at all.

Each toast now carries a 4px left bar plus a tinted border and background, in the hues the
desktop status line already uses: `--warn` for `Warn`, `--t-bool` for `Error` (also bolded),
`--t-string` for `Success`. `Info` stays neutral — it is the default and needs no signal. The
tint goes on the border rather than the text because the chip is dark in both themes and the
text has to stay legible against it.

**A screenshot lied and a computed-style check caught it.** The first version used `--drop` for
`Success`, matching desktop exactly. It rendered as a white border, which read as "thin green,
probably fine" in the screenshot. Querying `getComputedStyle` instead:

```
success  bg=rgba(0,0,0,0)  border=oklch(0.95 0.01 240)   ← transparent + currentColor
warn     bg=oklch(0.38 0.0392 222)  border=oklch(0.8 0.14 75)
```

`--drop` is **not defined in `web/touch/style.css` at all** — the two web palettes are not the
same set — so `var(--drop)` fell back to `currentColor` and the whole `color-mix` collapsed.
`--t-string` (identical in both files) stands in, with the reason in the CSS comment. The token
gap itself is recorded under *Watching*: a shared token file would remove the class of bug, but
it is not worth it for one token.

Verified in a real browser at a 420×820 touch viewport, all four severities, both themes.

`MESSAGES.md` §5.3 now describes the styling as part of touch's notice contract, and its §8
entry is gone. **All three original §8 items are now closed**; the two that remain there (F7,
F15) were found by measuring while closing them.

### Update - 2026-09-09 (29)

**The web diag cursor outlived the session it was counting**

F4, the last of the three `MESSAGES.md` §8 diag items. `web/ui.ts`'s `lastSeenSeq` is a
high-water mark over the ring's `seq`, advanced only inside `drainDiagIfEnabled` and reset
nowhere. A new `Session` brings a **new ring, restarting at `seq = 0`** — so after opening a
second document every event sat below the inherited mark and was skipped.

Measured in a real browser (`?diag=1`, `console.debug` captured):

| | before | after |
|---|---|---|
| 5 keystrokes, first document | 20 events logged | 22 |
| swap document, 3 more keystrokes | **0** | **19** |

Not "some events delayed" — the trace goes completely silent for the rest of the session, since
the new ring would have to emit as many events as the old one before it caught up. Exactly the
failure mode where a debug channel is worse than none: it looks like nothing is happening.

The fix is one line plus its reasoning, at `openText` — the single site that swaps the session.

**The record was wrong about the shape of the fix.** It said **8** paths replace the
`ConfySession`, and concluded the reset "belongs next to a single session-replacement helper,
not at each site". There is **one** assignment in `ui.ts` (and one in `touch/app.ts`), both
already funnelled through `host-io.ts`'s `replaceSession`. The refactor the finding recommended
had already happened; only the reset was missing.

**Split out as F15:** touch has no `?diag=1` drain at all — `drainDiagIfEnabled` exists only in
`ui.ts`, so the trace is desktop-only even though both hosts drive the same ring. That is a
missing surface, not a defect in the drain, so it gets its own row rather than blocking this one.

`MESSAGES.md` §4.1's `?diag=1` row now records the reset and the reason for it; §8's entry is
replaced by the touch gap.

### Update - 2026-09-09 (28)

**The `~` diag overlay showed the ring's head; it now shows its tail — and the measurement found
something worse behind it**

F3. `draw_diag_overlay` collected the whole ring oldest-first, sized the box `min(len, 20)`, and
handed the **full** vector to a `Paragraph` — which renders from line 0. So past 20 events the
overlay clipped exactly the recent activity an operator opened it to see.

Real-binary evidence, identical keystrokes (`wwwwwwwwd~` — eight saves, then a delete that
fails):

| | before | after |
|---|---|---|
| title | ` Diagnostics ` | ` Diagnostics — last 20 of 25 ` |
| last visible line | `notice … "no changes to save"` (event 20 of 25) | `notice severity=Error … "delete error: path not found"` |
| the failure just triggered | **absent** — the overlay was byte-identical to the one drawn before the `d` | present, at the bottom |

The fix takes the tail before rendering, clamps the window to what the terminal can actually
show, and reports the window in the title so a clipped view is never mistaken for the whole
ring. An empty ring now says `(no events yet)` instead of drawing a two-row borders-only sliver.

**What the measurement exposed.** Getting to 20 events was harder than it should have been: 34
navigation keystrokes produced an **empty ring**. In a live TUI session the *only* Intent that
ever reaches `dispatch` is `SetHostNotice`, so every recorded event is the same fixed triple:

```
[Debug] dispatch SetHostNotice
[Info ] notice  severity=… source=HostTui text="…"
[Info ] mutation SetHostNotice ok
```

The diagnostic channel records *messages the host already displayed* and nothing else — no
navigation, no mutation, no mode change — because ~15-20 sites in `tui/app.rs` and `tui/mod.rs`
set `Session` fields directly instead of dispatching. A `dispatch` line that always names the
same Intent is not a trace. That is **F7**, whose priority is **raised P3 → P2** with this
evidence attached: this fix made a small window useful, it did not make the channel complete.

`MESSAGES.md` §4.1's overlay row now describes the tail window, and its §8 entry is replaced —
the old "shows the oldest 20" is fixed, the new one records what is actually wrong with the
channel.

Also seen, unrelated and not fixed: `d` on the Root row reports *delete error: path not found*.
The root is not missing, it is undeletable — the same class of wrong-variant message F1 cleaned
up for Remark. Noted for F8.

### Update - 2026-09-09 (27)

**The "recorded not fixed" observation gets a real home, in three places**

Closing F1 and F14 each surfaced a variant mismatch that was noted in a commit message and
nowhere else — which is exactly how a finding gets lost. Both are now written down where someone
would look for them:

- **`F8` in the backlog** carries the measured table. JSON reports an unterminated fragment as
  `Illegal("expected R_BRACE, found None")` where TOML reports `Fragment("unexpected token")`;
  the gesture-does-not-apply half (`Unsupported` vs `Illegal` vs `NotFound`) was unified by hand
  per backend in `72805c0`, which is precisely the manual work this finding exists to remove.
  Priority **raised P3 → P2**, with the reason recorded inline: it now has a measured,
  user-visible symptom rather than being an internal tidiness item.
- **`MESSAGES.md` §8** gains the entry from the *message* angle — one typo produces two
  different severity classes depending only on the file's format — and its header now tracks
  **F3/F4/F5/F8** instead of "all three".
- The **YAML caveat travels with it**: that row is not missing validation, its lexer accepts an
  unterminated scalar on load too, so `Replace` and load agree. Anyone tightening it is pointed
  at the backlog's *Watching* section first.

Explicit instruction in all three: **fix it in F8, not per-backend.** The last two commits show
what per-backend patching costs.

### Update - 2026-09-09 (26)

**A YAML fragment is one node, and the surplus is no longer thrown away**

F14, opened yesterday's-commit-ago while measuring the Remark parity work. The 定性 pass on it
**overturned half the filing** — worth recording, because the correction is the interesting part.

Filed claim: the YAML backend accepts `Replace` fragments its own grammar rejects, listing
`"unclosed`, `[1, `, `{a: ` and a two-entry `x: a\ny: b`. Measured against the **loader**:

| fragment | `Replace` | `from_str_as` (load from disk) |
|---|---|---|
| `x: "unclosed` | `Ok` | **`Ok`** |
| `x: [1, ` | `Ok` | **`Ok`** |
| `x: {a: ` | `Ok` | **`Ok`** |

The loader accepts all three too. The subset lexer is lenient **by design** — nearly any text is
a legal YAML plain scalar — so `Replace` and load already agree, and tightening one without the
other would produce a value you can open but cannot retype. Those three rows are **not a defect**;
they moved to the backlog's *Watching* section with that reasoning attached.

What is left is a real one, and it is **silent data loss**:

| fragment on `x: 1` / `z: 9` | before | after |
|---|---|---|
| `x: a` + `y: b` | `x: x: a` — corrupt **and** `y: b` gone | `Err(Fragment)` |
| `2` + `y: b` | `x: 2` — surplus gone, no message | `Err(Fragment)` |
| `[1, 2]` + `y: b` | `x: [1, 2]` — surplus gone | `Err(Fragment)` |
| `a` + `# c` | `x: a` — comment gone | `Err(Fragment)` |

`parse_value_fragment` extracted the first value node and never checked what followed it. It now
asserts the value covers the whole wrapped fragment; anything left over is
`Fragment("fragment must be a single value")`. Legal multiline values are unaffected — literal
`|`, folded `>`, and flow `{…}`/`[…]` all still replace normally.

**Reproduced on the real binary** with a fake `$EDITOR` writing two lines, identical keystrokes
(`9jE`, then `w`): before, `x: 1 / z: 9` became `x: x: a / z: 9` and the save succeeded; after,
the status line reads *invalid YAML: fragment must be a single value* followed by *no changes to
save*, and the file is untouched.

`BEHAVIOR_MATRIX.md` §8 gains the invariant — **a fragment is exactly one node** — and
`tests/format_parity.rs` gains a 10th behavior asserting it across all three formats. The
per-format fork the suite carried for this defect is **gone**: `1\n2` is now rejected by all
three, so the atomic-failure case runs one shared fragment as originally intended.

Also observed, recorded not fixed: JSON returns `Illegal` where TOML and YAML return `Fragment`
for the same class of bad input — the same variant confusion F1 cleaned up for Remark. Folded
into F8 (`MutateError` taxonomy) rather than patched per-backend.

### Update - 2026-09-09 (25)

**One gesture, three formats, one outcome — and a suite that keeps it that way**

`r` (Remark) behaved three different ways on the same node. Measured before touching anything:

| gesture | TOML | JSON | YAML |
|---|---|---|---|
| remark a **multiline array element** | `Unsupported` | `Illegal("cannot remark an array element")` | **works** |
| remark a **single-line collection member** | `Unsupported` | *succeeded, writing broken output* | `NotFound` |

Two backends disagreed on the *result*, and two more on **which error variant** means "this
gesture does not apply here" — and that variant is user-visible: `MESSAGES.md` §2 maps
`Unsupported` and `Illegal` to different severities. `NotFound` was simply false: every one of
those paths is addressable, as `Delete` and `Replace` on the same path prove.

**The rule now, in all three: Remark needs a line of its own.** It applies to any node that
occupies its own line(s) — a keyed member, an **array element**, a whole table — and
un-remarking restores the source byte-for-byte. It does not apply inside a single-line
collection, where a comment leader would swallow the siblings, nor to a read-only node; that
case is **`Unsupported`** everywhere. Written into `BEHAVIOR_MATRIX.md` §8.

That made array elements remarkable in TOML and JSON, matching YAML. Both formats already
*project* a comment inside an array as a first-class node, so only the two edit directions were
missing. Both are now text rewrites of the container (`array_rewrite_span` for TOML, the
existing `rebuild_multiline` for JSON) rather than token surgery — the neighbouring-padding and
indent-ownership traps in a multiline array are what made the direct splice not worth writing.
The element's `,` and its EOL comment travel *into* the comment line, so nothing is stranded
live. Un-remarking a merged block restores every element it holds.

**`tests/format_parity.rs`** is the new enforcement: 9 behaviors — remark round-trip, the
single-line rejection, addressability, delete, rename, replace, trailing comment, lossless
round-trip, atomic failure — each run against **all three** `DocFormat`s in one loop. Per-format
fixtures are chosen by an **exhaustive `match`**, so a fourth backend cannot compile until its
expectations are written down. Verified to fail before this change (2 of 9) and pass after.

This is the shape the existing multi-format tests were missing: `external_edit_clears_trailing_
comment.rs` loops Json+Toml with Yaml in a block below, `insert_after_trailing_comment.rs` loops
Toml+Yaml with Json below — and a format handled outside the loop is exactly where drift hides.

Verified on the **real binary** with identical keystrokes (`9jjr`, then `r` to restore) against
one fixture per format: before, TOML reported "operation not supported by this format"; after,
all three remark and restore byte-for-byte.

**Found while measuring, recorded not fixed:** the YAML backend does not validate a `Replace`
fragment at all. `x: 1` accepts `"unclosed`, `[1, ` and `{a: ` verbatim, and a two-entry
fragment collapses to a corrupt `x: x: a`, losing the sibling. Only a literal tab is rejected.
Tracked as **F14** in the backlog with the full table; the parity suite's one per-format fork
marks the spot.

### Update - 2026-09-09 (24)

**Docs — every follow-up now has one home**

New living record [`docs/plan/2026-09-09-open-follow-ups.md`](docs/plan/2026-09-09-open-follow-ups.md):
**13 open items (F1-F13)** plus a *Watching* section and a *Done* log. Each row carries its
evidence (file + symbol, never a line number), the date it was verified against current code,
priority, effort, and an **acceptance criterion** — so picking one up does not start with
re-deriving what it means.

The problem this solves: the open work was scattered across a frozen audit, a frozen
re-verification record, and a `MESSAGES.md` footnote. Frozen records are the right form for
*evidence*, and the wrong form for a *backlog* — nothing can be ticked off in them, so an item's
status was only ever knowable by re-reading code. This record is therefore the **one exception**
to the freeze-on-landing rule in `docs/plan/`: rows move to *Done* with the commit that closed
them, and are never deleted, so the history of what was open stays readable. `CONTEXT.md` states
that rule at the top level, and `MESSAGES.md` §8 and `docs/audit/README.md` now point at it
rather than being the de-facto tracker.

**The `Approved` JSON/JSONC parser-simplification plan is closed** as `Shipped (2026-09-09)`,
with the reason recorded in the document itself: the premise was **refuted, not abandoned** —
both halves were already done (the comment write-gate is gone; the "unify two parsers" half had
no work because `model/json/parse.rs` is the only parser). Left `Approved` it read as
agreed-but-unstarted work. Live work across all four folders is now exactly one record: this
backlog.

Two items were opened by today's own work rather than inherited: **F10**, the remaining `Move`
cost (the capture phase and `insert_with` traverse *through* an index by design, so they still
pay the 11-17× mutable-tree penalty — removing it needs a `CstIndex` of paths rather than live
handles), recorded so the ceiling is known rather than rediscovered; and the *Watching* note
that `JsonDocument` does not override `rename_key_segs` — not a live defect, since JSON keys are
always quoted and the trait default coincides, but the last asymmetry in that area.

### Update - 2026-09-09 (23)

**Perf — the two P0s from the 2026-08-29 audit**

**1. One serialize per mutation instead of two.** `apply` already returned the new text and
`on_mutation_success` already pushed it into `History`, but `sync_schema_hint()` →
`detect_and_request_schema()` then re-serialized the whole document just to hand the text to
`schema::hints::detect_hint` — on every mutation, whether or not the document has a hint at
all. `sync_schema_hint` now takes `&str`, and the new `detect_hint_in(text)` does the detection;
all three call sites already held the text (a mutation's `apply` output, or the snapshot
undo/redo just restored). The public `detect_and_request_schema()` is unchanged for hosts that
have no text in hand. Removes a full document serialize from every keystroke.

**2. `Move` de-quadraticated — and the audit's root cause was wrong.** Profiling the audit's
own benchmark first, because the arithmetic did not add up: three CST walks at 5.5 ms cannot be
96% of a 527 ms move.

| phase of `Move ×1` @ 7,001 nodes | time |
|---|---|
| first `walk` | 5.5 ms |
| capture + anchor | 143 ms |
| delete | 195 ms |
| per-fragment `walk` (×1) | 85 ms |
| insert | 83 ms |

An 85 ms walk — the *same* `walk(tree, "")` that had just cost 5.5 ms, with nothing changed
between them. The isolating experiment:

| `walk(tree, "")` @ 7,001 nodes | time |
|---|---|
| nothing else alive (×3, consecutive) | 5.58 / 5.59 / 5.59 ms |
| a previous walk's index still in scope (×4) | 97 / 98 / 97 / 97 ms |

Not a warm-up effect and not amortized — **every** traversal under a live index pays it. Cause,
confirmed in `rowan::cursor`: `clone_for_update` yields the *mutable* representation, where a
parent locates a child by scanning its **live** children (a sorted linked list of `NodeData`).
A `CstIndex` holds one live `SyntaxElement` per node, so with a whole-document index in scope
every child lookup degrades to a list scan and traversal becomes quadratic.

So the fix is not "fewer walks" — it is **never traverse a mutable tree while a whole-document
index is alive**. `cst_edit::move_nodes` and `cst_edit::delete` now `drop` their `(proj, idx)`
the moment the owned data they need (fragments, spans, anchor path) is extracted, before any
splice:

| TOML `Move` @ 7,001 nodes | before | after | |
|---|---|---|---|
| 1 source | 527 ms | **275 ms** | −48% |
| 4 sources | 2.09 s | **1.04 s** | −50% |
| 8 sources | 4.18 s | **2.22 s** | −47% |

Two consequences worth recording. **JSON needed no change**: its `project()` keeps only the
owned `NodeTree` and `resolve()` discards its index on return, so it was never exposed — the
audit's "same superlinear shape in the JSON and YAML `move_nodes`" was pattern-matching on
call-site counts, not on the mechanism. And YAML's 27× lead over TOML has the same explanation
as the fix: it reuses one index rather than overlapping several. The remaining TOML cost is the
capture phase and `insert_with`, both of which traverse *through* an index by design;
eliminating those needs a structural change (an index that stores paths rather than live
handles), not another `drop`. The invariant is now written down in `docs/reference/MUTATIONS.md`
§ *Mutation mechanics*, since it is the kind of rule that silently regresses.

`Replace` and `Rename` are unchanged at 14.7 ms — the same `drop` was tried there, measured no
difference (their splice is one token, not a section), and reverted rather than left in.

Verified on the real binary, not just the bench: a cut-and-paste `Move` of `[b]` into `[a]`
produces the correct `[a.b]` and preserves the `#:schema` hint line, and `z` restores the
original byte-for-byte — exercising the threaded `sync_schema_hint(&snapshot)` path.

### Update - 2026-09-09 (22)

**Docs — re-verify every open finding, with fresh measurements**

New record [`docs/audit/2026-09-09-open-findings-reverification.md`](docs/audit/2026-09-09-open-findings-reverification.md):
a per-finding triage of everything still recorded as open — the 21 items of the 2026-08-29
architecture audit, the three `MESSAGES.md` §8 follow-ups, and the one `Approved`-but-unshipped
plan. Four read-only verification passes (verdict only where a symbol could be pointed at) plus
fresh `cargo bench -p confy-core --bench perf` runs at the audit's own document sizes.
No code fixed; the point is that the next work session starts from measured ground truth.

**22 findings: 8 FIXED, 5 PARTIAL, 9 OPEN.** Both headline P0s are still open — and both are
*differently* open than recorded:

- **`Move` is quadratic — worse than the audit measured.** Every one of its numbers is
  exceeded: at 7,001 nodes a single-source `Move` is **527 ms** (audit: 752 ms was the *upper*
  bound at that size) and an 8-source move is **4.18 s**; at 98,001 nodes one move is
  **101 s**. 14× the nodes costs 36× a `Replace` but **192×** a `Move`. The decisive new datum
  the audit never took: **YAML's `move_nodes` is already 53× faster** than TOML's at 7k nodes
  (79 ms vs 4.18 s for 8 sources), because it threads one `walk` result through the opaque
  check, the capture and the shift. TOML does `1 + |S| + |F|` full walks, JSON `2·|S| + 1`.
  So the fix needs no design — YAML is the working in-repo reference, and its margin is the
  measured target. This also downgrades the audit's "one shared fix for three backends":
  YAML is largely done.
- **The double serialize per keystroke is still there, via a different route.** The audit's
  exact claim *is* fixed — `apply` returns the text and `on_mutation_success` pushes that very
  string into `History`, so the undo snapshot no longer serializes. But
  `sync_schema_hint()` → `detect_and_request_schema()` calls `doc.serialize()` unconditionally
  on every mutation, purely to hand the text to `detect_hint` — even when the document has no
  schema hint. 16.4 ms per keystroke at 98k nodes. The fix is now XS: `on_mutation_success`
  already holds the text; pass it in. Both other callers (in `undo_redo.rs`) also have it.

Cross-backend drift: the empty-document insert and the JSON quoted-key rename are **fixed**
(JSON now detects an already-quoted key instead of blind-wrapping, and compares decoded against
decoded). **Remark-an-array-element is not** — still `Unsupported` (TOML) / `Illegal` (JSON) /
`Ok` (YAML), one gesture and three outcomes, two of which disagree about which error variant
means "does not apply here" (a user-visible severity difference). The parity test gap is
`PARTIAL` in the shape that matters: two files now iterate formats, but each leaves the third
format in a separate block below the loop — exactly what lets drift through.

Also verified fixed since the audit: `ViewRow.badge_label`/`badge_note` (so `kind-labels.ts` no
longer re-derives badges and `help-content.ts` reads the i18n catalog), `to_view_row`'s
per-row allocations, web CI running both the wasm smoke and the web specs, a `cargo audit` job,
`proptest` round-trip properties for all three backends, and the inline-test extraction for
three of the four "god objects" (`json/edit.rs` is the one left, 2,864 lines and unsplit).
Still open: `Intent` bypass (~15-20 sites), `String` undo snapshots, `MutateError` mixing
interactive outcomes with real errors, and `CHANGELOG.md` — which has **grown 42%** since being
flagged (482 KB / 4,130 lines); the trend is the finding, not the size.

`MESSAGES.md` §8 sharpened with what verification turned up: the TUI `~` overlay goes blind
after **7 interactions with notices, 10 without** (a dispatch emits 2 events, 3 with a notice),
and the defect is TUI-only. The web `lastSeenSeq` reset has **8** session-replacement paths, so
it belongs in one helper rather than at each site, and touch has no drain at all. Touch's
`sev-*` toasts differ only by a 3000 ms vs 1600 ms timer — while `web/style.css` *does* tint
`sev-warn`/`sev-success` for the desktop footer, so the two hosts disagree on whether severity
is visible at all.

The `Approved` JSON/JSONC parser-simplification plan's **premise is refuted**: both halves are
effectively done (the comment write-gate is gone, and the "unify two parsers" half has no work
because `model/json/parse.rs` is the only parser). It should be closed rather than left reading
as agreed-but-unstarted work.

### Update - 2026-09-09 (21)

**Docs — full audit and reorganization**

A repo-wide documentation audit (six parallel read-only passes over every reference doc,
`CLAUDE.md` and the root files) followed by a structural reorganization onto the
`wens-dev-principles` **docs** domain. No runtime behavior changed; one stale test invariant
was tightened.

**Structure** (was violating four `MUST` principles):

- New root **`CONTEXT.md`** — the single documentation entry point: one table of the `docs/`
  folders (what each holds, whether it is canonical, its lifecycle), the reading order, and the
  greppable status-line contract.
- `docs/superpowers/{specs,plans,audits,debug}/` → **`docs/{spec,plan,audit,debug}/`**, each with
  its own indexing `README.md` listing every document with a one-line summary and status, live
  work in an `## In progress` section at the top. 60 files moved with `git mv`; the 178 internal
  links and 48 source-comment path references were swept to match. `CHANGELOG.md` entries written
  before today keep the old paths and are left alone — they are frozen history.
- **`docs/reference/CONTEXT.md` split** into [`glossary.md`](docs/reference/glossary.md) (the
  vocabulary, in the fixed `**Term**:` / definition / `_Avoid_:` entry format) and
  [`MUTATIONS.md`](docs/reference/MUTATIONS.md) (insert/move legality, per-`Mutation` mechanics,
  `e` block-edit scope, multiline-array layout, kind-switch rules). The old name collided with
  the mandated root index, and the file was two documents in one.
- The **condensed duplicate** of the nested-behavior matrix is gone; `BEHAVIOR_MATRIX.md` is the
  only copy. Two copies of one matrix drift.
- `RELEASES.md` → `docs/reference/RELEASES.md` (the repo root now keeps only `README.md`,
  `CHANGELOG.md`, `CLAUDE.md`, `CONTEXT.md`, `LICENSE`, `PRIVACY.md`).
  `scripts/sync-releases-md.sh`, which CI runs on every release to patch the version column,
  was updated to the new path — it would otherwise have failed the next tagged build.
- `docs/reference/PORTING.md` → `docs/spec/2026-06-17-headless-core-port.md`. It is a design
  record with completed-milestone tracking, a superseded API sketch and a "pre-port surface"
  inventory — reference describes current behavior only.
- **51 emoji status banners → greppable `Status:` lines** as the second line of every working
  record, from the fixed value set (`Shipped (date)` / `Resolved (date)` / `Approved` /
  `In progress`). Six records had no status marker at all; each now carries one. The prose facts
  those banners carried (landing commits, review pointers, supersessions) were preserved.
- Six working records renamed to the `YYYY-MM-DD-kebab-title.md` convention (`2026-04-XX-…`
  had no real date; four were `SHOUTY_PLAN.md`). `docs/debug/2026-09-01-pointer-drop-pasteslot-probe/`
  gained the sibling `.md` that a script directory must hang off.
- `docs/audit/2026-08-29-code-audit.md` was sitting unindexed in `docs/tmp/`; it is now a real
  audit record — and, on inspection, an **honest `In progress`**: its quadratic-`Move` P0 and its
  cross-backend-parity P1 are still open (the JSON empty-document insert it flagged *is* fixed).
- Stale `docs/tmp/` scratch archived to `docs/tmp/archive/2026-09-pre-reorg.tar.gz`.

**Accuracy** — every finding verified against the code before editing:

- **`MESSAGES.md` §2 severity counts were wrong and self-contradictory** ("43 keys (11 Error +
  15 Warn + 7 Success + 9 Info)" — that sums to 42, then 43). `severity_of` actually classifies
  **68** keys: 45 `core.*` notice keys (12 Error + 17 Warn + 7 Success + 9 Info) and 23 host
  keys. The doc also conflated notice keys with the 102 `core.*` catalog keys, most of which
  would *panic* if passed to `severity_of`. Both facts corrected.
- **The invariant test that was supposed to prevent exactly that had drifted too.**
  `severity_of_covers_the_full_catalog_table` asserted `cases.len() == 43` while `severity_of`
  had grown to 45 `core.*` arms — `core.blank.error` and `core.action.unavailable` were
  classified but never asserted, so the test's name was a lie. Added both cases, corrected the
  count and rewrote the assertion message to state the real breakdown and its own scope.
- `MESSAGES.md` §4 claimed five `DiagEvent` kinds; there are **three** (`dispatch`, `mutation`,
  `notice`) and exactly three `diag.push` call sites. `schema` and `convert` are never emitted,
  and a host notice is not a separate `host_notice` kind — it lands in the same `set_notice` and
  records as `notice` with its provenance in the `source=` field.
- **New known follow-up recorded** (`MESSAGES.md` §8): the TUI `~` diag overlay renders the ring
  oldest-first into a box sized `min(len, 20)`, so past ~7 interactions it shows the *oldest* 20
  events and clips the newest — the opposite of what its docstring claims and of what an
  operator opens it for. Documented, not silently reframed as intended behavior.
- `glossary.md`: the `(B)/(Q)/(D)/(-)` key-sign prefix was described as a KIND-column prefix; it
  is a Type-filter facet and a Detail-popup `Sign:` line, and the KIND tag is strictly the
  8-cell type slot. `Format`'s "eventual format-toggle operation" has shipped as `K`/
  `ConvertKind`. `Scalar` was defined as TOML-only (no `null`, no JSON/YAML). The worked
  paste example asserted the wrong destination — pasting after an *expanded* branch inserts as
  its first child, not as a sibling.
- `MUTATIONS.md`: `SetTrailingComment` was missing from the `Mutation` table entirely and
  `ConvertKind` had no row; the multiline-array section still said "three rules" after the
  fourth landed; `aot_entry_end` is `aot_entry_end_from`; the kind-switch table listed only
  TOML's targets, with no JSON or YAML rows.
- `TUI.md`: `edit_node` was described as truncating a path at the enclosing array — it keeps the
  full path and flags an unaddressable bare element for wrapping instead. `a` was described as
  directly inserting a sibling of the cursor's kind; it opens `Mode::AddPicker`.
- `KEYMAP.md`: only three row families are surface-prefixed (`tui.help.row.filter_lock`,
  `tui.help.row.convert_jsonc_toggle`, `web.help.row.pointer_*`) — `l`, `~` and `Ctrl+o` use
  shared `help.row.*` keys. The TUI Detail popup is a centered floating box, not full-screen.
- `WEBUI.md`: `schemaInfo` returns `string | undefined` (no `SchemaInfo` type); `schemaViolations`
  returns `ViolationView[]`; the panel's Actions row was removed by ADR 0009; `wirePanel`'s
  signature had drifted; `nudgeRepr` was missing from the FFI table; `SessionSnapshot` (21
  fields), `ModeView` (`AddPicker`) and `ViewRow` (`badge_label`/`badge_note`) lists were
  incomplete; five shared UI modules were unmentioned.
- `ROW_STATE_MODEL.md`: §3's visual table still presented the **pre-Phase-1** world as "Current"
  and the shipped design as "Target", years after Phase 1 landed — rewritten against the actual
  CSS and `tui/ui.rs`. §8's all-`[x]` checklist was replaced by a pointer to the five frozen
  phase plans, with the two behavioral facts that lived only inside it moved into §5. One test
  name was wrong (`remark_selection_tracks_scattered_rows`), as was the `Enter` keybinding row.
- `README.md` advertised an `x86_64` macOS desktop `.dmg`; CI has built Apple Silicon only since
  v0.12.2.
- **41 rotting `file.rs:123` line-number citations** stripped from `docs/reference/` (37 of them
  in `ROW_STATE_MODEL.md` alone, most already pointing at the wrong line). File and symbol names
  are stable; line numbers are not.

**`CLAUDE.md`** slimmed from 766 to ~526 lines: the ~280-line Architecture narrative — which
restated `glossary.md`, `MUTATIONS.md`, `TAURI.md` and `MESSAGES.md` almost paragraph for
paragraph — became a one-paragraph orientation plus a topic→document table, per
`wens-dev-principles docs 3` (an instruction file states conduct and points at reference; it
does not become a second, drifting copy of it). What stayed: build/test commands, release
process, known risks, the module map, terminology. Also corrected there: the taplo call-site
counts (48 / 28 / 2, not 47 / 18 / 2), the claim that **no** taplo DOM is used (`Node::validate`
*is* — it is the TOML duplicate-key backstop), a `load_as` function that does not exist, the
`functional_smoke.mjs` check count (129), the VS Code extension's publish status (it contradicted
`RELEASES.md`: it is on the Marketplace and Open VSX), and a test inventory missing 11 files.

### Update - 2026-09-08 (20)

**Fixed**

- **The first element added to a TOML array holding only comments landed in column 0.**
  `a = [`⏎`  # only`⏎`]` + one element gave `  # only`⏎`0]`. `array_insert`'s no-elements branch
  spliced the value bare before the `]` — correct for `[]`/`[ ]`/`[`⏎`]`, but a comment-holding
  array already ends in a NEWLINE, so the element started a fresh line with no indent. When the
  element before the `]` is a NEWLINE, the value is now preceded by the indent of the array's
  last comment line (`array_comment_indent`), so the first element joins that column:
  `  # only`⏎`  0]`. A flush `#` asks for no indent and gets none; genuinely empty arrays are
  unchanged.

This clears the last rough edge recorded for multiline-array element editing; `CONTEXT.md`'s
*Multiline-array layout* section now lists four rules and no open cases.

### Update - 2026-09-08 (19)

**Fixed**

- **Deleting a standalone comment line inside a TOML array doubled the next element's indent.**
  The comment's delete span started at the `#`, so the `WHITESPACE` that indented that line
  survived and stacked on top of the following element's own indent: `a = [`⏎`  # lead`⏎`  1,`
  came back as `  ` + `  1,` = `    1,`. A span that owns a whole line now takes that line's
  indent with it (`retract_over_line_indent`, the head-side mirror of `extend_over_newline`),
  which also fixes the same comment in the middle and tail positions, a multi-line `#` block, and
  a comment that is the array's only child. Unindented comments at root and table scope are
  unaffected, and a *trailing* comment's separating space is explicitly out of the span.
- **Appending to an array with padding before its `]` put the comma behind that padding.**
  `a = [ 1 ]` + one element became `a = [ 1 , 0]`. taplo bakes that padding *into* the last
  element's `VALUE` node, so the append landed after it; the padding is now detached and
  re-emitted after the new element, giving `a = [ 1, 0 ]`. Covers `[ 1, 2 ]`, a tab pad, and a
  multiline array whose `]` shares the last element's line.

One rough edge remains and is recorded in `CONTEXT.md`: inserting into an array whose only child
is a comment appends at column 0.

### Update - 2026-09-08 (18)

**Fixed**

- **A TOML multiline array lost its layout on every element insert.** `array_insert` always
  spliced the single-line `, ` separator, so adding an element to `a = [`⏎`  1,`⏎`  2,`⏎`]`
  produced `  2, 0,` — the new element landed on its neighbour's line, and repeated adds
  progressively collapsed a one-element-per-line array. The separator is now the array's *own*
  measured layout (`array_element_lead`): `,` plus the trivia found in front of its existing
  elements, so 2-space, 4-space and tab indents all come back verbatim, a per-element EOL comment
  stays on its own line, and a single-line array still gets `, `.
- **Deleting the last element of a trailing-comma multiline array stranded its indent.**
  `a = [`⏎`  1,`⏎`  2,`⏎`]` became `a = [`⏎`  1,`⏎`  ]` — the cut took the comma after the
  element, leaving that element's leading whitespace in front of the `]`. The span now retracts
  back over that whitespace (stopping at the newline, which the `]` still needs), and only when
  nothing else still owns the indent — a surviving element, or the deleted element's own EOL
  comment, keeps its column.

Two rough edges remain in the same area and are now recorded concretely in `CONTEXT.md` instead of
as one vague note: deleting an array's leading standalone comment doubles the next element's
indent, and inserting before a `2 ]`-style inline close writes `2 , 9`.

### Update - 2026-09-08 (17)

**Fixed**

- **A YAML flow item's projected value carried the padding before its delimiter.** The lexer's
  plain-scalar token runs to the `,`/`]`/`}`, so `g: [ 1, 2 ]` projected the second element as
  `"2 "` and `g: [ a , b ]` projected both as `"a "`/`"b "`, while the unpadded `g: [1, 2]`
  projected `"2"` — `classify_scalar_token` already trimmed for *type detection* but kept the raw
  text as the repr. The one user-visible consequence was that **filtering depended on the author's
  spacing**: a needle ending in a space matched the padded spelling and not the unpadded one (and
  the per-char highlight marked the invisible trailing space). Block style, quoted tokens, TOML and
  JSON were never affected. Decoding (`to_value`/convert/schema), type detection, the inline
  editor's buffer, `serialize_fragment` and the `←`/`→` nudge all trimmed or bypassed the repr
  already, so nothing else changed — and serialization concatenates CST tokens, so round-trip is
  unaffected.

### Update - 2026-09-08 (16)

**Fixed**

- **`e` on a read-only node opened the editor anyway.** Only the `$EDITOR` route checked
  `Node.read_only`; `e` on a one-line value routes to the **inline** editor, which didn't — so a
  YAML opaque span or a JSONC `/* */` block comment let you type, then failed at commit with a
  parser-level `invalid value: expected ':', found Some(NEWLINE)`. The guard now lives in
  `Session::begin_inline_edit`, i.e. in core, so every host (TUI, web, touch, VS Code) refuses
  before an editor opens and reports the same read-only message the `E`/`d`/`x`/`r` paths do.
- **The read-only rejection no longer mislabels its source.** One hard-coded
  "read-only node (block comment)" served both sources of the flag, so a YAML anchor/alias/merge/tag
  was reported as a block comment. `core.readonly` is split into `core.readonly.comment` (JSONC
  block comment) and `core.readonly.opaque` ("read-only node (out-of-subset YAML:
  anchor/alias/merge/tag)"), picked by node kind in `Session::readonly_notice_key`; the TUI's
  now-redundant host duplicate `tui.host.readonly-comment` is gone.

### Update - 2026-09-08 (15)

**Fixed**

- **Pasting a comment into a YAML flow collection destroyed it.** `find_container` resolves a flow
  parent to the `FLOW_SEQ`/`FLOW_MAP` itself, and `insert_comment` then handed its `[`/`,`/`]`
  tokens to the block-item collector: the rebuild emitted the comment *instead of* the collection
  and **reported success**. Copying a comment and pasting it into `g: [ 1, 2 ]` (answering `y` to
  "single-line array — reformat to multiline and insert?") saved `g: # hi` — the array silently
  gone. YAML's `InsertComment` now rejects a flow container with `Unsupported`, and the paste
  destination check classifies a YAML flow sequence as illegal-for-comments instead of offering an
  upgrade prompt only TOML/JSON can honor, so the message is
  "comments can only go into a table or the document" and the document is untouched. (`K` converts
  the sequence to block layout first if you want the comment there.)
- **An anchor/alias/tag inside a one-line flow collection is no longer silently dropped.** The flow
  body parser has no case for those tokens, so they floated as bare tokens no projected node
  covered: `g: [ &a 1, 2 ]` showed a plain `1` with the anchor invisible, and `g: [ *a, 2 ]` showed
  a **single** element `2` — the alias missing from the tree and every later element's ordinal
  shifted, so editing "element 1" hit the wrong item. Such a collection is now fenced as one
  read-only **opaque node** (`[opaq ]`, whole `[ … ]`/`{ … }` shown verbatim), the same call the
  block level already makes for an anchored value: it renders and copies, every mutation answers
  `Unsupported`, and the file round-trips byte-identically.

### Update - 2026-09-08 (14)

**Fixed**

- **A nested flow collection used as a YAML flow-seq element is addressable.** `g: [ {x: 1}, 2 ]`
  index 0 (and `g: [ [1, 2], 3 ]`) had **no projected `Target` at all** — `walk_flow_seq` registered
  one for every scalar element but not for a nested-node element — so the resolver found nothing:
  the multiline editor opened on an empty buffer and `Replace`/`Delete`/`Move` all reported
  `✗ error: path not found`. The projection now registers the same ordinal-addressed
  `Target::Element(<the whole FLOW_SEQ>)` a scalar element gets (the edit layer's item spans already
  counted nested collections in that order), so the element's fragment is the collection itself, an
  untouched round trip is byte-identical, and a real edit changes only it
  (`g: [ {x: 9, y: 8}, 2 ]`). CLAUDE.md's "each member is individually addressable/editable" claim
  for nested YAML flow values now actually holds for the seq case.
- **A bare flow-map fragment is a value, not a member keyed `{x`.** `key_colon` scanned for the
  key/value colon at quote depth but not at *flow* depth, so `{x: 1}` looked keyed and inserting it
  produced `Illegal("expected a mapping key, found Some(L_BRACE)")` — which is what moving a nested
  flow map out of its sequence hit once it became addressable (a flow **seq** element `[1, 2]`, having
  no colon, was unaffected). Colons inside `{…}`/`[…]` are now skipped, mirroring
  `split_top_level_commas`, so `Move` synthesizes `g_0: {x: 1}` correctly.
- **`ConvertKind` cannot retarget a flow seq through one of its items.** A flow-seq item shares the
  whole `FLOW_SEQ` as its target, so `resolve_value_node` would have handed the *parent sequence* to
  a conversion requested on the item (previously masked for scalar items by an accidental
  `NotFound`); it now answers `Unsupported`, matching `kind_options`, which offers a one-line item no
  block layout.

### Update - 2026-09-08 (13)

**Docs**

- **The behavior matrix's "own external precise edit" rows were stale.** Tables A and C read
  `⚠ whole repr` for both flow parent scopes (seq-flow / map-flow) — a claim that already
  contradicted §6.3 ("captures and Replaces just the edited node in every backend") and is now
  measurably wrong in all six combinations: a TOML inline-array element / inline-table member, a
  JSON array element / object member, and a YAML flow-seq element / flow-map member each capture
  that item alone and round-trip byte-identically. Corrected in `BEHAVIOR_MATRIX.md` and its
  `CONTEXT.md` copy, with the per-item capture rule spelled out in §6.3, and pinned by
  `external_edit_of_a_flow_item_is_precise_in_every_backend`.
- **One residual gap recorded** (found by that survey, not fixed): a nested flow collection used as
  a YAML flow-**seq** element (`g: [ {x: 1}, 2 ]`, index 0) is indexed by neither the scalar-element
  nor the member rule, so its fragment is empty and every mutation on it returns `NotFound` — the
  document is untouched and nothing is truncated. Reached through a *key*
  (`f: { n: {x: 1} }`) it is precise. Table A note ³, and pinned by
  `a_nested_flow_collection_as_a_seq_element_is_unaddressable`.

### Update - 2026-09-08 (12)

**Fixed**

- **A YAML flow-seq element's fragment is the element, not its collection.** A flow-seq *scalar*
  element has no `Target` of its own — the projection indexes it as `Target::Element(<the whole
  FLOW_SEQ>)`, since every edit needs the collection plus an ordinal — and `fragment_of` returned
  that target's whole text. So every fragment capture over-captured: opening element 0 of
  `g: [ 1, 2, 3 ]` in the multiline editor handed over `[ 1, 2, 3 ]` and saving nested the
  collection into its own element (`g: [ [ 1, 2, 3 ], 2, 3 ]` — data loss); copying one element and
  pasting produced a whole nested sequence; a `Move` of one element re-inserted the entire
  collection in its place. `fragment_of` now takes the target's path and, for a `FLOW_SEQ`, slices
  out the item at the path's ordinal (`flow::flow_item_text`, trailing whitespace excluded), so the
  buffer is `1`, an untouched round trip is byte-identical, a real edit changes only that element,
  and copy/`Move` carry the element alone. Flow *map members* were never affected (their target is
  the `FLOW_ENTRY`).

### Update - 2026-09-08 (11)

**Fixed**

- **A YAML flow collection keeps its own inner spacing through an edit.** Editing the last member
  of `{ a: 1, b: 2 }` committed `{ a: 1, b: 2}` — an untouched multiline-editor round trip was not
  byte-identical, breaking the lossless promise. Two causes: a plain scalar token swallows the
  spaces before the closer (`b: 2 ` *is* the member's range), so splicing over the raw range ate
  the padding; and `rebuild_flow` — the path every delete/insert took — re-emitted a canonical
  `{a, b}`, discarding the author's padding and separator style (`{ a: 1, b: 2 }` → `{a: 1}` on a
  delete, `[ 1,2 ]` → `[1, 2 ]` on an element edit). Now a *replace* splices over the
  member's/element's own trailing-whitespace-excluded span (untouched ⇒ byte-identical, a real
  value change keeps the padding), and a *rebuild* re-emits the spacing it measured from the source
  (`flow_style`): the padding after the opener, before the closer, and the member separator — so a
  tight `{a: 1,b: 2}` stays tight, `[ 1, 2, 3 ]` stays padded, and emptying a collection invents no
  padding (`{}`).

**Notes**

- Found while verifying the above, **not** fixed here: a YAML flow-**seq** element's
  `serialize_fragment` returns the *whole* `[ … ]` (the element's resolver target is the
  collection), so opening one in the multiline editor and saving nests the collection into that
  element (`g: [[ 1, 2, 3 ], 2, 3 ]`); copy over-captures the same way. Predates this work and is
  unrelated to the padding. Recorded as a known rough edge in CONTEXT.md.

### Update - 2026-09-08 (10)

**Fixed**

- **A blank run the node's own span already swallowed is no longer deleted on edit.** Editing a
  YAML block map/sequence entry (`m:` / `s:`), a YAML literal `|` or folded `>` scalar, or a JSON
  `//` comment block removed every blank line after it — pulling the next node up — even when the
  buffer was handed back untouched. Those spans reach *past* their trailing blanks, so the anchor
  landed after the run: `count_after` reported 0, the editor packaged nothing, and the node's own
  `Replace` overwrote the lines. `blank_lines::anchor_at` now **retracts the anchor back over the
  run** in every backend (TOML's section extent already did this locally), so the run is visible,
  packaged, and restored. One rule, one place — the count, the mutation and the editor buffer read
  the same offset.
- **A whole-document edit no longer strips the file's trailing blank lines.** The root packages no
  run (there is no node after it to anchor one), and trimming the blanks out of the buffer into a
  count the commit then had no anchor to restore deleted them outright. A node that cannot carry a
  run — the whole-document path, an inline-collection member, a YAML opaque span — now packages its
  fragment **verbatim**: nothing trimmed, no terminating newline invented, so an unterminated file
  also round-trips byte-identically.
- **A member of a single-line `{ … }` / `[ … ]` no longer claims its container's blank run.** Every
  member of `t = { a = 1, b = 2 }` reported (and would rewrite) the blank lines following the whole
  line. The new shared `blank_lines::owns_line_tail` rule — a node carries a run only if nothing
  but its separator comma, whitespace or a trailing comment follows it on its line — makes those
  members `Unsupported`, which is what YAML's flow-map members already were. The hosts' read-only
  "Blank after" readout shows nothing there instead of a number belonging elsewhere.
- **A comment block's run now follows its last line, not its first.** `# a` / `# b` project as one
  Comment node; with the anchor after `# a`, `# b` was not blank so the node reported no run at
  all. TOML resolves the block through the same `comment_block_range` its `EditComment` uses, JSON
  through a new `comment_block_end` mirroring `comment_block_text`'s walk.

**Notes**

- Reachability, for the record: **every** node kind can open the multiline editor and therefore
  edit its run — `E` (`BeginEditExternal`) on the keyboard hosts, Action-menu *Edit in editor*
  everywhere. A child scalar is not special; `e` on a single-line scalar remains the *inline*
  editor (BEHAVIOR_MATRIX §6), which edits the value only and leaves the run untouched.
- The run at a branch's end belongs to its **last child and the branch alike** — both anchors are
  the same contiguous extent end, so the same run is editable from either, which is the intended
  ownership model and is now pinned by a test.
- Found but **not** fixed here (pre-existing, unrelated to blank runs): a `Replace` on a YAML
  flow-map member rebuilds the `{ … }` without its closing padding, `{ a: 1, b: 2 }` →
  `{ a: 1, b: 2}`. A bare `Mutation::Replace` reproduces it with no editor involved. Recorded as a
  known rough edge in CONTEXT.md.

### Update - 2026-09-08 (9)

**Changed**

- **A node and the blank lines after it are now edited as one package in the multiline editor —
  and that is the only way to change them.** Opening the editor on any node (TUI `$EDITOR`,
  web/touch pop-up, VS Code) shows its trailing blank lines as literal empty lines at the end of
  the buffer; add lines to grow the run, delete them to remove it. The two Action-menu items
  **Add a blank line after** / **Remove a blank line after**, `Intent::SetTrailingBlank`,
  `ActionId::BlankAdd`/`BlankRemove` and the `PromptKind::BlankReparent` confirmation are all
  **removed**: stepping a hidden counter ±1 from a menu was blind and needed a prompt to explain
  what it was about to do, where the buffer simply shows it. The read-only **Blank after**
  readout (TUI `i` Detail popup, web/touch panel) stays.
- Behavior before this change was inconsistent per backend and one-directional: the buffer never
  contained the run, and a returned buffer's stray trailing newlines landed inside a TOML
  section's `Replace` span (so blanks could be *added* after a table) but were dropped entirely
  for a scalar entry (whose `Replace` only swaps the value token). Removing a blank line from the
  editor was impossible in every backend.
- The buffer now has exactly one producer for every host,
  `Session::multiline_edit_initial(path)` (fragment + `trailing_blank_lines`, via
  `blank_lines::with_trailing_run`); the TUI no longer builds its own `$EDITOR` initial, and the
  keyless-element wrap (`scalar_fragment(None, …)`) moved into core's `apply_external_replace`,
  which now takes `wrap_element` — it has to run *after* the blank split or it would eat the run.
- The node's own splice runs **first**, the blank run second (a TOML section's extent swallows
  its separator blanks, so only a later pass can normalize them), and both fold into a **single**
  `on_mutation_success` — one undo step for the whole package. An untouched buffer applied back
  is byte-identical; an invalid fragment leaves the document, blank run included, untouched.
- Comment nodes go through the same package: the blanks are split off before
  `Mutation::EditComment`, which would otherwise splice them *inside* the comment block — where a
  blank line breaks it into two projected nodes.

**Notes**

- This supersedes the "Rejected alternative: carrying blank lines in the `$EDITOR` buffer" note
  in *Unreleased Update - 2026-09-07 (4)*. All three of its objections resolve: the multiline
  editor opens for **any** node, not just containers and multiline scalars (`E` /
  `BeginEditExternal` on a plain scalar is a supported path, verified on the real binary); the
  comment a TOML 0↔1 blank change re-parents is *visible in the buffer*, since a section's
  extent reaches the next header, so nothing is silent; and blank runs still never enter
  `serialize_fragment`, so a copied fragment does not carry the spacing of where it came from.
- Known edge: a buffer that also **renames** the node's key leaves the run as the node's own
  splice left it — by then the path no longer resolves. Same pre-existing limitation as a
  renamed node's trailing comment.
- Known edge (YAML): the trailing blanks of a `|+` keep-chomped block scalar are semantically
  part of its value but sit outside the node's fragment, so they are packaged as the node's
  blank run. Round-trip is byte-identical; only a deliberate edit changes the value.
- i18n: `core.action.blank-add`, `core.action.blank-remove`, `core.blank.set`,
  `core.blank.unsupported`, `core.prompt.blank-reparent`, `web.prompt.title.blankReparent`,
  `web.prompt.btn.continue` and `tui.prompt.blank-reparent.legend` are retired from both
  catalogs; `core.blank.error` remains for a failed blank splice.

### Update - 2026-09-08 (8)

**Fixed**

- **A mutation that changes nothing is no longer an undo step.** `History::push` drops a
  snapshot identical to the current one, so adding a node and committing its seed unchanged is
  one `z`, not two (the touch host always takes that path — it cannot keep an inline editor
  open, so it commits the seed), and the same holds for Enter on an unedited value or a `K`
  switch to the notation a node already uses. Defining it in `History` rather than at each
  call site keeps one definition of "an undoable entry" and keeps `history_len` — which the VS
  Code host diffs to mirror the undo stack — honest. A no-op push keeps the redo `future`: with
  the text unchanged there is nothing to diverge from.

### Update - 2026-09-08 (7)

**Fixed**

- **`Escape` closes an open sheet on touch, like desktop.** It only ever reached core (peel
  filter → clear selection), so the popup editor, the detail sheet, the `K` kind sheet, the
  Save/Open/URL sheets and the ⋯ menu had no keyboard exit at all — and the popup editor and
  URL sheet swallowed *every* key, since their text field owns the keyboard. `Escape` now runs
  before those guards and dismisses an open host-local sheet through the same `dismissSheets()`
  the scrim/×/swipe use (so a pending external edit is peeled in core too); with no sheet open
  it reaches core unchanged.

- **A sheet no longer keeps the keyboard after it is hidden.** `closeSheets()` left focus in
  the field it hid, so `onKey`'s `INPUT`/`TEXTAREA` guard swallowed every following key — the
  same dead-keyboard symptom as the add-picker freeze, reachable by dismissing the popup editor
  from the scrim. It now blurs a focused field inside the sheet it closes.

- **Every keyboard exit from a touch picker closes its sheet.** The shared `kind` sheet has two
  owners (`Mode::SchemaEnum`/`Mode::AddPicker` render into it, `K` opens it host-locally), so
  no `else` on either mode could close it and each keyboard path needed its own host-side
  close. `render()` now tracks which owner opened it and closes it when that mode leaves.

- **`Escape` cancels the constrained-value picker on a host with no focused widget.** The
  shared keymap's `Mode::SchemaEnum` branch had no `Escape` (desktop cancels from the focused
  `<select>`), so touch's value sheet could only be committed, never cancelled — and cancelling
  is what removes a freshly-added placeholder (`created_on_add`).

### Update - 2026-09-08 (6)

**Fixed**

- **The touch UI no longer freezes after adding a node with the keyboard.** Committing an
  Add-picker choice seeds a scalar and leaves core in the **inline editor** (`Mode::Edit`,
  `created_on_add`) — a surface touch does not render, so every following key resolved as an
  edit keystroke and Space fell through to native scrolling instead of toggling a branch. The
  add now commits the seeded default and opens the detail sheet on the new node (`EditCancel`
  is not an option there: `created_on_add` would roll the whole insert back). The tap path had
  the same freeze, and the keyboard path additionally left the picker sheet on screen —
  `AddPickerCommit`/`ExitAddPicker`/`SchemaEnumCommit` now close it, as the tap handlers do.

- **Touch `e` reaches the popup editor again, like desktop.** It always opened the detail
  sheet, bypassing core's `BeginEdit` routing entirely, so a branch or a multi-line
  scalar/comment could only be edited via `E`. It now dispatches the raw intent and lets core
  route it (container / multi-line → external editor, `bool`/enum → value picker), backing out
  only of the inline-editor branch — the one case with no touch surface — into the panel.

### Update - 2026-09-08 (5)

**Changed**

- **A TOML datetime's badge now names its type, borrowing the TUI KIND column's vocabulary:**
  `date·odt` / `date·ldt` / `date·ldat` / `date·ltim`. `Format` carries no datetime notation, so
  three of the four used to badge a noteless `date` and a local time a noteless `time` — the only
  scalars whose badge didn't name their own variant, next to `int·0x` and `str·'…'`.

- **The `K` datetime option row is now `name  [D:tag]`**, exactly the two-column shape a notation
  row has (`dotted table  [T/D]`), instead of carrying the resulting literal.

- **The TypeChange confirm previews the rewritten value:** `type offsetdatetime → localdate?
  (1979-05-27T07:32:00Z → 1979-05-27, drops the time, drops the offset)`. That is the last moment
  before the value changes, and it names **one** concrete outcome instead of four hypothetical
  ones — so the picker rows stay short and the disclosure gets more room, not less.

**Fixed**

- **The TUI's prompt overlay no longer truncates.** It was a `Paragraph` with no `Wrap` in a
  fixed 3-row box, so a long question lost its tail at the right edge (the new preview made this
  visible). It now wraps and sizes its box from the question's *display* width, so a zh-TW
  confirm gets the rows its double-width glyphs need.

### Update - 2026-09-08 (4)

**Fixed**

- **The datetime kind popover now *is* the kind popover, not a lookalike.** The previous entry
  put it in the right element but built its own markup, so it silently lost everything the
  notation list gets for free: no `Current: …` header and separator, and — because it never set
  `kindMenuPath` — none of the popover interactions. A second click on the same badge reopened
  instead of toggling shut, and a click on another node acted *inside* the still-open picker
  mode (the deferred outside-click closer then shut whatever that click had just opened).

  Both lists now paint through one function, `paintKindMenu`; only the pick differs
  (`CommitKind` vs `SchemaEnumMove`+`SchemaEnumCommit`). Verified in a browser, datetime vs
  table side by side: identical `Convert kind` / `Current: …` / separator / aligned-column
  structure, and same badge → toggles shut, other node → closes + selects that node, other
  badge → closes + opens that node's list, right-click → closes + opens the Action menu,
  outside click / Esc → cancels with the document untouched.

  `onTreeClick` also cancels a live picker before the click's own navigation runs: the mode is
  modal in core, so hiding the popover without leaving the mode was never enough.

### Update - 2026-09-08 (3)

**Changed**

- **The desktop web UI now shows the datetime type switch in the same list box as every other
  kind switch.** It was an inline `<select>` dropdown inside the row's value cell, because core
  routes it through `Mode::SchemaEnum` (the *value* picker) and that is the surface desktop
  draws a value pick in. Same operation, different widget — it read as an inequality between
  kinds rather than as a feature. Desktop now routes by entry point, matching the notation
  lists exactly: a **kind-badge click** opens the `#kindMenu` popover anchored at the badge
  (click an option → commit, outside click / Esc → cancel, arrow keys move the highlight in
  place), and **`K`** opens the `#overlay` list next to `Mode::KindSwitch`. The inline
  `<select>` is gated off for it, so the widget is never drawn twice.

  New wire field `ModeView::SchemaEnum.from_kind_switch` carries this. Unlike its neighbour
  `from_schema` (which only titles the popup) it selects a **widget**: these options are kind
  options, so a host with a dedicated kind-option surface renders them there. Inferring it
  host-side from "the last intent I sent was `OpenKindSwitch`" was rejected as shadow state.
  ADR 0012 Amendment 2.

- **Kind option columns now actually line up in the browser.** Core pads the labels, but HTML
  collapses runs of spaces, so both the popover and the overlay list rendered them as single
  spaces. Their labels now sit in a `.kind-label` cell (mono + `white-space: pre`) — so the
  notation list reads `literal string     '…'` / `multiline literal  '''…'''` and the datetime
  list `local date      1979-05-27`, exactly as the TUI does.

  TUI and touch needed no change: each already had one surface for both kinds of list.

### Update - 2026-09-08 (2)

**Changed**

- **A datetime `K` option is now just the type and the resulting literal**, and what the
  switch drops or auto-fills moved to the change confirmation. A row was
  `local date  1979-05-27  (drops the time, drops the offset)` — up to 55 columns, clipping
  the TUI popup (40% of terminal width) and reading nothing like a kind option. It is now
  `local date      1979-05-27`, and the confirm carries the cost:
  `type offsetdatetime → localdate? (drops the time, drops the offset)`. Nothing is lost by
  disclosing one keypress later — the confirm is where the value is actually rewritten, and
  `n` still cancels for free. ADR 0012 Amendment 1.

  The disclosure is derived from the **old and new values**, not from how the edit started,
  so a hand-typed `e` that retypes a datetime now discloses the same thing the picker does.

- **Every kind/type picker label comes from one shared formatter.** `kind_options` in all
  three backends and the datetime type list now build labels with
  `model::kind_label::align_options` — `"<name>  <sample>"` with the name column padded so
  every sample in a list starts at the same column. Padding used to be hand-typed into each
  label literal, which meant a new option silently broke the column and a *translated* list
  could not align at all (a zh-TW datetime list was visibly ragged). Padding is measured in
  **display cells**, so `本地日期` and `本地日期時間` line up.

  Side effects of the single format: YAML's string-style rows read `single quoted  '…'` /
  `literal block  |` (were `single` / `literal |`), YAML's integer rows `hex  0x…` (was
  `hex 0x`), and TOML's integer/array/table rows are aligned by the helper instead of by
  hand-counted spaces. New `confy-core` dependency: `unicode-width` (pure, wasm-safe).

  Translated punctuation went with it: the loss list joins with `、` in zh-TW, and the
  confirm's parenthetical is `（…）` with no leading space (`core.prompt.note`,
  `core.list.sep`).

### Update - 2026-09-08

**Fixed**

- **The TOML datetime kind switch was unreachable on the web and touch UIs.** `K` on a
  datetime is a *value* `Replace`, not a notation switch (ADR 0012), so the four datetime
  types are deliberately absent from `kind_options` and core's `open_kind_switch` diverts
  them to the value picker instead. Both web hosts, however, asked `kindOptions(path)` first
  and stopped at an empty list with "No notation switches available" — so the feature had no
  route from the kind badge, the panel's Kind button, or (on touch) the `K` key. Desktop
  keyboard `K` was the only entry that worked, because it alone dispatched the intent to
  core. An empty option list is now handed to `OpenKindSwitch`, which opens the picker when
  it can and raises core's own `core.kind-switch.unsupported` when it genuinely cannot. No
  new UI: both hosts already rendered `Mode::SchemaEnum` (desktop as the inline value select,
  touch as a bottom sheet) for schema-constrained values and booleans.

  The originating plan asserted "no host code change" and the web test suite stayed green
  throughout — nothing was broken, so nothing failed. Reported by user testing.

### Update - 2026-09-07 (4)

**Added**

- **The blank lines after a node are now editable.** Vertical spacing is a real part of how a
  config file reads, and confy could preserve it byte-perfectly but never let you change it: the
  only way to add or remove a blank line was `E` on a container (which doesn't exist for a
  scalar) or an external editor. Two new Action-menu items — **Add a blank line after** /
  **Remove a blank line after** — step the count, and the current value is a read-only
  **Blank after** readout in the TUI's `i` Detail popup and in the shared web/touch panel.
- The core operation is `Mutation::SetTrailingBlankLines { path, n }`, the one variant that is a
  **format-neutral text splice** (`model/blank_lines.rs`) rather than a rowan green-tree splice —
  blank runs are pure inter-token whitespace, identical in all three formats, so all three
  backends share one implementation and supply only an offset via the new
  `ConfigDocument::trailing_blank_anchor`. That anchor is the node's **contiguous extent end** —
  the same extent `Delete` covers — so a `[table]`'s blank run sits after its last member, not
  after its header line. `trailing_blank_lines` is a provided trait method defined in terms of
  the anchor, so the count query, the mutation and the confirmation guard read one source and
  cannot disagree.
- `Intent::SetTrailingBlank(delta)` is **relative and clamped at 0**, so one item pair grows,
  shrinks and fully removes a run, and each step is independently undoable. Both menu items are
  gated on the anchor resolving: a node that cannot carry a blank run (a YAML flow member, an
  opaque `&anchor`/`!tag` span, the Root) shows them *disabled* rather than erroring on pick,
  and "Remove" is disabled at 0.
- **A TOML-only confirmation guards the one case that changes meaning.** TOML's comment-ownership
  rule turns on the 0↔1 blank boundary (`CONTEXT.md` *Comment*): a comment between a table's last
  entry and the next `[header]` belongs to the preceding scope when a blank separates it, and to
  the following header when it hugs it. Crossing that boundary while a comment follows would
  silently re-parent it, so `PromptKind::BlankReparent` asks first. The guard is deliberately
  narrow — TOML only (JSON and YAML have explicit delimiters and no such rule), only across 0↔1
  (1 blank and 3 blanks parent a comment identically), and only when the next non-blank line is
  actually a comment. Everything else applies with no prompt.

**Notes**

- **Rejected alternative:** carrying blank lines in the `$EDITOR` buffer. It fails on three
  counts — the editor only opens for containers and multiline scalars, so a plain scalar would
  still have no way to change its spacing; a whole-buffer rewrite silently re-parents comments
  with no confirmation; and blank runs would leak into `serialize_fragment`, making a copied
  fragment carry the spacing of where it came from. Blank runs stay a property of the document
  at a position, never of a fragment — nothing was added to the clipboard or paste paths.
- Deliberate non-goal: no absolute numeric entry ("set to 4") and no key binding; the two
  Action-menu items are the whole surface.

### Update - 2026-09-07 (3)

**Added**

- **TOML's four datetime types are mutually switchable from `K`**, with the cost disclosed
  before you commit. `expires = 2026-01-01` → `2026-01-01T00:00:00Z` previously meant retyping
  the whole literal by hand; `K` reported `this node's kind cannot be switched`.
  A cross-type datetime switch is a **value `Replace`, not a `Mutation::ConvertKind`** —
  `ConvertKind`'s invariant is "another notation of the *same* kind", and the four datetimes are
  four *types* carrying different information. So `K` on a datetime diverts to the existing
  value picker (the widget a `bool`'s `true`/`false` list already uses) listing the other three
  types, each row showing the resulting literal and, in parentheses, everything the switch
  drops or auto-fills — `local date  1979-05-27  (drops the time, drops the offset)`,
  `local datetime  1979-05-27T00:00:00  (fills 00:00:00)`. Enter then hits the **existing**
  `TypeChange` confirmation, because `schema_enum_commit` already routes through `edit_commit`
  precisely so a picked value that changes a node's type gets gated. Net new machinery is one
  pure module (`session/datetime.rs`): no new `Mutation`, `KindTarget`, `Mode`, `PromptKind`,
  wire-contract or host change, and the feature reaches all four hosts with zero host code.
  Fill policy: a missing time becomes a fixed `00:00:00` (so the authored date stays exactly
  what you see and the result is reproducible), a missing offset becomes `Z`, and only a
  missing **date** reads the clock — UTC, never host-local. The fractional second and offset
  text are kept verbatim, so `.5` never becomes `.500` and a same-kind round-trip is
  byte-identical. No new dependency: the existing wasm-safe UTC helpers are reused.
  `e` on a datetime is unchanged (free-form literal editing). **ADR 0012.**

**Changed**

- **The `a` Add-type picker offers one `Datetime` row for TOML instead of four.** It seeds a
  full offset datetime — the widest of the four — so a later `K` switch only ever narrows and
  never has to fill anything in. The other three types are reachable from `K` (above). Four
  now-dead `core.add.type.*` catalog keys were pruned from both catalogs.

### Update - 2026-09-07 (2)

**Added**

- **`←/→` toggles a `bool` again — on the keyboard only** (TUI `←/→`, web `←/→`/`+`/`-`).
  Commit `534dd4a` removed every bool-nudge affordance by deleting the `Bool` arm from
  `nudge_scalar`, but the defect it was fixing was the *web tree's hover-and-scroll wheel*
  toggling a bool with nothing armed or focused — the keyboard was collateral, and a bool was
  left with only the three-keystroke `true`/`false` picker. The flip now lives in
  `Session::nudge` (the `Intent::Nudge` path) instead of `nudge_scalar`, so the wheel/swipe
  path (`nudge_repr`) stays numeric-only and the old misfire cannot return. Both directions
  flip, and the **authored casing is preserved** (`TRUE` → `FALSE`, never `false`): the casing
  table that already backed the picker is extracted as `bool_pair` and shared, so there is one
  rule rather than two that can drift. `nudge_scalar`'s and `nudge_repr`'s `Bool` → `None`
  tests stay as they were — they now document the pointer-surface contract. **ADR 0011.**

**Fixed**

- The `f` popup's cell pad was 17 columns wide, not 16: `[T/I] inline-tbl` is exactly 16
  characters, so once it moved into column 1 (above) it sat flush against the next cell's
  checkbox.

### Update - 2026-09-07

**Changed**

- **Type filter: `[A/T]` array-of-tables moved from the Tables group to the Arrays group**
  (`session/type_filter.rs`). `Group::Array` stays JSON's list; TOML's Arrays `all` row now
  uses a new `Group::ArrayToml` (`[A/I]`, `[A/M]`, `[A/T]`) — sharing one list would have left
  JSON's `all` row permanently `Partial`, since no JSON node classifies as `Aot`.

**Added**

- **`[T/E]` — an array-of-tables *entry* now has its own KIND tag.** One `[[fruit]]`
  occurrence projects as a `Table` with `Format::Plain`, TOML's only such shape, so it
  classified as `[T/S] scope` (a standard `[header]` table) in the TUI KIND column and
  badged a bare `{}` on the web while every other table notation carried a note. New
  `TypeToken::AotEntry` → TUI `[T/E]`, web badge `{}·entry`, and a `[T/E] aot-entry`
  type-filter cell under Tables (so `[T/S]` no longer selects AoT entries too).
  `kind_options` is unchanged — an AoT entry still converts to nothing. No web-side change
  was needed: `web/typefilter.ts` renders from `TypeFilterView` and the badge text comes
  from core's `badge_label_note`.

**Fixed**

- **TOML's Tables `all` row could never read `[x]`.** `Group::Table` carried the JSON-only
  `[T/M]` multiline-object token, which the TOML layout never renders, so ticking every
  visible cell left the tristate stuck on `[~]`. The group is now TOML's own set
  (`[T/I]`, `[T/S]`, `[T/D]`, `[T/E]`), with a regression test asserting every `all` row's
  tokens are all reachable from the rendered layout.

## [v1.1.0] - 2026-09-04

### Update - 2026-09-03 (3)

**Fixed**

- `web/build.mjs` shipped a **stale wasm core** without a word: it only *copies*
  `crates/confy-ffi/pkg/` into `web/pkg`/`web/dist` and never runs `wasm-pack`, so a
  `confy-core` fix (the notation-aware schema nudge clamp above) stayed invisible in every
  web/touch/Tauri/VS Code host until someone re-ran `wasm-pack build --target web` by
  hand — the fix was verified on the TUI binary and reported as landed while the browser
  still ran the old core. The build now compares `pkg/confy_ffi_bg.wasm`'s mtime against
  the newest `.rs` under `crates/confy-core/src` and `crates/confy-ffi/src` and prints a
  loud `WARNING: … ships a stale core` with the exact command to run. It warns rather than
  fails, so a TS-only rebuild still works without a Rust toolchain; `web/cf-build.sh`
  (CI/deploy) already rebuilt the wasm first and is unaffected.

**Added**

- Regression test `nudge_keeps_schema_grid_after_kind_switch_to_hex` (`schema_headless.rs`)
  covering the user-facing repro directly: `K`-switch a schema-constrained integer to hex,
  then nudge — the preview (`nudge_repr`, the web wheel/swipe path) and the keyboard
  `Intent::Nudge` must both walk the `multipleOf` grid and render in hex.

**Docs**

- New `WEBUI.md` § *Local build (`web/build.mjs` copies the wasm, it never rebuilds it)*
  spells out the boundary, the silent symptom (correct in the terminal, unchanged in the
  browser, no error anywhere), and the rule it implies: a `confy-core` change must have
  its verification re-run against a freshly built wasm, because that wasm *is* the web
  hosts' real binary (`functional_smoke.mjs` is the cheap way). `VSCODE.md` §
  *Build/test workflow* now marks its `wasm-pack` step as non-optional for a core change
  — skipping it stages a stale wasm into the extension's `media/`. `CLAUDE.md`'s build
  commands annotate `npm run build` as a wasm **copy**, never a rebuild.

### Update - 2026-09-03 (2)

**Fixed**

- Value nudge (`←`/`→`, wheel, touch swipe) ignored every schema rule on a **non-decimal
  integer** and quietly rewrote its notation. `Session::schema_clamp_nudge` decoded the
  repr with `f64::from_str`, which rejects `0x…`/`0o…`/`0b…` outright — so a hex/octal/
  binary node fell through the early-return and stepped a bare ±1, ignoring `multipleOf`,
  `minimum` and `maximum` (`mask = 0xFF` with `multipleOf: 5` went to `0x100`, not
  `0x104`), and any value the clamp *did* produce was rendered as decimal. The clamp now
  decodes and re-renders in the **node's own notation** (new
  `schema_hint.rs::parse_repr`/`format_nudged_like`): the radix prefix and the authored
  hex digit case survive, and underscore grouping is re-applied (`1_000` +
  `multipleOf: 5` → `1_005`, not `1005`) — matching the grouping the unconstrained nudge
  already preserved. Floats keep the same guarantee (a grouped float no longer loses its
  `_`), and non-decimal integers now walk the schema grid and clamp inward to its bounds
  exactly like a decimal one.
- Detail panel (web/touch): a wheel/swipe nudge showed the stepped value in the panel but
  never reached the document or the tree. `web/panel.ts` committed on the input's `change`
  event only, and every engine resets its "text as of last change event" baseline on a
  *script* write — so the programmatically written nudge could not fire `change`, and
  blur/Enter committed nothing. The panel now also commits on blur whenever the field's
  text differs from what was rendered (one-shot guarded, so a typed edit still commits
  exactly once; Escape still cancels, since it restores the rendered text before
  blurring). Mirrors the tree inline editor, which always committed on blur and never had
  the bug.

### Update - 2026-09-03

**Fixed**

- Detail popup (`i`): a branch's `Format:` line reported the *kind* word instead of the
  node's notation, so a dotted table (`[T/D]`) read `Format: table`, a standard scope
  (`[T/S]`) read `table`, and a multiline array (`[A/M]`) read `array`. The line is now
  derived from the node's `format` (`dotted` / `scope` / `multiline` / `inline` /
  `block`), with the kind word kept only as the `Plain`-format fallback (Root,
  array-of-tables entries). This matters beyond cosmetics: the popup is the TUI's
  recovery path for notation that a tree row does not spell out.
- Web kind badge: a TOML inline table and a YAML flow map badged a bare `inline` — the
  *kind* appeared nowhere. `NodeKind::InlineTable`'s label was the notation word
  `"inline"`, and a "don't repeat the label" guard then deleted the identical notation
  note. Both now read `{}·inline` / `{}·flow` on the row and `table · inline` /
  `table · flow` in the detail panel's Kind field. JSON was never affected (its objects
  are `NodeKind::Table` at both notations).

**Changed**

- Web kind badge: a container's label is now an **outline glyph** — `{}` for every
  table/map notation, `[]` for every array/sequence notation — with the notation kept in
  the note (`{}·scope`, `{}·dotted`, `[]·multi`, YAML `·block`/`·flow`). Scalars keep
  their short words (`str·"…"`, `int·0x`). An array-of-tables reads `[]·AoT`: it carries
  no `Format` of its own, so the note is the only thing separating `[[a]]` from a plain
  array under the shared glyph. YAML's inline note is now spelled **flow**, matching the
  term its `K` popup and legend already use. The kind as a word survives on the two
  surfaces with room for one: the badge's hover tooltip (composed with the schema hint)
  and the detail panel's Kind field. TUI rows are untouched — they keep the dense
  `[T/S]`-style kind tag, which no host shares.
- Web kind pill font size 10.5px → 10px, so the new `{}`/`[]` glyph labels sit in the pill
  without reading as visual noise.
- `tauri-plugin-dialog` 2.7.2 → 2.7.3, `tauri-plugin-fs` 2.5.1 → 2.5.2 (lockfile only).

**Security**

- Dependabot #11 (`glib` unsoundness in `VariantStrIter`, RUSTSEC/GHSA moderate, fixed in
  0.20) is **not reachable in any shipped build** and is dismissed rather than patched.
  `glib 0.18.5` enters the lockfile only through `gtk 0.18` ← `tao`/`wry`/`muda` ← `tauri`,
  all of which are `cfg(target_os = "linux")`-gated; `cargo tree -i glib` finds nothing on
  `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`, or
  `aarch64-linux-android` — the four targets confy builds. It cannot be bumped either:
  `gtk 0.18` requires `glib ^0.18`, and the whole tauri v2 GTK stack moves together. This
  becomes real the day Linux is targeted (see CLAUDE.md § Known Risks).

## [v1.0.1] - 2026-09-02

**Deployment fix release.** No functional change to the editor itself; the release exists to
publish a corrected web deployment (the hosted site's build output was missing three boot
scripts).

### Fixed

- fix(web): `web/cf-build.sh` no longer wipes and re-copies `web/dist` with its own stale file
  list. `node build.mjs` already runs `web/assemble-dist.mjs` (the single source of truth for the
  runtime file set); the duplicated `cp` list in the Cloudflare build command predated the
  CSP-driven move of the boot scripts into external files, so the deployed site shipped an
  `index.html` referencing three files that were never copied. `entry-desktop.js` 404'd, so the
  coarse-pointer router never ran and a phone opening the site stayed on the desktop UI (the
  touch entry appeared to vanish); `register-sw.js` 404'd too, leaving the deployed PWA with no
  service worker registered.

## [v1.0.0] - 2026-09-02

**First stable release** — confy reaches 1.0.0 across the desktop app (Tauri), the terminal
(TUI/CLI), the web/touch UI, and the VS Code extension.

### Added
- feat(add): type picker replaces copy-cursor-kind add-node
- feat(web): PageUp/PageDown page the tree cursor (desktop + touch)
- feat(web): built-in samples rewritten around a shared backbone tree + per-format showcase
  branches (TOML dotted keys/AoT/radix/exponent/datetime, JSON comments/null/multiline arrays,
  YAML flow/block/literal/folded/anchor)
- refactor(tui,web): unified `?` Help overlay into a shared, i18n-driven Section/Row keymap
  model; new `docs/reference/KEYMAP.md` single source of truth, machine-checked against both
  implementations so a binding can no longer drift from the docs or the other surface

### Changed
- feat(nudge)!: removed boolean nudging everywhere; a schema-constrained number now steps along
  the schema's `multipleOf` grid (instead of freezing or snapping to the nearest multiple), with
  bounds clamping inward to the grid and new type-safety guards against retyping a node
- security(tauri): enabled a Content Security Policy for the desktop shell; inline boot
  `<script>` blocks moved to external files
- fix(tui): `$EDITOR` is shell-split, matches the open document's format for the scratch-file
  extension, and repaints via resize instead of clear after an external edit
- deps(tui): bumped `ratatui` 0.28 → 0.30 and `crossterm` 0.28 → 0.29

### Fixed
- fix(schema): a YAML anchor/alias/merge-key/tag no longer silences every schema-violation
  marker in the document; schema validation now lowers through a lenient conversion pass that
  skips (rather than aborts on) out-of-subset YAML nodes
- fix(web): remote `$schema` URL hints only upgrade `http://` to `https://` on an `https://`
  page, fixing schema loading on local dev servers and Tauri's Windows origin
- fix(convert): converting an empty document to YAML no longer aborts
- fix(cli): `confy convert` refuses to silently overwrite an existing destination without
  confirmation or `--yes`
- fix(tui): a leading UTF-8 BOM no longer makes a file unloadable; saves are now atomic
  (temp file + rename) so a crash mid-write can't truncate a config
- fix(core): container nesting is capped at 256 levels, so hostile/deeply-nested input is a
  parse error instead of a stack overflow
- fix(web): the `E` (edit-external) shortcut now works on web/touch/VS Code, matching the TUI
- fix(pointer): drag/gesture drops resolve through `PasteSlot` end to end (ADR 0010); inline/flow
  containers regain their drop-into band
- fix(i18n): `core.add.placeholder` notice now says `F2`, matching the actual rename binding

### Docs
- docs(claude): refreshed module map against the tree
- docs: recorded the lenient schema lowering and schema-grid nudge across the reference docs

### Unreleased Update — 2026-09-02T15:40:00Z
- docs(nudge): record the schema-grid nudge step across the reference docs and make the Help
  overlay's own row honest about it. `KEYMAP.md`'s `ArrowRight`/`ArrowLeft` rows now note that a
  schema `multipleOf` *is* the step and that bounds clamp inward to that grid; `TUI.md` gains the
  three type guards (fractional `multipleOf` ignored on an integer-style value, a float keeping
  its decimal point, so a nudge never retypes the node); `WEBUI.md` names `nudge_repr` as the same
  core pipeline the TUI's `←/→` walks. The `help.row.nudge` catalog string changed in both
  languages ("±1 number" → "Step a number (schema step if any)" / "數字加減一階（有 schema
  時依其級距）"), since `±1` is no longer the step on a constrained node — verified rendering
  aligned and inside the popup on the real binary in `en` and `zh-TW`.

### Unreleased Update — 2026-09-02T15:10:00Z
- fix(schema): a schema-constrained number no longer **freezes** under nudge (`←`/`→`, mouse
  wheel, touch swipe). `Session::schema_clamp_nudge` snapped the nudged value to the *nearest*
  `multipleOf`, while the step it snapped was only ±1 — on any grid coarser than 2 the snap
  always landed back on the value the step came from, so the value never moved. The built-in
  sample's `schema.poll_ms` (`multipleOf: 5`) was stuck at `255` in **both** directions;
  `multipleOf: 2` was stuck going down, and a float's `10^-places` step froze against any
  fractional grid the same way. The nudge now **steps along the schema's grid**: an on-grid
  value moves `delta` whole steps (253 → 255 → 260 → 265), an off-grid value aligns in the
  nudge's own direction on the first step (253 up → 255, down → 250), and a multi-step delta
  (web wheel bursts) moves that many steps. `minimum`/`maximum` now clamp **inward to the
  nearest in-range grid point**, so parking at a bound can't leave a value the schema itself
  rejects (nor oscillate against the snap). Three type-safety guards came with it: a
  fractional `multipleOf` is ignored on an integer-style repr (a nudge must not retype an
  Integer node as a Float), a whole-numbered float result keeps its decimal point (`5` → `5.0`,
  which previously retyped a Float node as Integer), and a grid's own decimal count sets the
  output precision so a `0.1` grid can't surface float noise (`0.30000000000000004`). Without a
  schema constraint the step is unchanged (±1, or ±1 at the displayed precision for a float).
  New `schema_hint.rs::format_nudged`; `schema_clamp_nudge` takes the pre-nudge repr + delta,
  and both callers (`nudge`, and the Web/touch `nudge_repr` query) pass them — no host, FFI or
  TypeScript signature changed.

### Unreleased Update — 2026-09-02T14:10:00Z
- docs: record the lenient schema lowering across the reference docs. `CONTEXT.md`'s **JSON
  projection** and **Violation** glossary entries now name `convert::tree_to_value_lenient` +
  `value_bridge::bridge` as the lowering pair and state that a YAML opaque node carries no
  Violation while its siblings/ancestors do; `BEHAVIOR_MATRIX.md` §8's YAML-opaque invariant
  gains the "schema validation skips them, conversion aborts" split; `CLAUDE.md`'s module map
  and JSON Schema section point at the lenient variant.

### Unreleased Update — 2026-09-02T13:30:00Z
- fix(schema): a YAML file containing an anchor, alias, `<<:` merge key or tag no longer loses
  **every** schema-violation cue. `Session::revalidate_schema` lowered through
  `ConfigDocument::to_value()`, which aborts the whole document on the first opaque
  (out-of-subset) node, so validation bailed and no row carried a `violations` entry — no ▲/△
  marker, no dashed warn frame, no KIND `!`, no "N schema warning(s)" status. The Detail
  popup/panel kept showing schema info and constraint text (those resolve the sub-schema by
  path and never lower the document), which is exactly how the bug hid; TOML and JSON were
  unaffected because neither has opaque nodes. Validation now lowers through a new
  `convert::tree_to_value_lenient`, which **skips** an opaque node instead of aborting;
  `value_bridge::walk` skips the same nodes so the Node↔Value pairing stays 1:1 and every
  sibling *after* an anchor still resolves to its own path (a skipped sequence element shifts
  the JSON array, and the pointer map translates it back correctly). The opaque node itself is
  never flagged — confy cannot decode its value. `convert()` is unchanged and still aborts:
  dropping data is fine for an advisory validation pass, not for writing a converted file.
  Reproduced and confirmed fixed on the real `confy` binary (`▲ port` / `[S:str!]` /
  "1 schema warning(s)" now render for a YAML doc with `&pin`/`*pin`); the built-in YAML
  **sample** hit this on every load, since it ends with `pinned: &pin "confy"`.

### Unreleased Update — 2026-09-02T11:05:00Z
- fix(web): the built-in sample no longer reports "Schema failed to load: NetworkError" on an
  http origin. `resolveSchemaFetchRequest`'s `http://` → `https://` mixed-content upgrade was
  unconditional, but the sample's `$schema` is derived from `location.href` (`samples.ts`), so a
  local dev server (`http://localhost:8080`) or Tauri's Windows origin (`http://tauri.localhost`)
  got rewritten to an https URL nothing serves. The upgrade is now gated on the *page* being
  https — the only case where the browser blocks the plain-http fetch as mixed content — via a
  new exported `upgradeForMixedContent()`; `host-io.spec.mjs` covers both page protocols.

### Unreleased Update — 2026-09-02T09:45:00Z
- docs(claude): refresh the module map against the tree. Adds the 16 source files it had
  drifted past — `session/action_menu.rs`, `session/add_picker.rs`,
  `tui/overlay_action_menu.rs`, `tui/overlay_add_picker.rs`, and the shared web modules
  `host-io.ts`, `key-intent.ts`, `mode.ts`, `escape.ts`, `kind-labels.ts`, `samples.ts`,
  `help-content.ts`, `convert-dialog.ts`, `typefilter.ts`, `fab.ts`,
  `action-menu-items.ts`/`add-picker-items.ts` — plus the new entry scripts, lists all three
  `confy-tui/tests/` files (was only `convert_cli.rs`), and corrects the
  `functional_smoke.mjs` check count (92 → 128).

### Unreleased Update — 2026-09-02T09:20:00Z
- security(tauri): set a real `app.security.csp` (was `null`, i.e. no CSP at all) —
  `default-src 'self'`, `script-src 'self' 'wasm-unsafe-eval'`, `object-src 'none'`,
  `base-uri 'self'`, `frame-ancestors 'none'`, with `https:`/`http:`/`ipc:` allowed in
  `connect-src` for remote `$schema` hints, Open-from-URL and Tauri IPC. The desktop shell loads
  remote content, and `fs:scope` is intentionally `**`, so this is the layer that keeps an
  escaping bug from reaching the disk. Rationale + the full directive breakdown are in
  `docs/reference/TAURI.md §Content Security Policy`.
- refactor(web): the two inline boot `<script>` blocks in `index.html`/`touch.html` moved to
  external `entry-desktop.js` / `entry-touch.js` / `register-sw.js` (added to
  `assemble-dist.mjs`). Required by the CSP above: verified under headless Chrome served with
  the exact policy that both blocks were being *blocked* ("Executing inline script violates …"),
  which would have silently killed the desktop↔touch redirect and the PWA registration; after
  the move the same load reports no CSP violations and the tree renders.

### Unreleased Update — 2026-09-02T08:40:00Z
- fix(convert): converting an empty document to YAML no longer aborts with `internal: converted
  output did not re-parse: expected a mapping key, found Some(L_BRACE)`. `render_yaml` emitted
  `{}` for an empty root, which the YAML backend (no root-level flow collections in its subset)
  rejects in the reparse safety net; an empty map root now renders as an empty document, which
  the backend loads as an empty mapping. New unit test walks every (from, to) pair on an empty
  TOML/YAML/JSON source.

### Unreleased Update — 2026-09-02T08:25:00Z
- fix(cli): `confy convert` no longer overwrites an existing destination silently. It now asks
  `<out> already exists. Overwrite it? [y/N]` on a TTY and refuses on a pipe unless `--yes` is
  given — the same contract the lossy-warning prompt already had. New i18n keys
  `cli.convert.overwrite` / `cli.convert.refuse-overwrite` (en + zh-TW); the two prompts share
  one `confirm_or_bail` helper. Two new `convert_cli.rs` integration tests.

### Unreleased Update — 2026-09-02T08:05:00Z
- fix(tui): a leading UTF-8 BOM no longer makes a file unloadable (`parsing bom.json` error).
  `load_document` strips it, remembers it (`LoadedDocument::bom`), and `App::save`/`confy
  convert` put it back on write — verified with `confy convert` on a BOM'd `.json`.
- fix(tui): saves are atomic. `App::save`, the TUI `C` convert output, and `confy convert` now
  go through `confy_tui::write_document` — write to a sibling `.confy-*.tmp`, fsync, carry over
  the destination's Unix permission bits, rename over the target — instead of a bare
  `fs::write` that could leave a truncated config behind on a crash or kill.

### Unreleased Update — 2026-09-02T07:40:00Z
- fix(core): cap container nesting at `MAX_NESTING_DEPTH` (256) in all three backends. A
  `[[[[…` 100k deep used to abort the TUI with `fatal runtime error: stack overflow` (and trap
  the wasm instance in the web hosts) for `.json`, `.yaml` and `.toml` alike; it is now a plain
  parse error at load and a rejected (atomic, doc-untouched) `Replace`/`Insert` on the `$EDITOR`
  path. JSON/YAML count depth in their parsers; TOML pre-scans brackets (string/comment-aware)
  before taplo. New `tests/hostile_input.rs` pins the boundary (255 loads, 257 rejects, 200k
  does not overflow) — verified on the real `confy convert` binary.

### Unreleased Update — 2026-09-02T06:55:00Z
- fix(tui): `$EDITOR` is now shell-split (`shell-words`), so values carrying flags —
  `EDITOR="code --wait"`, `"emacsclient -t"` — launch instead of failing with
  "launching editor: code --wait"; an empty `$EDITOR` falls through to `$VISUAL`/`vi`. The
  scratch file now carries the open document's extension (`.json`/`.yaml`, not always `.toml`)
  so the editor applies the right syntax mode.
- fix(tui): after an external edit the screen is repainted via `Terminal::resize` instead of
  `Terminal::clear` — ratatui 0.30's `clear` first queries the cursor position, which aborted the
  whole session ("cursor position could not be read") on PTYs that don't answer DSR. Verified on
  the real binary under a supervised PTY: the `E` round trip now returns to the tree with the
  edited rows.
- test(tui): the three tests that mutate `$EDITOR` share a `parking_lot` mutex
  (`editor::tests::ENV_LOCK`) — they raced under the parallel test runner.

### Unreleased Update — 2026-09-02T06:25:00Z
- deps(tui): bump `ratatui` 0.28 → 0.30 and `crossterm` 0.28 → 0.29. Clears the two `unsound`
  advisories `cargo audit` raised through ratatui's old `lru 0.12` (RUSTSEC-2026-0253,
  RUSTSEC-2026-0002) and the unmaintained `paste`; the remaining 17 warnings are all GTK/unic
  crates behind Tauri on Linux, which confy doesn't target.
- build: track `Cargo.lock` (removed from `.gitignore`). The workspace ships binaries (`confy`,
  the Tauri app), so the lockfile is what makes release builds reproducible and what
  `rust-ci.yml`'s `cargo audit` step actually audits — previously it audited a fresh resolution
  on every run. `web/` and `editors/vscode/` already tracked their `package-lock.json`.

### Unreleased Update — 2026-09-02T06:10:00Z
- style(rust): `cargo fmt` pass over `tui/keys.rs` + `session/session.rs` — the Help overlay
  refactor (`811e5b6`) landed unformatted, so `rust-ci.yml`'s `cargo fmt --check` gate was red.

### Unreleased Update — 2026-09-02T04:00:02Z
- feat(web): rewrote the built-in sample end-to-end (`web/samples.ts`) around one **shared
  backbone tree** (about/basics/servers/types/schema/links — identical keys/values in
  TOML/JSON/YAML, so the format pill reads as the same doc in three coats) plus a per-format
  `showcase` branch exercising each backend's exclusive notations: TOML dotted keys/AoT/radix
  ints/exponent/inf/datetime, JSON `//` comments + null + multiline array, YAML flow seq/block
  map/literal/folded/anchor+alias. Comments — not narrated values — are the teaching voice;
  the word "banana" is seeded 5× across `basics`/`servers` for the `/` filter-highlight demo.
  The JSON sample now deliberately carries comments (JSONC), which fires the host's
  comment-advisory underline **and** still resolves the `$schema` hint (`schema/hints.rs`
  detects through the JSONC-aware parser) — the E-2 no-dropped-notice repro shape.
  `schema.advanced` ships collapsed-by-default with a schema-invalid seed so the collapsed
  `schema` parent demos `has_descendant_violation` (E-3).
- feat(web): `web/schema-sample.json` gains `$defs`/`$ref` (the `editor` enum now resolves
  through `#/$defs/editorName`, surfacing a schema `description`), a `multipleOf`-bounded
  `poll_ms` with a description, and a new `pattern`-constrained `schema.advanced.retry_pattern`
  (seeded invalid: `"abc"`) — 3 seeded violations across 3 constraint kinds, all demoed on the
  collapsed `schema.advanced` branch.
- feat(about): the About panel now linkifies **every** URL (`help-content.ts` regex widened to
  global) and `ABOUT_TEXT`/`ABOUT_TEXT_ZH_TW` gain four resource lines — Live demo, VS Code
  Marketplace, Open VSX, MS Store — so all hosts (TUI/web/touch/Tauri/VS Code) show clickable
  links; README's Desktop-app Windows bullet gains the Microsoft Store link.

### Unreleased Update — 2026-09-02T10:35:00Z
- refactor(tui,web): unified the `?` Help overlay's keymap content into a shared, i18n-driven Section/Row model (Navigation/Selection/Edit/File & App), grouping and two-columning what was an ad-hoc four-column TUI layout and a `·`-strung Web cheatsheet. New `help.section.*`/`help.row.*` catalog entries in `i18n/{en,zh-TW}.json` (119 keys) back both `crates/confy-tui/src/tui/keys.rs::help_sections` (rendered with `unicode-width`-aligned columns inside a now-padded popup, `Padding::new(2, 2, 1, 1)`, fixing the missing gap between border/title and content) and `web/help-content.ts` (rendered as a CSS grid, `.help-grid`/`.help-key`/`.help-desc`, replacing the old `<pre>` text blob and its 4 now-deleted `HELP_TEXT*` constants). Reflowed the 6 `web.help.legend.*` Kind-legend strings to one label/description pair per line. zh-TW wording tightened for the external-editor rows ("編輯器"/"強制開啟編輯器" replacing the looser "多行對話框"/"強制對話框"). `docs/reference/KEYMAP.md` gains "Help overlay parity" and "Editor (inline/external) parity" sections documenting the shared-row model, the VS Code variant split, and why the TUI/Web Kind-legend vocabularies stay intentionally un-unified. See `docs/superpowers/plans/2026-09-02-HELP_OVERLAY_PLAN.md`.

### Unreleased Update — 2026-09-02T01:34:17Z
- docs(keymap): new `docs/reference/KEYMAP.md` — the TUI ↔ Web **single source of truth** for keyboard bindings. Documents the full normal-mode key table (49 rows: canonical key, TUI `KeyAction`, Web `KeyResolution`, status), the deliberate divergences (`w` TUI-only, `g`/`G` and `+`/`-` web-only, `l` lang picker vs toolbar dropdown, `Tab` `.json`/`.jsonc` toggle vs the `Jsonc` `<select>` option, `Ctrl+O` web-only, `~` TUI-only, `q` suppressed under `vshost`), and the per-surface input-handling differences (web's native `<input>` inline edit with `stopPropagation()` vs the TUI's core-driven buffer — which is why `EditCursor*`/`EditDelete` are declared but unused in `web/types.ts`; the three presentations of the one `external_edit` handshake; clipboard-guard ownership; `treePageStep` vs `terminal_height / 2`). Registered in `docs/reference/README.md` and cross-referenced from `TUI.md`/`WEBUI.md`.
- test(keymap): the KEYMAP.md table is **machine-checked against both implementations**, so a binding can no longer drift from the docs or from the other surface — the root cause of the `E` regression fixed in the previous entry. `crates/confy-tui/src/tui/keys.rs` gains four `keymap_doc_*` tests (TUI column vs `map_key`, status-column consistency, completeness scan, unbound-really-unbound) and `web/keymap-parity.spec.mjs` runs the equivalent four against `resolveKeyIntent`. Both parse the same markdown table between `<!-- KEYMAP-TABLE:BEGIN/END -->` markers. The completeness scans reject any binding present in an implementation but absent from the doc; the TUI scan skips `Ctrl+<letter>` combinations that are mere modifier-wildcard aliases of the unmodified key (`map_key`'s char arms match `_` modifiers) so only genuinely distinct Ctrl bindings like `Ctrl+S` are required. `KeyAction` now derives `Debug, PartialEq, Eq` to supply the variant names. Verified by six mutation tests: deleting a row, corrupting either binding column, corrupting the status column, deleting the `E` case from the implementation, and adding an undocumented binding each fail the appropriate guard.

### Unreleased Update — 2026-09-02T01:19:45Z
- fix(web): the `E` (Shift+E) shortcut did nothing on the web/touch/VS Code hosts — `resolveKeyIntent` (`web/key-intent.ts`) had no `"E"` case, so `onKey` bailed on the `null` resolution and the keystroke was silently dropped. `E` now resolves to `Intent::BeginEditExternal`, aligning with the TUI's `E` -> `KeyAction::EditExternal` (`crates/confy-tui/src/tui/keys.rs`): it force-opens the popup/external editor on **any** node, regardless of kind or schema, whereas plain `e` only routes External when core's `edit_target_kind()` says so (multiline string / comment). This was the only forced-external path missing from the web keyboard — the panel's "Editor" button already sent the same intent. Desktop opens `#ext-modal`, touch opens its `.ext-sheet`, both via the existing `snap.external_edit` handshake; core's `begin_external_edit` owns the clipboard-armed guard, so no host-side check was added. Documented in the Help overlay (all four `web/help-content.ts` variants, en + zh-TW) and `docs/reference/WEBUI.md`; new `web/key-intent.spec.mjs` case.

### Unreleased Update — 2026-09-02T00:00:00Z
- fix(web): remote `$schema` URL hints with an `http://` scheme never loaded on the web/touch hosts — `resolveSchemaFetchRequest` fetched the URL directly from the browser, and an https page blocks an `http://` fetch as mixed content before json-schema.org's 301 redirect to https can run, surfacing "Schema failed to load: Failed to fetch" (TUI/VS Code resolve natively, so only the browser hosts were affected). `http://` hints are now upgraded to `https://` before the browser fetch — the https endpoints of schema hosts (e.g. `https://json-schema.org/draft-07/schema`) send `access-control-allow-origin: *`, so the fetch succeeds. Tauri/VS Code branches unchanged (native fetch, no mixed-content restriction). Covered by two new `web/host-io.spec.mjs` checks.

### Changed (2026-09-01)
- feat(nudge)!: remove boolean nudging everywhere (TUI arrows, web keyboard, wheel); bools edit only via the true/false picker — `nudge_scalar` no longer touches `Bool`
- feat(web): wheel/swipe value nudge now requires inline-edit focus, captures all wheel ticks/horizontal swipes page-wide while armed, and writes via the new stateless `nudge_repr` core query (single `CommitEdit` on blur/Enter — no per-tick document mutation)

---

Older releases (v0.x) live in
[`docs/reference/changelog/v0.x.md`](docs/reference/changelog/v0.x.md).
