# Node operations are centralized in one core-owned Action menu

Status: accepted (2026-08-30) — design approved, implementation pending
Spec: [`../superpowers/specs/2026-08-30-action-menu-design.md`](../superpowers/specs/2026-08-30-action-menu-design.md)

Node operations had accumulated across five surfaces that disagreed with each other: the
desktop per-row `⋮` menu (ten items, hardcoded English, no i18n keys), the shared detail
panel's four action buttons, the floating `+` FAB's context-aware add, the Tauri native
Edit menu's three clipboard verbs, and the TUI's single-keystroke bindings. Touch had no
menu at all, so a multi-node **Locked selection** was unoperable there. The same logical
operation was implemented three times with three different eligibility rules.

We decided to make **one Action menu** whose item list, per-item eligibility, localized
labels, and open state are owned by `confy-core` as a `ModeView::ActionMenu` variant,
rendered three ways (desktop popup, touch bottom sheet, TUI overlay on `m`). The per-row
`⋮`, the panel's action buttons, and the FAB's add heuristic are deleted; rows keep only a
move grip on both web surfaces, and the detail panel becomes editing-and-information only.

## Why core-owned rather than a shared TypeScript module

A web-shared module plus a TUI-local Rust list was cheaper, and is exactly the drift the
codebase already suffers from — three eligibility computations is how we got here. Putting
the item model in core buys one eligibility computation, one i18n source (labels resolved
core-side via `tr`, as `ModeView::Prompt.question` already is), and one keyboard cursor,
which is why the web menus gain arrow-key navigation they have never had. The cost is a
`ModeView` variant plus wire types in three hosts.

## Membership rule

An operation belongs to the Action menu when core can express it as a single intent over
the target set, **unless the node already carries a dedicated, always-visible control for
it**. In-place text entry belongs to the detail panel.

The rule was written because the original item list was inherited from the hardcoded
`buildCtxMenu` rather than derived, which made every marginal item arguable. It excludes
exactly two things:

- **Kind switch** — the kind badge (`render.ts`) is always visible on every row *and* in
  the panel, and it is a self-labeling control that *displays* the current kind while
  offering to change it. That is categorically different from the generic, unlabeled `⋮`
  this ADR deletes. Touch's row badge is not tappable today, so touch reaches Kind switch
  through the panel; that asymmetry is pre-existing and deliberately left alone.
- **Append comment** — it has no single intent (`EditField` has only `Value` and `Name`),
  the TUI cannot create a trailing comment at all, and both web hosts already create,
  change, and clear one through the panel's trailing input, which we keep.

Swipe-revealed delete/remark on touch do *not* exempt Delete or Toggle comment from the
menu: they are hidden at rest, so they are gestures, not controls. Detail's
double-click/tap likewise.

Dimming follows from the type signatures rather than from policy: an item is
single-node-only exactly when the core state behind it carries one `Path`
(`ExternalEditKind{path}`, one insertion point, one rendered `ViewRow`). The set-applying
intents — `CopySelected`, `CutSelected`, `Remark`, `DeleteSelected` — stay enabled on a
multi-node selection. Ineligible items are shown disabled rather than hidden, because a
selection containing a read-only node can disable seven of eight items, and a menu that
silently collapsed to one row would teach the user nothing.

## Consequences

- **The detail panel closes when the Action menu opens.** `Mode` is a single-slot enum
  with no mode stack, and the desktop panel is `Mode::Detail`-driven. Rather than invent
  core's first return-mode mechanism for one case, we accept the close: the Cursor is
  unchanged, so the menu targets the same node. This is not new behavior — the panel's own
  kind badge already does exactly this via `Mode::KindSwitch`.
- **Paste is not an Action menu item.** Paste is legal only while **Clipboard-armed**, and
  opening the menu is refused in that state (armed already blocks mutations, ADR 0005 §5),
  so the item could never be reached. This also makes today's `⋮` Paste entry visibly what
  it always was: dead code. Pasting stays on the armed Paste button, click/tap-to-target,
  and `v`.
- **The native Edit menu keeps Copy/Cut/Paste Node, exempt from the item model.** An OS
  Edit menu is expected to hold clipboard verbs, and it is reached by muscle memory rather
  than discovery. Its items carry no eligibility, so a read-only node is offered "Cut
  Node" there while the Action menu disables it. Unifying that is a separate decision;
  recorded here so it is not mistaken for an oversight.
- **Two affordances are given up.** The per-row hover-to-act menu (covered by right-click,
  the Action button, and `m`) and the FAB's one-tap context-aware add (Add child and
  Append sibling are now explicit items, so no host-side heuristic guesses between them).
- **VS Code inherits the change for free** — its host is a message-passing adapter over
  the same desktop UI, contributing no node-operation commands of its own.

## Rejected alternatives

- **Host-local open state, core-owned items only.** Keeps `Mode` untouched, but each host
  then owns its own open/close and keyboard cursor — three implementations of the thing we
  are consolidating, and no shared arrow-key navigation.
- **A return-mode on `Mode::ActionMenu`** so `Escape` restores the detail panel. Cleaner
  for the user, but it introduces a mode-stack concept core does not have, and would
  immediately make the long-shipped kind-badge behavior look like a bug.
- **Keeping Delete adjacent to Cut**, as the `⋮` menu has it. Moved below a separator
  instead: the same list is now a full-width touch sheet where a mis-tap is cheap and
  irreversible-looking. Existing desktop muscle memory is the accepted cost.
