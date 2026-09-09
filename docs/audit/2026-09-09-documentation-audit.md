# Documentation audit — accuracy, cleanliness, organization
Status: Resolved (2026-09-09)

A full sweep of every living document in the repo — `docs/reference/`, the four working-record
folders and their indexes, `CLAUDE.md`, `README.md`, `CONTEXT.md` — checked against the code as
it stands after the 2026-09-09 remediation wave (F1–F15). Frozen records in `spec/`, `plan/`,
`debug/`, `audit/` were *not* audited for accuracy: they are historical by design and are
allowed to describe superseded behavior.

Method: four mechanical passes (folder-index coverage, `Status:` line grammar, filename
convention, relative-link resolution) plus five parallel read-only source verifications, one per
document group. A finding was only filed when a file and symbol could be pointed at; counts were
recounted rather than trusted. Everything below was fixed in the same commit as this record.

## Verdict

The structure was already sound — **zero** broken markdown links, **zero** unindexed documents,
**zero** filename-convention violations, and every `Status:` value drawn from the legal set. All
33 README keybindings and the entire 49-row `KEYMAP.md` table verified against both
implementations. The defects were **drift**: reference docs describing the code as it was before
2026-09-09, and one contradiction between a record and the two indexes pointing at it.

| Class | Found | Note |
|---|---|---|
| WRONG (code contradicts the doc) | 11 | worst: `MUTATIONS.md` and `BEHAVIOR_MATRIX.md` disagreed on Remark |
| STALE (describes what no longer exists) | 8 | incl. 10 dead `docs/superpowers/…` paths |
| MISSING (real behavior absent) | 11 | mostly module-map and protocol-table gaps |
| Organization | 5 | index contradictions, an unfrozen audit, unowned assets |

## 1. Reference docs contradicted the code

- **`MUTATIONS.md` said Remark on an array element was "YAML-only, by design"** and that TOML and
  JSON return `Unsupported`/`Illegal`. F1 unified this on 2026-09-09: all three formats remark any
  node that occupies its own line. `BEHAVIOR_MATRIX.md` §8 already said so, so the two reference
  docs actively disagreed. Evidence: `cst_edit/replace_delete.rs` (`Target::ArrayElement`),
  `json/edit/mutations.rs` (`Target::Element`), and
  `tests/format_parity.rs::remark_an_array_element_round_trips_in_every_format`. Rewritten to the
  own-line rule, pointing at §8 as the owning statement rather than restating it.
- **`glossary.md` mis-stated the Detail popup's `Sign:` line**, claiming it shows `(B)`/`(Q)`/
  `(D)`/`(-)`. Those glyphs belong to the Type-filter popup (`session/type_filter.rs`); the Detail
  line spells the word out — `bare`/`quoted`/`dotted`/`none` (`status_fmt.rs::key_sign_label`).
- **`glossary.md`'s integer KIND tags dropped their padding** (`[I:dec]` for the real `[I:dec ]`).
  Every tag is padded to 8 display cells *inside* the brackets so the column aligns
  (`tui/app.rs`). The section calls itself "full vocabulary" but also omitted all four datetime
  tags and filed JSON's `[T/M]` under TOML. All three corrected.
- **`MESSAGES.md` §4 claimed a review finding was "still open as a follow-up, §8"** — it was
  closed on 2026-09-09, and §8 no longer listed it.
- **`TUI.md` cited `tui/type_filter.rs`** for the type-filter popup; that file only re-exports
  from core. The renderer is `overlay_type_filter.rs`.
- **`CLAUDE.md` claimed `web/vscode.ts` is imported by `editors/vscode/`.** It is not — it runs
  only inside the webview. The extension imports `web/vscode-protocol.js`.
- **`WEBUI.md` said the typed wrapper covers 14 methods** (`web/confy.ts` defines 16), and typed
  `docFormat` as `() => DocFormat` when `confy-ffi`'s `doc_format` returns a lowercase `String`
  and `DocFormat` in `web/types.ts` is the PascalCase union — the two could never match.
- **`WEBUI.md` §Deployment described a `cf-build.sh` that no longer exists**: it now runs
  wasm-pack under `CARGO_PROFILE_RELEASE_OPT_LEVEL=z`, then `functional_smoke.mjs`, then
  `npm ci && node build.mjs && npm run typecheck && npm test`, with no separate assembly step.

Recounted metrics in `CLAUDE.md`: `taplo::parser::parse` is **49** call sites, not 48;
`crates/confy-core/tests/` holds **19** integration suites, not 18. Two claims that were
*correct* and are recorded here so the next audit need not redo them: `functional_smoke.mjs` is
exactly 129 checks, and the `taplo::syntax::*`/`taplo::rowan::*` (28) and `taplo::dom` (2) counts
are exact.

## 2. Ten dead `docs/superpowers/…` paths inside living reference docs

Working records moved to `docs/{spec,plan,audit,debug}/` on 2026-09-09, but ten citations in six
reference docs still pointed at the old tree. They survived the link check because they are
inline code spans, not markdown links — a class of rot no automated check in this repo catches.
Corrected in `TUI.md`, `MESSAGES.md` (×2), `WEBUI.md`, `VSCODE.md` (×2), `TAURI.md` (×3) and
`ROW_STATE_MODEL.md`; one also had the wrong filename (`vscode-m1_5-` for `vscode-m1-5-`).

`CONTEXT.md`'s history note and the frozen `changelog/v0.x.md` keep their old paths deliberately.

## 3. Reference carried history that belongs in a working record

`MESSAGES.md` §8 ("Known follow-ups") had become a 46-line resolved-bug backlog — four *FIXED*
bullets whose own preamble said "all three were closed", inside the folder whose README states
"current behavior only". Its content is already in the 2026-09-09 re-verification, the backlog,
and `CHANGELOG.md`. Section removed; the current behavior it described was already documented in
§§4–5.

`ROW_STATE_MODEL.md` cited four guards as `session.rs:1441, 1453, 1467, 1485` — all four had
moved. Replaced with the symbol names (`toggle_select`, `set_selection`, `extend_select_up`,
`extend_select_down`), matching the "name files and symbols, never line numbers" rule the
re-verification record set. Same for a stale `web/touch/app.ts:1364` citation.

## 4. Behavior present in code, absent from the docs

- `TUI.md` never documented the `C` convert overlay at all, and cited neither `overlay_help.rs`
  nor `overlay_lang_picker.rs` by filename.
- `TUI.md`'s `~` diagnostics overlay still described the pre-F3 behavior (whole ring, newest
  last) instead of the windowed tail with its `Diagnostics — last N of M` title and empty-ring
  line.
- `VSCODE.md`'s protocol table omitted four `HostToWebview` variants that exist in
  `web/vscode-protocol.ts`: `schema-file`, `schema-file-error`, `schema-url`, `schema-url-error`.
- `TAURI.md`'s File-menu breakdown omitted **Save As**, which `web/menu.ts` really adds.
- `CHROME.md`'s button inventory omitted the **format pill**, an interactive control on both
  hosts (`#fmtPill` / `data-act="cyclefmt"`).
- `BEHAVIOR_MATRIX.md` §7's facet table omitted `rename_key_segs` and
  `fragment_trailing_comment`, and attributed `external_edit_path` to `App` when `Session` owns it.
- `MUTATIONS.md` omitted the single-value `Replace` fragment rule (F14) and the `suggested_key`
  synthesis — a bare value pulled out of `b = [x, y]` becomes `b_1`, not `placeholder`.
- `RELEASES.md` gave the release trigger as `v*.*.*`; the workflow deliberately uses the
  digit-anchored `v[0-9]*.[0-9]*.[0-9]*` because the plain glob also matches `vscode-v*.*.*`.
- `CLAUDE.md`'s module map omitted `web/touch/`, `web/touch.html`, `web/path-utils.ts`,
  `web/vscode.ts`, `web/vscode-protocol.ts`, `web/sw.js`, `web/assemble-dist.mjs`,
  `schema/mod.rs`, `tests/format_parity.rs`, and `model/json/edit/tests.rs`. Every path it *did*
  name exists — the map under-covers, it does not lie.
- `README.md`'s Usage block omitted the `--format` override.

## 5. Organization

- **The backlog contradicted both indexes.** `docs/plan/2026-09-09-open-follow-ups.md` had
  `Status: Resolved`, while `plan/README.md` listed it under *In progress* and `CONTEXT.md` called
  it "the only living record in `docs/plan/`". Resolved in the direction that keeps `CONTEXT.md`
  true: `MESSAGES.md` §7.2 recorded a real unfixed item (convert warnings bypass i18n —
  `ConvertResult.warnings` is raw English) that had never been filed as a row, which is exactly
  the "survives only where nobody looks" case the backlog exists to prevent. Filed as a row; the
  record is `In progress` again.
- **`2026-08-29-code-audit.md` was still `In progress`** although all 21 findings are closed
  (F1–F15 plus the two perf items) or consciously parked in *Watching*. Frozen as
  `Resolved (2026-09-09)`, with the resolution stated in the record and its three sub-reports
  turned into real links rather than bare filenames.
- **`2026-09-09-open-findings-reverification.md` carried its `Status:` on line 3**, the only
  record in the repo not following the "line 2, immediately after the H1" rule.
- **Two prototype `.html` assets sat unowned in `docs/spec/`** — no `Status:`, no index row, but
  cited as the verbatim visual source by `WEBUI.md` and `CLAUDE.md`. Added a *Prototype assets*
  table to `spec/README.md` naming each and what ported it.
- **`docs/reference/README.md` gained a fourth machine-checked row** for `BEHAVIOR_MATRIX.md` §8,
  now enforced by `tests/format_parity.rs`.

## Not fixed — maintainer action

`docs/tmp/` is **616 MB**, of which `claude-scratch/frag_probe/target/` is a stale cargo build
directory from a one-off probe. The tree is `.gitignore`d, so nothing reached the repository and
no CI or clone is affected; `CONTEXT.md`'s stated policy is to tar loose scratch into
`docs/tmp/archive/YYYY-MM.tar.gz` when it goes stale. Left in place — deleting a scratch tree is
the maintainer's call, not an audit's.

## Follow-up

One row opened in [`../plan/2026-09-09-open-follow-ups.md`](../plan/2026-09-09-open-follow-ups.md):
convert warnings bypass i18n.

Two rot classes this audit found that no check catches, worth a future gate: a path inside an
inline code span (the `docs/superpowers/…` class) and a `file.rs:NNNN` line-number citation. Both
are one grep each.
