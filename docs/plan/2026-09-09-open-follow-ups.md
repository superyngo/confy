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

### F15 — Touch has no `?diag=1` drain

Priority **P3** · Effort **XS** · Opened 2026-09-09 (split out while closing F4)

`drainDiagIfEnabled` lives only in `web/ui.ts`; `web/touch/app.ts` has no equivalent, so the
`?diag=1` console trace is desktop-only even though both hosts drive the same `ConfySession`
and the same ring. Not a defect in the drain that exists — a missing surface. Small enough to
fold into whichever touch task comes next.

**Acceptance.** `?diag=1` on the touch entry prints the same `[confy-diag] …` lines, with the
cursor reset on session swap as the desktop host now does.

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

Priority **P2** (raised from P3 on 2026-09-09: measured, and the consequence is larger than
"not exhaustive" suggested) · Effort **S** · Verified 2026-09-09 · From audit 2026-08-29

~15-20 sites in `crates/confy-tui/src/tui/app.rs` and `tui/mod.rs` mutate `Session` directly
(`self.session.mode = Mode::Normal`, `session.paste_slot = Some(…)`, `session.toggle_expand()`),
bypassing `dispatch` and therefore invisible to the diag ring and to `ApplyOutcome`. All the
needed variants already exist. Closing this unlocks deterministic record/replay: a session
becomes a replayable `Vec<Intent>`.

**Acceptance.** No direct `session.<field> =` outside `confy-core`; a scripted session replays
headlessly from its captured `Vec<Intent>`.

**Measured 2026-09-09** (real binary, while fixing F3). The consequence is not theoretical: in
a live TUI session, **the only `Intent` that ever reaches `dispatch` is `SetHostNotice`**. 34
navigation keystrokes (`j`/`k`/`9`) produced an **empty** diag ring — the `~` overlay drew as a
two-row borders-only sliver. Every event that does appear comes in the same fixed triple:

```
[Debug] dispatch SetHostNotice
[Info ] notice  severity=… source=HostTui text="…"
[Info ] mutation SetHostNotice ok
```

So the TUI's diagnostic channel currently records *messages the host already displayed* and
nothing else — no navigation, no mutation, no mode change. A "dispatch" line naming an Intent
that is always the same one is not a trace. This is the strongest argument for closing
F7, and it means F3's tail-take fix made a small window *useful*; it did not make the channel
*complete*.

### F8 — `MutateError` mixes interactive outcomes with real errors

Priority **P2** (raised from P3 on 2026-09-09: it now has a measured, user-visible symptom, the
table below) · Effort **S** · Verified 2026-09-09 · From audit 2026-08-29

`Collision` and `Fragment` are prompts the user answers; `NotFound`, `Illegal` and `Unsupported`
are failures. A host cannot tell them apart by type. `anyhow` is already out of the parse
signature (`AnyDocument::from_str_as` returns `Result<Self, ParseError>`), so this is the
remaining half.

**Two measured cases are folded in here** (found 2026-09-09 while closing F1/F2/F14 — do not
patch them per-backend, they are symptoms of this taxonomy):

| input | TOML | JSON | YAML |
|---|---|---|---|
| unterminated fragment (`"unclosed`, `[1, `) | `Fragment(…)` | **`Illegal("expected R_BRACE, found None")`** | `Ok` — lexer is lenient, see *Watching* |
| gesture does not apply here | `Unsupported` | *was* `Illegal(…)` | *was* `NotFound` |
| `d` on the Root row (measured 2026-09-09, real binary) | `NotFound` → *"delete error: path not found"* | — | — |

The second row was unified to `Unsupported` everywhere by F1 (`72805c0`) — by hand, per backend,
which is exactly the work this finding removes the need for. The first row is still uneven: JSON
reports a *rule violation* for what TOML calls *unparseable input*, and `MESSAGES.md` §2 maps
those to different severities, so a user sees a different message per format for one mistake.

The third row is the same mistake in a different place: the root is not *missing*, it is
*undeletable*, so `Unsupported` is the honest variant and the message should say so. A user
reasonably reads "path not found" as a confy bug.

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
- **The YAML subset lexer accepts unterminated scalars.** `x: "unclosed`, `x: [1, ` and
  `x: {a: ` all lex as plain scalars. This was filed as half of F14 and is **not** a defect:
  `AnyDocument::from_str_as` accepts the same text from disk, so `Replace` and load agree.
  Tightening one without the other would mean a value you can open but cannot retype. Only
  fix these together, and only if the subset parser is made strict on purpose.
- **`MutateError` variant choice on a bad fragment is still uneven.** JSON returns
  `Illegal("expected R_BRACE, found None")` where TOML and YAML return `Fragment(…)` for the
  same class of input. Same family as F1's `Unsupported`/`Illegal` split; fold into F8
  (`MutateError` taxonomy) rather than patching per-backend.
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
| 2026-09-09 | **F5** Touch `sev-*` toasts styled — border/left-bar tint per severity, matching the desktop status hues | (this commit) |
