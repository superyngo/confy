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
- **Kind glyph** — the **one** rendering of that facet on every host's tree row
  (`{}`, `abc`, `@`), type-only, produced by core's new `kind_glyph`.
- **Notation word** — the full-text notation (`scope`, `dotted`, `multiline`,
  `hex`, `literal`), shown where there is room for words: the TUI's `i` Detail
  popup, the web detail panel, the `K` popup, and the `f` type filter.

Two terms are **retired**. "Kind badge" named the pre-change web control and is now
ambiguous. "Kind tag" named the TUI's dense `[T/S]`/`[S:lit ]` rendering, which this
spec removes from the product along with `type_tag` itself — the bracketed forms
survive only as **documentation shorthand** for a (kind, notation) pair (§5), never
again as something a user sees.

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
- **D.** The cost/benefit *looked* inverted between hosts: in a fixed-pitch grid an
  8-column KIND column is *a column* and crowds nothing, whereas on the web the badge
  is an inline chip competing with value and comment. The pre-review draft concluded
  from this that the TUI should keep the dense tag. **Rendered side by side on the
  real binary this was overruled** (§3): the 8-col tag is visual noise in front of
  every key, and the 5 columns it costs are worth more than notation the VALUE cell
  usually shows anyway. The finding survives only as the reason the *Detail popup*
  keeps notation words while the *row* does not.
- **E.** The pre-change tag was **unclippable**: it lived in its own
  `Constraint::Length(8)` column, so it rendered at every depth and width. Any
  indent-following annotation forfeits that. With a 3-column glyph the exposure is
  ~5x smaller (at 60 columns clipping needs roughly depth 11, not depth 8), and what
  gets clipped is a type hint whose notation was never there — so the loss is
  accepted rather than designed around (§7).

## 2. Goals

1. The kind annotation moves **before the key** on every host (VS Code outline
   position).
2. The annotation **simplifies to a kind glyph on every host**, TUI included;
   notation is no longer shown on any tree row.
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
| TUI scope | **Full glyph parity** — the KIND column *and* the 8-col kind tag retire; the TUI renders the same 3-column glyph, after the indent and branch toggle, before the key (§4.3) | Keeping the dense tag (the pre-review choice, twice): rejected on sight of both layouts running — the tag reads as noise in front of every key, and one vocabulary across five surfaces is worth more than row-level notation that the VALUE cell already implies. Column-anchoring the annotation at x=1 (implemented, then reverted): a fixed first-painted column flattens every row's visual origin and the indent stops reading as structure. Web-only glyph: maximum divergence |
| Branch notation | **Not compensated on any row** — the `i` Detail popup, the web detail panel, the desktop hover title, the `K` switch, and the `f` type filter carry it | Notation-aware container glyphs (`{.}`/`{…}`/`[[]]`: grows the set and stops being VS Code's clean vocabulary); folding it into the item-count cell (`3 items · scope`: puts the removed noise back on the same row) |
| Glyph ownership | **core** (`kind_glyph`), for **one reason only: compile-time exhaustiveness** — a new `TypeToken`/`ScalarType` must break the build, not degrade to a fallback glyph at runtime | **One shared table in `web/kind-labels.ts`** — the genuine runner-up, and strictly cheaper: it also cures finding A (all four consumers are in `web/`, and the TUI consumes no glyph at all), with zero core files, no FFI wire field, no `web/types.ts` mirror edit, and no `functional_smoke.mjs` update. Rejected because a table keyed on a `type_label` string is unchecked and drifts silently. Recorded as ADR 0011 |
| Glyph inputs | **`classify`-derived** (`kind, format, doc, read_only`) | `&NodeKind` alone: cannot express a YAML **opaque** node, so an unmutatable node would render identically to a normal one, and the claim that tag and glyph are two densities of one decision table would be false |
| Hue ownership | **web** (`kind-labels.ts`), as the single `hueFor(typeLabel)` table | core (hue is a CSS-token concern; the TUI has its own palette); a *third* hue spelling alongside the existing `valueHue`/`valueTypeClass` (would recreate finding A inside the module meant to cure it) |
| Kind switch discoverability | **added to the core Action menu**, single-Node-only | Leaving ADR 0009's exception in place: it rests on the control being "self-labeling", which the glyph is not |
| Violation cue in the annotation | **Deleted** — the row's dedicated warning column (`▲` own violation / `△` descendant) is the sole cue | Appending `!` to the glyph (4 columns, and the `!` glyph already means "opaque"); recolouring the glyph (collides with the value-type hue). The old in-tag `!` overwrote the tag's internal pad space and was always redundant with `▲`/`△` |
| Help legend | **One shared 10-row glyph legend** for every host, plus a line pointing at `i` / `K` / `f` for notation words — retiring 66 `tui.help.legend.*` keys and 3 `web.help.legend.*` keys | Per-format legends (nothing left to differ on: the glyph set is format-independent by construction); keeping the 66 tag rows (they would document a rendering no host emits) |
| The 5 reclaimed columns | **To VALUE** — `value_col_width`'s formula is unchanged, so `TYPE_WIDTH` 8 → 3 widens VALUE by 5; key text room is unchanged (the merged cell shrinks 5, its prefix shrinks 5) | Re-deriving the formula to hold VALUE constant and give NAME the 5: NAME is already 40% of the width, and VALUE is the column that truncates first (timestamps, long strings). Cost: the pre-review draft's "`value_col_width` is invariant" safety property is deliberately given up (§6 re-verifies the inline editor because of it) |
| Root glyph in a terminal | **Keep `⌂` (U+2302)** | A TUI-only ASCII stand-in, or ASCII everywhere: both break the one-vocabulary premise to fix a 1-column shift on a single row (`⌂` is East-Asian *ambiguous*; `unicode-width` calls it 1, a few terminals draw 2) |

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

### 4.3 TUI — glyph parity: retire the KIND column *and* the kind tag

The tree `Table` goes from three columns to two and `TYPE_WIDTH` from 8 to 3:

- merged first column width = `name_col_width(total) + TYPE_WIDTH + 1` (absorbing one
  column gap),
- `value_col_width`'s **formula** is unchanged, so its **result grows by 5** — the
  deliberate choice in §3. Nothing narrows; the inline editor's window, overflow hint,
  and `/` filter input all simply get more room (re-verified per §6, since the
  pre-review draft leaned on this value being constant).

The NAME cell, in outline order:

```
sel-marker + indent + branch-marker + warn + glyph(3) + space + key
```

The glyph is **after** the indent and branch toggle: the row's own indent must stay
its visual origin, or the tree flattens into a list (learned by implementing the
alternative and looking at it). A depth-1 key lands at x=10 (`1 + 2 + 2 + 1 + 3 + 1`).

**`type_tag` retires entirely** (`tui/app.rs`, 36 arms). `RowSnapshot.type_tag:
String` becomes the glyph carried on `ViewRow.kind_glyph`, which deletes
`rebuild_rows`'s per-row `tree.node_at(&vr.path)` lookup — that lookup existed *only*
to recover a `NodeKind` for `type_tag`, so the host stops re-walking the tree once per
visible row per keystroke. The violation `!` splice goes with it (§3): `▲`/`△` in the
warning column is the only violation cue, as it effectively already was.

`type_col_cell` folds into the name-cell builder, which becomes a `Line` of `Span`s
(`Span::raw(lead)`, `Span::styled(glyph, hue)`, `Span::raw(" ")`). The TUI keeps its
**own palette** keyed on `type_label` (hue is a per-host concern — §4.1); the
`has_fill` skip-colour rule and the outermost selection marker are unchanged. The
3-column pad is measured by **display width**, not `chars().count()`, so `⌂` follows
the `unicode-width` verdict (§3's accepted 1-column risk on the root row only).

Tests: `KEY_X` 15 → 10; `paste_target_into_fill_suppresses_kind_tag_color`'s `kind_x`
→ 6; `tests.rs`'s 36-row tag table (`"[T/S]   "` …) becomes a 10-row glyph table. The
depth-step guard (`kind_tag_follows_the_indent_so_depth_stays_readable`) **stays** — it
is the regression net for the flattening mistake. The finding-E clip guard is **not**
reinstated (§1 E).

Files: `crates/confy-tui/src/tui/ui.rs`, `tui/app.rs`, `tui/tests.rs`.

### 4.4 Detail panel

The Kind field's anonymous `.dotc` colour dot is replaced by the **same glyph** (mono,
hue-coloured via `hueFor`); the `type_label · note` text stays. The locked field order
(Key → Value → Trailing comment → Kind → Path → Children → Sign) is untouched.

The panel is deliberately **not** simplified: once the tree gives up notation, the
panel is its only always-visible home.

File: `web/panel.ts`.

### 4.5 Column header (TUI)

The glyph rides each row's indent, so no fixed column exists for a `KIND` header to
sit over: the merged cell is labelled **`NAME` alone** and `tui.header.kind` retires
from both catalogues. `tui.header.name` also drops the two hand-tuned leading spaces
it carried (`"  NAME"` → `"NAME"`) so no catalogue string smuggles layout; the single
leading space is applied in code.

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

The legend collapses to **one shared table, ~10 rows**, under a new `core.help.legend.*`
prefix (core owns the catalogue, and the table is now host- *and* format-independent):
one key per glyph plus `core.help.legend.notation-hint`, a single line stating that
notation words live in `i` / the detail panel, the `K` switch, and the `f` type filter.

Retired from `i18n/en.json` + `i18n/zh-TW.json`: **all 66 `tui.help.legend.*` keys**
(TOML 28 / JSON 14 / YAML 24 — they document a rendering no host emits any more), the
**3 `web.help.legend.{toml,json,yaml}`** monolithic tables, and `tui.header.kind`
(§4.5). Added: `core.help.legend.*` and `core.action.kind-switch` (§4.6).
`tui.header.name` loses its padding.

This is the change's largest i18n simplification: 69 keys out, ~11 in, and the two
catalogues stop having to keep a per-format symbol table in sync.

Files: `web/help-content.ts` (shared by desktop, touch, and the VS Code host) and the
TUI's `overlay_help.rs`, which now renders the same shared rows.

## 5. Documentation

New `docs/reference/ROW_ANATOMY.md`, following the cross-platform reference convention
of `ROW_STATE_MODEL.md` / `MESSAGES.md`:

- the canonical glyph table (glyph ← `classify`, hue: core-free — web `hueFor`, TUI
  palette),
- per-surface row anatomy for TUI / web desktop / touch / breadcrumb / panel,
- **what still differs, now that the vocabulary is shared**: the TUI's glyph is
  fixed-pitch and coloured from the TUI palette, is not clickable (`K` is the keyboard
  route), and has no hover title; the web glyph is a button with a composed
  `kind · notation` + schema-hint title. Rationale bullets: scalar notation is already
  printed in the VALUE cell so removing it is lossless (finding B); branch notation is
  deliberately off every row and retrieved via `i` / panel / `K` / `f` (finding C);
  the annotation follows the indent because **depth legibility outranks
  annotation survivability** (findings D, E).
- the note that `[T/S]`-style bracketed forms are **documentation shorthand only**
  from now on, so `BEHAVIOR_MATRIX.md`, `CLAUDE.md`, and CONTEXT.md may keep using
  them without implying a rendering.

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

1. `cargo fmt --check` → `cargo clippy -- -D warnings` → `cargo test`, with tests for:
   `kind_glyph` coverage over every `TypeToken` (including `Opaque` → `!` and `Root` →
   `⌂`), the TUI rendering one 3-column glyph per row with the **notation-bearing tag
   gone** (no `[T/S]` in any rendered buffer), the glyph's x **stepping one indent
   level per depth** (the flattening guard), the header carrying `NAME` and no `KIND`,
   and `value_col_width` being exactly 5 columns wider than before (the §3 choice,
   asserted rather than assumed).
2. `cd web && npm run typecheck && npm test && npm run build`, updating
   `render.spec.mjs`, `touch-render.spec.mjs`, `panel-schema.spec.mjs`,
   `touch-clip-source.spec.mjs`, `touch-comment-advisory.spec.mjs`.
3. `cd crates/confy-ffi && wasm-pack build --target web && node functional_smoke.mjs`
   (the `ViewRow`/`ChildView` wire contract gained a field).
4. **Real-binary check** (green unit tests are not the bar — this spec has already
   been corrected twice by looking at the running product): `cargo run -- <fixture>`
   at 60 and 100 columns, at depth ≥ 10, in `en` **and** `zh-TW`, exercising
   **inline value edit, inline name edit (`Tab`), the `/` filter input, and the `i`
   Detail popup** — the four surfaces that read the widened VALUE column or the
   retired notation. Plus `?` showing the new shared legend, and `web/dist` opened on
   both the desktop and touch entries (composed hover title still shows the schema
   hint; the touch glyph's hit box steals neither caret nor row taps).

## 7. Accepted losses

- A branch's notation (`scope` / `dotted` / `inline` / `multiline` / `AoT`) is no
  longer visible on **any** tree row, TUI included. Recovery paths: the `i` Detail
  popup / web detail panel (whose branch `Format:` line was itself wrong until
  `c153d0c` — the popup reported the kind word, not the notation), the `K` switch, and
  the `f` type filter, which still indexes all 36 facets.
- On touch, with no hover, the only recovery path is the detail sheet.
- The TUI loses its dense at-a-glance notation scan (`[I:hex ]` vs `[I:dec ]` in a
  column) and, because the glyph follows the indent, vertical alignment; on a very
  deep row in a narrow terminal the glyph itself clips (~depth 11 at 60 columns).
- Help no longer **teaches** the notation vocabulary; it points at the three surfaces
  that show it. 66 TUI legend rows are deleted, not rewritten.

## 8. Impact

core 3 files (`type_filter.rs`, `view.rs`, `session.rs`) + `action_menu.rs` · web 8
files (`render.ts`, `touch/render.ts`, `breadcrumb.ts`, `kind-labels.ts`, `panel.ts`,
`ui.ts`, `types.ts`, `help-content.ts`, + 2 CSS) · i18n 2 files (**69 keys out, ~11
in**) · TUI 4 files (`ui.rs`, `app.rs`, `tests.rs`, `overlay_help.rs`) · docs 8 files
(new `ROW_ANATOMY.md`, new ADR 0011, `CONTEXT.md`, ADR 0009, `README.md`, `TUI.md`,
`WEBUI.md`, `CLAUDE.md`) · web specs 5.
