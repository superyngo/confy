# ADR 0012 — A TOML datetime cross-type switch is a value `Replace` behind `K`, not a `ConvertKind`

- **Status:** Implemented (2026-09-07)
- **Scope:** `confy-core` — `session/datetime.rs`, `Session::open_kind_switch`,
  `session/add_picker.rs`
- **Related:** [0009](0009-centralized-action-menu-core-owned.md) (core owns the surfaces hosts
  render), `docs/superpowers/plans/2026-09-07-datetime-kind-switch.md`

## Context

TOML has four datetime types: `ScalarType::{OffsetDatetime, LocalDatetime, LocalDate,
LocalTime}`. A user editing `expires = 2026-01-01` who wants `2026-01-01T00:00:00Z` had no way
to say so except retyping the whole literal by hand in the inline editor.

`K` (kind switch) is the obvious key for it, but `K` is built on
`Mutation::ConvertKind`, whose documented invariant (`model/document.rs`) is *"rewrite a node's
kind/notation in place — another notation of the **same** kind"*. `[T/I]`↔`[T/S]`, hex↔decimal,
literal↔folded: same value, different spelling. TOML's four datetimes are not four notations of
one kind, they are four **types** carrying different information (a local date has no time; an
offset datetime has a zone). That is why `kind_options` returns an empty list for a datetime
node (`model/cst_doc.rs`) and `K` reported `core.kind-switch.unsupported`.

Separately, the `a` Add-type picker listed all four datetime types as four consecutive rows —
a quarter of the TOML picker spent on one scalar family.

## Decision

**`K` on a datetime node diverts to the existing `Mode::SchemaEnum` value picker
(`from_schema: false`) instead of `Mode::KindSwitch`.** Each option's *value* is the
fully-rendered target literal; each option's *label* is `"<type>  <literal>"`, built by the
shared `model::kind_label::align_options` helper (Amendment 1 — it was `"<type>  <literal>
(<loss/fill>, …)"` as first shipped). The pick commits as an ordinary value `Replace`.

Two properties make this cheap rather than a workaround:

1. `Session::schema_enum_commit` deliberately routes through `edit_commit` rather than applying
   a `Replace` directly, *precisely* so a picked value that changes the node's underlying type
   hits `Mode::Prompt(PromptKind::TypeChange)`. The confirmation this feature needs therefore
   **already existed** — no new `PromptKind`, no new prompt plumbing.
2. `Mode::SchemaEnum` is already rendered by all four hosts (TUI popup, web `<select>`, touch
   sheet, VS Code webview), so the feature reaches every host with **zero host code**.

Net new machinery is one pure module, `session/datetime.rs`: `parse_toml_datetime` decomposes a
literal, `retype` re-renders it as any of the four types and returns an ordered `Vec<Loss>`
naming everything dropped or auto-filled.

The `a` picker's four datetime rows collapse to one, seeded as an **offset datetime** — the
widest of the four, so a later `K` switch only ever narrows and never has to fill.

### Rejected alternative: four `KindTarget::Datetime*` variants

The direct reading of "make `K` do it" is to add `KindTarget::{OffsetDatetime, LocalDatetime,
LocalDate, LocalTime}`, return them from `kind_options`, and add a cross-type arm to
`convert_scalar`. Rejected on four counts:

- It breaks `ConvertKind`'s same-kind invariant, the one property that makes a kind switch
  safe to apply without asking.
- `kind_switch_commit` applies directly with no prompt gate, so the loss confirmation would
  need new plumbing there — the exact machinery the chosen route inherits for free.
- `KindTarget` is `Serialize`/`Deserialize` and mirrored in `web/types.ts`: four new variants
  are a wire-contract change.
- `convert_scalar` exists in all three backends, so a TOML-only feature would touch JSON and
  YAML code paths.

## Fill policy

A widening switch must invent the component the source lacks. The rules are fixed, not
clock-happy:

| Missing | Filled with | Why |
|---|---|---|
| time | `00:00:00` | A fixed midnight keeps the authored date exactly what the user sees, and makes the result reproducible. Reading the clock here would silently attach an unrelated time to a date the user chose. |
| offset | `Z` | UTC, matching the `a` picker's existing datetime seeds. |
| date | the **UTC** clock | The only component with no meaningful fixed default; `1970-01-01` would be a lie. UTC, never the host timezone — the rule `add_picker.rs`'s seeds already follow. |

`parse_toml_datetime` keeps the fractional second (with its leading `.`) and the offset
(`Z`/`+09:00`) **verbatim**, so an authored `.5` is never re-rendered as `.500` and a same-kind
round-trip is byte-identical.

## Consequences

- Zero new `Mutation`, `KindTarget`, `Mode`, `PromptKind`, wire-contract, or host changes.
- ~~The loss/fill disclosure lands in the **picker label**, one step *before* the
  confirmation prompt.~~ **Amended 2026-09-08 — see Amendment 1.** The disclosure lands on
  the confirmation prompt; the picker label is `"<type>  <literal>"`.
- **`K` now opens one of two popups depending on the node** (the kind-switch list, or the
  value picker for a datetime). This is the real cost of the decision and is documented in
  `docs/reference/TUI.md`.
- `e` on a datetime is unchanged: free-form literal editing stays the inline editor's job.
  Only `K` opens the picker.
- No new dependency. Date arithmetic reuses `add_picker`'s existing wasm-safe
  `now_utc_parts`/`civil_from_days` (public-domain Hinnant algorithm).
- JSON and YAML are structurally unaffected: neither projects a datetime `ScalarType`, so the
  divert can key off the node's type with no format check.

## Amendment 1 (2026-09-08) — the disclosure moves to the confirm, and the label format is shared

Two things were wrong with putting `(<loss/fill>, …)` in the option label, both visible the
moment the feature was used:

1. **It read nothing like a kind option.** Every other `K` row is `"<name>  <sample>"`
   (`literal string  '…'`, `hex  0x…`); a datetime row was `"local date  1979-05-27  (drops
   the time, drops the offset)"` — up to 55 columns, wrapping or clipping the TUI popup, whose
   width is 40% of the terminal. The "before the confirmation" argument also oversold itself:
   the confirmation is one keypress later and cancels for free, so nothing is lost by
   disclosing there.
2. **The literal column never lined up**, because the type names are *translated* and so of
   variable width. Every other list hand-typed its padding into the label literals — which a
   translated list cannot do, and which silently breaks whenever an option is added.

So: the label is now `"<type>  <literal>"`, the cost moves to the `PromptKind::TypeChange`
question via a new `note: Option<String>` field (`datetime::change_note`, derived from the
**old and new values** — so a hand-typed `e` that retypes a datetime discloses the same
thing), and both the datetime list and all three backends' `kind_options` now build their
labels through one shared helper, `model::kind_label::align_options`. It pads the name column
in **display cells** (`unicode-width`, a new `confy-core` dependency — pure, wasm-safe), which
is what makes an aligned *translated* list possible at all.

The original decision — the divert itself, the value-`Replace` commit, the fill policy — is
unchanged. The "**zero host changes**" claim in the Decision section was separately found to be
false and is corrected in the plan document; it was a web-host routing bug, not an ADR one.

## Amendment 2 (2026-09-08) — reusing the value *mode* must not mean reusing the value *widget*

The Decision's second cheapness argument ("`Mode::SchemaEnum` is already rendered by all four
hosts, so this reaches every host with zero host code") was true and still cost something it
didn't predict: each host renders that mode in whatever surface it uses for picking a **value**.
On the desktop web UI that is an inline `<select>` in the row's value cell — so the one kind
switch that went through this route appeared as a dropdown inside the row, while every other
kind switch was a list box (a popover on a badge click, the `#overlay` list on `K`). Same
operation, two different gestures and two different widgets: the user's report was that it read
as an inequality, not as a feature.

`SchemaEnumState`/`ModeView::SchemaEnum` therefore gain **`from_kind_switch: bool`**. Unlike the
neighbouring `from_schema` — which hosts use only to *title* the popup — this one selects a
**widget**: these options are kind options, so a host with a dedicated kind-option surface
renders them there. Desktop now routes by entry point, matching the notation lists exactly
(badge click → `#kindMenu` popover anchored at the badge; `K` → `#overlay` list), and gates its
inline `<select>` off (`render.ts::valuePicker`) so the widget is never drawn twice. TUI and
touch already had one surface for both and needed no change.

The alternative — inferring it host-side from "the last intent I dispatched was
`OpenKindSwitch`" — was rejected: it is shadow state duplicating something core already knows,
and it survives re-renders only by accident (every `SchemaEnumMove` re-enters the render pass).

This is the honest correction of the "zero host changes" claim: the *mode* was free, the
*presentation* was not.
