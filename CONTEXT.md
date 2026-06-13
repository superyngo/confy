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
A **Comment** node whose `Node.read_only` flag is set. Currently produced by JSONC `/* */` block
comments, which are displayed in the tree and copyable but reject edit (`e`/`E`), delete (`d`),
cut (`x`), and remark (`r`). Also the planned mechanism for opaque YAML nodes in Phase 3 (nodes
that survive round-trip but cannot be mutated safely without full YAML write support).

**JSONC upgrade**:
The prompt shown when a user triggers `r` (remark) on a node in a pure `.json` file (one loaded
without `supports_comments()` true). Confirming (`y`) flips the document's comment support on,
so the remarked node is written with a `//` prefix and subsequent remarks work without prompting.
The file extension is never rewritten; `.json` files with `//` comments are valid JSONC.

**DocFormat**:
The backend's self-reported syntax, one of `Toml` / `Json` / `Yaml`. Returned by
`ConfigDocument::format()` and used by the TUI to select format-appropriate help text, `K`
kind-switch options, `f` type-filter facets, and the comment prefix (`#` for TOML, `//` for
JSON/JSONC). Mapped from the file extension by `detect_format`; overridable via `--format`.

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
(JSON null), datetime types. `[G]` root, `[C]` comment.
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
| **`[A/T]`** array-of-tables | ⏸ not supported yet | ⏸ | ⏸ | ❌ |

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
- ⏸ = array-of-tables sources are deferred to a later round (they currently report an error rather
  than moving).

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
