# Open engineering follow-ups
Status: In progress

The **single live backlog** for confy. Every recorded-but-unfixed item lives here with its
evidence, its verdict date, and an acceptance criterion — so no defect survives only inside a
frozen audit or a doc footnote.

Scope and lifecycle:

- This is a *working* record, and the one exception to the "frozen once it lands" rule that
  governs the rest of `docs/plan/`. Rows move to **Done** with the commit that closed them;
  the row is never deleted, so the history of what was open stays readable.
- A finding enters here only after being **verified against current code** — file and symbol
  named, never a line number. The source records
  ([2026-08-29 audit](../audit/2026-08-29-code-audit.md),
  [2026-09-09 re-verification](../audit/2026-09-09-open-findings-reverification.md),
  [2026-09-09 documentation audit](../audit/2026-09-09-documentation-audit.md)) stay frozen;
  this file is where their open items are tracked. `MESSAGES.md` §8 used to be a fourth
  source; its items are all in *Done* below and the section was removed from the reference
  doc on 2026-09-09, since `docs/reference/` carries current behavior only.
- When the last row reaches Done, this record's `Status:` becomes `Resolved (date)` and it
  joins the frozen set.

Effort is XS (< 1 h) / S (a session) / M (multi-session).

---

## Open

| Opened | Item | Evidence | Effort | Acceptance |
|---|---|---|---|---|
| 2026-09-14 | **Q4 — caret → cursor, the inverse of the Raw pane's breadcrumb jump.** A breadcrumb pick selects a Node's source span in the Raw pane (path → offset, R14–R16); moving the caret/text-selection in the Raw pane does **not** move the tree cursor or breadcrumb back (offset → path). Settled out of scope at ship time (Q4, `docs/spec/2026-09-11-raw-write-mode-design.md`). | `web/ui.ts`'s `jumpSelectRawSpan` is one-way only. The private `Session::path_at_offset` (`session/inline_edit.rs:852`) is **not** reusable: its predicate is `start >= offset` (first Node at or after a splice anchor), so a caret *inside* a Node's span misses it and resolves to the next Node — a real query needs containment plus innermost descent. Scoped 2026-09-15. | S–M | A core containment query exists and is exported through `Session`/FFI, `web/text-offset.ts` gains the `codeUnitToByte` inverse, and a Raw-pane caret move updates the tree cursor/breadcrumb on both Raw states without re-entering `jumpSelectRawSpan` (latch + debounce). TUI/touch/VS Code are out of scope: no Raw pane, no breadcrumb, write mode suppressed by R10. |

---

## Watching (no action planned)

- **A rejected Block reports a document-space offset, not a buffer one.** Moved here from Open
  on 2026-09-15: the row assumed a document offset was already in hand and only needed
  `offset - start` arithmetic. It is not. Per backend — TOML has one upstream
  (`taplo::parser::Error { range: TextRange, .. }`) but `reparse_document`
  (`cst_edit/replace_delete.rs:51-55`) discards it via `e.to_string()`; **JSON and the YAML
  subset track no offsets at all** (`parse(&str) -> Result<GreenNode, String>`, lexers emit
  `(SyntaxKind, String)`). The real prerequisite is span tracking in two hand-rolled lossless
  parsers, plus a new `SessionSnapshot` field (cross-host) and a `None` rule for an error that
  legally lands *outside* the spliced region (unclosed delimiter, or a duplicate key far away).
  A TOML-only fix would book format-parity debt. Against that: a Block is typically 1-10 lines
  and every host already keeps the user's text on rejection (TUI re-spawn loop; web/touch modal
  stays open). Revisit if the JSON/YAML parsers gain spans for another reason.
- **`taplo` is unmaintained upstream.** The decision and its `cargo audit` trigger are in
  `CLAUDE.md` §Known Risks; the measured surface and the ~1,240-LOC vendoring scope are in
  `docs/reference/ARCHITECTURE.md` §Dependency surface. `rust-ci.yml`'s `cargo audit` step is
  the agreed trigger; do not migrate pre-emptively — `tombi` is still not a usable dependency.
- **`JsonDocument` does not override `rename_key_segs`.** TOML and YAML both do, to decode a
  rename's literal with their own key lexer. Not a live defect — JSON keys are always quoted, so
  the trait default coincides — but it is the last asymmetry in that area, and it would become a
  defect if JSON ever grew unquoted keys.
- **The YAML subset lexer accepts unterminated scalars.** `x: "unclosed`, `x: [1, ` and
  `x: {a: ` all lex as plain scalars. This was filed as half of F14 and is **not** a defect:
  `AnyDocument::from_str_as` accepts the same text from disk, so `Replace` and load agree.
  Tightening one without the other would mean a value you can open but cannot retype. Only
  fix these together, and only if the subset parser is made strict on purpose. Asserted as
  the documented exception in `tests/format_parity.rs` (F8), so the day it changes, a test says so.
- **The two web palettes are not the same set.** `web/style.css` defines `--drop` (the
  drop-indicator green, also the desktop status line's Success hue); `web/touch/style.css`
  does **not** define it at all, so `var(--drop)` silently falls back to `currentColor` on
  touch — caught 2026-09-09 only because a computed-style check contradicted the screenshot.
  The touch severity toasts use `--t-string` for Success instead, with the reason in the CSS
  comment. A shared token file would remove the class of bug; not worth it for one token.
- **`Into([])` resolution — settled 2026-09-14 on main at `21cbea9`: it appends.**
  Measured as S0 evidence E3 of the root-hidden record: `Into([])` is paste-slot **index 0**
  yet `slot_target` resolves it to `Target { parent: [], index: children.len() }` — the
  document's *end* — while `After([])` resolves to index 0, the top. The branch's "prepend"
  observation does not reproduce and is treated as a branch-mode artifact. This contradiction
  between screen order and resolution is P4, and D5 of
  `docs/spec/2026-09-11-root-hidden-alignment-design.md` re-orders it away.
- **Structured row-diff transport (the old G2 idea).** The full-snapshot transport is the
  shipped baseline; if re-render latency ever becomes measurable on large files, the additive
  upgrade is a `delta` field on `SessionSnapshot` (or a sibling `dispatchDelta`) plus
  `Path`-keyed row patching, with `snapshot()` kept as the resync fallback. Nothing is built,
  and the `Path`-keyed `ViewRow` already is the identity such a diff would key on. Moved here
  from `WEBUI.md` on 2026-09-15 — `docs/reference/` carries current behavior, not roadmap.

---

## Done

| Closed | Item | Commit |
|---|---|---|
| 2026-09-09 | Double serialize per mutation — `sync_schema_hint` now takes the text | `57630e4` |
| 2026-09-09 | TOML `Move` quadratic — live-index release in `move_nodes`/`delete`, −48% | `57630e4` |
| 2026-09-09 | JSON/JSONC parser-simplification plan — premise refuted, both halves already done | (record closed) |
| 2026-09-09 | **F1** Remark semantics unified — own-line rule, uniform `Unsupported`, array elements now remarkable in TOML+JSON | `72805c0` |
| 2026-09-09 | **F2** 3-format parity suite — `tests/format_parity.rs`, 9 behaviors, exhaustive-`match` fixtures | `72805c0` |
| 2026-09-09 | **F14** YAML `Replace` silently dropped everything past the first node — now `Fragment("fragment must be a single value")`; two of the four filed rows were misdiagnosed and moved to *Watching* | `08fc59c` |
| 2026-09-09 | **F3** TUI `~` overlay windowed the ring's head — now the tail, with a `last N of M` title and an empty-ring line | `0db6b79` |
| 2026-09-09 | **F4** Web `?diag=1` cursor now resets on session swap — the record's "8 replacement sites" was one (`openText`) | `ccb0999` |
| 2026-09-09 | **F5** Touch `sev-*` toasts styled — border/left-bar tint per severity, matching the desktop status hues | `4379723` |
| 2026-09-09 | **F6** `json/edit.rs` split — 8 production files (max 377 lines, was 1,774 in one) + sibling `tests.rs`; all 67 tests preserved | `739132e` |
| 2026-09-09 | **F7** Diag taps moved `dispatch()` → `apply()` — the TUI's `~` ring went 0 → 32 events on the same 16 keystrokes; the filed "~15-20 bypass sites" was 5, of which 4 now go through Intents (`ConvertWriteDone`, `SetStrictJson`, `SetPasteSlot`) | `8cc0ccb` |
| 2026-09-09 | **F8** `MutateError` taxonomy documented + enforced — Root delete `NotFound`→`Unsupported` (3 backends), JSON unterminated string `Illegal`→`Fragment` (lexer emits ERROR), 2 new parity tests | `8cc0ccb` |
| 2026-09-09 | **F15** `?diag=1` drain extracted to shared `web/diag.ts`; touch went 0 → 12 logged lines on 3 keystrokes, desktop unaffected | `5612849` |
| 2026-09-09 | **F13** `.cargo/config.toml` with `incremental = false` — `target/debug/incremental` was 23 GB / 120k files (47% of `target/`) for ~1.6 s per edit rebuild | `1959801` |
| 2026-09-09 | **F11** `jsonschema 0.30 → 0.55` (fields became methods) and `fuzzy-matcher 0.3 → nucleo-matcher 0.3` (unmaintained → maintained); `ureq 2` still deliberately deferred | `1959801` |
| 2026-09-09 | **F9** — premise refuted by measurement (undo is 15 ms at 1 MB; a green tree costs ~70× its text). Fixed the real axis instead: a 16 MiB byte cap beside the 200-entry cap, ADR 0003 amended | `88215e1` |
| 2026-09-09 | **F10** — three live-index rules, not one: spans from the index, section text off the green tree, `insert_with` owns and drops its index before the splice. `Move ×8` 2.27 s → 158 ms (−93%) | `5691819` |
| 2026-09-09 | **F12** — `CHANGELOG.md` split by series: root keeps `[Unreleased]` + v1.x (534 KB → 119 KB), v0.x archived verbatim under `docs/reference/changelog/` | `9f7c58e` |
| 2026-09-14 | **The clipboard could be armed with the Root, dead-ending the modal lock** (retrospective P1) — closed by ADR 0013 D7's `guard_root_operand` + D1 (the Root has no row to put a cursor on) | `74eace5`, `e780379` |
| 2026-09-14 | **Two web-only blank trees** (retrospective P2) — closed structurally: D2 expands the Root by contract (`0`/`Space` can no longer hide the first layer) and D11 retired the `[G] root` facet | `74eace5`, `e780379`, `4328778` |
| 2026-09-14 | **The TUI still drew the Root row / ADR 0013 was accepted but unimplemented** — closed by `docs/plan/2026-09-11-root-hidden-alignment.md` T0–T12: no host draws the Root, all seven web stand-ins deleted | `dc69a47`…`4328778` |
| 2026-09-14 | **P3** The web has no entry point for whole-document operations — root-hidden retrospective P3, closed by `docs/plan/2026-09-14-raw-write-mode.md` T1–T10 (`Intent::BeginEditDocument`, `ActionId::EditDocument`, desktop Raw pane write mode, touch's existing sheet, VS Code suppressed per R10; ADR 0014) | `5190d7a`…`d54539a` |
| 2026-09-15 | A YAML multi-line comment block's extent stopped after its first line — `flush_comment_block` now records the run's **last** token, `extent_end_offset` walks the remaining `#` lines, and `yaml/edit/spans.rs`'s two local workarounds are gone (one algorithm for the block's end) | `c724ea8` |
| 2026-09-15 | Eleven orphaned i18n keys deleted from both catalogs — four `tui.prompt.*` prompt strings (the TUI renders core's question and uses the `.legend` siblings), `web.prompt.q.{arrayUpgrade,confirmQuit}`, `core.action.title`, and the four unreachable `web.host.{add.*,kind.no-options}` arms of `severity_of` (host-key count 23 → 19) | `b8465c6` |
| 2026-09-15 | `Block` ↔ `Remark` relationship stated — both glossary entries and `MUTATIONS.md`'s Remark row now say Remark comments the Block's lines minus its trailing blank run, which is where the own-line rule comes from | `b8465c6` |
| 2026-09-15 | **Convert warnings bypass i18n** — the thirteen lossy-normalization notes are now a structured `ConvertWarning` enum in `model/` with `catalog_key()`, translated at the edge (`Session`'s convert projection, the CLI); `ConvertView.warnings` stays `Vec<String>`, so the wasm wire contract and every host renderer are untouched. The row's own examples were invented: convert never drops comments and never merges duplicate keys | `6bfc5c2` |
| 2026-09-15 | **The trailing-comment double pass** — closed as **stale**, not fixed. `edit_commit` splits the comment off before building the fragment, so `replace_value` returns `None` and `cst_edit/mod.rs:81-84` takes the `None => tree` arm: a plain inline value edit on a commented Node never calls `set_trailing_comment`. No host commits per keystroke either (TUI on `Enter`, web on `Enter`/`blur`). The spec §1 **14× does not reproduce**: measured 2026-09-15 on 5000 sections (release, median of 9) — TOML `Replace` 227 ms with or without a trailing comment on the Node, `SetTrailingComment` 615 ms, `Replace` with a comment-bearing fragment 664 ms (2.7–2.9×, and only when the user changes a comment); YAML 59–61 ms on every route. Reopen only on a fresh measurement | `6bfc5c2` |
