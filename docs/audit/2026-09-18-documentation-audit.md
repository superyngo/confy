# Documentation audit — drift since 2026-09-15, and a raw catalog key on screen
Status: Resolved (2026-09-18)

A third full sweep of every **living** document — `docs/reference/`, the six folder indexes,
`CONTEXT.md`, `README.md`, `CLAUDE.md`, and the one living backlog — against the code at
`4248c11`. The [2026-09-15 audit](2026-09-15-documentation-audit.md) is the baseline: its findings
were all fixed, so this record covers the 48 commits since (`91fae9e..HEAD`) plus what that sweep
missed. Frozen records in `spec/`, `plan/`, `debug/`, `audit/` were again **not** audited for
accuracy — they are historical by design.

Method: pass 1 mechanical, scripted over `git ls-files` (index coverage both directions,
status-line grammar against the fixed value set, filename convention, Markdown link resolution,
inline-code-span path resolution, `file.rs:NNN` citation grep, reference-folder hygiene,
commit-citation reachability, the four-file version quartet) + pass 2 accuracy — six parallel
read-only verifications, one per document group, every reported claim re-checked against the
source by the author before it was written down. Counts were re-derived, never trusted: one
reported count (`i18n` totals, 434) was wrong and is 433 here.

## Verdict

Structure held for the third sweep: **zero** unindexed documents, **zero** ghost index rows,
**zero** filename violations, **zero** illegal status lines, **zero** broken links in living docs.
The defects were again **drift concentrated in the period's features** — Block edit via whole-file
reparse, the Raw control band, VS Code write mode — plus one live user-visible bug that no
document mentioned because no document was wrong about it.

**41 accuracy defects** across eight reference files plus `CLAUDE.md` and `README.md`, **3**
structure defects, and **4** live code defects. Per file, so the count is checkable:

| File | Defects | The worst one |
|---|---|---|
| `WEBUI.md` | 8 | the wasm wire in the wrong case — `externalEdit` in prose and as two camelCase table rows for methods the wrapper does not expose |
| `TUI.md` | 7 | the Action menu was documented with **eight** items; `action_menu_items` returns nine |
| `glossary.md` | 6 | §Mixed/§Dotted table still described the per-node fragment route `25d3f21` deleted |
| `ARCHITECTURE.md` | 5 | `scripts/` missing from the repository layout tree entirely |
| `BEHAVIOR_MATRIX.md` | 4 | §7's facet table omitted `node_text_spans`, the facet Block edit is parameterized on |
| `HOST_PARITY.md` | 4 | §3 said the band toggle's label becomes **Apply** in write mode; it becomes **Cancel** |
| `KEYMAP.md` | 3 | "All three surfaces drive the same `snap.external_edit`" — the TUI drives none of it |
| `CLAUDE.md` | 2 | "cites **six** such commits", contradicting the "exactly five" two lines above |
| `MUTATIONS.md` | 1 | the mixed-table Block row's synthesized-`[a]`-header claim |
| `README.md` | 1 | the `m` row listed eight of the nine Action-menu items |
| — structure — | 3 | `debug/README.md` listed a `Resolved (2026-09-18)` record under *In progress* |
| — code — | 4 | the `?` help overlay printed the literal string `tui.help.legend.json.containers.6` |

By class: 10 WRONG, 5 STALE, 12 MISSING, 5 CONTRADICTION (two living docs disagreeing — a class
that produced a defect in each of the three sweeps), 6 wrong counts, 3 entry-format violations.
`ROW_STATE_MODEL.md`, `CHROME.md`, `VSCODE.md`, `RELEASES.md`, `MESSAGES.md`, `CONTEXT.md` and
`reference/README.md` produced **zero** between them.

## 1. The code defect — a raw catalog key rendered as a legend row

`help_legend_text` hardcodes each format's legend row counts. They were `[2, 6, 6]` for JSON and
`[3, 7, 14]` for YAML, but the catalogs define **5** `json.containers.*` and **6**
`yaml.containers.*` entries. The overshoot asked `tr` for
`tui.help.legend.json.containers.6` / `tui.help.legend.yaml.containers.7`, and `tr`'s last branch
— commented in `i18n.rs` as *"a bug signal, never hit in normal use"* — leaks the key itself as a
`&'static str`. So the `?` overlay's Containers group ended with the raw key as a visible row, on
**every JSON and YAML file, in both languages**, for as long as the per-format legend has existed.
TOML was unaffected (`[4, 8, 16]` all match).

Reproduced and confirmed on the **real binary**, not in a unit test: `confy legend.json --lang en`
→ `?` → scroll, before and after. Second leg `confy legend.yaml --lang zh-TW` to cover the other
format and the other catalog.

Counts corrected to `[2, 5, 6]` and `[3, 6, 14]`. The counts stay hand-written, so the guard is
exhaustive rather than a second copy of the numbers: `legend_requests_no_missing_catalog_key`
renders all three formats × both languages and asserts no output contains `tui.help.legend.`.
That is the lesson the 2026-09-15 sweep wrote down as a follow-up — *an invariant test whose
count is the documented number should assert exhaustiveness, not a hand-copied literal* — applied
to the counting site itself.

Three more code-side defects, all found in the same pass:

- **`help.row.convert` still said "(Root node)"** — in both catalogs, and it is a *shared* key, so
  the phrasing ADR 0013 retired was on screen in the TUI `?` overlay **and** the web/touch help
  panel. The 2026-09-15 sweep struck the same wording from `README.md` and missed the catalog.
  Now "Convert document to another format" / 「將文件轉換為其他格式」.
- **Three orphaned catalog keys** that `adfc3b1` missed: `tui.status.filter-results-status`
  (superseded by `tui.status.filter-results-notice`), `web.common.confirm`, `web.help.title`.
  Deleted from both catalogs; 433 → 430 keys each, parity intact. The sweep that found them was
  re-run independently against all 243 source files **plus** `web/*.html`, and turned up exactly
  two dynamic key builders — `help_legend_text`'s `format!` and `help-content.ts`'s
  `` `web.help.legend.${…}` `` — which is why the `tui.help.legend.*` and `web.help.legend.*`
  families look orphaned to a literal grep and are not.
- **`severity_of_covers_the_full_catalog_table` cited `MESSAGES.md §2.2`**, a subsection that does
  not exist (the table is under §2; §2.1 is *Catalog key prefixes*). Retargeted to §2.

## 2. Reference contradicted the code

- **Block edit was documented as the route `25d3f21` deleted.** `glossary.md` §Mixed table and
  `MUTATIONS.md`'s mixed-table row both described `e` consolidating into a "canonical scope form"
  — a synthesized `[a]` header with dotted members folded under it. `node_text_spans` captures
  **verbatim member spans in document order** and `splice_spans` lands at the **first** span; no
  header is synthesized. `glossary.md` §Dotted table cited `replace_inline_dotted_table`, which is
  the keyed-path route, not the Block route (`Session::apply_block_text`).
- **`BEHAVIOR_MATRIX.md` §7's facet table omitted `node_text_spans`**, the facet the whole Block
  feature is parameterized on, implemented on all three backends.
- **§8 claimed Insert/Replace "never silently drops"** while `MUTATIONS.md` said TOML `Replace`
  drops surplus nodes. `replace_value` swaps the first entry's VALUE and drops the rest, so §8 was
  the wrong one. §8 also omitted `0206923`'s key-matching invariant — a keyed-path `Replace` whose
  fragment renames the key now returns `MutateError::Fragment`.
- **§6.3 credited `flow::flow_item_text`** with slicing a YAML flow-seq element by ordinal.
  `yaml::edit::spans::node_text_spans` reads the projected Node's `text_range` directly;
  `flow_item_text` survives only on the clipboard-copy path.
- **The Raw control band was documented pre-`598ace4`** in `glossary.md` and `HOST_PARITY.md` §3:
  one primary control relabelled Edit → Apply. It is a left `#btnRawToggle` toggling
  Edit ↔ **Cancel** plus a static right `#btnRawApply`. `HOST_PARITY.md` §3 also gave write mode
  to desktop web only while §5 and ADR 0015 give it to VS Code too, and two of its §-anchors named
  headings that do not exist (`CHROME.md §Per-host trimming`, `TAURI.md §Save As`).
- **The Action menu's ninth item was invisible in three documents.** `Session::action_menu_items`
  returns nine; `TUI.md` said "eight" and listed eight, `README.md`'s `m` row listed eight. Worse,
  `TUI.md` attributed the separator rule to Delete — `separator_before` is true on **EditDocument**
  — and its dim rule sorted every item into single-path or set-applying, so it mis-described the one
  **document-scoped** item, which never dims.
- **`KEYMAP.md` claimed all three surfaces drive `snap.external_edit`**, in two places. The TUI runs
  `$EDITOR` **synchronously** (`edit_node` → `edit_block_at`) and dispatches `ApplyBlockText`; only
  web and touch use the async request/response. Its "raises `core.clipboard.action-locked` before
  dispatching" also described a dispatch that never happens — `edit_node` posts `SetHostNotice` and
  aborts.
- **`TUI.md` §Comments** still said `E` and multi-line comments open `$EDITOR` "with the raw text";
  they edit the Comment's **Block**. §Language said the picker "dispatches `Intent::SetLang`";
  `lang_picker_commit` calls `Session::set_lang` directly. §Editing omitted `BackTab`.
- **`WEBUI.md` had the wasm wire in the wrong case**: `externalEdit` in two prose sites and as a
  camelCase table row, when the serde field is `external_edit` and `web/confy.ts` wraps neither
  `external_edit` nor `schema_violations`. It also said a breadcrumb jump routes a dirty
  write-mode exit through the same gate as the header toggle — `jumpSelectRawSpan` never calls
  `exitRawWrite`; it stays in write mode and reports `web.raw.jump-needs-apply`. Two sites called
  the Save `.split-btn` the toolbar's "right-side" control (it sits mid-row, before `editGroup` and
  `#btnMore`), and the touch language selector was placed in the ⋯ menu when it lives in
  `.edit-grp` and folds at ≤600px — both contradicting `CHROME.md`, which was right.
- **`glossary.md` listed four YAML string styles.** There are five: `Format::Plain` classifies as
  `TypeToken::StrBasic` and renders `[S:str ]`.
- **`glossary.md` broke its own entry format on 17 of 56 entries.** `wens-dev-principles docs 5`
  makes the `_Avoid_:` line mandatory — `_Avoid_: —` when no synonym was rejected — precisely so
  a reader never wonders whether synonyms were considered. `Scalar`, `Format`, `DocFormat`,
  `Value`, `Projection`, `Opaque node`, `Indent engine`, `Native menu bar` and nine more had
  none. All 56 carry one now. Five core model types were also used as terms across the reference
  docs with no entry at all: `Path` (and `Seg`), `Target`, `Mutation`, `MutateError`,
  `ConvertWarning`.

Counts: `web/*.spec.mjs` **39 → 40** (`block-edit.spec.mjs`); `taplo::syntax`/`taplo::rowan`
**28 → 29** (`cst_edit/spans.rs`); `web/confy.ts`'s wrapper **18 → 20** including `free`
(`nodeAtOffset` from `3f2b4d1`); `CLAUDE.md`'s "six such commits" **→ five** (the branch holds six,
the record cites five hashes — the grep re-run on 2026-09-18 still yields exactly those five);
`i18n` **433** keys each before this sweep's three deletions — not the 434 first reported — and
**430** after. Re-verified as still correct: `taplo::parser::parse` 49,
`taplo::dom` 2, the 1,241-LOC vendoring estimate, 21 core integration suites,
`functional_smoke.mjs` 176 checks, `SessionSnapshot` 22 fields, `ViewRow` 21, 19 VS Code message
variants, 119 `core.*` catalog keys, 49 `core.*` notice keys (13 Error + 20 Warn + 7 Success +
9 Info), 13 `ConvertWarning` variants, both catalogs at exact key parity.

## 3. Reference held nothing it shouldn't

Unlike the last two sweeps: no `reference/` file carries a History, Roadmap, Future, TODO, or
backlog section, and none carries a `~~strikethrough~~` resolved-bug list. The §8 *Boundaries*
rewrite in `ROW_STATE_MODEL.md` held. `ROW_STATE_MODEL.md`, `CHROME.md`, `VSCODE.md`,
`RELEASES.md`, `MESSAGES.md`, `CONTEXT.md` and `reference/README.md` produced **zero** defects
between them.

## 4. Structure

- **`debug/README.md` listed `2026-09-17-msix-headless-cli-handoff.md` under *In progress***, but
  the file reads `Resolved (2026-09-18)` — the status flipped that morning and the index row did
  not move with it. Moved to *Landed*. This is the one class pass 1 cannot catch by grep alone:
  the document *is* indexed, so coverage is clean; the row just lies about its state.
- **The living backlog still held one `file.rs:NNN` citation.** `4248c11` replaced two line-range
  citations the previous commit and missed a third, `cst_edit/mod.rs:81-84`, inside a *Done* row.
  Now `cst_edit::apply`'s `Mutation::Replace` arm and its `None => tree` branch.
- **ADR files come in two shapes** — 0001–0008 carry YAML front matter with `status:`, 0009 onward
  put it in the H1 and let the index table hold the status — and `adr/README.md` documented
  neither. An ADR is never edited (`wens-dev-principles docs 13`), so the eight keep the shape
  they landed with and the index now states which shape a **new** ADR uses.
- Commit-citation reachability: 74 distinct hashes cited across `CHANGELOG.md` and the docs,
  **five** unreachable, all five the deliberately-abandoned `root-row-alignment` ones `CLAUDE.md`
  documents. Branch still alive locally and on `origin`.
- Version quartet (`Cargo.toml`, `web/package.json`, `editors/vscode/package.json` + its lock,
  `CHANGELOG.md`'s `## [v1.3.2]`) all agree at `1.3.2`.
- **A `§`-anchor in this repo points at a heading *or* at a bold-labelled paragraph.** Checking
  all 23 cross-file `§` references mechanically leaves eight that no heading satisfies, and six
  of those are the second kind — `WEBUI.md §Swipe actions` and `§Shared edit/detail panel` are
  `- **Swipe actions.**` and `**Shared edit/detail panel — …**`, and `KEYMAP.md §Editor parity`
  is a shortening of *Editor (inline/external) parity*. Only the two `HOST_PARITY.md` §5 anchors
  named nothing at all in either form, and those are the two that were fixed. Writing the
  convention down here so the next sweep does not re-flag the six: an anchor is a defect when
  neither a heading nor a bold label matches, not merely when no heading does.

## Not fixed — deliberate

- **`debug/2026-09-16-vscode-pane-edit-parity.md` keeps `In progress`.** It was edited three times
  on 2026-09-18 and its P1–P4 shipped in `3573085`/`41eb0fb`, with only P5 open and already a
  `BACKLOG.md` row — which is the "frozen record holds the evidence, the backlog holds the open
  item" split principle 17 describes. Freezing it as `Resolved` was proposed and **declined**: the
  2026-09-17 `CHANGELOG` entry records the `In progress` status as a deliberate choice, and
  reversing it is a maintainer call, not an audit's.
- **`CLAUDE.md`'s "shape in one paragraph" stays**, byte-identical to `ARCHITECTURE.md`'s opening
  paragraph. It is the duplication `wens-dev-principles docs 3` forbids and the mechanism behind
  two of the 2026-09-15 defects, but the orientation value at the top of the conduct file was
  judged worth the risk. Proposed and declined.
- **`CONTEXT.md` vs `.gitignore` on `docs/tmp/`.** `CONTEXT.md`'s folder table says `tmp/` is
  "Archived to `docs/tmp/archive/YYYY-MM.tar.gz` when stale", but `.gitignore` ignores all of
  `/docs/tmp/`, so `git ls-files docs/tmp` is empty and none of the three tarballs in `archive/`
  is in the repository — the documented lifecycle is unobservable, and `.gitignore`'s
  `/docs/tmp-archive-*.tar.gz` rule points at a path that moved. Reconciling either direction was
  proposed and declined; `docs/tmp/` is 616 MB, almost all `claude-scratch/frag_probe/target/`, and
  remains the maintainer's call — unchanged from the 2026-09-09 and 2026-09-15 findings.
- **Two broken links inside frozen plans**, unchanged and for the same reason as last sweep:
  `2026-08-28-json-jsonc-comment-gate-removal.md`'s link target is a Rust path rather than a file,
  and `2026-09-07-datetime-kind-switch.md`'s ADR link is missing its `../adr/` prefix.
  Principle 7 allows only a `Status:` edit on a frozen record.
- **`RELEASES.md` names `TAURI.md` as an MSIX authority** while `TAURI.md` has no MSIX section.
  Reported by the sweep as a missing section and **rejected**: `ARCHITECTURE.md` names `msix/` in
  the module map and `RELEASES.md` routes the real detail to `crates/confy-tauri/msix/STORE.md`,
  which is accurate and current. `TAURI.md` owns the app shell; the cross-reference is loose, not
  wrong, and rewriting it would move packaging content into the wrong file.

This sweep's scratch fixtures are `docs/tmp/claude-scratch/doc-audit-2026-09-18/legend.json` and
`legend.yaml` (the two-line documents used to drive the real-binary repro). Both are gitignored.

## Follow-up

The two rot classes with no automated gate are unchanged from both prior sweeps — a repo path
inside an inline code span, and a `file.rs:NNN` line citation — and this sweep found one of each
again, which is now three sweeps in a row. Both are one grep, both were run by hand here, and the
line-citation one has now escaped a dedicated cleanup commit (`4248c11`) by exactly one row. A
third class earns a name here: **an index row's status text is a second copy of the document's
`Status:` line**, and nothing compares them. All three are cheap to check and are the natural
content of a `docs-hygiene` check in repo tooling rather than a fourth hand-run sweep.
