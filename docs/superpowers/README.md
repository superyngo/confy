# Working record — specs, plans, audits, debug notes

**Everything in this directory is a historical record.** These files capture how confy got to
where it is: what was proposed, what was decided, what was tried and abandoned. They are dated
and deliberately *not* kept in sync with the code.

For current behavior, read [`../reference/`](../reference/README.md), `CLAUDE.md`, and
`CHANGELOG.md` — never a file in here.

## Layout

| Directory | Holds | Lifecycle |
|---|---|---|
| [`specs/`](specs/) | Design records — the shape of a change and the alternatives weighed, written before implementation. | Frozen once approved. |
| [`plans/`](plans/) | Task-by-task implementation plans derived from a spec. | Frozen once shipped. |
| [`audits/`](audits/) | Point-in-time sweeps for bugs, dead code, and inconsistency. | Frozen once findings are addressed. |
| [`debug/`](debug/) | Handoff notes from an in-progress investigation. | Frozen once the bug is fixed. |

Durable decisions extracted from this material live in [`../adr/`](../adr/README.md).

## Status banners

Every finished document opens with a banner so a reader knows within one line whether it
describes reality:

```
✅ **Shipped — historical reference.** See `CHANGELOG.md` for current behavior; this plan is
kept for context, not as a live task list.
```

Audits and debug notes use `✅ **Resolved — historical reference.**` instead.

**A file without a banner is live work.** As of 2026-08-28 exactly one qualifies:

- `plans/2026-08-28-json-jsonc-parser-simplification-ssot.md` — consensus reached, not yet
  implemented. Its comment-gate-removal half shipped separately (CHANGELOG 2026-08-28); the
  parser-unification half has not.

## Conventions

- Filenames are `YYYY-MM-DD-kebab-title.md`, dated when the document was written, not when the
  work landed.
- Plans and specs pair up by slug (`…-vscode-schema-hints.md` ↔ `…-vscode-schema-hints-design.md`).
- Superseded documents are never deleted or rewritten. The banner points at whatever replaced
  them — a false start is part of the record and often explains a later design more clearly than
  the design doc does.
