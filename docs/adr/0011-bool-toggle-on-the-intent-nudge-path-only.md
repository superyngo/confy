# A bool toggles on the `Intent::Nudge` path only, never inside `nudge_scalar`

Status: accepted and implemented (2026-09-07)
Partially reverses: commit `534dd4a` (`feat(nudge)!: drop bool nudge; gate wheel/swipe numeric
nudge to inline-edit via nudge_repr`) and its plan
`docs/plan/2026-09-01-nudge-redesign.md` §1.

## Context

`534dd4a` removed **every** boolean nudge affordance on every platform: the TUI's `←/→`, the web
keyboard's `←/→`/`+`/`-`, and the web tree's mouse wheel. It did so with one edit — deleting the
`ScalarType::Bool` arm from `nudge_scalar` — which silently no-ops every caller at once.

The defect that motivated it was specific and real, but it was **not the keyboard**:

- `web/ui.ts` had a delegated `wheel` listener on the whole tree (`onTreeWheel`) plus a
  `toggleBool()` helper. A bool flipped when the pointer merely *rested* over its row and the user
  scrolled the page. Nothing was armed, focused, or clicked first.
- `web/panel.ts` had the analogous touch problem: a horizontal swipe anywhere near the value field
  stepped it.

The same commit fixed those properly and independently — the wheel/swipe nudge now requires the
value field to be focused (inline-edit mode), arms from a document-level `pointerdown` capture, and
writes through the new stateless `Session::nudge_repr` rather than dispatching per tick. Dropping
`Bool` from `nudge_scalar` was a second, broader change bundled into the same commit, and the
keyboard was collateral: a TUI `←/→` requires the cursor to already be on the node and cannot fire
from hovering, because there is no hover.

The cost of that collateral removal is that a bool's only remaining edit affordance is the
two-option picker (`Mode::SchemaEnum`, `from_schema: false`) — a popup, three keystrokes
(`e`, select, `Enter`), for a value with exactly two states.

## Decision

Restore the keyboard bool toggle, and **only** the keyboard one, by putting the flip in
`Session::nudge` (`session/inline_edit.rs`) rather than back in `nudge_scalar`.

```rust
let stepped = if st == ScalarType::Bool {
    bool_flip(&repr)
} else {
    nudge_scalar(st, format, &repr, delta)
        .and_then(|new| self.schema_clamp_nudge(&path, &repr, &new, delta))
};
```

Consequences of that placement, which is the whole point of this ADR:

| Caller | Path | Bool |
|---|---|---|
| TUI `←/→` | `KeyAction::{Dec,Inc}Value` → `Intent::Nudge` → `Session::nudge` | toggles |
| Web keyboard `←/→`, `+`/`-` | `key-intent.ts` → `Intent::Nudge` → `Session::nudge` | toggles |
| Web tree wheel, panel wheel | `ui.ts`/`panel.ts` → `Session::nudge_repr` → `nudge_scalar` | unchanged (numeric-only) |
| Touch swipe | `panel.ts` → `Session::nudge_repr` → `nudge_scalar` | unchanged (numeric-only) |
| Touch, no keyboard | — | picker only |

Supporting decisions:

- **Direction-independent.** Both `←` and `→` flip. A two-member domain has no ordering worth
  encoding, and this matches the pre-`534dd4a` behaviour and the word users reach for ("toggle").
  The alternative — `→` sets `true`, `←` sets `false` — is idempotent under key-repeat but invents
  a polarity the format does not have.
- **Authored casing is preserved.** YAML accepts `true`/`True`/`TRUE`; flipping `TRUE` to `false`
  would silently re-case the document. The casing table that already backed the picker is extracted
  as `bool_pair` and shared by `bool_picker_options` and the new `bool_flip`, so there is one rule,
  not two that can drift.
- **`nudge_scalar` keeps returning `None` for `Bool`.** Its unit test and the `nudge_repr` bool
  test both stay as `534dd4a` wrote them — they now document the pointer-surface contract rather
  than a global one.
- **The picker stays.** It is still what `e` opens on a bool, still what touch has, and still what
  a schema `enum` outranks. The toggle is an accelerator, not a replacement.

## Consequences

- The keyboard affordance returns on the TUI and web desktop at the cost of one branch in
  `Session::nudge`; no wire contract, no `Intent`, and no host code changes.
- `nudge_scalar`'s meaning narrows from "everything nudgeable" to "everything nudgeable **by a
  pointer gesture**". That distinction is not self-evident from the name, so both `Session::nudge`
  and `bool_flip` carry doc comments pointing here.
- A future surface that wants a numeric nudge must go through `nudge_repr`/`nudge_scalar` and will
  get the safe behaviour by default. One that wants the keyboard semantics dispatches
  `Intent::Nudge`. Adding a bool flip to a *pointer* surface now requires deliberately calling
  `bool_flip`, which is the review checkpoint this split exists to create.
