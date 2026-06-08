# CST backend migration — comments as real, independent nodes

> **Status:** approved direction (Option B). Not yet started. No `model/` rewrite code
> has been written. This is the implementation plan to review before execution.

**Goal:** Replace `toml_edit::DocumentMut` as the single source of truth with a **lossless
syntax tree (CST)** in which standalone comments are *real, independently-positioned nodes* —
not decor glued to the following item. This removes the whole class of "comment travels /
can't insert below a comment / decor whack-a-mole" problems at the root, and makes
`serialize()` a byte-identical token concatenation.

**Backend:** [`taplo`](https://crates.io/crates/taplo) parser → a [`rowan`](https://crates.io/crates/rowan)
green/red syntax tree. In rowan, `# comment`, whitespace and newlines are **tokens with real
positions**, so a comment is a first-class sibling, round-trip is lossless by construction, and
the projection becomes a near-direct map.

**Seam:** the existing `ConfigDocument` trait (`load` / `project` / `serialize` / `apply`).
Build a new `CstDocument` implementing it **side by side** with `TomlDocument`; keep the old
backend until the new one reaches parity on the full test corpus, then switch `cli.rs`/`main.rs`
and delete `TomlDocument` last. Nothing flips until parity.

**Tech stack:** Rust, taplo, rowan, ratatui — `cargo test`, `cargo clippy -- -D warnings`,
`cargo fmt --check`.

---

## Why this is large (read before estimating)

rowan trees are **immutable/persistent** — there is no ergonomic "insert key / remove key" API
like `toml_edit`. Every edit = rebuild/splice green nodes. So the win (lossless + comments as
nodes + free serialize + simpler projection) is paid for by **re-implementing the entire
mutation layer** (`Insert`, `Delete`, `Replace`, `Rename`, `Move`, `Remark`, `EditComment`,
`InsertComment`) plus every existing special case (dotted keys vs dotted headers, AoT entries,
inline tables, scalar `Format`/`ScalarType` detection, exact-position reorder, collision modes)
as token-tree surgery, re-validated against the current corpus.

**Off-ramp (recorded, not chosen):** Option A (stay on `toml_edit`, add localized decor edits per
operation) delivers the same *user-visible* behaviour far cheaper. If midway the cost outweighs
the benefit, A remains available.

---

## Central design decision — how comments are addressed (resolve in Phase 2)

Today a comment is a synthetic child with key `#comment:N`; `Seg::Key("#comment:0")`. With real
CST nodes that hack goes away. **Proposed scheme (recommend):** a parent's children become a
single ordered sequence in which comments are first-class. Keyed items are still addressed by
`Seg::Key(name)`; **positional-only** nodes (comments, array/AoT elements) are addressed by
`Seg::Index(i)` where `i` is the index **within the parent's full child sequence**. The projection
stops inventing `#comment:N`; `Target.index` stays a child-sequence index (it already is). This
ripples into the TUI's `#comment:N` checks (filter exclusion, paste partition, delete/edit
routing) — they get *simpler* (ask `NodeKind::Comment` instead of sniffing a key prefix) but must
be rewritten. Confirm this scheme at the start of Phase 2 before building the projection.

---

## File map

| File | Change |
|---|---|
| `Cargo.toml` | add `taplo` (and `rowan` if needed transitively-but-explicit) |
| `src/model/cst_doc.rs` *(new)* | `CstDocument`: holds the rowan root; `load`/`serialize`/`project`/`apply` |
| `src/model/cst_project.rs` *(new)* | CST → `NodeTree` (comments as real nodes; `Format`/`ScalarType` from syntax) |
| `src/model/cst_edit.rs` *(new)* | rowan splice helpers; one fn per `Mutation` variant |
| `src/model/mod.rs` | export the new backend |
| `src/model/node.rs` | drop `#comment:N` assumptions if the addressing scheme changes paths |
| `src/tui/*` | remove `#comment:N` special-cases; switch construction to `CstDocument` (Phase 5) |
| `src/model/toml_doc.rs`, `project.rs` | **deleted last**, only after parity |

`model/` stays pure (no TUI deps) and unit-testable, as today.

---

## Phase 0 — Clean baseline

- [ ] Commit + push the current comment-move fix (branch is 13 commits ahead of `origin/main`).
      Start the migration from a pushed, green baseline so it can be reverted wholesale if needed.
- [ ] Create a working branch (e.g. `cst-backend`).

## Phase 1 — Foundation: parse + serialize, byte-identical (folds in the spike)

- [ ] Add `taplo` to `Cargo.toml`; pin a version; `cargo build`.
- [ ] `CstDocument::load(path)`: read file → `taplo` parse → keep the rowan root. **Reject parse
      errors** (preserve "invalid TOML is rejected, doc untouched"); confirm taplo surfaces errors.
- [ ] `CstDocument::serialize()`: `root.to_string()` (token concatenation).
- [ ] **Round-trip test (the contract):** for every fixture in `tests/fixtures/` and `test.toml`,
      `load → serialize` is **byte-identical**. This is the spike — if taplo can't round-trip a
      fixture, stop and reassess (off-ramp) before going further.
- [ ] Pin down the actual taplo API names here (parser entry, `SyntaxNode`, `SyntaxKind` variants);
      record them at the top of `cst_doc.rs`.

## Phase 2 — Projection: CST → NodeTree (comments become real nodes)

- [x] Confirm the addressing scheme (see *Central design decision*).
- [x] `CstDocument::project()`: walk the syntax tree → `NodeTree`. Map syntax kinds to `NodeKind`
      (Table / ArrayOfTables / Array / InlineTable / Scalar(ScalarType) / Comment). A `COMMENT`
      token between items projects as a standalone `Comment` node **in document order** — no decor
      sniffing, no `comment_blocks` merging hack needed (though consecutive `#` lines may still be
      grouped for display; decide and document).
- [x] Re-derive `ScalarType` and `Format` from syntax kinds (string/integer/float/bool/datetime;
      hex/oct/bin/basic/literal/multiline).
- [x] Preserve current structural behaviour: dotted **keys** collapse; dotted **headers** nest;
      AoT entries; inline-table members.
- [x] **Parity tests:** snippet battery is full-parity; fixtures are structural-parity (value/trailing
      differ by two intended *improvements* — no spurious leading space; EOL comment split into `trailing_comment`).

## Phase 3 — Mutations on rowan (the bulk)

Port one variant at a time, **TDD**: lift each existing `toml_doc.rs` mutation test, point it at
`CstDocument`, make it pass via rowan splicing. Build `cst_edit.rs` splice helpers first
(insert-token-sequence-before / remove-node / replace-node, with newline/indent normalization).

- [x] `Replace` (scalar value; whole-document on empty path; structured array/inline; whole `[table]` section).
- [x] `Rename` (position- & decor-preserving — swap the key token in place; collision-checked).
- [x] `Insert` (keyed under table/root; bare element into array; collision modes Overwrite/Rename/Cancel).
- [x] `Delete` (keyed entry; array element; comment node — plain removal, no decor sweep; `[table]` section; `[[aot]]` entry).
- [x] `Remark` (toggle live node ↔ comment).
- [x] `EditComment` (edit a comment node's text — token splice).
- [x] `InsertComment` (insert a standalone comment node at a child-sequence position).
- [x] `Move` (atomic; comments do not travel — they are separate nodes, so `detach_leading_comments` is gone for good).
- [x] `cst_edit` tests green (44) + full suite green (245). Resolver tied to projection by a single `walk` + consistency test.

**Phase 3 deferred long-tail edges** (revisit before/within Phase 5 as the TUI needs them):
inline-table *member* delete (carries a `,`), AoT entry **move/remark**, whole-AoT-group delete/replace,
byte-perfect **multiline-array** element insert/delete spacing, dotted-key rename collision nuances.

## Phase 4 — New capability enabled by the model

- [ ] **Insert a node below a comment:** with comments as real ordered nodes, an insertion target on
      a comment row is just "insert after this child index". Add the TUI affordance + a model test.
      (This is the originally-requested feature #2, now natural.)

## Phase 5 — Switch the TUI over, retire `toml_edit`

- [ ] Replace `#comment:N` special-cases across `tui/` with `NodeKind::Comment` checks (filter
      haystack exclusion, paste node/comment partition, delete/edit routing, render).
- [ ] Point `cli.rs`/`main.rs` construction at `CstDocument`.
- [ ] Full `cargo test` (incl. `tests/roundtrip.rs`) green on the new backend; manual TUI smoke test
      by the user (per the no-pty-TUI-testing rule).
- [ ] Delete `toml_doc.rs`, `project.rs`, and the `toml_edit` dependency **only now**.

## Phase 6 — Docs + cleanup

- [ ] Rewrite `CLAUDE.md` architecture section (the "CST projection" / decor paragraphs are mostly
      obsolete — comments are real nodes now).
- [ ] `CHANGELOG.md` Unreleased entry.
- [ ] Remove dead helpers (`detach_leading_comments`, `clipboard_fragment` strip, decor sweeps).

---

## Parity strategy (the safety net)

The contract throughout is the **existing test corpus**: `tests/roundtrip.rs` (byte-identical
round-trip) + every `model/` unit test + fixtures. Each phase must keep them green against the
backend currently wired in. `CstDocument` is never user-facing until it passes all of them; the
flip in Phase 5 is the single risky moment and is guarded by the full suite + a manual smoke test.

## Risks / watch-items

- **taplo round-trip fidelity** (Phase 1 gate). If any fixture isn't byte-identical, reassess.
- **rowan edit verbosity** — mutations are lower-level; budget the most time for Phase 3.
- **Addressing-scheme ripple** into the TUI (`#comment:N`) — contained but touches several files.
- **Format/ScalarType re-derivation** must match `detect_format` exactly (covered by parity tests).
- **Dotted keys vs dotted headers / AoT / inline tables** — re-prove each from syntax kinds.
- This is a multi-session effort; keep `TomlDocument` as the live backend until Phase 5.
