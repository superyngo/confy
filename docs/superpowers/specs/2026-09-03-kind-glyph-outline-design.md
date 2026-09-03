# Kind annotation as a VS Code-style outline glyph before the key

- **Date:** 2026-09-03
- **Status:** Approved design (implementation plan not yet written)
- **Scope:** presentation only — no `Mutation`, no document semantics, no keymap change

## 1. Problem

One facet (a node's kind/notation) is spelled three different ways across five
surfaces, and the vocabulary the user wants to standardize on lives in a
web-only local constant:

| Surface | Position | Rendering | Source |
|---|---|---|---|
| TUI tree | between NAME and VALUE, fixed 8-col KIND column | `[T/S]` `[S:lit ]` `[I:hex ]` | `type_tag` (`tui/app.rs`) ← core `classify` |
| Web desktop tree | after value, before trailing comment | `table·scope ⌄` button (`.kind`) | `ViewRow.badge_label` / `.badge_note` |
| Touch tree | same | same | same |
| Breadcrumb | **before the segment label** | `{}` `[]` `abc` `123` `tf` `??` `@` `#` + hue | `breadcrumb.ts` local `GLYPHS` const (web-only) |
| Detail panel | Kind field (locked 4th) | `● table · scope` (anonymous colour dot) | `type_label` + `badge_note` |

Three findings drive the design:

- **A.** The glyph table is a web-only local const, keyed on a label string
  rather than `NodeKind`, and is not reachable by the tree, touch, or panel.
  That is an existing single-source-of-truth break (`wens-dev-principles ui 1`).
- **B.** For a **scalar**, notation is *already* fully visible in the VALUE
  cell, because `ViewRow.value` carries the raw source repr (`0xFF`, `'lit'`,
  `"""…"""`, `1e3`, `inf`). The `·0x` / `·'…'` / `·1e` suffix is pure
  redundancy — deleting it loses nothing.
- **C.** For a **branch** it is not redundant: a branch row has no VALUE cell,
  so `·scope` / `·dotted` / `·inline` / `·multi` / `AoT` exists nowhere else on
  the row. This is the only real information loss, and it is accepted
  deliberately (§7).
- **D.** The cost/benefit is inverted between hosts. In a fixed-pitch grid an
  8-column KIND column is *a column* — it crowds nothing. On the web the badge
  is an inline chip competing with value and comment for the same row. So
  "simplify" has real value on the web and near-zero value in the TUI, where it
  would be pure information loss.

## 2. Goals

1. The kind annotation moves **before the key** on every host (VS Code outline
   position).
2. On the web hosts the annotation **simplifies to a type glyph**; notation is
   no longer hinted on the tree row.
3. The annotation stays **clickable** and still opens the kind switch
   (`Mode::KindSwitch`).
4. The detail panel is aligned to the same symbol.
5. The remaining TUI ↔ web rendering differences are documented.

Non-goals: changing `Mutation`, `kind_options`, the `f` type filter, the `K`
popup contents, the keymap, or the panel's locked field order.

## 3. Decisions

| Decision | Choice | Rejected alternatives |
|---|---|---|
| TUI scope | **Position parity, density preserved** — the KIND column is retired and the existing 8-col tag moves before the key | Full parity (3-col glyph in the TUI too — loses the power surface's at-a-glance notation and breaks the documented `type_tag` ↔ `classify` arm-for-arm pairing); web-only (maximum divergence, TUI unmoved) |
| Branch notation | **Not compensated on the row** — hover title, detail panel, and the `f` type filter carry it | Notation-aware container glyphs (`{.}`/`{…}`/`[[]]` — grows the set from 8 to 13 and stops being VS Code's clean vocabulary); folding it into the item-count cell (`3 items · scope` — puts the removed noise back on the same row) |
| Glyph ownership | **core** (`kind_glyph`) | per-host tables (the status quo break) |
| Hue ownership | **web** (`kind-labels.ts`) | core (hue is a CSS-token concern; the TUI has its own palette) |

## 4. Design

### 4.1 core — one new field, zero removals

`session/status_fmt.rs` gains:

```rust
pub fn kind_glyph(kind: &NodeKind) -> &'static str
```

an exhaustive `match` over `NodeKind` (so the compiler enforces coverage),
returning the breadcrumb vocabulary verbatim: `{}` Table/InlineTable, `[]`
Array/ArrayOfTables, `abc` String, `123` Integer/Float, `tf` Bool, `??` Null,
`@` all four datetimes, `#` Comment, `""` Root.

It is published as `ViewRow.kind_glyph` and `ChildView.kind_glyph`
(`session/view.rs`), filled in `Session::to_view_row` and the `children()`
builder (`session/session.rs`).

`badge_label` / `badge_note` are **retained** — the panel, the `K` popup's
`Current:` header, and the new desktop hover title all still need them. core is
therefore purely additive: one field on two structs.

**Hue does not go into core.** The hue table currently local to
`breadcrumb.ts` moves to `kind-labels.ts` as `hueFor(typeLabel)` and is shared
by the desktop tree, touch tree, breadcrumb, and panel. Net result: two single
sources of truth, each with one owner — **glyph = core, hue = web**.

### 4.2 web + touch tree row

New row anatomy:

```
indent → caret → [warn] → glyph → key → = → value → [trailing]
```

`renderKindBadge` is replaced by a glyph renderer emitting the glyph between
the schema-warning triangle and the key (or the comment text). The label,
the `·note` suffix, and the chevron are removed from the row.

Interactivity follows the current rule exactly: a `<button data-kind="1">`
when `!read_only && !isCommentRow(r)`, a plain `<span>` otherwise — today
read-only and comment rows carry no badge at all, so no row gains or loses a
control. **Comment rows now show the `#` glyph**, which they previously
lacked entirely; this is an intentional improvement that aligns the tree with
the breadcrumb.

Desktop adds `title="table · scope"` (built from `badge_label`/`badge_note`);
touch does not, having no hover.

Files: `web/render.ts`, `web/touch/render.ts`, `web/breadcrumb.ts` (drops its
local `GLYPHS`/`glyphHTML` derivation and reads `ChildView.kind_glyph` +
`hueFor`), `web/kind-labels.ts` (+`hueFor`), `web/style.css` and
`web/touch/style.css` (`.kind-glyph` reusing `.crumb-glyph` sizing and the
`t-*` hue tokens; the `.kind` badge and `.chev` rules retire, `.kind-note`
stays for the panel).

### 4.3 TUI

The tree `Table` goes from three columns to two:

- merged first column width = `name_col_width(total) + TYPE_WIDTH + 1`
  (absorbing one column gap),
- **`value_col_width` is left byte-for-byte unchanged**, so the VALUE column is
  identical to today's and the inline editor's windowing, overflow hint, and
  `/` filter input see zero change.

The NAME cell becomes:

```
marker + indent + branch-marker + warn + [S:lit ] + space + key
```

`type_tag` is unchanged and still consumed; `type_cell`'s per-type colouring
and its `has_fill` skip-colour rule fold into the name-cell builder. The header
row loses its KIND cell (`tui.header.kind` retires). `KEY_X` moves from 7 to
16.

**Accepted consequence:** once the tag enters the indentation flow it is no
longer vertically aligned, so scanning a whole column of tags is no longer
possible. That is the unavoidable price of position parity with the web, and it
is recorded in the documentation rather than mitigated.

Files: `crates/confy-tui/src/tui/ui.rs`, `crates/confy-tui/src/tui/tests.rs`.
`tui/app.rs` is untouched.

### 4.4 Detail panel

The Kind field's anonymous `.dotc` colour dot is replaced by the **same glyph**
(mono, hue-coloured); the `type_label · note` text stays. The locked field order
(Key → Value → Trailing comment → Kind → Path → Children → Sign) is untouched.

The panel is deliberately **not** simplified: once the tree gives up notation,
the panel is its only always-visible home.

File: `web/panel.ts`.

### 4.5 Legend / i18n

`i18n/en.json` and `i18n/zh-TW.json`:

- `web.help.legend.{toml,json,yaml}` — currently three label·notation tables —
  are rewritten as a **glyph legend plus one line** stating that notation words
  live in the detail panel, the `K` switch, and the `f` type filter.
- `tui.help.legend.*` per-tag entries keep their content unchanged (the tags
  only moved).
- `tui.header.kind` is removed from both catalogues.

File: `web/help-content.ts` follows whatever structural change the legend
rewrite needs.

## 5. Documentation

New `docs/reference/ROW_ANATOMY.md`, following the cross-platform reference
convention of `ROW_STATE_MODEL.md` / `MESSAGES.md`:

- the canonical glyph table (glyph ← `NodeKind`, hue ← `type_label`),
- per-surface row anatomy for TUI / web desktop / touch / breadcrumb / panel,
- the divergence table and its rationale: a fixed-pitch grid affords 8 columns
  and a proportional inline chip does not (finding D); scalar notation is
  already printed in the VALUE cell so removing it is lossless (finding B);
  branch notation is deliberately off the row and retrieved via hover, panel,
  or `f` (finding C); `type_tag` and `kind_glyph` are two densities of the same
  `classify` decision table.

Updated: `docs/reference/README.md` (index row), `docs/reference/TUI.md`
§Rendering (the KIND-column paragraph becomes the kind-tag prefix),
`docs/reference/WEBUI.md` (row anatomy + link), `CLAUDE.md` module map
(`render.ts`, `breadcrumb.ts`, `status_fmt.rs`, `view.rs`, `ui.rs` lines),
`CHANGELOG.md` (Unreleased Update entry).

## 6. Verification

1. `cargo fmt --check` → `cargo clippy -- -D warnings` → `cargo test`, with new
   tests for: `kind_glyph` coverage, the TUI tag rendering before the key on the
   same row, the header no longer containing `KIND`, and `value_col_width`
   being unchanged.
2. `cd web && npm run typecheck && npm test && npm run build`, updating
   `render.spec.mjs`, `touch-render.spec.mjs`, `panel-schema.spec.mjs`,
   `touch-clip-source.spec.mjs`, `touch-comment-advisory.spec.mjs`.
3. `cd crates/confy-ffi && wasm-pack build --target web && node functional_smoke.mjs`
   (the `ViewRow` wire contract gained a field).
4. **Real-binary check** (green unit tests are not the bar): `cargo run -- <fixture>`
   inspected at 60 and 100 columns, and `web/dist` opened in a browser on both
   the desktop and the touch entry.

## 7. Accepted losses

- A branch's container notation (`scope` / `dotted` / `inline` / `multi` /
  `AoT`) is no longer visible on a web tree row. Recovery paths: desktop hover
  title, the detail panel's Kind field, and the `f` type filter (which still
  indexes every notation facet).
- On touch, with no hover, the only recovery path is the detail sheet.
- In the TUI, tags are no longer vertically alignable for column scanning.

## 8. Impact

core 3 files · web 7 files (2 of them CSS) · i18n 2 files · TUI 3 files · docs 6
files · web specs 5.
