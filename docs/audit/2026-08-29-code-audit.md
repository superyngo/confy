# confy architecture audit — 2026-08-29
Status: Resolved (2026-09-09)

Focus: better implementation / design / tech stack / architecture (not bug hunting).
Six parallel read-only audits. Baseline: clean tree on `main`, 35 commits ahead of `origin/main`.

Detailed reports:
- [`01-backend-abstraction.md`](2026-08-29-code-audit/01-backend-abstraction.md) — three-backend model layer
- [`02-session-host-seam.md`](2026-08-29-code-audit/02-session-host-seam.md) — core/host boundary
- [`05-core-api-perf.md`](2026-08-29-code-audit/05-core-api-perf.md) — core API design + measured perf
- Tech stack, web frontend, and tests/CI findings are inlined below (those agents reported
  structured findings rather than writing files), which is why 03/04/06 have no file.

**Resolution.** Every finding here was re-verified on 2026-09-09
([`2026-09-09-open-findings-reverification.md`](2026-09-09-open-findings-reverification.md))
and then closed or consciously parked: the fixes are the F1–F15 rows in
[`../plan/2026-09-09-open-follow-ups.md`](../plan/2026-09-09-open-follow-ups.md), and the
deliberately-not-fixed remainder is that record's *Watching* section.

---

## Headline: the architecture is sound; two concrete defects and one supply-chain risk

Every audit independently concluded the load-bearing design choices are right:
lossless CST as single source of truth, Node tree as a rebuilt projection, atomic
`clone_for_update` + validate + commit, headless filesystem-free core, `Intent` as the
host-facing command vocabulary. None of the six recommends changing any of these.

What did surface: one severe measured performance defect, a cluster of cross-backend
behavioral drift, and an abandoned upstream dependency.

---

## P0 — `Move` is quadratic and freezes the UI (verified on the real binary)

Confirmed by running `cargo bench -p confy-core --bench perf` directly, not just from
agent report:

| doc size | `apply(Replace scalar)` | `apply(Move 1 source)` |
|---|---|---|
| 2,801 nodes (43 KB) | 5.97 ms | 87.5 ms |
| 7,001 nodes (110 KB) | 15.8 ms | 752 ms |
| 70,001 nodes (1.1 MB) | 326 ms | **40.2 s** |

Multi-source is worse: 8 sources at 7,001 nodes = **5.87 s**. The 70k-node run blew a
900 s benchmark budget before reaching the multi-source cases.

Root cause (`model/cst_edit/move_paste.rs:868-1126`): `move_nodes` re-walks the whole CST
per source and per fragment — `walk(tree, "")` costs 5.67 ms at 7k nodes and runs 25+ times
for an 8-source move. `delete` (`replace_delete.rs:1255`) and `insert` (`move_paste.rs:49`)
each walk again internally.

Fix: batch deletions into one multi-target pass; resolve anchors via the existing `CstIndex`
instead of re-walking before and inside every insert. **S effort, Low risk.**

Note the same superlinear shape exists in the JSON and YAML `move_nodes`
(`json/edit.rs:1167-1258`, `yaml/edit/mutations.rs:281-385`) — audit 01 F4 flags these three
as near-identical duplicated orchestration, so one shared fix addresses all three.

## P0 — every keystroke serializes the document twice

`apply` computes `(syntax, text)`, uses `text` only for the dirty flag, drops it
(`cst_doc.rs:69`, `json/doc.rs:54`, `yaml/doc.rs:54`). Then
`Session::on_mutation_success` (`session.rs:1868`) calls `doc.serialize()` again for the
undo snapshot. Wastes 1.2 ms per keystroke at 7k nodes.

Ironic given `5053026` explicitly deduped the *reparse* for exactly this reason; the
serialize half was missed. Fix: thread the already-computed `text` through. **S effort, Low risk.**

---

## P1 — cross-backend behavioral drift, and the test gap that allows it

These two findings came from different agents and are causally linked.

Concrete divergences with no documented reason (audit 01 F2):
1. **Insert into an empty document**: TOML succeeds (`move_paste.rs:49`), YAML succeeds via
   `insert_into_empty_document` (`yaml/edit/block.rs:839-844`), **JSON fails** with
   `NotFound` (`json/edit.rs:452,535-544`) — pressing `a` on an empty `.json` file just fails.
2. **Rename with a quoted key**: JSON wraps input as `format!("{{\"{new_key}\": 0}}")`
   (`json/edit.rs:971`), producing `"\"key\""` for a key that already carries quotes;
   its collision check compares a raw literal against a decoded name (`json/edit.rs:951`).
3. **Remark an array element**: YAML succeeds (`yaml/edit/mutations.rs:122`), TOML returns
   `Unsupported` (`replace_delete.rs:1159`), JSON returns `Illegal` (`json/edit.rs:1089`) —
   three formats, three different outcomes for one gesture.

Why it persists: backend parity is tested **ad hoc**, not systematically. Parity tests are
written as separate per-format functions rather than one table-driven loop over `DocFormat`
— `external_edit_clears_trailing_comment.rs` (json:29, toml:42, yaml:49),
`insert_after_trailing_comment.rs` (json:21, toml:38, yaml:49),
`session_headless.rs:248-265`. `BEHAVIOR_MATRIX.md`'s stated goal is "one model for three
formats", but nothing mechanically enforces it.

Fix as one unit: convert 6-8 high-value parity tests to
`for (fmt, src, expected) in [...]` loops, which will fail on exactly the drift above,
then fix the three divergences. **S-M effort, Low risk.**

## P1 — `taplo` is abandoned upstream (verified independently)

The maintainer stepped down in Dec 2024 ([tamasfe/taplo#715](https://github.com/tamasfe/taplo/issues/715)),
having been inactive 1-2 years; the repo is stalled but not archived, and no ownership
transfer has happened. `docs/reference/glossary.md:210-227` already documents that a TOML parse-stage bug can
only be worked around at the integration layer or reported upstream — that escape hatch is
now closed. It also pins `rowan =0.15.18` for the two hand-rolled backends.

The good news, which changes the calculus: **confy's taplo surface is tiny.**

```
47  taplo::parser::parse
 9  taplo::syntax::*  /  9  taplo::rowan::NodeOrToken  /  6  SyntaxElement
 1  taplo::dom::Node  /  1  taplo::dom::Error::ConflictingKeys
```

Vendoring only what is used = `parser/mod.rs` (941) + `parser/macros.rs` (21) +
`syntax.rs` (277) ≈ **1,240 LOC**. taplo's 1,330-line formatter and 2,800-line DOM are not
needed — confy uses the DOM solely for duplicate-key detection, which the JSON and YAML
backends already implement themselves in `validate_semantics`. Vendoring would also unpin
`rowan` and drop `globset`, `schemars`, `arc-swap`, `itertools`, `once_cell` from the tree
(shrinking the wasm bundle). The one snag: taplo's lexer uses `logos 0.12` (current 0.15).

Tombi, the tool the community migrated to, is **not** a usable dependency — `tombi` on
crates.io is a reserved placeholder and its sub-crates are unpublished.

Recommendation: don't migrate now. Record it as a known risk in `CLAUDE.md` with the
vendoring plan and its 1,240-LOC scope, and add a `cargo audit` CI job so a rowan/ahash
advisory becomes the trigger. This converts an open-ended liability into a costed,
pre-planned contingency. **S effort now, M effort if triggered.**

---

## P2 — the "god objects" are 80-93% inline tests

The most useful reframing in this audit. Three files that look like architectural problems
are mostly test code:

| file | total | production | tests |
|---|---|---|---|
| `model/cst_edit/mod.rs` | 3,383 | **279** | 3,103 |
| `crates/confy-tui/src/tui/app.rs` | 4,447 | **~900** | 3,528 |
| `model/yaml/edit/mod.rs` | 2,034 | **136** | 1,897 |

So `cst_edit` was already decomposed (Task 15) and the tests were left behind; `App` is a
thin facade, not a bloated host. The right action is moving tests to `tests.rs` siblings —
mechanical, zero runtime change — **not** splitting modules. Similarly `Session`'s 26 fields
and ~140 methods look like a god object but the cohesion is real: nearly every mutation needs
`doc` + `tree` + `history` + `expanded` + `cursor` + `mode` + `notice` together, and splitting
would force `Rc<RefCell<_>>`. Group fields into `FilterState` / `PendingEditContext` (26 → 18)
and stop there.

Only genuinely monolithic production file: `model/json/edit.rs` (2,612 LOC, all production),
which would benefit from matching YAML's `block/flow/mutations/convert` split.

## P2 — presentation logic re-derived in TypeScript

- `web/kind-labels.ts` (134 LOC) reconstructs KIND badges from raw `ViewRow` fields, including
  heuristics like `scalar_type === "Float" && format === "Plain" → "dec"`, duplicating what
  core's `classify()` (`session/type_filter.rs:46-120`) already knows. Fix: add
  `badge_label` / `badge_note` to `ViewRow` and delete the module.
- `web/help-content.ts` (~180 LOC) hardcodes help text and KIND legends as TS constants while
  the TUI reads them from `i18n/*.json`. `about_text` was already unified this way
  (`state.rs:about_text()`) — apply the same pattern. `web/i18n.ts:7-13` already imports the
  catalogs, so this is nearly free.

Both **S effort, Low risk**, and both remove a class of TUI-vs-web visual drift.

---

## P3 — smaller items

- **`Intent` isn't exhaustive.** TUI bypasses it with direct field mutation
  (`tui/mod.rs:462,477`, `app.rs:420,508`) and direct calls (`tui/mod.rs:280,386-403`),
  skipping the `ApplyOutcome` log and diagnostic ring. Closing this unlocks deterministic
  record/replay: any session becomes a `Vec<Intent>` replayable headlessly. All the needed
  variants already exist. **S effort.**
- **Undo via `rowan::GreenNode` instead of `String`.** `History` stores full text snapshots,
  200 deep (`session/state.rs:209-257`); undo re-parses (1.09 ms). Green trees are immutable
  and refcounted, so snapshots would share unchanged subtrees: ~80-90% less RAM and O(1)
  undo. **M effort** — genuinely cheaper now than when ADR 0003 was written.
- **Allocation nits.** `to_view_row` (`session.rs:247,250`) heap-allocates `type_label` and
  `key_sign` per row — 14,002 needless allocations per frame at 7k rows. Make them
  `&'static str`. **S effort.**
- **`anyhow` in public signatures** (`any_doc.rs:42`, all three `from_str`) prevents hosts
  from matching structured parse failures; `MutateError` conflates interactive outcomes
  (`Collision`, `Fragment`) with real errors (`NotFound`, `Illegal`). **S effort.**
- **CI gap.** `web-ci.yml` builds the web bundle but never runs `web/render.spec.mjs` or
  `confy-ffi/functional_smoke.mjs` (92 checks), so wasm regressions can ship.
  `rust-ci.yml:33-39` correctly gates fmt + clippy `-D warnings` + test. **S effort.**
- **Property tests for the round-trip invariant.** Three lossless parsers with a
  byte-identical guarantee, covered by 14 curated fixtures. Minimal addition: `proptest` dev-dep,
  one fixture-seeded round-trip property per backend, ~40-80 LOC total. **S effort.**
- **Dependency upgrades**, bundled: `thiserror 1→2`, `unicode-width 0.1→0.2`, `dirs 5→6`
  (all S/Low); `ratatui 0.28→0.30` + `crossterm 0.28→0.29` and `jsonschema 0.30→0.44`
  (M/Low). `fuzzy-matcher 0.3` is unmaintained — `nucleo-matcher` is ~6x faster (M/Med).
  `ureq 2→3` is a Sans-IO rewrite for a single GET — defer.
- **`CHANGELOG.md` at 341 KB / 2,042 lines** — split by version series before v1.0.0.
- **Slow `cargo test` is a build-hygiene problem, not a test-suite problem** — see below.

---

## Late finding (P1) — `target/` is 71 GB and it is eating your dev loop

Found while measuring test time, not by any agent. `cargo test --workspace` showed
`real 854s` against `user 5.13s` — blocked on I/O, not computing. Cause:

| path | size |
|---|---|
| `target/debug/incremental/` | 34.8 GB (2,200 dirs) |
| `target/debug/deps/` | 29.3 GB — **664,122 files** |
| `target/debug/` total | 59.7 GB |
| Android cross-targets (4) | 9.7 GB |
| `target/wasm32-unknown-unknown/` | 1.1 GB |
| **total** | **71 GB** |

Proof of the cost: `cargo test -p confy-core --lib` runs **546 tests in 0.09 s** but takes
**110 s wall**. Over 99.9% of that is cargo stat-ing fingerprints across 664 k files. Even
`du -sh target` takes 31 s.

So the earlier "854 s test suite" is not a test-architecture problem — the monolithic
`session_headless.rs` is exonerated twice over. It is pure build hygiene.

Remedy (maintainer to run — deliberately not executed by this audit):
```bash
rm -rf target/debug/incremental     # reclaims 34.8 GB; incremental is a cache, safe to drop
cargo clean                         # full reset, reclaims all 71 GB, costs one cold rebuild
```
Incremental compilation buys little here because CI and most local runs are full builds;
consider `CARGO_INCREMENTAL=0` for the test path, and periodically pruning the Android/wasm
target dirs which are only needed at release time. **S effort, very high daily payoff.**

## Deliberately left alone

- **Lossless CST + full reprojection per mutation.** 5.67 ms at 7k nodes. Incremental
  subtree reprojection would collide with TOML dotted-table promotion, AoT regrouping and
  scattered section spans for no felt gain. Ceiling: comfortable to ~20-30k nodes, perceptible
  lag past ~50k. Config files don't go there.
- **No shared "generic CST splice" engine.** ~93% (~8.4 k LOC) of edit code is irreducible
  format semantics — taplo's external rowan types vs two internal ones, open-set section spans
  vs `{}`-bounded containers vs 2-D indentation math, three different mutation techniques.
  Only ~600 LOC is genuinely shareable (collision-suffix loops, move orchestration, comment
  prefixing). `BEHAVIOR_MATRIX.md §7` already argued this; the audit confirms it.
- **The web frontend.** Genuinely disciplined: stateless render from `SessionSnapshot`, DOM
  refs cached once, no state duplicated between DOM and JS, no listener leaks, no imperative
  overlay machines. Avoids every classic no-framework failure mode — adding a reactive layer
  would be a regression. The desktop/touch fork is justified (16 of 23 modules already shared);
  keyboard-hybrid vs touch-primary are orthogonal interaction models.
- **`session_headless.rs` at 100 KB / 137 tests.** Monolithic on purpose to avoid Rust's
  one-binary-per-integration-test-file compile cost. Correct call.
- **esbuild over Vite; hand-rolled bench over criterion.** Both right for this codebase.
- **Headless schema fetch handshake** (`schema_fetch_request` ↔ `Intent::SchemaLoaded`) keeps
  core free of I/O so it compiles to wasm with no shims. Schema compilation is already cached.
- **Flat `Node` struct** over a polymorphic enum — an enum would force pattern matching at
  every tree hop in `flatten`/`node_at`/filters for zero benefit.
- **Single i18n JSON read by both Rust `include_str!` and TS `import`.** Already correct.

---

## Suggested sequence

1. `Move` de-quadratication + drop the double serialize (both P0, both S/Low, same area).
2. Table-driven parity suite, then fix the three drift divergences it exposes.
3. Extract inline tests out of the three production files (mechanical, big readability win).
4. Document the taplo risk + vendoring plan; add `cargo audit` and the missing web CI steps.
5. `ViewRow` badge fields + i18n help legends → delete `kind-labels.ts`, shrink `help-content.ts`.
6. Bundled low-risk dep upgrades.
