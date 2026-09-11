# Architecture Decision Records

One file per decision that was expensive to reach and would be expensive to reverse. An ADR
records *why* a road was taken and which alternatives were rejected — it is a historical record,
not a live specification. Current behavior always lives in
[`../reference/`](../reference/README.md) and `CHANGELOG.md`.

An ADR is never edited to match new behavior. If a decision is revisited, add a new ADR and mark
the old one superseded.

| # | Decision | Status |
|---|---|---|
| [0001](0001-android-save-as-persistable-grant.md) | Android Save As uses a custom SAF plugin command, not stock `tauri-plugin-dialog` | Implemented (2026-08-06) |
| [0002](0002-jsonschema-crate-for-validation.md) | JSON Schema validation uses the `jsonschema` crate, not a hand-rolled validator | Implemented (2026-08-10) |
| [0003](0003-audit-remediation-undo-cap-and-tui-dispatch-boundary.md) | Undo history is capped and lossy; TUI routes only mutations through `dispatch(Intent)` | Implemented (2026-08-11) |
| [0004](0004-unified-clipboard-move-targeting.md) | Unify node copy/cut/paste/move targeting across TUI, web keyboard, web mouse, and touch | Implemented (2026-08-19, v0.20.0); §1 partly superseded by [0010](0010-pointer-drops-resolve-through-pasteslot.md) |
| [0005](0005-row-cursor-selection-clipboard-state-model.md) | Formalize the row cursor/selection/clipboard-source state model and unify its interaction and visual language across TUI, desktop, and touch | Implemented (2026-08-18) |
| [0006](0006-outline-symbol-representative-span-anchoring.md) | Editor-outline symbol ranges for scattered-definition nodes anchor at the first member, never an envelope | Implemented (2026-08-20) |
| [0007](0007-vscode-schema-session-in-place-replace.md) | VS Code native-editor schema session updates in place via `ApplyReplace`, never rebuilt per keystroke | Implemented (2026-08-21) |
| [0008](0008-in-session-diagnostic-ring-over-tracing.md) | Diagnostics are an in-Session ring buffer, not `tracing` | Implemented (2026-08-21) |
| [0009](0009-centralized-action-menu-core-owned.md) | Node operations are centralized in one core-owned Action menu, replacing the per-row `⋮`, the detail panel's action buttons, and the `+` FAB | Implemented (2026-08-30) |
| [0010](0010-pointer-drops-resolve-through-pasteslot.md) | Pointer drops resolve through `PasteSlot` end to end (hosts derive no parent/index), and inline containers keep their `Into` band | Implemented (2026-09-01) |
| [0011](0011-bool-toggle-on-the-intent-nudge-path-only.md) | A bool toggles on the `Intent::Nudge` (keyboard) path only, never inside `nudge_scalar` — pointer wheel/swipe nudging stays numeric | Implemented (2026-09-07) |
| [0012](0012-datetime-cross-type-switch-is-a-value-replace.md) | A TOML datetime cross-type switch is a value `Replace` behind the `K` key, not a `ConvertKind` | Implemented (2026-09-07) |
| [0013](0013-root-visibility-is-core-state.md) | Root visibility is `Session` state (root-visible / root-hidden), hosts render no compensation, and the Root takes the cursor but is never selected | Superseded (2026-09-11) — direction reversed before any release; replacement will be a fresh 0013 on `main` |

ADR 0004 §1's `format != Format::Inline` clause and its host-computed `MoveSelectionTo`
`target`/`index` payload are superseded by ADR 0010. ADR 0011 partially reverses commit `534dd4a`
(bool nudge removal) for the keyboard only. ADR 0013 was superseded in full (2026-09-11) before
any release — direction reversed; the branch keeps its implementation, and a fresh 0013 on `main`
will record the replacement decision.

See also [`../spec/`](../spec/README.md), [`../plan/`](../plan/README.md),
[`../audit/`](../audit/README.md), and [`../debug/`](../debug/README.md) for the design records,
plans, sweeps, and investigations these decisions came out of.
