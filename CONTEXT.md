# CONTEXT

Entry point for all of confy's documentation. Root-level files (`README.md`, `CHANGELOG.md`,
`CLAUDE.md`, `LICENSE`, `PRIVACY.md`) stay here; every other document lives under `docs/`.

| Folder | Holds | Canonical? | Lifecycle |
|---|---|---|---|
| [`docs/reference/`](docs/reference/README.md) | Current behavior: glossary, per-subsystem contracts, per-host contracts | Yes — the only source of truth | Kept in sync with the code |
| [`docs/adr/`](docs/adr/README.md) | Decisions that were expensive to reach and would be expensive to reverse | No — historical | Never edited; superseded by a new ADR |
| [`docs/spec/`](docs/spec/README.md) | Design records written before implementation | No — historical | Frozen once approved; only `Status:` changes |
| [`docs/plan/`](docs/plan/README.md) | Task-by-task implementation plans derived from a spec | No — historical | Frozen once shipped; only `Status:` changes |
| [`docs/debug/`](docs/debug/README.md) | Handoff notes from investigations, with repro scripts | No — historical | Frozen once resolved; only `Status:` changes |
| [`docs/audit/`](docs/audit/README.md) | Point-in-time sweeps for bugs, dead code, inconsistency | No — historical | Frozen once findings are addressed; only `Status:` changes |
| `docs/tmp/` | Scratch, no naming or index rules | No | Archived to `docs/tmp/archive/YYYY-MM.tar.gz` when stale |

Every document in `spec/`, `plan/`, `debug/`, and `audit/` carries a greppable status line as its
second line — `Status: Draft | Approved | In progress | Shipped (YYYY-MM-DD) | Resolved (YYYY-MM-DD) | Superseded by <path> | Abandoned`.
List live work across all four folders:

```sh
rg -n '^Status: (Draft|Approved|In progress)' docs/{spec,plan,debug,audit}/*.md
```

**Known-but-unfixed work has one home:**
[`docs/plan/2026-09-09-open-follow-ups.md`](docs/plan/2026-09-09-open-follow-ups.md). It is the
only living record in `docs/plan/` — every verified open defect and improvement is a row there,
with its evidence, effort and acceptance criterion, and rows move to *Done* with the commit that
closes them. A finding recorded in a frozen audit or a reference footnote **also** gets a row;
otherwise it survives only where nobody looks.

## Reading order

1. [`docs/reference/glossary.md`](docs/reference/glossary.md) — the vocabulary every other file
   and every identifier uses. Read first; the terms are not interchangeable with their synonyms.
2. [`docs/reference/README.md`](docs/reference/README.md) — the subsystem and host map.
3. [`docs/adr/README.md`](docs/adr/README.md) — why the shape is what it is.
4. `CLAUDE.md` — build/test commands, release process, module map, and repo conduct.
5. `CHANGELOG.md` — what changed recently.

## Conventions

The repository's documentation layout, glossary entry format, status-line value set, and
freeze-on-landing lifecycle follow the `wens-dev-principles` **docs** domain. UI behavior
follows its **ui** domain. Deviating from a `MUST` principle in either requires an ADR that
cites the principle by domain and number.

Working records used to live under `docs/superpowers/{specs,plans,audits,debug}/`; they moved to
`docs/{spec,plan,audit,debug}/` on 2026-09-09. `CHANGELOG.md` entries written before that date
cite the old paths.
