# confy behavior matrix

A normalized, cross-backend (TOML / JSON / JSONC / YAML) account of how a node's **nesting scope**
governs each editing behavior in the TUI. This is the canonical, self-contained reference; the
condensed form also lives in `CONTEXT.md § Nested behavior matrix`, and the per-row mechanics live in
each backend's `edit.rs` (`cst_edit.rs`, `json/edit.rs`, `yaml/edit.rs`).

The goal of the matrix is **one model for three formats**: the TUI implements each behavior *once* and
parameterizes the cross-backend differences through `ConfigDocument` facets — so the TUI never
name-checks a backend (see [§7 Abstraction](#7-abstraction--the-facet-layer)).

---

## 1. Governing rule

> Every behavior is governed by **exactly one container**, and the matrix column is always that
> *governing container's* scope — never the acted-on node's own kind, unless the behavior is about the
> node's own *insides* or *self-representation*.

So the **same node is looked up under different columns for different behaviors**:

- its **trailing comment** / **external-edit precision** / **add-a-sibling** → its **parent's** scope
  (tables **A** and **C**);
- what it may **hold**, how **children form on insert**, its **layout switch** → its **own** scope
  (table **B**).

Two governance classes fall out of this: **① parent-governed** (A, C) and **② self-governed** (B).

---

## 2. Scopes (the matrix columns)

Scope = **`kind × layout`** — five legal combinations. Each maps onto a concrete construct per backend:

| scope | TOML | JSON/JSONC | YAML |
|---|---|---|---|
| **global** (root, block-map) | top-level table | top object / array | top block map |
| **seq-flow** | `[A/I]` inline array | inline array | flow seq `[ … ]` |
| **seq-block** | `[A/M]` array · `[[AoT]]` group | multiline array | block seq `- ` |
| **map-flow** | `[T/I]` inline table · `[T/D]` dotted¹ | inline object `[T/I]` | flow map `[T/F]` |
| **map-block** | `[T/S]` scope · `[[AoT]]` entry | multiline object `[T/M]` | block map `[T/B]` |

¹ `[T/D]` dotted table: block *layout* but **map-flow rules** (rebuilds its members on edit, holds no
comments). It is the one construct whose layout and rule-set diverge.

**2×2 observation.** Of the two axes, **`layout` (flow vs block) is the primary discriminator** — it
decides trailing comments, insert forming, layout switch, and external-edit precision. **`kind` (seq
vs map) is secondary** — it only decides whether children are *keyed*: seq elements are keyless, so
they have no rename and no `Tab`-to-Name in the inline editor.

---

## 3. Table A — Branch node as a **child** (governed by parent; column = parent scope)

How a container behaves *as an item inside another container*.

| behavior \ parent scope | global | seq-flow | seq-block | map-flow | map-block |
|---|---|---|---|---|---|
| own trailing comment | ✓ | ✗ (flow) | ✓ | ✗ (flow) | ✓ |
| own external precise edit | ✓ | ⚠ whole repr | ✓ | ⚠ whole repr | ✓ |
| add: collapsed → sibling | ✓ | ✓ (rebuild) | ✓ | ✓ (rebuild) | ✓ |
| paste-in forming | — | see *Insert / move legality* in `CONTEXT.md` | | | |

- **flow parents (seq-flow / map-flow)** hold their children on one line, so a child has no own line
  for a trailing comment (✗) and isn't independently `Replace`-addressable as text — an external edit
  takes the **whole inline repr** (⚠).

---

## 4. Table B — Branch node as a **container** (governed by self; column = its own scope)

How a container behaves *toward its own children and its own shape*.

| behavior \ own scope | global | seq-flow | seq-block | map-flow / `[T/D]` | map-block |
|---|---|---|---|---|---|
| holds standalone comment node | ✓ | ✗ | ✓ | ✗ | ✓ |
| insert / append child forming | add line | rebuild `[ … ]` | add line | rebuild `{ … }` | add line / section |
| add: expanded → append child | ✓ scalar (clamp) | ✓ bare elem (rebuild) | ✓ bare elem | ✓ member (rebuild) | ✓ scalar (clamp) |
| switch layout flow↔block (`K`) | ✗ (root) | ✓ → block | ✓ → flow² | ✓ → block | ✓ → flow² |

² **`K` layout switch** toggles a container between its flow and block layout (TOML `[A/I]`↔`[A/M]`
and `[T/I]`↔`[T/D]`↔`[T/S]`; JSON object/array Inline↔Multiline; YAML map/seq block↔flow). The
**collapse-to-flow** direction is rejected (`Illegal`) when the container **holds a comment** or a
**multi-line element**, because a flow layout can hold neither. The criterion is symmetric: every flow
scope can expand to block, and every block scope holding only inline-representable children can
collapse to flow.

- **flow containers** can hold no standalone comment node (✗); an insert **rebuilds** the one-line
  `[ … ]` / `{ … }` from its members plus the new one.
- **block containers** add a child as a new line / section; a scalar appended into the root or a
  block-map branch is **clamped** to the leading region (before any `[table]`/`[[aot]]`) so it stays
  legal TOML.

---

## 5. Table C — Leaf node as a **child** (governed by parent; column = parent scope)

How a scalar (or comment) behaves *as an item inside a container*.

| behavior \ parent scope | global | seq-flow | seq-block | map-flow | map-block |
|---|---|---|---|---|---|
| own trailing comment | ✓ | ✗ (flow) | ✓ (multiline elem) | ✗ (flow) | ✓ |
| own external precise edit | ✓ | ⚠ whole repr | ✓ just the element | ⚠ whole repr | ✓ |
| inline editor | ✓ single-line | ✓ as repr | ✓ | ✓ | ✓ (multiline str → `$EDITOR`) |
| add: collapsed leaf → sibling | ✓ | ✓ | ✓ | ✓ | ✓ |

---

## 6. Criteria (the design goals the matrix must satisfy)

### 6.1 Universal scalar inline editing

> Every **single-line scalar** leaf is inline-editable with **precise (element-level) `Replace`** in
> *every scope*, independent of nesting depth.

This covers global, both seq layouts, both map layouts, TOML `[T/D]`/`[T/S]`, and AoT-entry members —
so the table-C "inline editor" row is ✓ across all columns for single-line scalars. The **only** route
to `$EDITOR` is a scalar's **Format**, never its scope: a multiline / literal `|` / folded `>` string
opens `$EDITOR` because it cannot round-trip through a one-line field.

A single-line **plain-array element** follows the same rule **wherever the array sits** — even nested
under a key (`array_int[1].vals[0]`); `Replace` addresses the element directly. The gate is simply
"immediate parent is a plain `Array`" (an AoT group is `ArrayOfTables`, not `Array`, so its entries
stay `$EDITOR`).

### 6.2 Symmetric layout switch

> Every flow scope can switch to block and back (`K`); collapse-to-flow is rejected only when the
> container holds something a flow layout can't represent (a comment or a multi-line element).

See table B, note ².

### 6.3 Uniform external-editor precise range

> `e` / `E` captures and Replaces **just the edited node** in every backend — no truncation.

`App::external_edit_path` resolves the capture:

- A standard-array **element** (`x[0]`, `x[0][1]`) has no key; its bare repr isn't
  `Replace`-addressable on its own in TOML/JSON, so its edited repr is wrapped as the value-Replace
  form (`scalar_fragment(None, …)` → TOML `__elem__ = …`, JSON a bare value). YAML's `- value`
  fragment is addressable directly — no wrap.
- A key / index reached **through** an array index (`x[0].a`, `x[0].a.b`) is `Replace`-addressable
  directly too: the inline splice rebuilds the enclosing `{ … }` / `[ … ]` element in place. So the
  whole path is kept and the edit lands precisely (this closed the last TOML/JSON gap; earlier those
  truncated to the whole array).

---

## 7. Abstraction — the facet layer

The matrix is realized as **single TUI implementations parameterized by `ConfigDocument` facets**,
not as per-backend branches. The trait carries every cross-backend difference, so adding a fourth
format is purely additive:

| facet | what it parameterizes | TOML | JSON | YAML |
|---|---|---|---|---|
| `scalar_fragment(key, value)` | value / member forming | `key = value` | `"key": value` | `key: value` |
| `array_element_fragment(value)` | bare keyless element | `value` | `value` | `- value` |
| `empty_container_fragment(kind, key)` | the `a`-add container seed | `[table]` / `[[aot]]` | `{}` / `[]` | `{}` / `[]` |
| `array_elements_addressable()` | element / through-array `Replace` precision (routing + wrap) | `false` | `false` | `true` |
| `rename_can_change_type()` | dotted-key rename → `[T/D]` type-change check | `true` | `false` | `false` |
| `kind_options(path)` | the `K` flow↔block popup list | per-node | per-node | per-node |
| `split_value_comment(buffer)` / `replace_preserves_trailing_comment()` | trailing-comment edit | `#` lexer / `true` | `//` lexer / `true` | `#` lexer / `false` |

**Not abstracted, by design:** the per-backend splice engines (`cst_edit.rs`, `json/edit.rs`,
`yaml/edit.rs`) share a **contract** (the `Mutation` enum), not a **mechanism** — the three `rowan`
green trees have different shapes (taplo vs hand-rolled JSON vs YAML reindent). The `Mutation` enum
*is* the abstraction; a shared splice core would add complexity for no behavior gain.

---

## 8. Invariants (not scope-dependent — hold everywhere)

- **Comment merging.** Consecutive `#` / `//` comment lines project as one multi-line Comment node; a
  blank or non-comment line breaks the group.
- **First-class comments.** A standalone comment is a real node in document order — navigable,
  selectable, movable, deletable — and moving/copying another node never drags a comment with it. An
  end-of-line comment is instead the owning node's `trailing_comment` decoration and travels with it.
- **YAML opaque nodes** (`&anchor`, `*alias`, `<<:` merge, `!tag`, multi-line flow) are read-only:
  every behavior on or into them returns `Unsupported`, whatever the underlying kind.
- **Atomic mutations.** Every mutation edits a scratch tree and commits only on success, with a
  semantic post-check — a failed edit leaves the document byte-for-byte untouched.

---

## 9. Terminology

Use **Node** (recursive), never "Entry" (which is wenv's flat, line-based term and is a rename bug in
confy). Node subtypes: **Root**, **Branch node**, **Leaf node**, **Scalar**, **Comment**. The
operation toggling a live Node ↔ Comment is **Remark** (`r`). See `CONTEXT.md` for the full glossary.
