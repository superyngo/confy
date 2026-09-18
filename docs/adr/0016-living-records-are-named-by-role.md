# ADR 0016 — Living records are named by role, not dated (deviates from docs principle 9)

- **Status:** Accepted (2026-09-18), implemented (2026-09-18)
- **Scope:** `docs/plan/BACKLOG.md` (renamed from `docs/plan/2026-09-09-open-follow-ups.md`),
  the 39 citations of it across 18 files, `CONTEXT.md`, `docs/plan/README.md`, `CLAUDE.md`
- **Deviates from:** `wens-dev-principles docs 9` (working-record filenames are
  `YYYY-MM-DD-kebab-title.md`), as required by `wens-dev-principles docs 15`
- **Related:** `wens-dev-principles docs 7` (freeze-on-landing) and `docs 17` (the one living
  backlog is its only exception)

## Context

Docs principle 9 requires every working record under `spec/`, `plan/`, `debug/`, `audit/` to be
named `YYYY-MM-DD-kebab-title.md`. Principle 17 carves out exactly one file from principle 7's
freeze-on-landing: the living backlog. Those two rules met in a filename that described the file
as something it is not — `docs/plan/2026-09-09-open-follow-ups.md`, a permanently-edited document
wearing a single date.

The date earns its place on a frozen record: it marks when that judgement was formed, it sorts
the folder chronologically, and it pairs a spec with its plan by slug. A living record has none of
those properties — its content is always current, nothing pairs with it, and its creation date is
recoverable from git.

Two concrete costs were paid in this repo before the rename:

- The file reads as a snapshot of 2026-09-09, so a later sweep filed its findings *beside* it
  rather than *into* it — `docs/audit/2026-09-15-open-follow-ups-evaluation.md` exists because the
  tracker looked like a dated artifact to be evaluated rather than the live list to be edited.
- On 2026-09-17 a refile sweep re-opened two rows whose fixes had already shipped (VS Code
  Raw-write parity P1–P4 in `3573085`, the MSIX headless-CLI manifest in `5b4bf5f`). The table had
  no column saying when each row was last checked; the only date in sight was the one in the
  filename, which belonged to the whole document.

## Decision

**A living record is named by role, in caps, with no date: `docs/plan/BACKLOG.md`.** Dated
filenames are reserved for frozen-lifecycle records. The resulting repo-wide rule is readable off
any filename without opening it:

> **dated ⇔ frozen snapshot; undated ⇔ living.**

`CONTEXT.md`, `CHANGELOG.md`, and everything in `docs/reference/` already obeyed it; `adr/NNNN-*`
is the deliberate third case (frozen, but sequence-numbered because ADRs are cited by number).

The 39 citations in 18 files were rewritten in the same commit as the rename, including the 11 in
frozen records. That path repair is not a revision — findings, evidence, and conclusions are
untouched — and the alternative is a dead link in every frozen record that cited the old path,
precisely when someone is tracing a decision. The skill was amended in the same session to permit
mechanical path repair explicitly (`wens-dev-principles` docs 7) and to prescribe the undated name
(docs 9, 17), so the next repo does not repeat this.

## Considered options

- **Keep the dated name** — rejected: the two costs above are recurring, not one-off, and the
  citation count only grows. 39 was the smallest this rename would ever be.
- **Root `BACKLOG.md`, symmetric with `CHANGELOG.md`** — rejected: docs principle 1 restricts the
  repo root to `README.md`, `CHANGELOG.md`, `CONTEXT.md`, `LICENSE`, and platform-mandated files,
  and that restriction is itself a MUST worth more than the symmetry.
- **`docs/BACKLOG.md`, outside the four working-record folders** — rejected: principle 2 fixes
  `docs/` to seven folders; a loose file at that level opens the door to more. The backlog is the
  one live `plan/`, and `plan/README.md` already indexes it in its `## In progress` section.
- **Leave the old path as a stub that forwards** — rejected: it keeps the frozen records
  untouched, but leaves a file in `plan/` whose only purpose is redirection, which `plan/README.md`
  would then have to explain forever.
- **Rename but leave frozen citations dangling** — rejected: a literal reading of principle 7 that
  makes the archive unnavigable to protect a path string nobody cited for its own sake.

## Consequences

- One undated file lives in `docs/plan/`; the folder `README.md` and the documentation-audit
  checklist both name it as the sole exception, so a *second* undated file there is a finding.
- Frozen records may be touched again if `BACKLOG.md` ever moves. The rule is narrow: path string
  only, in the same commit as the move.
- Two pre-existing line-range citations into the old filename
  (`docs/spec/2026-09-11-root-hidden-alignment-design.md` §7 and
  `docs/audit/2026-09-15-open-follow-ups-evaluation.md`) were left as written. They were already
  rotted — line numbers into a living file cannot survive — and rewriting them would be a content
  edit to a frozen record rather than path repair.
