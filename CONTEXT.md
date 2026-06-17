# CONTEXT

Glossary for the confy codebase — a single-file TUI editor for structured config files
(TOML first). Resolved terminology only; no implementation details.

confy is modeled on [wenv](https://github.com/superyngo/wenv) for UX, but its domain language
is deliberately **different**: wenv is line-based and flat (`Group / File / Entry`); confy is a
**tree** over a structured document. confy does **not** use the term "Entry".

## Language

### Tree vocabulary

**Node**:
Any single element in the config tree. The umbrella term for everything the user navigates and
operates on. (Where wenv would say "Entry" — confy never says Entry.)
_Avoid_: Entry, item.

**Root**:
The single top-of-tree **Node** whose key is the filename. Every other Node descends from it.
There is exactly one Root per open file.
_Avoid_: File header (as a separate concept), top node.

**Branch node**:
A **Node** that has children and can be expanded/collapsed: a table, array-of-tables, array, or
inline table.
_Avoid_: Container, object, parent (parent is a relationship, not a kind).

**Leaf node**:
A **Node** with no children: a scalar value or a comment.
_Avoid_: Value node (a comment is a leaf but not a value), terminal.

**Parent / Child / Sibling**:
Standard tree relationships between **Nodes**. Siblings share a Parent and a key namespace
(the same table), which is why key collisions are resolved per-Parent.

### Node kinds

**Scalar**:
A **Leaf node** holding a typed TOML value — string, integer, float, bool, or one of the four
datetime types (offset-datetime, local-datetime, local-date, local-time).

**Format**:
The *writing style* of a Scalar, orthogonal to its type — e.g. an integer written as `0xFF` (hex)
vs `255` (decimal), or a string written `"…"` (basic) vs `'…'` (literal) vs `"""…"""` (multiline).
Derived (read-only) during projection from the rendered repr; round-trips byte-identically. The
eventual format-toggle operation is the write-side counterpart.

**Key sign**:
How a Node's *own key* is written, orthogonal to its type/format — **bare** (`port`), **quoted**
(`"a.b"`), **dotted** (`a.b.c`), or **none** (keyless: array elements, comments, AoT entries, Root).
Derived (read-only) during projection. Surfaced as the `(B)/(Q)/(D)/(-)` prefix in the KIND column
and as one half of the **Type filter**. Note: a top-level/scope dotted key now **nests** into
synthetic **Dotted tables** (see below); the whole decomposed chain (tables **and** leaf) reads
the **dotted** sign, so `(D)` marks any dotted-key origin (per-segment `bare`/`quoted` is not
surfaced for a decomposed chain). A dotted key *inside an inline table* decomposes the same way:
`t = { x.y = 1, x.z = 2 }` projects a synthetic **Dotted table** `x` under the inline table, and
operations on it route through the inline-table machinery (members stay `{ … }` entries).

**Member spans**:
The discrete pieces of source that *constitute* a table: its own `[a]` section (if written),
every descendant `[a.sub]` / `[[a.list]]` section — wherever it sits in the file — and any flat
dotted member lines. A table's definition is an **open set**: TOML lets these spans scatter and
interleave with foreign sections. `[T/D]`, `[T/S]`, **implicit** (only `[a.sub]` written) and
**mixed** (dotted members *plus* header sub-sections) tables are the four compositions of one
span list, and serialize/edit/delete/move all fan out over it.

**Dotted table** (`Format::Dotted`, KIND tag `[T/D]`):
A Table that exists only because dotted keys defined it (`a.b.c = 1` → tables `a`, `b`), with no
`[table]` header. A synthetic projection node merging the dotted entries that share a prefix
**within one scope**, shown at the table's **first** definition position (where a consolidating
block-rewrite lands). The value leaves stay mapped to their original source entries, so an
*untouched* file round-trips byte-identically. Editing it, though, does rewrite: a child `add`
seeds a scalar and inserts write a scope-relative dotted entry (`a.b.x = …`); `e` block-edits all
member lines and **consolidates** them at the first position; `d` deletes all members; renaming a
plain key to a dotted one (`foo` → `foo.x`) converts the scalar into a `[T/D]` table.
Whole-table move/copy fans out over the member lines.

A dotted key **inside an inline table** (`t = { x.y = 1, x.z = 2 }`) decomposes the same way, but
ops on the synthetic `[T/D]` route through the **inline machinery**, never the flat-ROOT splices
(`inline_ancestor_len` guards the path): insert/add re-prefixes the key scope-relative (`q = 9` into
`t.x` → member `x.q = 9`) and lands via `inline_table_insert` with the projected index translated to
a raw member slot (`inline_raw_member_index`); collision is exact full path (a shared prefix merges);
`Delete` and move/copy fan out over the member entries (`inline_member_entries`); the block edit
consolidates at the first member (`replace_inline_dotted_table`, single-line entries only); comments
are rejected (`{ … }` holds none). **Comments are never inside a `[T/D]` table**: a comment adjacent
to a dotted member is an independent scope-level node (stays put on table move/copy/delete and the
consolidating edit), and `InsertComment` targeting a `[T/D]` re-routes to the scope level — landing
directly above the table's first member, never rejected, never bound. `dotted_member_entries` counts
only **flat-ROOT** entries — an entry nested inside an inline-table/array *value* belongs to that
value, not the table, so its interior is never pulled out as a stray top-level line. Dotted *headers*
(`[x.a]` with no `[x]`) still project as a real nested `Scope` branch.

**Mixed table**:
A table defined by dotted members *and* header sub-sections (the TOML-spec `fruit.apple`
pattern: `apple.color = …` under `[fruit]`, plus `[fruit.apple.texture]`). The spec forbids
giving such a table its own header while any dotted definition remains, so: inserting an entry
writes a dotted member; inserting a sub-table writes a header section (legal); `e` consolidates
the whole table to **scope form** — a synthesized `[fruit.apple]` header with the dotted members
folded under it, then the member sections — the only header form that leaves nothing behind.

**Comment**:
A **standalone** comment line (occupies its own line) surfaced as a first-class **Leaf node**.
Navigable, selectable, remarkable, and movable like any Node. A "disabled" setting is just a
Comment whose text happens to be valid TOML; toggling it (see _Remark_) re-parses it back into a
live Node.
_Avoid_: Disabled entry, ghost node — these were earlier names for the same idea; the canonical
concept is "a Comment that is valid TOML".

**Trailing comment**:
An end-of-line comment that shares a line with a value (`port = 8080  # http`). It is **not** a
Node — it is decoration belonging to that Node, travels with it on edit/remark/move, and is shown
in the Node's Detail view. Only standalone comments become **Comment** Nodes.
_Avoid_: Inline comment node (it is never a node), suffix comment.

**Read-only node**:
A node whose `Node.read_only` flag is set: displayed in the tree and copyable, but rejecting edit
(`e`/`E`), delete (`d`), cut (`x`), and remark (`r`). Produced by JSONC `/* */` block comments
(a Comment node) and by YAML **opaque nodes** (any kind).

**Opaque node**:
A YAML node holding an out-of-subset construct — `&anchor`, `*alias`, `<<:` merge key, `!tag`, or
multi-line flow — projected as a **read-only node** with the KIND tag `[opaq ]` (whatever its
underlying kind). It survives round-trip byte-identically but cannot be mutated safely without full
YAML write support, so every mutation on or into it (or on any entry whose *value* is opaque)
returns `Unsupported`, leaving the document untouched. Copy is allowed.

**YAML subset**:
The slice of YAML 1.2 that confy edits as first-class nodes: a single document (optional leading
`---`), block + single-line flow maps/sequences, 5 scalar styles (plain, single-quoted,
double-quoted, literal `|`, folded `>` with chomping), and `#` comments. Anything outside it becomes
an **opaque node**; multi-document files are rejected at load.

**Core schema typing**:
YAML 1.2 core-schema scalar typing as confy applies it — `null`, `bool`, `int` (dec/hex/oct),
`float` (incl. `.inf`/`.nan`/exponent), else `string`. confy deliberately has **no datetime** type
in YAML: a date- or time-looking plain scalar is a string.

**Indent engine** (`reindent`):
The YAML splice core — the analogue of JSON's comma/brace normalization. It re-flows a fragment from
its captured source indentation to the destination's indent level when inserting/moving, so block
structure stays well-formed without per-call token surgery.

**JSONC upgrade**:
The prompt shown when a user triggers `r` (remark) on a node in a pure `.json` file (one loaded
without `supports_comments()` true). Confirming (`y`) flips the document's comment support on,
so the remarked node is written with a `//` prefix and subsequent remarks work without prompting.
The file extension is never rewritten; `.json` files with `//` comments are valid JSONC.

**DocFormat**:
The backend's self-reported syntax, one of `Toml` / `Json` / `Yaml`. Returned by
`ConfigDocument::format()` and used by the TUI to select format-appropriate help text, `K`
kind-switch options, `f` type-filter facets, and the comment prefix (`#` for TOML and YAML, `//`
for JSON/JSONC). Mapped from the file extension by `detect_format`; overridable via `--format`.

**Conversion** (document-level):
Producing a new file in a *different* `DocFormat` from a loaded document (key `C` in the TUI, or
`confy convert <in> <out>`). The document is lowered to a format-neutral **`Value`** tree, then
re-rendered in the target's **default style** — so it is deliberately **lossy on notation/style**
(radix, string style, inline-vs-block, dotted keys, array-of-tables are normalized, with an
**up-front warning list**), but **comments carry across** with the target marker. A conversion
**aborts** (writes nothing) when the source holds something the target cannot represent: a `null`
into TOML, or a YAML **opaque node** into any target. The **source file is never modified**.
_Avoid_: confusing this with **Kind switch** (`K`), which converts one node's *notation in place*
within the same format.

**Value** (neutral tree):
The format-independent intermediate the conversion pipeline lowers to (`model/value.rs`):
`Null/Bool/Int/Float/Str/Datetime` scalars plus ordered `Seq`/`Map` of `Item`s, where an `Item`
is either a standalone `Comment` or a `Node { key, value, trailing }`. It carries decoded data and
confy's first-class comments (standalone + trailing) in document order, but **no source notation**
— that is the point: rendering it re-imposes the target format's default style.

### Operations & projection

**Projection**:
The act of (re)building the Node tree from the backing document after every change. The backing
document — not the Node tree — is the single source of truth.

**Remark**:
The toggle that turns a live Node into a **Comment** (and back). Canonical name for what the
`r` key does.
_Avoid_: Disable/enable, comment-out (use these only as verbs in prose, never as the concept
name).

**Type filter** (`f`) vs **Text filter** (`/`):
Two independent ways to narrow the visible tree. The **Text filter** (`/`) fuzzy-matches a Node's
key/path (and a Comment's text). The **Type filter** (`f`) is a checkbox menu selecting **type
facets** — **Key sign** and **Format/kind** (the KIND-column vocabulary). Both narrow the same
filtered list and **intersect** (a Node must pass both); selections *within* the Type filter's two
halves union. _Avoid_: calling either one "search" exclusively — both are filters.

## KIND column tags (full vocabulary)

TOML: `[T/S]` scope table, `[T/D]` dotted table, `[T/I]` inline table, `[T/M]` multiline object
(JSON only), `[A/I]`/`[A/M]` inline/multiline array, `[A/T]` array-of-tables (TOML only).
Scalars: `[S:str ]`/`[S:mstr]`/`[S:lit ]`/`[S:mlit]` strings, `[I:dec]`/`[I:hex]`/`[I:oct]`/
`[I:bin]` integers, `[F:flt ]`/`[F:exp ]`/`[F:inf ]`/`[F:nan ]` floats, `[B:bool]`, `[S:null]`
(JSON/YAML null), datetime types. `[G]` root, `[C]` comment.
YAML: `[A/B]`/`[A/F]` block/flow sequence, `[T/B]`/`[T/F]` block/flow mapping (`[T/F]` also the YAML
inline table), `[S:sq  ]`/`[S:dq  ]`/`[S:lit ]`/`[S:fold]` string styles, `[opaq ]` out-of-subset
read-only (no datetime, no `[A/T]`/`[T/D]`, no `[I:bin]`).
Key sign prefix: `(B)` bare, `(Q)` quoted, `(D)` dotted, `(-)` keyless.

## Insert / move legality

What happens when a **source** Node is inserted (copy/paste) or moved (cut/paste) into a
**destination** container. The same rules apply to copy and to move. KIND tags are the
KIND-column vocabulary (`[T/S]` scope table, `[T/D]` dotted table, `[T/I]` inline table,
`[A/I]`/`[A/M]` array, `[A/T]` array-of-tables). ✅ = allowed (with the noted adaptation),
❌ = rejected with an error message.

| Source ＼ Dest | Table / Root | `[T/D]` dotted table | `[T/I]` inline table | Array (`[A/I]`/`[A/M]`) |
|---|---|---|---|---|
| **scalar** (keyed) | ✅ `k = v` | ✅ `pfx.k = v` (gets prefix) | ✅ inline member | ✅ wrapped `{ k = v }` |
| **array** (keyed) | ✅ | ✅ prefix | ✅ member | ✅ `{ k = [...] }` |
| **`[T/I]`** (keyed) | ✅ | ✅ prefix | ✅ nested member | ✅ `{ k = { … } }` |
| **`[T/S]`** scope table | ✅ nests → `[dest.k]` | ❌ scope table can't nest under a *pure* dotted table (a **mixed** dest accepts it) | ❌ table can't go into an inline table | ❌ table can't be an array element |
| **`[T/D]`** dotted table | ✅ members, prefix dropped | ✅ members, prefix adjusted | ✅ flattened to inline dotted keys | ❌ table can't be an array element |
| **array element** | single-key `{k=v}` → `k = v`; else `placeholder = …` | (same, then prefix) | (same, then member) | ✅ stays a bare element |
| **bare value** (no key) | ✅ `placeholder = …` | ✅ `placeholder` then prefix | ✅ `placeholder` member | ✅ stays a bare element |
| **comment** | ✅ | ✅ | ❌ inline tables hold no comments | ✅ (single-line array upgrades to multiline first) |
| **`[A/T]` group** (whole group) | ⏸ group move `Unsupported` | ⏸ | ⏸ | ❌ |

Notes:
- "prefix" = the destination's dotted-ancestor path is prepended so the moved Node merges into the
  destination `[T/D]` table; moving *out* of a `[T/D]` table drops that prefix (scope-relative).
- A **whole table** is moved/copied by fanning out over its **member spans** — all of them, even
  scattered (`[a] … [b] … [a.sub]` moves both sections; `[[a.list]]` sub-groups travel in entry
  order). Headers are captured scope-relative (`[a.sub]` cut as table `sub` → `[sub]`) and
  re-prefixed for the destination.
- An **entry into an implicit table** (only `[a.sub]` written) synthesizes the `[a]` section at
  the table's first definition. An **entry into a mixed table** joins the dotted-member run.
- **Collision** is decided on the inserted leaf's *exact full path*: dotted siblings sharing only a
  prefix (`a.x` beside `a.y`) merge; an identical full key clashes.
- **Position is clamped, not rejected, for a table destination**: an entry whose index points past
  the table's sub-sections lands at the end of its entry run (so the paste "Into" slot — append —
  always works, e.g. an entry into `[pt]` whose only children are `[pt.a]`/`[pt.b]`); a section
  targeted before the entries lands at the start of the section run. Only a Root-level
  out-of-partition insert still reports `Illegal`.
- A **`[T/D]` inside a `[T/I]`** (decomposed inline dotted keys) moves/copies like any `[T/D]`:
  fan-out over its `{ … }` member entries, captured scope-relative.
- ⏸ = an **AoT *group*** as a whole-group source is `Unsupported` for move (and degrades for
  copy). An AoT ***entry*** (`product[0]`) move/copy **works** — it splits into member fragments
  (`aot_entry_member_fragments`, sub-sections flattened to dotted entries), so into a table/root
  it lands as nodes (dotted re-prefix, per-leaf collision) and into another group/array it joins
  into one `[[entry]]`/`{ … }` element. A nested `[[…]]` sub-group has no dotted form: move →
  `Unsupported`, copy → full-section capture.

## `e` block-edit behavior (tables)

What the `$EDITOR` block edit captures for each table composition, and where the rewritten
block lands. Invariant: **the landing slot equals the node's projected position**, so the tree
row you edited is where the result appears.

| Composition | Captured block | Lands at | Notes |
|---|---|---|---|
| `[T/S]` contiguous | own section (+ contiguous descendants), verbatim | its own header | unchanged block ⇒ unchanged bytes |
| `[T/S]` scattered | **all** member sections, in document order | first definition | foreign sections/comments in between stay put |
| `[T/S]` implicit (no `[a]`) | all descendant sections | first definition | no header is synthesized (none is needed) |
| `[T/D]` dotted | member lines, full keys | first member line | dotted style kept |
| **Mixed** | canonical scope form: synthesized `[a]` header + dotted members folded under it + sections | first member *section* | dotted definitions are consumed — required for the header to be legal |

A consolidating rewrite (2+ spans) validates the returned block: every header must stay inside
the table's subtree and the block must start with a `[header]` line, else `Illegal` and the
document is untouched. A single-span (contiguous) edit keeps the old unchecked-splice freedom.

## Mutation mechanics

Per-variant behaviour of the closed `Mutation` set (the model layer's only document operations).
Each variant is a rowan green-tree splice with newline/indent normalization; **every mutation is
atomic** (edited on a `clone_for_update` copy, committed only on success, then
`validate_semantics`-checked). KIND tags are the KIND-column vocabulary.

| Variant | Behaviour |
|---|---|
| **Insert** | Adapts the fragment to the destination — forming/clamp rules are the *Insert / move legality* table. A keyed entry into a `[T/I]` inline table rebuilds the `{ … }` from members' verbatim source with normalized `, ` separators (front/middle/append; dup key = `Collision`; empty `{}` → `{ k = v }`). Keyed fragments into an `[A/T]` *group* synthesize one new `[[…]]` entry at the slot (`aot_group_insert`; multiple pasted nodes join via `joinable_entry` and pack into ONE entry; in-set dup keys follow on-rename/collision; a section fragment = `Illegal`). |
| **Delete** | Removes the node's full extent. An `[A/T]` entry takes its own section **plus** its sub-sections (`aot_entry_end`). A `[T/D]` table fans out to every member line. |
| **Replace** | Empty path = **whole-document reparse** (rejects invalid as `Fragment`; doc untouched) — the `$EDITOR`/root rewrite path. An `[A/T]`-entry path (`product[0]`) rewrites only that `[[…]]` entry; sibling entries and between-entry comments stay intact. |
| **Rename** | Swaps **only the key token in place** (position-preserving, collision-checked). Rewrites the **whole** key for a dotted rename (`foo` → `foo.x` converts the scalar into a `[T/D]` table). No separate user action — driven by the UI's rename flow. |
| **Move** | Atomic: delete-before-reinsert on a scratch tree, committed only on success, so a same-scope reposition is a move, not a `Key already exists` collision. An `[A/T]` *entry* moved/copied out splits into member fragments (`aot_entry_member_fragments` — body lines verbatim, one fragment each, **sub-sections flattened to dotted**: `[fruit.physical]` `color` → `physical.color`); into a table/root the members land as nodes, into another group/array they join into ONE `[[entry]]`/`{ … }` element. A whole-`[A/T]`-*group* Move degrades to `Unsupported`. A nested `[[…]]` sub-group has no dotted form: Move → `Unsupported`, Copy → full-section capture. |
| **Remark / EditComment / InsertComment** | Comments are first-class — see *Comment* / *Trailing comment*. |

**Known rough edge:** multiline-array element insert/delete spacing is not yet byte-perfect.

## Kind switch (`K`) rules

`Mutation::ConvertKind { path, target }` rewrites a node's kind/notation **in place**; targets come
from `ConfigDocument::kind_options(path)` so the UI never hard-codes a backend's notations. KIND tags
are the KIND-column vocabulary.

| Node | Convertible between | Rejects as `Illegal` |
|---|---|---|
| **string** | basic / literal / multiline / multiline-literal (content decoded then re-encoded) | a `'` in a literal form; `'''` in a multiline literal; a real newline in a single-line literal (single-line *basic* escapes newlines as `\n`, so mstr→str is lossless) |
| **integer** | dec / hex / oct / bin (`_` separators parse) | negatives have no prefixed form |
| **float** | plain ↔ exponent (exponent re-rendered from the parsed `f64`) | — |
| **bool / datetime / `inf` / `nan`** | (one notation — don't convert) | — |
| **array** | inline ↔ multiline | collapse rejects held comments or multi-line elements |
| **table** | `[T/I]` / `[T/D]` / `[T/S]` | a `[T/S]` target violating the D5 capture rule (mid-entry `[t]`, or a section preceded by a foreign header); an inline target holding comments. A nested `[s.t]` converts relative to its parent's capture. |
| **`[A/T]` group ↔ array** | group → inline/multiline array of inline tables (`convert_aot_to_array`: contiguous span, plain single-line entry bodies, no sub-sections/comments, replacement `key = […]` not captured by a foreign header); a keyed flat-ROOT array of **all** inline tables → `[[…]]` group (`convert_array_to_aot`) | array→group rejected when an entry follows before the next header (the sections would capture it) |
| **AoT entry / Root / comment** | (don't convert) | — |

Scalars switch **within their own type, never across types**.

## Nested behavior matrix

> The full, self-contained reference is **`BEHAVIOR_MATRIX.md`** at the repo root (scopes, tables
> A/B/C, criteria, the facet layer, invariants). This section is the condensed in-context form.

A normalized cross-backend (TOML/JSON/YAML) account of how the nesting **scope** governs each
editing behavior. **Governing rule:** every behavior is governed by exactly one **container**, and
the matrix column is always that governing container's scope — never the acted-on node's own kind
unless the behavior is about the node's *insides* or *self-representation*. So the same node is
looked up under different columns for different behaviors (its trailing comment under its **parent**
container; what it may *hold* under its **own** container).

**Scopes (columns).** `kind × layout` — five legal combinations:

| scope | TOML | JSON | YAML |
|---|---|---|---|
| **global** (root, block-map) | top-level table | top object/array | top block map |
| **seq-flow** | `[A/I]` inline array | inline array | flow seq `[…]` |
| **seq-block** | `[A/M]` array · `[[AoT]]` group | multiline array | block seq `- ` |
| **map-flow** | `[T/I]` inline table · `[T/D]` dotted¹ | inline object `[T/I]` | flow map `[T/F]` |
| **map-block** | `[T/S]` scope · `[[AoT]]` entry | multiline object `[T/M]` | block map `[T/B]` |

**2×2 observation.** **`layout` (flow vs block) is the primary discriminator** — it decides trailing
comments, insert forming, and external-edit precision. **`kind` (seq vs map) is secondary** — it only
decides whether children are keyed (seq elements are keyless → no rename / no `Tab`-to-Name).

¹ `[T/D]` dotted table: block *layout* but **map-flow rules** (rebuilds members, holds no comments).

### A — Branch node as a child (governed by **parent**; column = parent scope)

| behavior \ parent | global | seq-flow | seq-block | map-flow | map-block |
|---|---|---|---|---|---|
| own trailing comment | ✓ | ✗ flow | ✓ | ✗ flow | ✓ |
| own external precise edit | ✓ | ⚠ whole repr | ✓ | ⚠ whole repr | ✓ |
| add: collapsed → sibling | ✓ | ✓ rebuild | ✓ | ✓ rebuild | ✓ |
| paste-in forming | — | see *Insert / move legality* table | | | |

### B — Branch node as a container (governed by **self**; column = its own scope)

| behavior \ own scope | global | seq-flow | seq-block | map-flow / `[T/D]` | map-block |
|---|---|---|---|---|---|
| holds standalone comment node | ✓ | ✗ | ✓ | ✗ | ✓ |
| insert / append child forming | add line | rebuild `[…]` | add line | rebuild `{…}` | add line / section |
| add: expanded → append child | ✓ scalar (clamp) | ✓ bare elem (rebuild) | ✓ bare elem | ✓ member (rebuild) | ✓ scalar (clamp) |
| switch layout flow↔block (`K`) | ✗ root | ✓ →block | ✓ →flow² | ✓ →block | ✓ →flow² |

² `K` toggles a container between its flow and block layout (TOML `[A/I]`↔`[A/M]`, `[T/I]`↔`[T/D]`↔`[T/S]`;
JSON object/array Inline↔Multiline; YAML map/seq block↔flow). The **collapse to flow** direction is
rejected (`Illegal`) when the container **holds a comment** or a **multi-line element** (a flow layout
holds neither). The criterion is symmetric: every flow scope can expand to block and every block scope
that holds only inline-representable children can collapse to flow.

### C — Leaf node as a child (governed by **parent**; column = parent scope)

| behavior \ parent | global | seq-flow | seq-block | map-flow | map-block |
|---|---|---|---|---|---|
| own trailing comment | ✓ | ✗ flow | ✓ multiline elem | ✗ flow | ✓ |
| own external precise edit | ✓ | ⚠ whole repr | ✓ just the element | ⚠ whole repr | ✓ |
| inline editor | ✓ single-line | ✓ as repr | ✓ | ✓ | ✓ (multiline str → `$EDITOR`) |

**Criterion — universal scalar inline editing.** Every **single-line scalar** leaf is inline-editable
with **precise (element-level) `Replace`** in *every scope* — global, both seq layouts, both map
layouts, TOML `[T/D]`/`[T/S]`, and AoT-entry members — independent of nesting depth. The C row above
is therefore ✓ across all columns for single-line scalars. The **only** routes to `$EDITOR` are by a
scalar's **Format**, never by its scope: a multiline / literal `|` / folded `>` string opens
`$EDITOR` because it cannot round-trip through a one-line field. A single-line **plain-array element**
follows the same rule **wherever the array sits** — even nested under a key (`array_int[1].vals[0]`);
`Replace` addresses the element directly. The gate is just "immediate parent is a plain `Array`" (an
AoT group is `ArrayOfTables`, so its entries stay `$EDITOR`).
| add: collapsed leaf → sibling | ✓ | ✓ | ✓ | ✓ | ✓ |

**Invariants (not scope-dependent, so not in the matrix):** consecutive `#`/`//` comment lines merge
into one Comment node (a blank or non-comment line breaks the group); a YAML **opaque node** is
read-only — every row's behavior is rejected (`Unsupported`) whatever its underlying kind.

**External-editor precise range (uniform across backends).** `e`/`E` captures and Replaces **just the
edited node** in every backend (`App::external_edit_path` — no truncation). A standard-array
**element** (`x[0]`, `x[0][1]`) wraps its bare repr as the value-Replace form (`scalar_fragment(None, …)`
→ TOML `__elem__ = …`, JSON bare) so `Replace` splices only that element; YAML's `- value` fragment is
addressable directly (no wrap). A key/index reached *through* an array index (`x[0].a`, `x[0].a.b`) is
`Replace`-addressable directly too — the inline splice rebuilds the enclosing `{ … }`/`[ … ]` element
in place — so the whole path is kept and the edit lands precisely (this closed the last TOML/JSON gap;
earlier those truncated to the whole array).

## Flagged ambiguities

- **"Entry" is banned in confy.** It is wenv's term for a flat, line-based item and means
  something materially different there (no children, a contiguous line range). confy's
  equivalent is **Node**, which is recursive. Any use of "Entry" in confy docs/code/UI copy is a
  bug to be renamed to Node.

## Example dialogue

> **Dev:** When the cursor is on `[server]` and I press `v` to paste, where does it land?
> **Domain expert:** `[server]` is a Branch node. Paste inserts the clipboard Nodes as new
> Children of it... no — as **Siblings** after the cursor, in the cursor's Parent. So the new
> Nodes share `[server]`'s Parent and key namespace.
> **Dev:** And if one of those Nodes is a comment like `# port = 8080`?
> **Domain expert:** That's a Comment leaf. It pastes as-is. If the user later presses `r` on
> it, Remark re-parses the text — it's valid TOML, so it becomes a live `port = 8080` Scalar.
> A `# just a note` Comment, by contrast, can't be Remarked into a live Node — it isn't valid
> TOML.
