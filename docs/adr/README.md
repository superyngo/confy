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
| [0002](0002-jsonschema-crate-for-validation.md) | JSON Schema validation uses the `jsonschema` crate, not a hand-rolled validator | Implemented |
| [0003](0003-audit-remediation-undo-cap-and-tui-dispatch-boundary.md) | Undo history is capped and lossy; TUI routes only mutations through `dispatch(Intent)` | Implemented |
| [0004](0004-unified-clipboard-move-targeting.md) | Unify node copy/cut/paste/move targeting across TUI, web keyboard, web mouse, and touch | Implemented (2026-08-19, v0.20.0) |
| [0005](0005-row-cursor-selection-clipboard-state-model.md) | Formalize the row cursor/selection/clipboard-source state model and unify its interaction and visual language across TUI, desktop, and touch | Implemented (2026-08-18) |
| [0006](0006-outline-symbol-representative-span-anchoring.md) | Editor-outline symbol ranges for scattered-definition nodes anchor at the first member, never an envelope | Implemented (2026-08-20) |
| [0007](0007-vscode-schema-session-in-place-replace.md) | VS Code native-editor schema session updates in place via `ApplyReplace`, never rebuilt per keystroke | Implemented (2026-08-21) |
| [0008](0008-in-session-diagnostic-ring-over-tracing.md) | Diagnostics are an in-Session ring buffer, not `tracing` | Implemented (2026-08-21) |
| [0009](0009-centralized-action-menu-core-owned.md) | Node operations are centralized in one core-owned Action menu, replacing the per-row `⋮`, the detail panel's action buttons, and the `+` FAB | Implemented (2026-08-30) |

No ADR has been superseded to date.

See also [`../superpowers/`](../superpowers/README.md) for the plans, specs, and audits these
decisions came out of.
