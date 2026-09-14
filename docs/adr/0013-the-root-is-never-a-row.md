# ADR 0013 — The document Root is a model node, never a view row, on every host

- **Status:** Accepted (2026-09-11); **implemented 2026-09-14** (`dc69a47`…`4328778`, plan `docs/plan/2026-09-11-root-hidden-alignment.md`)
- **Scope:** `confy-core` (`session/session.rs`, `session/clipboard.rs`, `session/action_menu.rs`,
  `session/type_filter.rs`, `session/insertion.rs`), `confy-tui` (`tui/ui.rs`), `web/`
  (desktop + touch + VS Code hosts)
- **Related:** [0005](0005-row-cursor-selection-clipboard-state-model.md) (modal lock),
  [0009](0009-centralized-action-menu-core-owned.md) (core owns the Action menu hosts render),
  [0010](0010-pointer-drops-resolve-through-pasteslot.md) (slots, not host-computed indices);
  design record [`../spec/2026-09-11-root-hidden-alignment-design.md`](../spec/2026-09-11-root-hidden-alignment-design.md);
  retrospective [`../debug/2026-09-11-root-row-alignment-retrospective.md`](../debug/2026-09-11-root-row-alignment-retrospective.md)

## Context

`NodeTree::flatten` walks from `self.root`, so the document Root has always been row 0 at
depth 0 (`model/node.rs:255-272`). Exactly one host draws that row — the TUI. Both web hosts
filter it out and then compensate for the hole it leaves: a depth rebase in each renderer
(`web/render.ts:130`, `web/touch/render.ts:74`), a cursor re-target after every keyboard nav
(`drawnCursorFallback`), a paste-mode step correction (`overshotUndrawnRootSlot`), a stand-in
edge renderer for the Root's two paste slots (`rootSlotLine`), a selection filter
(`web/select.ts:11`) and a faked `SetCursor: []` before Convert (`web/host-io.ts:218`).
`HOST_PARITY.md` §2 exists for this one asymmetry.

Four defects follow from it, all re-verified on `main` at `db3f700` and enumerated as P1–P4 in
the design record: the clipboard can be armed with the Root and dead-end the modal lock; two
web-only routes produce a tree with no rows and no cursor; the web has no entry point at all to
the whole-document text edit the TUI reaches from the Root row; and `paste_slots()` emits
`Into([])` first while it resolves to an append at the document's *end*.

A first attempt (ADR 0013 on branch `root-row-alignment`, rewound before release) tried the
opposite direction — teach every host to draw the Root, made configurable by a
`Session::root_visible` flag. It was reversed because the two "modes" it made configurable never
existed as product requirements, and because the direction "TUI aligns to web" had never been
put on the options table at all.

## Decision

**The Root is a model node and never a view row.** `Session` stops emitting it to every host:
no Root row, no Root cursor, no Root-anchored selection. The Root keeps everything it is in the
model — `node_at(&[])`, `Target { parent: [] }`, whole-file `Replace`, the filter's ancestor
chain — so no mutation or backend changes.

Whole-document operations stop being "operations performed on the Root row" and become
**document-scoped commands** reachable identically on every host:

- **Edit whole file** is an Action-menu item (`ActionId::EditDocument`), placed immediately
  above `Delete` and carrying that section's separator. It dispatches the whole-file external
  edit that already exists (`external_edit_path(&[])` returns the empty path unwrapped).
- **Convert** (`C`) drops its root-cursor precondition; `core.convert.root-only` is retired.

Two invariants make the removal total rather than another host-side hide:

1. **Nothing is host-configurable.** There is no `root_visible`, no `Intent::SetRootVisible`.
2. **Every compensation is deleted, not ported.** Cursor seeding moves into core
   (`from_tree` seats the first top-level Node), depth is rebased in core, and `paste_slots()`
   is re-emitted in screen order (`After([])` first, `Into([])` last) so the correction each
   web host applied disappears for everyone. The only host-side rule that survives is the
   shared one both document-edge slots now need on *all* hosts: draw them as an insertion line
   on the first row's top / last row's bottom edge.

The Root-never-selected guard at the selection boundary is kept even though D1/D3 make its
routes unreachable from the current UI — it is the honest fix for P1 and a few lines of
insurance at the boundary a future host could still cross.

## Alternatives rejected

| Alternative | Why rejected |
|---|---|
| **Every host draws the Root row** (the reversed branch: `root_visible` + `SetRootVisible`, desktop root-visible, touch/VS Code root-hidden) | Built a per-host toggle before the product decision stabilized; the two modes were never both required. Made the TUI the parity oracle for a row the pointer hosts had spent seven stand-ins hiding, and left P3 (no web entry point for whole-file operations) as a *rendering* question instead of a command question. |
| **Keep the row in core, keep hiding it per host** (status quo) | It is the status quo that produced P1–P4. Every new host pays the compensation cost again, and "unreachable by current UI paths" — not "impossible" — is the invariant that let P1 rot. |
| **Drop the Root's two paste slots entirely** | "Insert above everything" and "append at the document end" would become unreachable: `After(<first row>)` is not root index 0, and `After(<last row>)` is inside whatever parent that row belongs to. The slots are re-ordered, not removed. |
| **Give `EditDocument` its own separator-led section below `Delete`** | Considered and settled against (design record D9): it puts a non-destructive item after the destructive one and moves `Delete` off the list's last row, which muscle memory already owns. The item goes *above* `Delete`, taking over the section separator. |
| **Keep a host-side `drawnCursorFallback` in the TUI instead of seeding the cursor in core** | Exactly the pattern that made the defect class possible: a host papering over core's seating. Core seats the cursor on a row it actually emits. |

## Consequences

- The TUI loses one visible row, its Root-row `e`/`i` routes (replaced by the menu item and by
  Detail-on-a-real-node), and four expand/collapse special cases (`collapse_all`'s root
  re-insert, `collapse_level`'s two guards, the Root's toggle). Its title bar must read the Root
  node's key directly instead of `rows.first()` — otherwise it blanks.
- An empty document is now a genuinely empty tree on every host: core resolves an add target to
  `{ parent: [], index: 0 }` with no cursor row, and every host needs an empty-state affordance.
  Previously the TUI always had one row to stand on.
- `HOST_PARITY.md` §2 is deleted rather than rewritten; *root-visible* / *root-hidden* never
  become host-facing vocabulary.
- The `[G] root` type-filter facet disappears — it could only ever filter *to* a row that no
  longer exists.
- Reversal cost, if this direction is ever revisited: the core changes are the reversible part
  (one flatten consumer, one slot order, one cursor seeding); the deleted host stand-ins are
  recoverable from `git` history and from branch `root-row-alignment`, which is kept.
