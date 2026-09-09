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
| 2026-09-09 | **F15** `?diag=1` drain extracted to shared `web/diag.ts`; touch went 0 → 12 logged lines on 3 keystrokes, desktop unaffected | (this commit) |
