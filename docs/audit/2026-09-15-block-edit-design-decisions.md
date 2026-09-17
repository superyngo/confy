# Settled decisions — block-edit save-parse redesign
Status: Resolved (2026-09-15)

Consensus reached 2026-09-15 (grilling rounds 1-5, all 21 questions answered "都同意", i.e.
the recommended answer in each case). Evidence behind them:
[`2026-09-15-block-edit-design-evaluation.md`](2026-09-15-block-edit-design-evaluation.md).
Became [`../spec/2026-09-15-block-edit-whole-file-reparse-design.md`](../spec/2026-09-15-block-edit-whole-file-reparse-design.md)
(Shipped 2026-09-15).

## Round 1 — mechanism
- **Q1** Option **A** is the endgame: text-splice the node's byte span into the document text,
  commit via the existing whole-document `Replace{path: []}`. **B rejected** (1:N per-backend
  green-tree splice: 3× code, still slower). **C not built** — instead a small standalone
  stopgap: TOML leaf `Replace` rejects a fragment whose key ≠ the path's key.
- **Q2** Buffer **scope unchanged**: leaf opens its own line(s), branch opens its section. No
  "edit the parent scope" gesture in this design.
- **Q3** One **stated contract** for all three backends: *a buffer is accepted iff it parses on
  its own as a legal node sequence at the edited node's container level; otherwise the whole
  commit is rejected atomically.* Divergence is allowed only where the format's grammar forbids
  it. Goes into `BEHAVIOR_MATRIX.md` + `HOST_PARITY.md` as a testable parity matrix.

## Round 2 — semantics
- **Q4** Span = node body + its trailing EOL comment + its trailing blank-line run. A preceding
  standalone comment node is **excluded** (it has its own row and its own `e`).
- **Q5** On rejection: document untouched **and the editor stays open with the user's text**
  (all hosts). TUI implements it by re-spawning `$EDITOR` seeded with that text. Errors travel
  only on the notice channel — never injected into the buffer.
- **Q6** Cursor re-anchor: surviving path → stay; else → the **first node of the edited span**
  (resolved from the span's start byte offset after the reparse); else → first row.
- **Q7** Empty buffer is **rejected** (notice points at `d`). No delete-on-empty.
- **Q8** New `Intent::ApplyBlockText { path, text }`; `ApplyReplace` / `ApplyEditComment` stay
  as-is so hosts migrate one at a time. Removing `ApplyEditComment` is a later cleanup commit.
- **Q9** **Multi-span nodes are in v1** (scattered `[T/S]`, `[T/D]`, scattered `[A/T]`): delete
  the later ranges in reverse document order, splice the buffer at the first, reparse.
- **Q10** The TOML key guard ships **now, as its own commit**, together with the correction of
  `MUTATIONS.md:135` (which currently promises surplus rejection that the TOML backend does not
  implement). It is deleted again when A lands.
- **Q11** YAML **opaque spans become editable** via `e` (raw text, no confirm prompt) — the
  whole-file `E` route can already rewrite them (`yaml/edit/mod.rs:108` skips the empty path).
  Cost accepted: `read_only` narrows from "not writable" to "not *structurally* editable";
  `glossary.md:169-175`, `BEHAVIOR_MATRIX.md:214-215` and two tests
  (`yaml/edit/tests.rs:795`, `tests/session_headless.rs:3746`) must be rewritten.

## Round 3 — shape
- **Q12** Lives in the **session layer**: `Session::apply_block_text(path, text)` computes the
  spans, splices, and calls the existing whole-document `Replace`. Backends only gain a
  read-only `node_text_spans(path) -> Vec<(usize, usize)>`. The `Mutation` set stays closed.
- **Q13** New data-driven `crates/confy-core/tests/block_edit_parity.rs` holding the full
  {9 node shapes} × {3 formats} × {6 outcomes} matrix, **plus** a proptest: an unmodified buffer
  round-trips byte-identically.
- **Q14** Core **and the TUI** land in the same commit (so Q5's re-spawn is verified on the real
  binary from day one), then web/touch, then VS Code (no-op). Transitional host differences are
  recorded in `HOST_PARITY.md` with their removal condition.

## Round 4 — hazards
- **Q15** In-flow members/elements are **in** the contract; the span is the node's own token
  range, **commas stay outside it** (they belong to the container). Q7 closes the dangling-comma
  case.
- **Q16** YAML buffers are **verbatim, indentation included** — no dedent, no `reindent` on
  commit. Keeps Q13's byte-identity a pure string equality.
- **Q17** A flow element's span is **trimmed of surrounding whitespace**, so taplo's baked
  padding (`[ 1 ]` ⇒ `VALUE "1 "`) never enters the buffer and
  `detach_value_trailing_pad` is unnecessary on this route.
- **Q18** `replace_table_spans`' two bespoke checks ("headers stay in subtree", "must start with
  a `[header]`") are **retired**; legality is whatever the reparse + DOM validation accept. A
  buffer that renames a scattered table's header is now an intent, not an accident.
- **Q19** **No `e` on Root.** Root has no visible row by design, and `E` carries a
  whole-document lock (`guard_document_edit_locked`, R21/R24) that `e` must not inherit.

## Round 5 — plumbing
- **Q20** Two **new** notice keys, both `Warn`: `core.block.invalid`, `core.block.empty`.
  `Warn` (not `Error`) precisely because Q5 keeps the buffer open — nothing is lost. Register in
  `severity_of`, the catalog test, `MESSAGES.md` §2.2, and both en / zh-TW message tables.
- **Q21** Schema revalidation passes **`None`** — always revalidate in full. The `touched`
  fast path (`session.rs:2178-2188`, `dirty_check.rs:118`) is unsound for a commit that can
  add or rename arbitrary paths, and a missed soft violation is a silently absent feature.

## Corrections made during the rounds (do not re-assert the originals)
- The 5031 ms outlier is caused by a **fragment carrying an EOL comment** triggering a second
  full `set_trailing_comment` pass (`cst_edit/mod.rs:81`), not the live-index quadratic and not
  nesting: 289 ms → 4103 ms on the same document. `2026-09-15-block-edit-design-evaluation.md`
  §9.
- Machinery A deletes is **two** mechanisms (`wrap_element`, `split_packaged_blank`), not four;
  `apply_packaged_blank` survives as an inert branch of `apply_replace`.
- A branch's free-form section edit (header rename, added sibling section) is **intended
  behavior**, not a bug: `MUTATIONS.md:86`, `cst_edit/tests.rs:928`.
