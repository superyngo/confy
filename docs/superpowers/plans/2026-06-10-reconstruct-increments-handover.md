# Handover — "reconstruct proposal" increments on the CST backend

> **Status:** handover for a fresh session. No implementation started. The expected first
> deliverable is an implementation plan (task list) for user approval — do not write code
> before the plan is approved, and resolve the **Open decision** below with the user first.

## Where the codebase stands (2026-06-10)

- The **CST migration is complete** (commit `f130556`): `CstDocument` (taplo → rowan) is the
  only backend; `toml_edit` is deleted. Comments are first-class nodes addressed by
  `Seg::Index` over the parent's full child sequence. Every mutation is atomic.
- All checks green: `cargo test` (201 lib + 2 integration), `cargo clippy -- -D warnings`,
  `cargo fmt --check`. v0.4.0 is the last release; the retirement commit is unreleased.
- Read `CLAUDE.md` (architecture section freshly rewritten — it is accurate) and `CONTEXT.md`
  (canonical glossary: Node/Root/Branch/Leaf/Scalar/Comment/Remark; never "Entry").
- Closed plan for background: `docs/superpowers/plans/2026-06-08-cst-backend-migration.md`.

## Origin: the user's "reconstruct proposal" and its assessment

The user proposed an abstract `[Key Sign, Value Sign]` node model with fixed-pitch TUI type
tags. Assessment (agreed with the user in the 2026-06-10 session):

**Already built** (do not rebuild): comments as keyless first-class nodes; the
`[Key Sign, Value Sign]` shape (it is `Node { key, kind, format }`); TOML nesting constraints
(taplo enforces at parse, `cst_edit` per mutation).

**Rejected** (do not implement):
- The Body/Facade abstraction (`TOMLTable` as `Map`, `TOMLArray` as `List` + format facades) —
  it re-creates the dual-source-of-truth problem the migration eliminated, and a `Map` cannot
  hold ordered keyless comment children. The CST stays the body; the Node projection is the facade.
- The "AoT Sameness Check" (vacuous: AoT children can't diverge by construction).
- A standalone duplicate-key validator presented as deterministic (flattened-path uniqueness
  misses real TOML rules, e.g. dotted-key table closure; the parser stays the authority).
- The "Sibling Termination Boundary" as stated (it forgets that standalone comments legally
  appear between/after `[table]` scopes; any such rule must exempt `[key/none, comment]`).

**To adopt — the three increments this session is for:**

### Increment 1 — `KeySign` + container `Format` facets (model layer)

- Add a `KeySign` facet to `Node`: `Bare | Quoted | Dotted | None` (None for array elements,
  comments, AoT entries, Root). Derived **read-only during projection** from taplo syntax
  kinds, exactly like a scalar's `Format` today. (Optionally split `Quoted` into
  basic/literal later; start simple.)
- Extend `Format` (currently scalar-only, `node.rs`) to containers:
  - Array: `Inline` vs `Multiline` vs (AoT is already its own `NodeKind`).
  - Table: `Scope` vs (InlineTable is already its own `NodeKind`).
  - The projection already knows single-line vs multiline: a single-line array/inline table
    carries its one-line repr in `node.value`; a multiline array leaves it `None`
    (`project_array`/`project_inline` in `cst_project.rs`). Make this explicit as Format
    instead of inferring from `value.is_none()`.
  - Also consider `inf`/`nan` as Float formats (proposal §2C) — cheap, same pattern.
- Update the golden tests in `cst_project.rs` (`norm` prints `fmt=`; new facets will change
  expected strings — regenerate deliberately, don't fudge).

### Increment 2 — fixed-pitch TYPE-column tags (ui layer only)

Render the TYPE column with the proposal's fixed-width tags, fed from `NodeKind` + `Format` +
`KeySign` (pure `ui.rs`/`app.rs` rendering change; no model semantics):

- Key sign, 3 chars: `(B)` bare, `(Q)` quoted, `(D)` dotted, `(-)` none.
- Container, 5 chars: `[G]  ` global/root, `[C]  ` comment, `[A/I]` inline array,
  `[A/M]` multiline array, `[A/T]` AoT, `[T/I]` inline table, `[T/S]` table scope.
- Scalar, 8 chars `[X:xxxx]`: `[S:str ] [S:mstr] [S:lit ] [S:mlit]`,
  `[I:dec ] [I:hex ] [I:oct ] [I:bin ]`, `[F:flt ] [F:inf ] [F:nan ]`, `[B:bool]`,
  `[D:odt ] [D:ldt ] [D:ldat] [D:ltim]`.
- Note: there is **no `[T/M]`** (see Open decision). The rationale for fixed pitch is visual
  alignment/stable column width — confy renders ratatui spans from the Node struct, so do
  NOT add regex/string-slicing "highlight" machinery from the proposal.
- Current labels to replace/coexist with: `node_type_label` (`app.rs`, also used by the
  inline editor's type-change detection — keep that comparison working),
  `branch_type_format` (`app.rs`), detail-popup labels, and the TYPE cell in `ui.rs`
  (inline tables currently render `inline-table`). Decide with the user whether the old
  word labels survive anywhere (detail popup?) or the tags replace them everywhere.

### Increment 3 — array comment-upgrade rule (mutation layer)

Today inserting/moving a comment **into a single-line array is rejected** (a `#` would
comment out the `]`); multiline arrays accept comments as first-class nodes
(`array_insert_comment` in `cst_edit.rs`, v0.4.0 changelog). The increment: instead of
rejecting, **auto-reformat the array to multiline** (one element per line, comment on its
own line) and then insert. This is valid TOML 1.0.
- Decide UX with the user: silently upgrade vs. prompt (`y/n`) — the user's global rules
  favour surfacing the tradeoff once in the plan.
- The inverse (last comment removed → collapse back to inline?) is **not** requested; don't
  build it speculatively.
- The table version of this rule is TOML 1.1 — blocked on the Open decision; default is NO.

## Open decision (ask the user before planning around it)

**TOML 1.1 stance.** The proposal's `table/ml-lne` (multiline inline tables `{\n … \n}`,
comments inside inline tables) is not valid TOML 1.0 — it's an unreleased TOML 1.1 draft
feature. Options: (a) drop it entirely (recommended — keeps confy interoperable),
(b) treat "comment into inline table" as a conversion to a `[table]` scope,
(c) gate behind an explicit TOML-1.1 mode. The user has not chosen yet. This determines
whether a `[T/M]` tag exists in Increment 2 and whether Increment 3 gets a table sibling.

## Working agreements (from the user's global rules + project memory)

- Plan first with a task list; wait for approval. Surface tradeoffs; don't pick silently.
- Never drive the TUI via pty or background processes — the user smoke-tests manually.
- `cargo test` / `cargo clippy -- -D warnings` / `cargo fmt --check` must be clean.
- After each task: CHANGELOG.md Unreleased entry; update CLAUDE.md if architecture facts
  change (e.g. Format gaining container variants changes its description).
- Commit only when the user says so; `main` is the branch.
