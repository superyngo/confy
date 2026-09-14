# Raw write mode — whole-document editing, task plan
Status: Approved (2026-09-14)

Design record: [`../spec/2026-09-11-raw-write-mode-design.md`](../spec/2026-09-11-raw-write-mode-design.md)
(decisions R1–R28, slices RD/RS0–RS4) · ADR
[0014](../adr/0014-whole-document-editing-reuses-each-hosts-text-surface.md) · sibling record
[`../spec/2026-09-11-root-hidden-alignment-design.md`](../spec/2026-09-11-root-hidden-alignment-design.md)
(this feature is its P3 entry point and **ships first**)

## How to run this plan

Each task below is a **session-sized unit**: one commit, one verification command, and a state
that survives a `compact` or a cold start in a new session. The durable state is always a file
— this plan's checkbox, the design record's Evidence section, and the commit — never the
transcript.

**Resuming cold.** Read, in order: this file's task table; the design record's *Decisions
taken* table; then only the files named by the task you are starting. Do **not** read the
sibling root-hidden record or ADR 0013 unless the task cites them — they are context for *why*,
already distilled into R1–R28.

**Token budget per task** is an estimate for a fresh session running that task alone: reads +
edits + verification output, excluding the plan/record reads above (~8k shared preamble). A
task estimated over ~60k is already split.

| # | Task | Est. tokens | Breakpoint state |
|---|---|---|---|
| T1 | RS0 evidence — identity Apply × 4 formats | ~35k | **Done (2026-09-14)** — Evidence §, no product code |
| T2 | RS0 evidence — failure, `doc_revision`, offsets | ~30k | **Done (2026-09-14)** — Evidence §, no product code |
| T3 | RS1a core — `doc_revision` + `history_len` warning | ~25k | **Done (2026-09-14)** — commit, `cargo test -p confy-core` green |
| T4 | RS1b core — intent, Action item, `apply_document_text` | ~55k | **Done (2026-09-14)** — commit, 4 headless + 4 smoke checks |
| T5 | RS1c core — empty-path mutation/undo guard (R21/R24) | ~30k | commit, headless tests |
| T6 | RS2a web — `rawState` refactor (R27), no new feature | ~40k | commit, `npm test` green |
| T7 | RS2b web — the `#rawEdit` textarea + R4–R8 + cues | ~60k | commit, `raw-write.spec.mjs` |
| T8 | RS2c web — crumbs control band (R13) + jump (R14–R17) | ~50k | commit, spec + manual pass |
| T9 | RS3 touch — empty-path sheet rules (R18/R19) | ~30k | commit, touch spec |
| T10 | RS4 VS Code suppression (R10) | ~20k | commit, typecheck |
| T11 | RS4 docs — 7 reference docs + CHANGELOG + follow-ups | ~45k | commit, docs only |

Total ≈ 420k across 11 sessions. **Never carry two tasks into one commit** — CLAUDE.md's
after-each-task rule applies per row, and a half-finished row is the one state this plan cannot
resume from.

---

## T1 — RS0: identity Apply, once per format

**Target.** `crates/confy-ffi/functional_smoke.mjs`; four fixtures (TOML, JSON, JSONC with a
leading comment block and an inline trailing comment, YAML). No product code, no `web/`.

**Change.** For each fixture: `from_text` → read `serialize()` as `before` → dispatch the
whole-file `Replace` with `before` **verbatim** → compare `serialize()` to `before`
byte-for-byte. Until T4 lands `Intent::BeginEditDocument`, drive the Apply through the existing
`ApplyReplace { path: [], text }` route — this task measures the **backend**, not the intent.
Include a trailing-newline-free fixture and one with CRLF if the harness can express it.

**Acceptance.** Four results written into the design record's *Evidence* section with the exact
byte diff (or "identical") per format, and Q1 marked answered. A non-identical format opens a
**blocking** sub-task recorded here as T1b before T6 may start (R23).

## T2 — RS0: failure, `doc_revision`, and offsets

**Target.** Same harness. Three measurements:

1. Apply of deliberately broken text (unbalanced brace / duplicate key / YAML `---` ×2) → no
   commit, Error notice, `serialize()` unchanged.
2. The R5 detector premise: on a **no-change** successful Apply, and again after
   `MAX_HISTORY`/byte-cap eviction, record what `history_len` does. This is the
   failing-before evidence for T3; it is expected to *not* move.
3. R14/R15: a fixture with CJK **and** an emoji before the target node — does `outline()`'s
   byte `text_range`, through the byte→code-unit conversion, select exactly that node's text in
   `serialize()`?

**Acceptance.** All three in the Evidence section; measurement 2 phrased as the test T3 must
turn green; measurement 3 leaves a concrete failing case for T7/T8's helper.

## T3 — RS1a: `doc_revision`

**Target.** `crates/confy-core/src/session/session.rs` (the field + `on_mutation_success`),
`session/view.rs` (`SessionSnapshot`), `session/state.rs` (`history_len`'s doc comment, R25),
`web/types.ts` (the mirror), `crates/confy-ffi` if the field needs no explicit wiring, verify.

**Change.** `doc_revision: u64`, incremented exactly once per successful commit, never
decremented (an undo is a commit). Surface it on `SessionSnapshot`. Add the R25 warning to
`history_len`.

**Acceptance.** `cargo test -p confy-core` + a headless test asserting the two T2 cases where
`history_len` is flat and `doc_revision` is not. `npm run typecheck` for the mirror.

## T4 — RS1b: the intent, the Action item, `apply_document_text`

**Target.** `session/intent.rs`, `session/dispatch.rs`, `session/inline_edit.rs`,
`session/action_menu.rs`, `session/view.rs` (`ActionId`), `i18n/en.json` + `i18n/zh-TW.json`,
`web/types.ts`.

**Change.** Cherry-pick `begin_external_edit_document` from branch `root-row-alignment`
`d945721` (R2). Add `Intent::BeginEditDocument`. Add `ActionId::EditDocument` —
`core.action.edit-document`, **immediately above `Delete`, same section**, carrying the
section-leading separator (root-hidden record D8/D9), always enabled, never dangerous. Add
`apply_document_text(text)` (R20): empty-path `Mutation::Replace` + `on_mutation_success`
only — **no** `split_packaged_blank`, no trailing-comment extraction, no `wrap_element`; route
the empty-path Apply to it instead of `apply_external_replace`. Add
`core.document.apply-failed` with the backend error as a `tr_args` arg, raised at
`Severity::Error` **regardless of the inner cause** — T2's F4 measured parse failures
noticing as `warn` (R26 as amended).

**Acceptance.** `cargo test -p confy-core`; headless tests for: the intent opening a pending
edit at `[]`; the armed-clipboard refusal (R11); an Apply at `[]` committing exactly once; a
failed Apply leaving the document untouched with the new notice key.

## T5 — RS1c: the empty-path lock (R21/R24)

**Target.** `session/dispatch.rs` (or wherever `guard_clipboard_locked` is applied),
`session/session.rs`.

**Change.** While a pending external edit at the **empty path** is in flight, mutating intents
**and `Undo`/`Redo`** are refused with a notice (R24). Per-node pending edits are unaffected.
VS Code is exempt by R10 — it never enters this state because the host suppresses the feature,
so no core-side host flag is needed.

**Acceptance.** `cargo test -p confy-core`; headless tests for a mutating intent refused, for
`Undo` refused, and for a **non**-empty-path pending edit still allowing both.

## T6 — RS2a: `rawState` (R27), pure refactor

**Target.** `web/ui.ts`, `web/style.css`, `web/touch/app.ts` (its own `setRawView`), whichever
specs read the flag.

**Change.** Replace `rawView: boolean` with `rawState: "off" | "view" | "write"`; derive
`body.raw-view` / `body.raw-write` in one place. **No behavior change** — `"write"` is
unreachable until T7.

**Acceptance.** `npm run typecheck`, `npm test` unchanged-green. This task exists precisely so
T7's diff is feature-only.

## T7 — RS2b: the textarea, R4–R8, the cues

**Target.** `web/index.html` (`#rawEdit`), `web/ui.ts`, `web/style.css`, `web/key-intent.ts`,
`i18n/*.json`, new `web/raw-write.spec.mjs`.

**Change.** R1 routing (empty-path pending edit → Raw write). The textarea sharing one selector
list with `#raw.raw-view` for every metric. R4 Apply (`⌘↩`) staying in write mode; R5's
`doc_revision` comparison deciding whether to re-seed; R6's apply-if-dirty-then-save on `⌘S`;
R7 `Esc`; R8's "render never writes the textarea while it is focused/dirty". The three cues and
the scroll/caret rules from the record's *Switching* table.

**Acceptance.** `npm run typecheck`; `raw-write.spec.mjs` covering render-never-clobbers, failed
Apply keeps the buffer, `⌘S` does not save after a failed Apply, scroll survives both swaps.
Manual pass in a real browser: the swap is jump-free.

## T8 — RS2c: the control band and the jump

**Target.** `web/ui.ts`, `web/breadcrumb.ts`, `web/toolbar-fold.ts`, `web/raw-write.spec.mjs`,
`i18n/*.json`.

**Change.** R13's crumbs-row Raw control band registered in `toolbar-fold.ts`; R12's un-hiding;
the shared `byteToCodeUnit` helper — T2 left it a concrete failing case (`drift === 8` on the
CJK+emoji fixture) — and the two selection branches for R14/R15, selecting the node's **whole
member, key included** (R29/F5: no value-only range exists); R16 one-way only; R17's jump
gated on a clean buffer with `web.raw.jump-needs-apply`.

**Acceptance.** `npm test` incl. a breadcrumb pick selecting the right span in both states and a
pick on a dirty buffer moving the cursor but not the caret; `npm run typecheck`.

## T9 — RS3: touch (R18/R19)

**Target.** `web/touch/app.ts` (the `openExternalEdit` call site), touch spec.

**Change.** Every path — including `[]` — keeps routing to the sheet (R18); the empty-path
failed Apply keeps the sheet open with the text intact (R19); the Action item reaches that
sheet. No write mode, no new surface, no breadcrumb.

**Acceptance.** `npm test` touch spec: an unparsable whole-file Apply leaves the sheet open with
the buffer intact. Real-device or emulator pass on Action item → sheet.

## T10 — RS4: VS Code suppression (R10)

**Target.** `web/ui.ts` (`VSHOST` gate), `editors/vscode/` if the Action item needs hiding.

**Change.** Under `isVsCode()`, the Raw pane's Edit control and the Action menu's *Edit whole
file* are suppressed — the workbench's own editor is this feature, and a second editable copy
would be two owners of one `TextDocument` (ADR 0007).

**Acceptance.** `npm run typecheck`; the extension builds; no whole-file editor reachable in the
webview.

## T11 — RS4: documentation

**Target.** `docs/reference/WEBUI.md` (Raw pane section + the Raw breadcrumb and jump),
`CHROME.md` (the crumbs-row Raw control band), `KEYMAP.md` (`⌘↩`, `⌘S`, `Esc` in Raw write),
`HOST_PARITY.md` (R9/R10/R12/R18 rows), `MESSAGES.md`
(`core.document.apply-failed`, `web.raw.jump-needs-apply`), `CLAUDE.md`'s module map if
`apply_document_text` warrants a line, `CHANGELOG.md`, and
`docs/plan/2026-09-09-open-follow-ups.md` (the root-hidden record's P3 row → *Done*; the new Q4
caret→cursor row added).

**Acceptance.** `npm test` + `node functional_smoke.mjs`; `rg` finds no reference doc still
describing Raw as read-only; this plan's Status → `Shipped (date)` and the design record's
Status → `Shipped (date)`.
