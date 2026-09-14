# ADR 0014 — Whole-document editing reuses each host's existing text surface

- **Status:** Accepted (2026-09-14), implemented (2026-09-14)
- **Scope:** `web/ui.ts` (Raw pane), `web/touch/app.ts` (external-edit sheet),
  `crates/confy-tui/src/tui/editor.rs` (`$EDITOR`), `editors/vscode/` (VS Code's own editor),
  `confy-core` (`session/inline_edit.rs`, `session/intent.rs`, `session/action_menu.rs`)
- **Related:** [0009](0009-centralized-action-menu-core-owned.md) (the Action menu owns the
  entry point), [0013](0013-the-root-is-never-a-row.md) (whole-document operations became
  document-scoped commands once the Root stopped being a row)
- **Design record:** [`../spec/2026-09-11-raw-write-mode-design.md`](../spec/2026-09-11-raw-write-mode-design.md)

## Context

ADR 0013 removed the document Root from every host's node tree. That deleted the one row a
user could put the cursor on to edit the whole file as text — a route only the TUI ever had
(`e` on the Root row). The capability has to come back, and it has to come back on four hosts
whose text-editing surfaces are already fixed by their platforms:

- the **TUI** spawns `$EDITOR` and owns no text widget of its own;
- the **desktop web** host already has a full-height read-only Raw pane (`session.serialize()`);
- the **touch** host already has a full-screen external-edit bottom sheet, and no room for a
  second full-pane surface;
- **VS Code** *is* a text editor, and its `TextDocument` is the single source of truth for
  content, dirty state, undo, and save (ADR 0007).

The tempting alternative is one uniform new surface — a whole-file modal, shared by every
host, the way the existing per-node external-edit popup is shared.

## Decision

**Whole-document editing is carried by each host's existing multi-line text surface, not by a
new uniform one.** Core owns the operation (one `Intent`, one document-scoped Action item, one
whole-file `Replace`); each host routes it to the surface it already has:

| Host | Surface |
|---|---|
| Desktop web | the Raw pane switches from **Raw view** to **Raw write** |
| Touch | the existing external-edit bottom sheet, with an empty path |
| TUI | `$EDITOR`, as it always did |
| VS Code | suppressed — the workbench's own editor already is this feature |

Core's contract is uniform and the surfaces are not. The predicate a host routes on is the
**empty path**: a pending external edit at `[]` is the whole document, anything else is a
node fragment.

## Consequences

- **The surfaces disagree, deliberately, and that is documented as parity rows** rather than
  smoothed over — `HOST_PARITY.md` carries one row per divergence.
- **No new modal, no new widget.** The desktop cost is a `<textarea>` behind the `<pre>` it
  already renders; touch's cost is an empty-path branch at one call site; the TUI's is zero.
- **VS Code loses nothing by being suppressed**, and gains no second editable copy of a
  buffer it already owns — which would have been two owners of one document (ADR 0007's
  failure mode).
- **A uniform modal would have been wrong on three of four hosts**: absurd on the TUI (which
  has no widget), redundant on VS Code, and an unusable full-file editor in touch's popup
  geometry. The alternative's only virtue — one code path — is preserved anyway, in core.
- **Cost of reversal:** replacing this with a uniform surface means writing that surface for
  three hosts and deleting three routes; the core contract (Intent, Action item, whole-file
  `Replace`) survives either way, so the reversible part is host-local.
