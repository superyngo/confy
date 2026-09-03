# Kind annotation as a VS Code-style outline glyph before the key

- **Date:** 2026-09-03 (revised after design review, same day)
- **Status:** Approved design (implementation plan: [`../plans/2026-09-03-kind-glyph-outline-plan.md`](../plans/2026-09-03-kind-glyph-outline-plan.md))
- **Scope:** presentation, **plus one behavioral addition** — Kind switch joins the
  core Action menu (§4.6), because the simplified web control no longer satisfies
  ADR 0009's membership exception. No `Mutation`, no document semantics, no
  keymap change.

## 0. Terminology (canonical, mirrored into CONTEXT.md)

One facet, two renderings — and this spec uses only these three terms:

- **Kind annotation** — the abstract facet: a node's kind/notation, wherever shown.
- **Kind tag** — the TUI's dense, notation-bearing rendering (`[T/S]`, `[S:lit ]`,
  `[I:hex ]`), 8 display columns, produced by `type_tag`.
- **Kind glyph** — the web hosts' outline rendering (`{}`, `abc`, `@`), type-only,
  produced by core's new `kind_glyph`.

"Kind badge" is **retired** as a term (it named the pre-change web control and is
now ambiguous between the two renderings).

## 1. Problem

One facet is spelled three different ways across five surfaces, and the vocabulary
the user wants to standardize on lives in a web-only local constant:

| Surface | Position | Rendering | Source |
|---|---|---|---|
| TUI tree | between NAME and VALUE, fixed 8-col KIND column | `[T/S]` `[S:lit ]` `[I:hex ]` | `type_tag` (`tui/app.rs:988`) ← core `classify` |
| Web desktop tree | after value, before trailing comment | `table·scope ⌄` button (`.kind`) | `ViewRow.badge_label` / `.badge_note` |
| Touch tree | same | same, no chevron (`touch/render.ts:56`) | same |
| Breadcrumb | **before the segment label** | `{}` `[]` `abc` `123` `tf` `??` `@` `#` + hue | `breadcrumb.ts:39` local `GLYPHS` const (web-only) |
| Detail panel | Kind field (locked 4th) | `● table · scope` (anonymous colour dot) | `type_label` + `badge_note` |

Findings that drive the design:

- **A.** The glyph table is a web-only local const, keyed on a `type_label` string
  rather than on the model, and is unreachable by the tree, touch, or panel. That
  is an existing single-source-of-truth break (`wens-dev-principles ui 1`).
- **B.** For a **scalar**, notation is *already* fully visible in the VALUE cell,
  because `ViewRow.value` carries the raw source repr (`0xFF`, `'lit'`, `"""…"""`,
  `1e3`, `inf`). The `·0x` / `·'…'` / `·1e` suffix is pure redundancy — deleting it
  loses nothing.
- **C.** For a **branch** it is not redundant: a branch row has no VALUE cell, so
  `·scope` / `·dotted` / `·inline` / `·multi` / `AoT` exists nowhere else on the
  row. This is the only real information loss, and it is accepted deliberately (§7).
- **D.** The cost/benefit is inverted between hosts. In a fixed-pitch grid an 8-column
  KIND column is *a column* — it crowds nothing. On the web the badge is an inline
  chip competing with value and comment for the same row. So "simplify" has real
  value on the web and near-zero value in the TUI, where it would be pure
  information loss.
- **E.** The TUI's tag is currently **unclippable**: it lives in its own
  `Constraint::Length(8)` column, so it renders at every depth and every terminal
  width. Any design that moves it *after* the indent forfeits that, silently — at
  60 columns `name_col_width` is 24, and a depth-10 row's prefix already consumes
  all 24 (`ui.rs:33-35, 38-51`; ratatui clips with no ellipsis). This finding is
  what shapes §4.3.

## 2. Goals

1. The kind annotation moves **before the key** on every host (VS Code outline
   position).
2. On the web hosts the annotation **simplifies to a kind glyph**; notation is no
   longer hinted on the tree row.
3. The annotation stays **clickable** and still opens the kind switch
   (`Mode::KindSwitch`) — and, because a bare glyph is a weaker affordance than a
   labeled pill with a chevron, Kind switch also becomes reachable from the Action
   menu (§4.6).
4. The detail panel is aligned to the same symbol.
5. The remaining TUI ↔ web rendering differences are documented.

Non-goals: changing `Mutation`, `kind_options`, the `f` type filter, the `K` popup
contents, the keymap, or the panel's locked field order.

## 3. Decisions

| Decision | Choice | Rejected alternatives |
|---|---|---|
| TUI scope | **Position parity, density preserved, tag column-anchored** — the KIND column *widget* retires but its *position* does not: the 8-col tag renders at x=1, before the indent (§4.3) | True adjacency (`indent → tag → key`): forfeits finding E, so the tag disappears entirely on deep/narrow rows — a strictly worse loss than the alignment it was trading away. Full glyph parity (3-col glyph in the TUI too): loses the power surface's at-a-glance notation. Web-only: maximum divergence |
| Branch notation | **Not compensated on the row** — hover title, detail panel, and the `f` type filter carry it | Notation-aware container glyphs (`{.}`/`{…}`/`[[]]`: grows the set and stops being VS Code's clean vocabulary); folding it into the item-count cell (`3 items · scope`: puts the removed noise back on the same row) |
| Glyph ownership | **core** (`kind_glyph`), for **one reason only: compile-time exhaustiveness** — a new `TypeToken`/`ScalarType` must break the build, not degrade to a fallback glyph at runtime | **One shared table in `web/kind-labels.ts`** — the genuine runner-up, and strictly cheaper: it also cures finding A (all four consumers are in `web/`, and the TUI consumes no glyph at all), with zero core files, no FFI wire field, no `web/types.ts` mirror edit, and no `functional_smoke.mjs` update. Rejected because a table keyed on a `type_label` string is unchecked and drifts silently. Recorded as ADR 0011 |
| Glyph inputs | **`classify`-derived** (`kind, format, doc, read_only`) | `&NodeKind` alone: cannot express a YAML **opaque** node, so an unmutatable node would render identically to a normal one, and the claim that tag and glyph are two densities of one decision table would be false |
| Hue ownership | **web** (`kind-labels.ts`), as the single `hueFor(typeLabel)` table | core (hue is a CSS-token concern; the TUI has its own palette); a *third* hue spelling alongside the existing `valueHue`/`valueTypeClass` (would recreate finding A inside the module meant to cure it) |
| Kind switch discoverability | **added to the core Action menu**, single-Node-only | Leaving ADR 0009's exception in place: it rests on the control being "self-labeling", which the glyph is not |

## 4. Design

### 4.1 core — one new function, one new field, zero removals

`session/type_filter.rs` gains, **directly beneath `classify`** (so a new
`TypeToken` breaks both matches in one file):

```rust
pub fn kind_glyph(kind: &NodeKind, format: Format, doc: DocFormat, read_only: bool) -> &'static str
```

an exhaustive `match` over the `TypeToken` that `classify` returns (36 arms → 10
glyphs, the compiler enforcing coverage):

| Glyph | Covers |
|---|---|
| `{}` | table (scope/dotted/inline/multiline), map (block/flow) |
| `[]` | array (inline/multiline), array-of-tables, seq (block/flow) |
| `abc` | every string notation |
| `123` | integer (every radix), float (plain/exp/inf/nan) |
| `tf` | bool |
| `??` | null |
| `@` | all four datetimes |
| `#` | comment |
| `⌂` | root |
| `!` | YAML **opaque** node (`t-null` hue) |

Two vocabulary notes against the pre-review draft: **Root is `⌂`, not `""`** — the
breadcrumb's existing hardcoded choice (`breadcrumb.ts:81,184`) is promoted to
canonical, because an empty glyph leaves a hole in the outline column. And the
breadcrumb's `··` unknown-key fallback (`breadcrumb.ts:57`) is **deleted**: an
exhaustive Rust match has no unknown key.

Published as `ViewRow.kind_glyph` and `ChildView.kind_glyph` (`session/view.rs`),
filled in `Session::to_view_row` and `children_of` (`session/session.rs:238,437`).
`ChildView` today carries none of the classify inputs, but the builder has all of
them in hand (`Node` carries `format`/`read_only`, `Session` knows the doc format),
so this is a fill-in, not new plumbing.

`badge_label` / `badge_note` are **retained** — the panel, the `K` popup's
`Current:` header, and the composed desktop hover title all still need them. core is
therefore purely additive: one function, one field on two structs.

**Hue does not go into core.** The hue table currently local to `breadcrumb.ts`
moves to `kind-labels.ts` as `hueFor(typeLabel)` and becomes the **single** hue
source: the existing `valueHue(r)` / `valueTypeClass(r)` are reimplemented as thin
wrappers over it, and `panel.ts:216`'s `valueHue(r) || "branch"` patch is deleted
(`hueFor` already covers branches and comments, which `valueHue` returns `""` for).
Net result: two single sources of truth, each with one owner — **glyph = core,
hue = web**.

### 4.2 web + touch tree row

New row anatomy:

```
indent → caret → [warn] → glyph → key → = → value → [trailing]
```

`renderKindBadge` (`render.ts:97`) and touch's forked `kindBadgeHTML`
(`touch/render.ts:56`) are replaced by one glyph renderer emitting the glyph between
the schema-warning triangle and the key (or the comment text). The label, the `·note`
suffix, and the chevron are removed from the row.

Interactivity keeps **today's data attributes verbatim** (`data-kind="1"` on desktop,
`data-act="kind"` on touch) so the delegated handlers in `ui.ts:1288` and
`touch/app.ts` need no change: a `<button>` when `!read_only && !isCommentRow(r)`, a
plain `<span>` otherwise. Two deliberate visibility changes, previously left implied:

- **Comment rows now show `#`**, which they previously lacked entirely — this aligns
  the tree with the breadcrumb.
- **Read-only rows now show a glyph too** (as a non-interactive span), and a YAML
  opaque node shows `!` rather than its underlying kind's glyph.
- The **breadcrumb mini-tree** therefore also changes: `!` for an opaque child, and
  no `··` fallback.

On touch, `.kind-glyph` gets a transparent **≥44px hit box** (padding / `::before`
overlay, not a larger glyph), because it now sits between two other hit regions (the
caret toggle and the row body) instead of standing alone at the row's end.

**Hover title (desktop).** The static `title="table · scope"` of the pre-review draft
is *not* used: `onTreeHover` (`ui.ts:1217-1228`) lazily writes the **schema hint**
into the `.title` of `[data-kind]` and short-circuits on `if (cell.title)`, so a
static attribute would permanently suppress the schema hint on that element. Instead
`onTreeHover` becomes the single writer and **composes** the title as
`kind · notation` (from `badge_label`/`badge_note`) plus the schema hint,
newline-joined. Touch gets no title, having no hover.

Files: `web/render.ts`, `web/touch/render.ts`, `web/breadcrumb.ts` (drops its local
`GLYPHS`/`glyphHTML` and reads `ChildView.kind_glyph` + `hueFor`),
`web/kind-labels.ts` (+`hueFor`, wrappers), `web/panel.ts` (§4.4), `web/ui.ts`
(composed title), `web/types.ts` (the hand-written serde mirror gains `kind_glyph` on
both structs), `web/style.css` and `web/touch/style.css` (`.kind-glyph` reusing
`.crumb-glyph` sizing and the `t-*` hue tokens; the `.kind` badge and `.chev` rules
retire, `.kind-note` stays for the panel).

### 4.3 TUI — retire the column widget, keep the column position

The tree `Table` goes from three columns to two:

- merged first column width = `name_col_width(total) + TYPE_WIDTH + 1` (absorbing
  one column gap — verified arithmetically exact: VALUE still starts at
  `name + TYPE_WIDTH + 2`, `ui.rs:89-92,301-308`),
- **`value_col_width`'s result is unchanged**, so the VALUE column is identical to
  today's and the inline editor's windowing, overflow hint, and `/` filter input see
  zero change. (Its *body* does reference the name width and gap count and is
  re-derived; only the value is invariant.)

The NAME cell becomes — note the tag is **before** the indent:

```
marker + [S:lit ] + space + indent + branch-marker + warn + space + key
```

This is the review's central correction. Placing the tag after the indent would
forfeit finding E: the tag would clip away entirely on deep or narrow rows, where
today it is always visible. Anchoring it at x=1 retires the column *widget* while
keeping the column *position*, so the tag stays unclippable **and** vertically
scannable — the pre-review draft's only accepted loss disappears. The cost is that
"before the key" is column-adjacent rather than glyph-adjacent; the outline reading
order is preserved.

A depth-1 key still lands at x=16 (`1 + 8 + 1 + 2 + 2 + 1 + 1`), the same offset
true adjacency would have produced, so test churn is identical either way.

`type_tag` is unchanged and still consumed. `type_col_cell`'s per-type colouring and
its `has_fill` skip-colour rule fold into the name-cell builder, which becomes a
`Line` of `Span`s (`Span::raw(marker)`, `Span::styled(tag, hue)`, `Span::raw(rest)`).
The **selection marker stays outermost**: it is a row-scope cue and must not be
indented behind a per-node facet. The `has_fill` skip stays verbatim — it asserts a
real legibility property.

`KEY_X` (a **test-only** const, `ui.rs:784`; nothing at runtime depends on it) moves
7 → 16, and `paste_target_into_fill_suppresses_kind_tag_color`'s
`kind_x = name_col_width(40) + 1` becomes `kind_x = 1` — depth- and width-independent,
i.e. the test gets simpler, which is the evidence this layout is the right one.

Files: `crates/confy-tui/src/tui/ui.rs` (rendering + its in-file `mod tests`),
`crates/confy-tui/src/tui/tests.rs`. `tui/app.rs` is untouched.

### 4.4 Detail panel

The Kind field's anonymous `.dotc` colour dot is replaced by the **same glyph** (mono,
hue-coloured via `hueFor`); the `type_label · note` text stays. The locked field order
(Key → Value → Trailing comment → Kind → Path → Children → Sign) is untouched.

The panel is deliberately **not** simplified: once the tree gives up notation, the
panel is its only always-visible home.

File: `web/panel.ts`.

### 4.5 Column header (TUI)

The merged column's header must read `KIND` at x=1 and `NAME` at x=16, or it would
label its column by content that no longer leads it. So **no i18n key retires** —
`tui.header.kind` stays and is composed into the single merged cell with
`tui.header.name`. The pad between them is computed in code from `TYPE_WIDTH` and the
same prefix arithmetic the row builder uses, measured by **display width** (zh-TW
`類型`/`名稱` are 2 chars but 4 columns). `tui.header.name` drops its two hand-tuned
leading spaces (`"  NAME"` → `"NAME"`) so no catalogue string smuggles layout.

### 4.6 Kind switch in the Action menu

ADR 0009's membership rule excludes an operation when "the node already carries a
dedicated, always-visible control for it", and justifies excluding Kind switch by the
badge being "a self-labeling control that *displays* the current kind while offering to
change it". A bare glyph still displays the kind but is no longer self-labeling and has
no disclosure affordance, so the exception no longer holds.

Kind switch is added to `session/action_menu.rs` as a **single-Node-only** item (by the
glossary's own test: the core state behind it, `kind_options(path)`, carries one Path),
positioned among the structural operations rather than at the top, with a new
`core.action.kind-switch` key in both catalogues. It opens `Mode::KindSwitch` for the
cursor path. ADR 0009's exception clause is **deleted**, not reworded, and
CONTEXT.md:301-304 follows.

### 4.7 Legend / i18n

`i18n/en.json` and `i18n/zh-TW.json`:

- `web.help.legend.{toml,json,yaml}` — currently three monolithic label·notation
  tables — are rewritten as a **glyph legend plus one line** stating that notation
  words live in the detail panel, the `K` switch, and the `f` type filter.
- `tui.help.legend.*` (66 keys) keep their content unchanged — the tags only moved.
- `tui.header.kind` is **kept** (§4.5), `tui.header.name` loses its padding.
- `core.action.kind-switch` is added (§4.6).

File: `web/help-content.ts` follows whatever structural change the legend rewrite
needs (it is shared by desktop, touch, and the VS Code host, which inherits every web
change here by embedding `web/dist` verbatim).

## 5. Documentation

New `docs/reference/ROW_ANATOMY.md`, following the cross-platform reference convention
of `ROW_STATE_MODEL.md` / `MESSAGES.md`:

- the canonical glyph table (glyph ← `classify`, hue ← `type_label`),
- per-surface row anatomy for TUI / web desktop / touch / breadcrumb / panel,
- the divergence table and its rationale: a fixed-pitch grid affords 8 columns and a
  proportional inline chip does not (finding D); scalar notation is already printed in
  the VALUE cell so removing it is lossless (finding B); branch notation is
  deliberately off the row and retrieved via hover, panel, or `f` (finding C); the tag
  is column-anchored rather than indent-following because the column position is what
  makes it unclippable (finding E); `type_tag` and `kind_glyph` are two densities of
  the same `classify` decision table.

New **ADR 0011** — the glyph-in-core / hue-in-web ownership split, recording the
shared-`kind-labels.ts` alternative and compile-time exhaustiveness as the deciding
reason. (No ADR for §4.3: cheap to reverse and self-evident once read.)

Updated: **`docs/reference/CONTEXT.md`** — section `KIND column tags (full vocabulary)`
→ **`Kind tag vocabulary`**; the five "KIND-column vocabulary" references (:322-323,
:445-446, :525, :549-550) updated; :301-304's Action-menu exception clause deleted
(§4.6); and :52's **already-stale** claim that the `(B)/(Q)/(D)/(-)` key sign is
"surfaced as the prefix in the KIND column" corrected in the same pass — it is not, and
never was in this codebase: `type_col_cell` renders only `row.type_tag`, and `(B) bare`
appears in the Detail popup (`ui.rs:904`) and the `f` type filter.

Also updated: `docs/adr/0009-*.md` (exception clause), `docs/reference/README.md` (index
row), `docs/reference/TUI.md` §Rendering (the KIND-column paragraph becomes the
column-anchored kind-tag prefix), `docs/reference/WEBUI.md` (row anatomy + link),
`CLAUDE.md` module map (`render.ts`, `breadcrumb.ts`, `type_filter.rs`, `view.rs`,
`ui.rs`, `action_menu.rs` lines), `CHANGELOG.md` (Unreleased Update entry).

## 6. Verification

1. `cargo fmt --check` → `cargo clippy -- -D warnings` → `cargo test`, with new tests
   for: `kind_glyph` coverage over every `TypeToken` (including `Opaque` → `!` and
   `Root` → `⌂`), the TUI tag rendering at x=1 **on a depth-10 row at 60 columns**
   (the finding-E regression guard), the header containing both `KIND` and `NAME` at
   the right offsets, and `value_col_width` being unchanged.
2. `cd web && npm run typecheck && npm test && npm run build`, updating
   `render.spec.mjs`, `touch-render.spec.mjs`, `panel-schema.spec.mjs`,
   `touch-clip-source.spec.mjs`, `touch-comment-advisory.spec.mjs`.
3. `cd crates/confy-ffi && wasm-pack build --target web && node functional_smoke.mjs`
   (the `ViewRow`/`ChildView` wire contract gained a field).
4. **Real-binary check** (green unit tests are not the bar): `cargo run -- <fixture>`
   inspected at 60 and 100 columns **and at depth ≥ 10**, and `web/dist` opened in a
   browser on both the desktop and the touch entry — confirming the composed hover
   title still shows the schema hint, and the touch glyph's hit box does not steal
   caret or row taps.

## 7. Accepted losses

- A branch's container notation (`scope` / `dotted` / `inline` / `multi` / `AoT`) is no
  longer visible on a web tree row. Recovery paths: the composed desktop hover title,
  the detail panel's Kind field, and the `f` type filter (which still indexes every
  notation facet).
- On touch, with no hover, the only recovery path is the detail sheet.
- In the TUI: **none**. The pre-review draft accepted losing vertical alignment; §4.3's
  column-anchored placement keeps both alignment and unclippability, so the TUI trades
  only the KIND header cell's independence for the merged cell.

## 8. Impact

core 3 files (`type_filter.rs`, `view.rs`, `session.rs`) + `action_menu.rs` · web 8
files (`render.ts`, `touch/render.ts`, `breadcrumb.ts`, `kind-labels.ts`, `panel.ts`,
`ui.ts`, `types.ts`, + 2 CSS = 9 counting both stylesheets separately) · i18n 2 files ·
TUI 2 files · docs 8 files (incl. new `ROW_ANATOMY.md`, new ADR 0011, `CONTEXT.md`,
ADR 0009) · web specs 5.
