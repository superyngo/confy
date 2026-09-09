# confy behavior matrix

A normalized, cross-backend (TOML / JSON / JSONC / YAML) account of how a node's **nesting scope**
governs each editing behavior in the TUI. This is the canonical, self-contained reference — there
is deliberately no condensed second copy anywhere. Vocabulary is in
[glossary.md](glossary.md), per-`Mutation` mechanics in [MUTATIONS.md](MUTATIONS.md), and the
per-row splice code in each backend's engine (`cst_edit/`, `json/edit/`, `yaml/edit/`).

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
| own external precise edit | ✓ | ✓ | ✓ | ✓ | ✓ |
| add: collapsed → sibling | ✓ | ✓ (rebuild) | ✓ | ✓ (rebuild) | ✓ |
| paste-in forming | — | see *Insert / move legality* in `MUTATIONS.md` | | | |

- **flow parents (seq-flow / map-flow)** hold their children on one line, so a child has no own line
  for a trailing comment (✗) — but it *is* precisely addressable: the splice patches or rebuilds the
  one-line `[ … ]` / `{ … }` around it, so an external edit captures and replaces that child alone
  (§6.3), keeping the collection's authored padding and separators byte-for-byte.
  A **nested** flow collection is an item like any other here — `g: [ {x: 1}, 2 ]` index 0 edits,
  replaces, deletes and moves as itself (it carried no projected `Target` at all until the
  registration landed, so every mutation on it returned `NotFound`). What a one-line item still
  cannot do is take a **block** layout: `kind_options` offers none and `ConvertKind` rejects it as
  `Unsupported`, since expanding it would break the line it lives on.
- **paste-in forming** is one instance of the cross-platform `PasteSlot` targeting model —
  ADR 0004.

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
  `[ … ]` / `{ … }` from its members plus the new one. The ✗ is enforced at the model layer —
  YAML's `InsertComment` answers `Unsupported` for a flow container, and a comment **paste** into
  one is refused outright rather than offered TOML/JSON's "reformat to multiline and insert?"
  prompt (which YAML's flow rebuild cannot honor).
- **block containers** add a child as a new line / section; a scalar appended into the root or a
  block-map branch is **clamped** to the leading region (before any `[table]`/`[[aot]]`) so it stays
  legal TOML.

---

## 5. Table C — Leaf node as a **child** (governed by parent; column = parent scope)

How a scalar (or comment) behaves *as an item inside a container*.

| behavior \ parent scope | global | seq-flow | seq-block | map-flow | map-block |
|---|---|---|---|---|---|
| own trailing comment | ✓ | ✗ (flow) | ✓ (multiline elem) | ✗ (flow) | ✓ |
| own external precise edit | ✓ | ✓ just the element | ✓ just the element | ✓ just the member | ✓ |
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

A **`bool`** leaf is the one single-line scalar that does *not* get a text field: `begin_inline_edit`
opens the two-option `true`/`false` picker (`Mode::SchemaEnum`, `from_schema: false`) instead, on
every host — the same widget a schema `enum` uses, since a bool's value domain is closed at two
members. Options follow the node's **authored casing** (YAML `True`/`TRUE` stay uppercase); a schema
`enum` on that same node outranks the fallback. Free-form text entry for a bool stays reachable
through the external editor (`BeginEditExternal` / TUI `E` / the panel's "Editor" button), which
never routes through the picker branch.

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
- An **item of a one-line flow collection** captures that item alone — the flow-map member
  (`b: 2`), the flow-seq element (`1`), the inline-table member (`y = 2`), the JSON object member
  (`"y": 2`) — and its commit splices over the item's own **trailing-whitespace-excluded** span, so
  the collection's authored padding survives an untouched round trip byte-for-byte. A YAML flow-seq
  *element* is the one item with no `Target` of its own (the projection indexes it as the whole
  collection plus an ordinal), so its fragment is sliced out by that ordinal
  (`flow::flow_item_text`); without it the capture was the entire `[ … ]`. That holds for a
  **nested collection** element (`g: [ {x: 1}, 2 ]`) too — the projection registers the same
  ordinal-addressed target for it, which is what made it editable at all.

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
| `array_elements_addressable()` | array **element itself** `Replace` precision (direct-index routing + external-edit wrap) | `false` | `false` | `true` |
| `array_member_keys_addressable()` | a **member reached via `Key` under an array index** (`x[0].a`) inline-vs-`$EDITOR` routing | `false` | `true` | `true` |
| `rename_can_change_type()` | dotted-key rename → `[T/D]` type-change check | `true` | `false` | `false` |
| `kind_options(path)` | the `K` flow↔block popup list | per-node | per-node | per-node |
| `split_value_comment(buffer)` / `replace_preserves_trailing_comment()` | trailing-comment edit | `#` lexer / `true` | `//` lexer / `true` | `#` lexer / `false` |

**Not abstracted, by design:** the per-backend splice engines (`cst_edit/`, `json/edit/`,
`yaml/edit/`) share a **contract** (the `Mutation` enum), not a **mechanism** — the three `rowan`
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
  every behavior on or into them returns `Unsupported`, whatever the underlying kind. **Schema
  validation skips them** rather than giving up on the file — an opaque node carries no Violation of
  its own (confy cannot decode its value), while every other node in the document validates
  normally. Document **conversion** is the stricter case: an opaque node aborts it outright.
- **Atomic mutations.** Every mutation edits a scratch tree and commits only on success, with a
  semantic post-check — a failed edit leaves the document byte-for-byte untouched.
- **A fragment is exactly one node.** `Insert`/`Replace` take one node's text; anything past it
  is rejected (`Fragment`), never silently dropped. This matters most in YAML, whose subset
  grammar is deliberately lenient — nearly any text lexes as a plain scalar, so a two-node
  fragment parses happily and the surplus would vanish without a message.
- **Remark needs a line of its own.** `r` applies to any node that occupies its own line(s) — a
  keyed member, an **array element**, a whole table/section — in all three formats, and
  un-remarking restores the source byte-for-byte. It does **not** apply inside a single-line
  collection (`a = [1, 2]`, `{"x": 1, "y": 2}`, `a: {x: 1}`), where a comment leader would swallow
  the siblings; nor to a read-only node (a JSON `/* … */` block, a YAML opaque span). Every
  backend reports the inapplicable case as **`Unsupported`** — never `Illegal` (which means a rule
  was broken) and never `NotFound` (the node is perfectly addressable; `Delete` and `Replace` both
  reach it). Enforced across all three formats by `tests/format_parity.rs`.

---

## 9. Terminology

Use **Node** (recursive), never "Entry" (which is wenv's flat, line-based term and is a rename bug in
confy). Node subtypes: **Root**, **Branch node**, **Leaf node**, **Scalar**, **Comment**. The
operation toggling a live Node ↔ Comment is **Remark** (`r`). See `glossary.md` for the full glossary.
