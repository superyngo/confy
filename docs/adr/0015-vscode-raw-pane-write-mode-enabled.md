# ADR 0015 — VS Code enables the Raw pane's write mode (partially supersedes ADR 0014)

- **Status:** Accepted (2026-09-16), implemented (2026-09-16)
- **Scope:** `web/ui.ts` (`VSHOST` gates around `EditDocument`/Raw write), `web/raw-jump.spec.mjs`
- **Related:** [0014](0014-whole-document-editing-reuses-each-hosts-text-surface.md) (this ADR
  partially supersedes R10 only — the desktop/touch/TUI routing in 0014 is unaffected),
  [0007](0007-vscode-schema-session-in-place-replace.md)

## Context

ADR 0014's R10 suppressed the Raw pane's write mode under the VS Code webview host, reasoning
that "a second editable text surface over the same `TextDocument` is two owners of one buffer."
Re-examining the mechanism: VS Code's webview already mutates the shared `TextDocument` for
every ordinary tree edit today — `notifyHost()` posts `edit { text: session.serialize() }` after
any `Session` mutation, and the extension host applies it as a minimal-span `WorkspaceEdit`
(`VSCODE.md`'s message protocol). The Raw pane's write-mode `Apply` is not a new channel: it
dispatches `Intent::BeginEditDocument` → commits via `apply_document_text` (`Mutation::Replace {
path: [] }`) → the same `on_mutation_success` → `notifyHost` → `edit` path every other mutation
already uses. There is no second buffer and no second owner of the `TextDocument` either way —
the "two owners" framing described a UX redundancy (VS Code's own tab-swap editor already offers
full-featured raw-text editing over the same file), not a data-integrity hazard.

## Decision

**Enable the Raw pane's write mode under the VS Code webview host too.** The four `VSHOST` gates
that suppressed it are removed: the Action menu's `EditDocument` item now renders and its keyboard
commit is no longer blocked, the crumbs-row band's primary control is no longer force-disabled,
and the toolbar overflow menu no longer filters `EditDocument`/`btnRawEdit` out. VS Code now
follows the same Raw pane routing as desktop web (ADR 0014's R1); only the TUI (`$EDITOR`, R9) and
touch (its own sheet, R18) keep their host-specific surfaces, unaffected by this ADR.

## Considered options

- **Keep R10 as-is** — rejected: the reasoning it was built on (a second buffer/owner) does not
  hold once the shared `edit`/`WorkspaceEdit` mechanism is traced end to end; the only real cost
  of enabling it is a redundant (if more convenient — no tab swap) path to a feature VS Code's
  native editor already has.
- **Enable it, but only when no side-by-side native text editor tab is open on the same
  document** — rejected as unnecessary complexity: the existing `text-changed`/stale-tree-pause
  machinery (`VSCODE.md` §Stale-tree pause, §Expansion + cursor restore) already handles
  concurrent side-by-side typing against the same `TextDocument` for ordinary tree edits, and
  Raw write's `Apply` is exactly that same mutation path — no new race is introduced.

## Consequences

- `HOST_PARITY.md`'s "Whole-document edit's Edit control" row and `VSCODE.md`'s "Whole-document
  editing is suppressed under this host" section no longer apply to VS Code and are updated.
- ADR 0014's decision table (R1, R9, R18, R20–R29) is otherwise unaffected — this ADR narrows only
  R10's VS Code row.
- **Cost of reversal:** re-adding the four `VSHOST` gates in `web/ui.ts` and flipping
  `raw-jump.spec.mjs`'s assertions back; no core or protocol changes are involved either
  direction.
