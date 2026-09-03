# Kind annotation → outline glyph: implementation plan

Spec: [`../specs/2026-09-03-kind-glyph-outline-design.md`](../specs/2026-09-03-kind-glyph-outline-design.md)
(read it first — this plan does not restate the rationale, only the work).

## Context

The kind annotation moves before the key on every host; the web hosts simplify it to a
type-only **kind glyph** owned by core, the TUI keeps its dense **kind tag** but
rendered after the indent and branch toggle with the KIND column retired. Kind switch joins the core
Action menu because the simplified control is a weaker affordance.

Baseline: clean tree at `6cc9549`. Six phases, each independently verifiable and
committable. Phases 1→2/3 have a hard dependency (the wire field); 2 and 3 are
independent of each other; 4 depends on 1; 5 and 6 are last.

## Phase 1 — core: `kind_glyph` + wire field

**Files:** `crates/confy-core/src/session/type_filter.rs`, `session/view.rs`,
`session/session.rs`, `web/types.ts`.

1. In `type_filter.rs`, **directly beneath `classify`**, add
   `pub fn kind_glyph(kind: &NodeKind, format: Format, doc: DocFormat, read_only: bool) -> &'static str`:
   call `classify`, then an exhaustive `match` over all 36 `TypeToken` variants → the
   10 glyphs in spec §4.1. No `_ =>` arm — the point of core ownership is that a new
   token breaks the build.
2. `view.rs`: add `pub kind_glyph: &'static str` (as `Cow<'static, str>` if serde needs
   it, matching the neighbouring `type_label`/`badge_label` convention) to **both**
   `ViewRow` and `ChildView`.
3. `session.rs`: fill it in `to_view_row` (~:238, beside `badge_label_note`) and in
   `children_of` (~:437, which needs `c.format` / `c.read_only` / the session's doc
   format — all in hand, no new plumbing).
4. `web/types.ts`: add the field to both mirrored interfaces (this file is the
   hand-written serde mirror; omitting it is a silent contract drift, and it was missing
   from the pre-review impact count).

**New tests** (`crates/confy-core/tests/session_headless.rs` or the existing
`type_filter` test module): every `TypeToken` maps to a non-empty glyph; `Opaque` → `!`;
`Root` → `⌂`; a YAML anchor node's row glyph differs from the same node's underlying
kind glyph.

**Verify:** `cargo test -p confy-core` → then
`cd crates/confy-ffi && wasm-pack build --target web && node functional_smoke.mjs`
(the wire contract changed; the smoke harness must still pass its 128 checks).

**Commit:** `feat(core): kind_glyph over classify + ViewRow/ChildView wire field`

## Phase 2 — TUI: column-anchored kind tag

**Files:** `crates/confy-tui/src/tui/ui.rs` (rendering + in-file `mod tests`),
`crates/confy-tui/src/tui/tests.rs`, `i18n/en.json`, `i18n/zh-TW.json`.
`tui/app.rs` is **not** touched — `type_tag` is unchanged.

1. Tree `Table`: three constraints → two,
   `[Constraint::Length(name_col_width(total) + TYPE_WIDTH + 1), Constraint::Min(10)]`,
   `column_spacing(1)` unchanged.
2. Name-cell builder becomes a `Line` of `Span`s in the order
   `sel-marker + indent + branch-marker + warn + tag + space + key` — the tag rides
   each row's indent (the x=1 anchoring shipped first was reverted on sight of the
   real binary: it flattened every row's visual origin). Fold in
   `type_col_cell`'s per-`type_label` colouring **and** its `has_fill` skip-colour rule
   verbatim; delete `type_col_cell`. Selection marker stays outermost.
3. `value_col_width`: keep the returned value identical; re-derive the body against the
   merged width so the arithmetic reads honestly.
4. `draw_column_header`: one merged cell labelled `NAME` alone — with the tag riding
   the indent there is no fixed column for `KIND` to label, so `tui.header.kind`
   **retires** from both catalogues and `tui.header.name` changes `"  NAME"` →
   `"NAME"` (its leading space is applied in code).
5. Tests: `KEY_X` 7 → 15; `paste_target_into_fill_...`'s `kind_x` → `6`;
   `type_format_column_shows_fixed_pitch_tag`, `inline_table_tag_differs_from_table_scope`,
   `column_header_and_type_value_columns_render`,
   `draw_tree_windows_to_the_viewport_...`, `comment_advisory_renders_underlined_...`
   updated for the merged column. `type_tag_is_fixed_pitch` (`tests.rs:75`) is unchanged.

**New test (the flat-tree guard):** assert the tag's x **steps 2 columns per depth
level**, so nothing can re-anchor it to a fixed column and flatten the tree again.
Finding E's clipping on deep/narrow rows is accepted, not guarded.

**Verify:** `cargo test -p confy-tui` → **real binary**: `cargo run -- <fixture>` at 60
and 100 columns, inspecting a depth-≥10 row and an inline edit in both fields.

**Commit:** `refactor(tui): retire the KIND column, render the kind tag before the key`

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
   `hueFor`. Root's hardcoded `⌂` at `:81`/`:184` now comes from core for the mini-tree
   rows; the bar's own root button may keep its literal.
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

## Phase 5 — legend / i18n

**Files:** `i18n/en.json`, `i18n/zh-TW.json`, `web/help-content.ts`.

Rewrite `web.help.legend.{toml,json,yaml}` as a glyph legend + one line pointing at the
panel / `K` / `f` for notation words. `tui.help.legend.*` (66 keys) unchanged. Confirm
both catalogues stay key-identical.

**Verify:** `npm run typecheck && npm test && npm run build`; open `?` Help on desktop,
touch, and (embedding `web/dist`) the VS Code host.

**Commit:** `docs(i18n): glyph legend replaces the web label·notation tables`

## Phase 6 — documentation + ADR

**Files:** new `docs/reference/ROW_ANATOMY.md`, new `docs/adr/0011-*.md`,
`docs/reference/CONTEXT.md`, `docs/adr/0009-*.md`, `docs/reference/README.md`,
`docs/reference/TUI.md`, `docs/reference/WEBUI.md`, `CLAUDE.md`, `CHANGELOG.md`.

1. `ROW_ANATOMY.md` per spec §5 (glyph table, five per-surface anatomies, divergence
   table citing findings B/C/D/E).
2. **ADR 0011** — glyph-in-core / hue-in-web, recording the shared-`kind-labels.ts`
   runner-up and exhaustiveness as the deciding reason. Follow ADR 0009's shape
   (title sentence, Status + Spec links, a "why not the cheaper option" section).
3. `CONTEXT.md`: rename the `KIND column tags` section to **`Kind tag vocabulary`**;
   update the five "KIND-column vocabulary" references (:322-323, :445-446, :525,
   :549-550); delete :301-304's Action-menu exception clause; fix :52's already-stale
   key-sign-in-the-KIND-column claim. Add the three canonical terms (Kind annotation /
   Kind tag / Kind glyph) and retire "kind badge".
4. ADR 0009: delete the Kind-switch exception bullet, noting ADR 0011/this spec.
5. `README.md` index row, `TUI.md` §Rendering, `WEBUI.md` row anatomy, `CLAUDE.md`
   module map, `CHANGELOG.md` Unreleased Update entry.

**Verify:** every cited `path:line` in the new docs re-checked against the post-change
code (line numbers shift across phases 1-4).

**Commit:** `docs: ROW_ANATOMY reference + ADR 0011 + glossary alignment`

## Final gate (before declaring done)

`cargo fmt --check` · `cargo clippy -- -D warnings` · `cargo test` ·
`cd web && npm run typecheck && npm test && npm run build` ·
`cd crates/confy-ffi && wasm-pack build --target web && node functional_smoke.mjs` ·
**real-binary pass**: TUI at 60 and 100 columns on a depth-≥10 fixture; `web/dist` in a
browser on the desktop and touch entries, checking (a) the composed hover title still
shows the schema hint, (b) the touch glyph's hit box steals neither caret nor row taps,
(c) an opaque YAML node renders `!` and rejects the kind switch.

## Risks

- **Wire-field drift** — `web/types.ts` is hand-written; a mismatch is a runtime
  `undefined`, not a compile error. Phase 1 lands both sides in one commit.
- **Two independent stylesheets** — `web/style.css` and `web/touch/style.css` are copies;
  a `.kind-glyph` rule added to one only is a silent touch regression. Phase 3 edits both.
- **Line-number rot in docs** — Phase 6's citations are written last, after phases 1-4
  have moved every referenced line.
