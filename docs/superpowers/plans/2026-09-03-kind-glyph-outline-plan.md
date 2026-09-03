# Kind annotation → outline glyph: implementation plan

Spec: [`../specs/2026-09-03-kind-glyph-outline-design.md`](../specs/2026-09-03-kind-glyph-outline-design.md)
(read it first — this plan does not restate the rationale, only the work).

## Context

The kind annotation moves before the key on **every** host and simplifies to a
type-only **kind glyph** owned by core — the TUI included, which retires both the KIND
column and the 8-column kind tag (`type_tag`). Kind switch joins the core Action menu
because the simplified control is a weaker affordance.

Baseline: clean tree at `6cc9549`. Phases 1→2/3 have a hard dependency (the wire
field); 2 and 3 are independent of each other; 4 depends on 1; 5 and 6 are last.

**Progress.** Phase 1 landed at `9c94012`. Phase 2 landed twice and was corrected
twice — `b09be63` (merged column, tag anchored at x=1) then `f2df110` (reverted to an
indent-following tag, because the anchored layout flattened the tree). Both were
written while the spec still kept the dense tag; **Phase 2b below replaces that tag
with the glyph** per the revised §4.3, and is the next unit of work.

A prerequisite defect found while reviewing the recovery paths is already fixed at
`c153d0c`: the `i` Detail popup reported a branch's *kind word* (`table`) instead of
its notation (`dotted`/`scope`/`multiline`). That popup is now the TUI's only
notation surface, so it had to be right before the glyph landed.

## Phase 1 — core: `kind_glyph` + wire field ✅ `9c94012`

**Files:** `crates/confy-core/src/session/type_filter.rs`, `session/view.rs`,
`session/session.rs`, `web/types.ts`.

1. In `type_filter.rs`, **directly beneath `classify`**, add
   `pub fn kind_glyph(kind: &NodeKind, format: Format, doc: DocFormat, read_only: bool) -> &'static str`:
   call `classify`, then an exhaustive `match` over all 36 `TypeToken` variants → the
   10 glyphs in spec §4.1. No `_ =>` arm — the point of core ownership is that a new
   token breaks the build.
2. `view.rs`: add `kind_glyph` (as `Cow<'static, str>`, matching the neighbouring
   `type_label`/`badge_label` convention) to **both** `ViewRow` and `ChildView`.
3. `session.rs`: fill it in `to_view_row` and in `children_of`.
4. `web/types.ts`: add the field to both mirrored interfaces (hand-written serde
   mirror; omitting it is silent contract drift).

**Verify:** `cargo test -p confy-core` → `wasm-pack build --target web && node
functional_smoke.mjs`.

## Phase 2 — TUI: merged column ✅ `b09be63` + `f2df110`

Landed: the tree `Table` went three constraints → two
(`name_col_width(total) + TYPE_WIDTH + 1`, then `Min(10)`), `type_col_cell` folded
into the name-cell `Line`, `draw_column_header` became one merged cell, and
`tui.header.kind` retired. The tag renders after the indent and branch toggle
(`f2df110`), guarded by `kind_tag_follows_the_indent_so_depth_stays_readable`.

## Phase 2b — TUI: glyph parity (retire `type_tag`) ⬅ next

**Files:** `crates/confy-tui/src/tui/ui.rs` (rendering + in-file `mod tests`),
`crates/confy-tui/src/tui/app.rs`, `crates/confy-tui/src/tui/tests.rs`.

1. `ui.rs`: `TYPE_WIDTH` 8 → 3. The merged-width and `value_col_width` **formulas are
   unchanged**, so VALUE gains 5 columns (spec §3) — do not "fix" that.
2. `app.rs`: delete `type_tag` (36 arms) and its `RowSnapshot.type_tag` field, replaced
   by the glyph read straight off `ViewRow.kind_glyph`. This also deletes
   `rebuild_rows`'s per-row `tree.node_at(&vr.path)` lookup, which existed only to
   recover a `NodeKind` for `type_tag`. Delete the violation `!` splice: `▲`/`△` in the
   warning column is the only violation cue (spec §3).
3. `ui.rs` name cell: `Span::styled(glyph, hue)` where the glyph is padded to 3
   **display** columns (`UnicodeWidthStr`, not `chars().count()`), keeping the
   `has_fill` skip-colour rule and the outermost selection marker. Row order stays
   `sel-marker + indent + branch-marker + warn + glyph + space + key`.
4. `paste_line_row`'s indent is unaffected (it already follows the row's own indent).
5. Tests: `KEY_X` 15 → 10; `paste_target_into_fill_...`'s `kind_x` → 6; `tests.rs`'s
   36-row tag table becomes a 10-row glyph table; every `[T/S]`/`[I:dec ]` assertion in
   `ui.rs`'s test module becomes its glyph. Keep the depth-step guard.

**New tests:** no rendered buffer contains a bracketed tag (`[T/`, `[S:`, `[I:`) — the
retirement is asserted, not assumed; and `value_col_width(total)` is exactly 5 greater
than `total - name_col_width(total) - 10` … i.e. pin the new arithmetic explicitly.

**Verify:** `cargo test -p confy-tui` → **real binary**, per spec §6.4: 60 and 100
columns, depth ≥ 10, `en` and `zh-TW`, exercising inline **value** edit, inline
**name** edit (`Tab`), the `/` filter input, and the `i` Detail popup — the four
surfaces that read the widened VALUE column or the retired notation.

**Commit:** `refactor(tui): kind glyph replaces the 8-column kind tag`

## Phase 3 — web: glyph row + single hue table

**Files:** `web/kind-labels.ts`, `web/render.ts`, `web/touch/render.ts`,
`web/breadcrumb.ts`, `web/ui.ts`, `web/style.css`, `web/touch/style.css`.

1. `kind-labels.ts`: add `hueFor(typeLabel: string): string` as the **single** hue table
   (covering branches and comments, which `valueHue` returns `""` for); reimplement
   `valueHue`/`valueTypeClass` as wrappers over it.
2. `render.ts`: replace `renderKindBadge` with a glyph renderer; move the call site from
   after value/count to **between the warn triangle and the key/comment text**. Keep
   `data-kind="1"` verbatim; `<button>` when `!read_only && !isCommentRow(r)`, `<span>`
   otherwise (so read-only and comment rows gain a glyph but no control).
3. `touch/render.ts`: same, deleting the forked `kindBadgeHTML`; keep
   `data-act="kind"` verbatim so `touch/app.ts` needs no change.
4. `breadcrumb.ts`: delete the local `GLYPHS`/`glyphHTML`; read `ChildView.kind_glyph` +
   `hueFor`. Root's hardcoded `⌂` now comes from core for the mini-tree rows; the bar's
   own root button may keep its literal.
5. `ui.ts`: `onTreeHover` becomes the single `.title` writer, composing
   `kind · notation` (`badge_label`/`badge_note`) + the schema hint, newline-joined, and
   dropping its `if (cell.title) return` short-circuit for the kind element. **No static
   `title` attribute** — that would suppress the schema hint.
6. CSS (both stylesheets, independent copies): add `.kind-glyph` reusing `.crumb-glyph`
   sizing and the `t-*` hue tokens; retire `.kind` and `.chev`; keep `.kind-note` (panel).
   Touch's `.kind-glyph` gets a transparent ≥44px hit box via padding/`::before`.

**Verify:** `cd web && npm run typecheck && npm test && npm run build`, updating
`render.spec.mjs`, `touch-render.spec.mjs`, `touch-clip-source.spec.mjs`,
`touch-comment-advisory.spec.mjs`.

**Commit:** `feat(web): kind glyph before the key; one hue table in kind-labels`

## Phase 4 — Action menu + detail panel

**Files:** `crates/confy-core/src/session/action_menu.rs`, `web/panel.ts`,
`i18n/*.json`, plus the three Action-menu renderers only if the item list is not fully
data-driven (verify first — ADR 0009 says it is).

1. `action_menu.rs`: add Kind switch as a **single-Node-only** item among the structural
   operations (not at the top), label key `core.action.kind-switch`, opening
   `Mode::KindSwitch` for the cursor path.
2. `panel.ts`: replace the `.dotc` dot with the glyph (mono, `hueFor`-coloured); keep the
   `type_label · note` text and the locked field order; delete the `|| "branch"` patch.

**Verify:** `cargo test -p confy-core`; `npm test` incl. `panel-schema.spec.mjs`; real
binary — open the Action menu on a single node and on a multi-node locked selection
(the item must be absent for the latter), then on touch via the FAB.

**Commit:** `feat: Kind switch in the Action menu; panel Kind field uses the glyph`

## Phase 5 — legend / i18n (the big deletion)

**Files:** `i18n/en.json`, `i18n/zh-TW.json`, `web/help-content.ts`,
`crates/confy-tui/src/tui/overlay_help.rs`.

1. Add **one shared legend** under `core.help.legend.*`: one key per glyph (10) plus
   `core.help.legend.notation-hint` — the line stating that notation words live in `i` /
   the detail panel, the `K` switch, and the `f` type filter.
2. Delete **all 66 `tui.help.legend.*` keys** (TOML 28 / JSON 14 / YAML 24) and the
   **3 `web.help.legend.{toml,json,yaml}`** monolithic tables from both catalogues.
3. `help-content.ts` and `overlay_help.rs` both render the shared rows; the TUI's
   per-format legend branch (which picked one of the three key sets by `DocFormat`)
   disappears, since the glyph set is format-independent.
4. Confirm both catalogues stay key-identical (69 out, ~11 in).

**Verify:** `cargo test -p confy-tui`; `npm run typecheck && npm test && npm run build`;
open `?` Help on the **TUI in both languages**, desktop, touch, and (embedding
`web/dist`) the VS Code host.

**Commit:** `docs(i18n): one shared glyph legend replaces 69 per-format legend keys`

## Phase 6 — documentation + ADR

**Files:** new `docs/reference/ROW_ANATOMY.md`, new `docs/adr/0011-*.md`,
`docs/reference/CONTEXT.md`, `docs/adr/0009-*.md`, `docs/reference/README.md`,
`docs/reference/TUI.md`, `docs/reference/WEBUI.md`, `CLAUDE.md`, `CHANGELOG.md`.

1. `ROW_ANATOMY.md` per spec §5 (glyph table, five per-surface anatomies, the shrunken
   divergence list citing findings B/C/D/E, and the note that bracketed `[T/S]` forms
   are documentation shorthand only from now on).
2. **ADR 0011** — glyph-in-core / hue-in-web, recording the shared-`kind-labels.ts`
   runner-up and exhaustiveness as the deciding reason. Follow ADR 0009's shape.
3. `CONTEXT.md`: the `KIND column tags (full vocabulary)` section becomes the **glyph
   vocabulary** plus the notation-word list (as shown by `i`/`K`/`f`); update the five
   "KIND-column vocabulary" references (:322-323, :445-446, :525, :549-550); delete
   :301-304's Action-menu exception clause; fix :52's already-stale
   key-sign-in-the-KIND-column claim. Add the canonical terms (Kind annotation / Kind
   glyph / Notation word) and retire both "kind badge" **and** "kind tag".
4. ADR 0009: delete the Kind-switch exception bullet, noting ADR 0011/this spec.
5. `README.md` index row, `TUI.md` §Rendering (the KIND-column paragraph becomes the
   glyph prefix), `WEBUI.md` row anatomy, `CLAUDE.md` module map (`type_tag` is gone —
   the `tui/app.rs` and `ui.rs` lines both change), `CHANGELOG.md` Unreleased Update.

**Verify:** every cited `path:line` in the new docs re-checked against the post-change
code (line numbers shift across phases 1-4).

**Commit:** `docs: ROW_ANATOMY reference + ADR 0011 + glossary alignment`

## Final gate (before declaring done)

`cargo fmt --check` · `cargo clippy -- -D warnings` · `cargo test` ·
`cd web && npm run typecheck && npm test && npm run build` ·
`cd crates/confy-ffi && wasm-pack build --target web && node functional_smoke.mjs` ·
**real-binary pass**: TUI at 60 and 100 columns on a depth-≥10 fixture in `en` and
`zh-TW`, with inline value edit, inline name edit, `/` filter, `i` popup and `?` help;
`web/dist` in a browser on the desktop and touch entries, checking (a) the composed
hover title still shows the schema hint, (b) the touch glyph's hit box steals neither
caret nor row taps, (c) an opaque YAML node renders `!` and rejects the kind switch.

## Risks

- **Wire-field drift** — `web/types.ts` is hand-written; a mismatch is a runtime
  `undefined`, not a compile error. Phase 1 landed both sides in one commit.
- **Two independent stylesheets** — `web/style.css` and `web/touch/style.css` are copies;
  a `.kind-glyph` rule added to one only is a silent touch regression. Phase 3 edits both.
- **Legend deletion is irreversible-looking** — 69 keys leave both catalogues in Phase 5,
  including the zh-TW translations of 24 YAML rows. They describe a rendering no host
  emits after Phase 2b, and git holds them if the decision is revisited.
- **Notation becomes single-sourced in the popups** — with no row-level notation, a bug
  in the Detail popup's `Format:` line (exactly the `c153d0c` defect) means notation is
  visible nowhere. That line now has a headless regression test; keep it.
- **Line-number rot in docs** — Phase 6's citations are written last, after phases 1-4
  have moved every referenced line.
