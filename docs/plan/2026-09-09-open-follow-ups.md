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
  [`MESSAGES.md` §8](../reference/MESSAGES.md)) stay frozen; this file is where their open
  items are tracked.
- When the last row reaches Done, this record's `Status:` becomes `Resolved (date)` and it
  joins the frozen set.

Effort is XS (< 1 h) / S (a session) / M (multi-session).

---

## Open

### F1 — Remark an array element diverges across all three backends

Priority **P1** · Effort **S** · Verified 2026-09-09 · From audit 2026-08-29

One gesture, three outcomes:

| backend | outcome | site |
|---|---|---|
| TOML | `Err(Unsupported)` | `cst_edit/replace_delete.rs`'s `remark`, wildcard arm |
| JSON | `Err(Illegal("cannot remark an array element"))` | `json/edit.rs`'s remark |
| YAML | `Ok(())` — succeeds | `yaml/edit/mutations.rs`'s remark |

Two of the three also disagree about which variant means "this gesture does not apply here",
and `Unsupported` vs `Illegal` is a **user-visible severity difference** (`MESSAGES.md` §2), not
an internal detail. So the semantics must be decided before the code: is remarking an array
element legal (YAML's answer), or is it a shape the format cannot express (TOML/JSON's)?

Do this together with F2 — the parity loop is what pins the answer down.

**Acceptance.** All three backends return the same outcome for the same gesture, asserted by
one table-driven case; the chosen semantics is written into `BEHAVIOR_MATRIX.md`.

### F2 — No table-driven parity loop over all three `DocFormat`s

Priority **P1** · Effort **S-M** · Verified 2026-09-09 · From audit 2026-08-29

`BEHAVIOR_MATRIX.md`'s stated goal is "one model for three formats", and nothing mechanically
enforces it. Two test files now iterate formats
(`tests/external_edit_clears_trailing_comment.rs` over Json+Toml,
`tests/insert_after_trailing_comment.rs` over Toml+Yaml) — but **each leaves the third format in
a separate block below the loop**, which is precisely the shape that lets drift through.
`tests/hostile_input.rs` iterates all three, but for recursion-depth fuzzing, not semantics.
`tests/session_headless.rs` has zero format loops.

**Acceptance.** 6-8 high-value behaviors run as `for (fmt, src, expected) in [...]` over all
three formats. Seeded with F1, the suite fails before the fix and passes after.

### F3 — TUI `~` diag overlay shows the oldest 20 events

Priority **P2** · Effort **XS** · Verified 2026-09-09 · From `MESSAGES.md` §8

`draw_diag_overlay` (`crates/confy-tui/src/tui/overlay_diag.rs`) collects the ring oldest-first
and sizes the box `lines.len().min(20)`; `Paragraph` renders from line 0, so everything after
the 20th is clipped — exactly the recent activity an operator opens the overlay to see. No
scroll state, no `.rev()`, no tail-take; `App` has no diag scroll field. Ring `CAPACITY = 256`;
a dispatch emits 2 events and 3 with a notice, so the overlay goes **blind after 7 interactions
with notices, 10 without**. TUI-only — no other host renders the ring. The docstring's
"newest last" describes an intent that was never implemented.

Fix: take the tail before rendering (`skip(len.saturating_sub(20))`), or add real scroll state.

**Acceptance.** Real-binary run: after 10+ mutations, `~` shows the most recent events with the
last one at the bottom. Same keystrokes before and after. `MESSAGES.md` §4.1's overlay row and
the `draw_diag_overlay` docstring corrected; the §8 entry removed.

### F4 — Web `?diag=1`'s `lastSeenSeq` is never reset

Priority **P3** · Effort **XS** · Verified 2026-09-09 · From `MESSAGES.md` §8

`web/ui.ts`'s module-level `lastSeenSeq` is advanced only inside `drainDiagIfEnabled` and reset
nowhere, while **8** paths replace the `ConfySession` (whose ring restarts at `seq = 0`) — so
post-swap events below the old high-water mark are skipped in the console drain. Because there
are 8 sites, the reset belongs next to a single session-replacement helper, not at each one.
Touch has no drain at all, so the trace is desktop-only. Debug-only blast radius.

**Acceptance.** Open file A, emit events, open file B: the first post-swap event appears in the
console. §8 entry removed.

### F5 — Touch `sev-*` toast classes have no CSS

Priority **P3** · Effort **XS** · Verified 2026-09-09 · From `MESSAGES.md` §8

`web/touch/app.ts`'s `renderNotice` applies the classes; `web/touch/style.css` has
`.toast`/`.toast.show` and zero `sev-*` rules, so a `Warn` differs from a `Success` only by its
auto-hide timer (3000 ms vs 1600 ms). `web/style.css` *does* tint `sev-warn`/`sev-success` — for
the desktop footer status line — so the two hosts currently disagree on whether severity is
visible at all. Cosmetic, MVP-deferred.

**Acceptance.** Warn/Error toasts are visually distinct from Success/Info on touch, using the
desktop footer's existing hues. §8 entry removed.

### F6 — `model/json/edit.rs` is the last unsplit production file

Priority **P2** · Effort **S** · Verified 2026-09-09 · From audit 2026-08-29

2,864 lines, of which 1,090 are an inline `#[cfg(test)] mod tests`. The other three "god
objects" were resolved by extracting tests to a sibling `tests.rs` via `#[path = "tests.rs"]`
(`cst_edit/mod.rs` 295 production lines, `tui/app.rs` 1,009, `yaml/edit/mod.rs` 147). JSON is
the one file that is genuinely monolithic *production* code, and it would benefit from YAML's
`block`/`flow`/`mutations`/`convert` split.

**Acceptance.** Tests in a sibling `tests.rs`; production code split along YAML's boundaries.
Pure code motion — no behavior change, no new tests.

### F7 — `Intent` is not exhaustive

Priority **P3** · Effort **S** · Verified 2026-09-09 · From audit 2026-08-29

~15-20 sites in `crates/confy-tui/src/tui/app.rs` and `tui/mod.rs` mutate `Session` directly
(`self.session.mode = Mode::Normal`, `session.paste_slot = Some(…)`, `session.toggle_expand()`),
bypassing `dispatch` and therefore invisible to the diag ring and to `ApplyOutcome`. All the
needed variants already exist. Closing this unlocks deterministic record/replay: a session
becomes a replayable `Vec<Intent>`.

**Acceptance.** No direct `session.<field> =` outside `confy-core`; a scripted session replays
headlessly from its captured `Vec<Intent>`.

### F8 — `MutateError` mixes interactive outcomes with real errors

Priority **P3** · Effort **S** · Verified 2026-09-09 · From audit 2026-08-29

`Collision` and `Fragment` are prompts the user answers; `NotFound`, `Illegal` and `Unsupported`
are failures. A host cannot tell them apart by type. `anyhow` is already out of the parse
signature (`AnyDocument::from_str_as` returns `Result<Self, ParseError>`), so this is the
remaining half. Best done **with F1/F2**, which is what exposes the confusion.

**Acceptance.** A host can match "needs an answer" vs "failed" without string inspection; the
severity mapping in `MESSAGES.md` §2 follows the type rather than the key.

### F9 — Undo stores full text snapshots

Priority **P3** · Effort **M** · Verified 2026-09-09 · From audit 2026-08-29

`History` (`session/state.rs`) keeps `String` snapshots, `MAX_HISTORY = 200`, and undo
re-parses. Green trees are immutable and refcounted, so `rowan::GreenNode` snapshots would share
unchanged subtrees: ~80-90% less RAM and O(1) undo. Genuinely cheaper now than when ADR 0003 was
written. No user-visible symptom today — this is a standing improvement, not a defect.

**Acceptance.** Undo/redo byte-identical to today across the full test suite, with a measured
memory reduction on the perf bench.

### F10 — Remaining `Move` cost: capture and `insert_with` traverse under a live index

Priority **P3** · Effort **M** · Opened 2026-09-09

The [live-index invariant](../reference/MUTATIONS.md) fix took TOML `Move` down ~48% by
dropping the whole-document index before the delete/insert phases. What is left — the capture
phase and `insert_with` — traverses *through* an index by design, so each of those traversals
still pays the 11-17× mutable-tree penalty. Removing it needs a structural change: a `CstIndex`
that stores paths/offsets rather than live `SyntaxElement` handles, resolved on demand.

Not urgent: at 7k nodes an 8-source move is 2.2 s, and config files of that size are already
outside the design target (`docs/audit/2026-08-29-code-audit.md` § *Deliberately left alone*).
Recorded so the ceiling is known rather than rediscovered.

**Acceptance.** TOML `Move ×8` at 7,001 nodes approaches YAML's 79 ms. Verified with
`cargo bench -p confy-core --bench perf -- --nodes 500`.

### F11 — Dependency upgrades not yet taken

Priority **P3** · Effort **S-M** · Verified 2026-09-09 · From audit 2026-08-29

Landed: `thiserror 2`, `unicode-width 0.2`, `dirs 6`, `ratatui 0.30`, `crossterm 0.29`.
Remaining: `jsonschema 0.30 → 0.44` (M/Low); `fuzzy-matcher 0.3` is unmaintained and
`nucleo-matcher` is ~6× faster (M/Med — it changes match *scoring*, so the fuzzy-filter
highlight tests are the real work); `ureq 2 → 3` is a Sans-IO rewrite for a single GET,
**deliberately deferred**.

**Acceptance.** Upgrades land as one bundled commit; the fuzzy swap keeps `KEYMAP.md`'s
machine-checked highlight parity green on both hosts.

### F12 — `CHANGELOG.md` should be split by version series

Priority **P3** · Effort **S** · Verified 2026-09-09 · From audit 2026-08-29

Flagged at 341 KB / 2,042 lines; now **482 KB / 4,130 lines** — it has grown 42% since. The
trend is the finding, not the size. Split before v1.0.0.

**Acceptance.** Root `CHANGELOG.md` holds the current series and links to
`docs/reference/changelog/` archives; the release workflow's version check still passes.

### F13 — `target/` build hygiene

Priority **P3** · Effort **XS** · Verified 2026-09-09 · From audit 2026-08-29 (late finding)

Maintainer action, not a code change. `target/debug/incremental` still exists, and there is no
`.cargo/config.toml` or `CARGO_INCREMENTAL` setting anywhere. The audit measured
`cargo test -p confy-core --lib` at 546 tests in 0.09 s but **110 s wall** — over 99.9% of it
cargo stat-ing fingerprints across 664 k files.

**Acceptance.** `rm -rf target/debug/incremental` (a cache, safe to drop) or `cargo clean`;
optionally `CARGO_INCREMENTAL=0` for the test path.

---

## Watching (no action planned)

- **`taplo` is unmaintained upstream.** Recorded as a known risk in `CLAUDE.md` with the
  ~1,240-LOC vendoring scope, and `rust-ci.yml` has an active `cargo audit` step — the agreed
  trigger. Do not migrate pre-emptively; `tombi` is still not a usable dependency.
- **`JsonDocument` does not override `rename_key_segs`.** TOML and YAML both do, to decode a
  rename's literal with their own key lexer. Not a live defect — JSON keys are always quoted, so
  the trait default coincides — but it is the last asymmetry in that area, and it would become a
  defect if JSON ever grew unquoted keys.

---

## Done

| Closed | Item | Commit |
|---|---|---|
| 2026-09-09 | Double serialize per mutation — `sync_schema_hint` now takes the text | `57630e4` |
| 2026-09-09 | TOML `Move` quadratic — live-index release in `move_nodes`/`delete`, −48% | `57630e4` |
| 2026-09-09 | JSON/JSONC parser-simplification plan — premise refuted, both halves already done | (record closed) |
