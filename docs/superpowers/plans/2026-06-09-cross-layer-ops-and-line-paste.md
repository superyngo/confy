# Cross-layer node ops + precise line-paste

> **Status:** spec for review. Not started. Sits on top of the `cst-backend` work
> (CstDocument is live per Phase 5b). No code written yet.

**Goal.** Two coupled changes:

1. **Any node, cross-layer** — copy / move / add / delete any node type across levels,
   *as long as the result is semantically legal TOML*. Illegality is caught at insert
   time and is non-destructive.
2. **Precise line-paste** — replace the "whole target row turns green" paste cue with a
   **green insertion line** that sits *between* nodes, so the user picks an exact slot.

---

## Locked decisions (from review 2026-06-09)

| # | Decision | Source |
|---|---|---|
| D1 | **Simple adaptation only.** Cross-layer paste auto-adapts the *easy* cases (bare→table: synthesize key; scalar/array/inline→array: drop key). The *hard* coercions (`[table]`↔inline table, `[table]`→array element, →AoT entry) are **rejected**, not coerced. | Q1 |
| D2 | **Green line defaults to the row-above's container** at a depth boundary (no extra ←/→ depth picker in v1). | Q2 |
| D3 | **Adopt idea 3** for normal-mode `a`: open branch → append as child; closed branch → append as next sibling of the same structured kind. Motivated by TOML's table-capture rule (D5). | Q3 |
| D4 | **Reject, don't clamp.** A paste whose slot is illegal for the node's type fails non-destructively (stays in paste mode, status explains why) — preserves the "precise position" promise. Mirrors the existing `do_paste` restore-on-failure design. | Q1/Q4 + existing design |
| D5 | **Source-order legality is part of the check** (see below), not just container-type compatibility. | TOML table-capture insight |

---

## D5 — the TOML table-capture rule (the load-bearing constraint)

A `[table]` / `[[aot]]` header opens a section: every `key = value` after it (until the next
header) belongs to that section. Therefore **within any table (including Root) the legal layout
is partitioned**:

```
[container]
  <scalars / arrays / inline-tables>   # the "leading region"
  <sub-tables / AoTs>                  # headers; everything after is captured
```

Consequences that ripple through every operation:

- A **scalar / array / inline-table** child may only land in the **leading region** — i.e. at an
  index **≤ the first sub-table/AoT header** of the container. Landing it after a header would
  re-key it into that header's section (the classic `root_scalar2` bug).
- A **sub-table / AoT** child lands in the **header region** (after all leading-region children).
- This is per-container and recursive (inside `[a]`: `a.x` before `[a.b]`).

So the semantic check = **container-type compatibility (matrix) ∧ source-order partition (D5)**.

---

## The adaptation / rejection matrix (D1)

Container kinds that can receive: `Root`/`Table`, `InlineTable`, `Array`, `ArrayOfTables`.
Cells: ✓ keep · `key↓` drop key · `key+` synth key · ✗ reject (v1).

| source ↓ \ dest → | Root / Table | InlineTable | Array | ArrayOfTables |
|---|---|---|---|---|
| **Scalar** (keyed) | ✓ *(leading region)* | ✓ | `key↓` bare scalar | ✗ |
| **Array** (keyed) | ✓ *(leading region)* | ✓ | `key↓` nested element | ✗ |
| **InlineTable** (keyed) | ✓ *(leading region)* | ✓ | `key↓` bare inline-table | ✗ |
| **Table** `[t]` | ✓ *(header region)* | ✗ coerce | ✗ coerce | ✗ coerce→entry |
| **ArrayOfTables** `[[t]]` | ✓ *(header region)* | ✗ | ✗ | ✗ |
| **bare Array element** (no key) | `key+` placeholder | `key+` placeholder | ✓ | ✗ |
| **Comment** | ✓ (decor, positional) | ✓ | ✓ | ✓ |

- `key+` = synthesize a key. Default `placeholder`; on collision append `_2`, `_3` (reuse the
  existing `OnCollision::Rename` retry already in `insert`).
- The ✗-coerce cells are the deferred "hard" cases (candidates for a later phase, not v1).
- Comments already route through `InsertComment` (no key, no collision) — unchanged.

---

## UX model — the green insertion line (req 2 / idea 1)

**Two paste-target shapes share paste mode:**

- **Line target (default):** a green horizontal rule between two visible rows. `↑/↓` move it
  through the N+1 slots (before-first … after-last). It resolves to `Target { parent, index }`
  via the row-above's container (D2). This replaces the green *row* fill in `ui.rs:314-317`.
- **Branch target (idea 2):** the user can still land *on* a branch node (not a between-slot)
  and paste → **append as last child** of that branch, regardless of open/closed. "Last child"
  obeys D5 (a scalar-type payload lands at the leading-region tail, not after sub-tables).

How the two are distinguished is a key-design detail to settle in Phase A (proposal: the slot
*on* a branch row = branch target; the slots between rows = line targets; or a modifier toggles).
Branch-node selection/toggle stays available in paste mode (idea 1) — selection mutators remain
frozen as today (`app.rs:496-521`), but cursor movement + expand/collapse stay live.

---

## Normal-mode add changes (idea 3 / D3, idea 5)

Current `resolve_target` (`insertion.rs:11`): Root or **expanded** branch → **first** child
(`index 0`); leaf or collapsed branch → sibling after cursor.

New behavior:

- **Expanded branch** → **last** child (not first), clamped by D5 to the correct region for the
  seeded kind.
- **Collapsed branch** → **next sibling**, seeded as the **same structured kind** (Table→Table,
  AoT→AoT entry). Always source-order-legal (a header after a header).
- **Leaf** → sibling after cursor (unchanged), seeded scalar.
- **Root** → last top-level child (was first), D5-clamped.

**Placeholder typing (idea 5).** Add normally seeds an empty-string scalar `""` (today's
behavior). If a scalar is **illegal at the resolved slot** (D5 — e.g. the only legal slot is in
the header region), fall back: seed an **empty node of the neighbor's kind** — the currently
selected node's kind, else the kind of the node just before the insertion slot — with key
`placeholder`. Empty-container fallback: seed by the container's expected child kind.

---

## Where the code changes land

| File | Change |
|---|---|
| `model/document.rs` | Possibly a new `MutateError` variant for "semantic/source-order illegal" (vs `Collision`). |
| `model/cst_edit.rs` | `insert` / `move_nodes`: enforce the matrix + D5 partition; implement `key↓` (strip key → array element via existing `array_insert`) and `key+` (synth key). Reject ✗ cells with the new error. |
| `model/cst_project.rs` | Helper to compute a container's **partition split index** (first header child) for D5 clamping/checking. |
| `tui/insertion.rs` | `resolve_target`: first→last child for branches; D5 clamp; collapsed-branch sibling kind. New line-slot → Target resolution. |
| `tui/app.rs` | Paste flow: line-target vs branch-target; `do_paste` keeps restore-on-failure (now also restores on semantic-reject, D4). `add_node` placeholder typing. |
| `tui/ui.rs` | Render green line between rows instead of green row fill; branch-target highlight. |
| `tui/state.rs` | Paste-mode state may need a "line slot" cursor distinct from row cursor. |
| `tests/` | New corpus: cross-layer legal moves (key drop/synth), every ✗ reject, D5 partition (scalar after table rejected), line-paste positions, idea-3 add. |

---

## Phased task list (each phase ends green: `cargo test && cargo clippy -- -D warnings && cargo fmt --check`)

**Phase A — green line UI (no new semantics).** ✅ done 2026-06-09 (258 tests, clippy/fmt clean).
- A1. State + render: green insertion line between rows; `↑/↓` move slot; branch-target case.
  → verify: visual slot moves through all N+1 positions; paste still lands where the old green
  row would have (behavior parity), branch-target appends last child.
- A2. `resolve_target` from a line slot using row-above container (D2).
  → verify: unit tests for boundary slots (depth drop, first, last).

**Phase B — semantic check + non-destructive reject (D1 matrix gate + D5 partition).** ✅ done 2026-06-09 (262 tests). *Note:* implemented the **D5 source-order partition gate** in `cst_edit::insert` + `MutateError::Illegal`; the full D1 type×container matrix (✗ cells) is still mostly caught by pre-existing `Fragment`/`Unsupported` errors and is finalized alongside Phase C adaptation.
- B1. `partition_split` helper in `cst_project`; D5 check in `insert`/`move_nodes`.
- B2. New error variant; `do_paste`/`add_node` restore-on-reject with explanatory status (D4).
  → verify: scalar-after-table paste rejected & clipboard intact; ✗ matrix cells rejected.

**Phase C — simple adaptation (D1 ✓ + key↓/key+).** ✅ done 2026-06-09 (266 tests). `parse_fragment_adapted` handles `key↓` (keyed→array via the existing VALUE-extracting `array_insert`) and `key+` (bare→table, synthesized `placeholder`, forced auto-rename). `[table]`/`[[aot]]`→array rejected. *Known gap:* **cut**(`Move`) of a bare array-element source is still `Unsupported` in `move_nodes` — `key↓`/`key+` adaptation currently only covers the **copy** path; cut of an array element is a follow-up.
- C1. `key↓`: keyed node → array drops its key (route to `array_insert`).
- C2. `key+`: bare element → table synthesizes `placeholder` key (collision retry).
  → verify: round-trip corpus for each adapted cell; reparse stays valid TOML.

**Phase D — add behavior (idea 3 / D3) + placeholder typing (idea 5).** ✅ done 2026-06-09 (269 tests). `add_node` rewritten: expanded-branch/root → last child (scalar D5-clamped to the leading region); collapsed-branch/leaf → next sibling; scalar-illegal slot → same-kind structured `placeholder` (`[placeholder]`/`[[placeholder]]`).
- D1. `resolve_target` first→last child; collapsed-branch same-kind sibling; D5 clamp.
- D2. Placeholder typing fallback when scalar illegal at slot.
  → verify: `a` on open `[table]` adds last child; on closed `[table]` adds sibling `[table]`;
  `a` where scalar illegal seeds neighbor-kind node keyed `placeholder`.

---

## Phase A details (settled 2026-06-09)
- **Hybrid line+row cue.** `↑/↓` step through a merged sequence of slots: a **green line**
  *between* rows = a precise *sibling* slot; the cursor *on* a branch row = the whole row turns
  **green** = *append as last child* of that branch (idea 2, any open/closed). Line for precision,
  row for "into this branch".
- **`a` stays cursor-relative** — no green-line placement mode; it applies the idea-3 rules
  directly to the cursor row.

## Follow-up work / backlog
- **#7 done** (2026-06-09): single-line arrays + inline tables show value in the VALUE
  column and edit inline (`project_array`/`project_inline` carry a one-line `value`;
  `edit_target_kind` routes them; structured `Replace` writes back). Commit `bcd27bb`.
- **#6 (in progress):** multiline-array interior **comments** as first-class nodes +
  edit/delete/move on them — projection change in `cst_project`.
- **Cut(Move) of a bare array element** is still `Unsupported` (Phase C gap).
- **CST bug (uncovered):** a *local datetime* (`1979-05-27T07:32:00`) **inside an array**
  projects as `Scalar(LocalDate)` instead of `LocalDatetime` (parity diff vs the legacy
  backend). The committed fixtures have no datetimes-in-arrays, so it is untested.
  Suspect `scalar_kind` / taplo token kind for an array element. Deferred.

## Out of scope (v1)
- Hard coercions (`[table]`↔inline, →array element, →AoT entry) — the ✗ cells.
- ←/→ depth picker on the green line (D2 fixed default instead).
