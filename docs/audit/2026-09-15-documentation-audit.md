# Documentation audit — drift since 2026-09-09, and CLAUDE.md's split
Status: Resolved (2026-09-15)

A second full sweep of every **living** document — `docs/reference/`, the six folder indexes,
`CONTEXT.md`, `README.md`, `CLAUDE.md`, and the one living backlog — against the code at
`b5b9647`. The [2026-09-09 audit](2026-09-09-documentation-audit.md) is the baseline: its
findings were all fixed, so this record covers only what drifted since (root-hidden S1–S6 /
ADR 0013, whole-document editing / ADR 0014, the Raw control band) plus what that sweep missed.
Frozen records in `spec/`, `plan/`, `debug/`, `audit/` were again **not** audited for accuracy —
they are historical by design; only their `Status:` lines were touched where the grammar was
illegal.

Method: pass 1 mechanical (index coverage, status-line grammar, filename convention, link
resolution, inline-code-span path resolution against `git ls-files`, `file.rs:NNN` citation
grep, reference-folder hygiene) + pass 2 accuracy — six parallel read-only verifications, one
per document group, every reported claim re-checked against the source by the author before it
was written down. Counts were re-derived, never trusted: `functional_smoke.mjs` was
instrumented and run to count its `check()` calls at runtime (the static call-site count, 150,
is wrong — the harness loops).

## Verdict

Structure held again: **zero** broken links in living docs, **zero** filename violations. The
defects were **drift concentrated in the week's two features**, plus one structural violation
that had been growing since the repo started.

| Class | Found | Worst instance |
|---|---|---|
| WRONG (code contradicts the doc) | 14 | `TUI.md`/`BEHAVIOR_MATRIX.md` said `a` appends as the **last** child; `resolve_target` returns `index: 0` |
| STALE (describes what no longer exists) | 8 | "Save As unavailable on Tauri Android" — M2 shipped `create_writable` on 2026-08-06 |
| MISSING (real behavior absent) | 6 | no glossary entry for whole-document editing / `doc_revision`; `span_of` absent from the FFI table |
| Counts | 5 | `functional_smoke.mjs` 129 → **176** |
| Organization / hygiene | 9 | `CLAUDE.md` carried 456 lines of reference content (principle 3) |
| Drifted invariant test | 1 | `severity_of_covers_the_full_catalog_table` asserted 45 keys; `severity_of` has 47 |

## 1. The structural finding — `CLAUDE.md` was 78% reference

`CLAUDE.md` was 587 lines, of which the module map (456), the host-I/O and packaging prose
(~35) and the `taplo` dependency analysis (~20) are **reference** content: what the code *is*,
not how to work on it. `wens-dev-principles docs 3` says the agent instruction file states
conduct and never restates reference, and the reason showed up in this very sweep — two of the
week's drift defects ("a band of three same-size controls", the Android Save-As claim) lived in
`CLAUDE.md` *because* the fact was duplicated there.

Split (approved before the edit):

- **New [`../reference/ARCHITECTURE.md`](../reference/ARCHITECTURE.md)** owns the workspace
  shape, the full module map, the host file-I/O boundary, per-host build/packaging, and the
  `taplo` dependency surface with its vendoring scope estimate.
- **`CLAUDE.md` keeps conduct**: build/test commands, the release process, the doc-pointer
  table, the two review-gate invariants (core is filesystem-free; every mutation is atomic and
  validated), the `taplo` *decision* plus its `cargo audit` trigger, and terminology. 587 → 121
  lines.
- `CONTEXT.md`'s reading order and `reference/README.md` gained the new file; five reference
  docs that cited "CLAUDE.md's module map" now cite `ARCHITECTURE.md`.

The module map was corrected while moving, not copied blind: it now names
`confy-core/benches/perf.rs`, `confy-tauri`'s `build.rs`/`msix/`/`play/`, the picker crate's
`src/*.rs` + `guest-js/` + `permissions/`, `web/privacy.html`/`manifest.webmanifest`/`icons/`/
`run-tests.mjs` + the 37 spec suites, and `editors/vscode/src/` + `test-integration/`.

## 2. Reference contradicted the code

- **`TUI.md` and `BEHAVIOR_MATRIX.md` Table B both said the `a`-add appends as the branch's
  *last* child.** `resolve_target` (`session/insertion.rs`) returns `Target { parent, index: 0 }`
  for an expanded branch and for the empty path — it inserts **first**. Both corrected; Table B's
  `global` cell now says what is actually reachable (only an empty document, where the host adds
  at `[]`, since no host draws a Root row).
- **`glossary.md` still listed `[G]` root** in the "full vocabulary", named the tag function
  `format_kind_tag` (it is `type_tag`), and stated the padding rule wrong: six-character
  scalar/opaque tags pad *inside* the brackets, but short container/comment tags pad *outside*
  via `format!("{slot:<8}")`. `classify` returns `None` for the Root, so it has no tag at all.
- **`glossary.md §Reveal` said the root "only takes the cursor".** `Session::reveal_path`
  retargets an empty path to the first visible row and reveals *that* (ADR 0013 D13).
- **`TUI.md`'s type filter still listed a `root` facet** — retired with `TypeToken::Root`.
- **`HOST_PARITY.md §1` had the Escape order backwards**: `Session::escape` peels the clipboard
  *before* the locked selection.
- **`HOST_PARITY.md §5` described fuzzy marks as "reverse/bold"**; `highlight_spans_styled`
  uses yellow + `BOLD | UNDERLINED`.
- **`ROW_STATE_MODEL.md` mapped desktop `Space` to `ToggleExpand`** — `resolveKeyIntent` returns
  `native:toggle-branches` so the host can batch one toggle per selected branch — cited a
  nonexistent `selection/selection.rs`, headed §6d "desktop-only" when touch mirrors it, and
  §7 referred twice to a "state #6" that §1's five-state taxonomy does not define.
- **`BEHAVIOR_MATRIX.md §7` omitted two facets** the trait defines and all three backends
  implement: `value_kind` and `trailing_blank_anchor`. Its Table B also understated the `K`
  switch: a `[T/D]` dotted table offers **both** `[T/I]` (flow) and `[T/S]` (block).
- **`WEBUI.md`** claimed the breadcrumb is "Hidden in Raw view" (it is visible in both Raw
  states — the band lives in that row), that the toolbar **Save** button opens `#convDlg`
  (`#btnSave` saves in place; `#btnSaveAs` opens the panel — `CHROME.md` had it right), that a
  "Load button (paste-into-textarea)" is the fallback (it is a hidden `<input type="file">`, and
  the Open button is never hidden), named touch's toast `#toast` (it is `.toast`), listed
  `add-picker-items.ts` as shared with touch (touch draws its own add grid), and called the port
  design record `PORTING.md` — a file that has never existed in this repo.
- **`README.md` twice tied `C` Convert to "the Root node"**, which no host draws.
- **`MUTATIONS.md`** called the empty-path `Replace` "the `$EDITOR`/root rewrite path"; it is the
  whole-document editing route shared by three hosts.

Counts: `functional_smoke.mjs` **129 → 176**; `web/confy.ts`'s wrapper **16 → 18** methods
(it omits only `schema_violations` and `external_edit` — `outline` and `spanOf` are wrapped);
`SessionSnapshot` **21 → 22** fields (`doc_revision` was missing from the list);
`MESSAGES.md` **68 → 70** keys and **45 → 47** `core.*` (13 Error + 18 Warn + 7 Success +
9 Info); `i18n/en.json` **102 → 105** `core.*`. Re-verified as still correct:
`taplo::parser::parse` 49, `taplo::syntax`/`rowan` 28, `taplo::dom` 2, 19 core integration
suites, ~1,240-LOC vendoring scope (1,241 exact), and the two catalogs at key parity (427 each,
no gaps in either direction).

## 3. The tripwire test had drifted with the doc

`MESSAGES.md` §2 pointed at `severity_of_covers_the_full_catalog_table` as the check that keeps
its count honest — and that test listed 45 keys and asserted 45, while `severity_of` has 47
`core.*` arms. ADR 0014's `core.document.apply-failed` (Error) and `core.document.edit-locked`
(Warn) were never added. The test passed because it only checks the keys it lists, so the same
defect was hidden twice, exactly as the audit checklist warns. Both keys added, the assertion
and its message bumped to 47, `cargo test -p confy-core --lib notice` green.

## 4. Reference held history and roadmap

- **`ROW_STATE_MODEL.md §8 "Implementation history"` and `§9 "Out of scope"`** — a phase-by-phase
  rollout log plus a resolved-bug list with `~~strikethrough~~` "fixed" notes — replaced by one
  §8 *Boundaries* section that states what the document does not own and points at the frozen
  plans and the integration audit. The phase plans are indexed by `plan/README.md`; reference
  carries current behavior.
- **`WEBUI.md §"Future structured-diff evolution"`** was roadmap, not behavior. Moved to the
  living backlog's *Watching* section, where an unscheduled idea can actually be tracked.

## 5. Records and indexes

- Three documents were **unindexed**: both 2026-09-11 specs and the root-hidden plan. Added.
- `plan/README.md` still listed the raw-write-mode plan as `Approved` under *In progress*, and
  `debug/README.md` the root-row retrospective — both shipped/resolved 2026-09-14. Moved to
  *Landed*.
- Four **status lines** broke principle 8's fixed value set: three carried trailing prose after
  the value and one read `Approved (2026-09-14)` (a value that takes no date, on a plan that had
  shipped). Normalized; the prose that was riding on those lines became a normal first paragraph,
  which is the only content edit a frozen record may take alongside its `Status:`.
- `adr/README.md` still showed **0013 and 0014** as "implementation pending" — both are
  `Implemented (2026-09-14)`.
- `HOST_PARITY.md` jumped §1 → §3 (ADR 0013 deleted the old §2) and its whole-document row
  pointed at that dead §2 while still claiming "web hides the Root row". Renumbered 1–5, row
  rewritten, ADR 0014 added as an authority.
- One new **backlog row**: four groups of orphaned i18n keys (`tui.prompt.*` legacy combined
  strings, `web.prompt.q.*`, `core.action.title`, `web.host.{add.*,kind.no-options}`) defined in
  both catalogs and referenced by no source file. Deleting catalog keys is a code change, so it
  is scheduled, not done here. The backlog's `taplo` *Watching* entry now points at
  `ARCHITECTURE.md` for the scope estimate.

## Not fixed — deliberate

- **Two broken links inside frozen plans**: `2026-08-28-json-jsonc-comment-gate-removal.md` uses
  a markdown link whose target is the Rust path `Self::set_filename` rather than a file, and
  `2026-09-07-datetime-kind-switch.md`'s ADR link is missing its
  `../adr/` prefix. Principle 7 allows only a `Status:` edit on a frozen record; a cosmetic
  link repair is not worth the exception, and neither link carries information the record needs.
- **`docs/tmp/` is 617 MB**, almost all `claude-scratch/frag_probe/target/`. Still `.gitignore`d,
  still nothing in the repository, still the maintainer's call — unchanged from the 2026-09-09
  finding. This sweep added one scratch file of its own,
  `docs/tmp/claude-scratch/doc-audit/smoke-count.mjs` (the instrumented copy of
  `functional_smoke.mjs` used to count checks at runtime).
- **`RELEASES.md`/`TAURI.md`'s `publish-play.yml`** references describe a workflow that does not
  exist yet; both phrase it as planned, so they are accurate as written.

## Follow-up

Two rot classes still have no automated gate, unchanged from the last sweep: a repo path inside
an inline code span, and a `file.rs:NNN` line citation. Both are one grep; both were run by hand
here. A third is now worth naming: **an invariant test whose count is the documented number**
should assert *exhaustiveness* (derive the list from the source of truth), not a hand-copied
literal — §3 is what a hand-copied literal costs.
