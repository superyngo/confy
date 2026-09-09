# Mutation mechanics

How each document operation behaves: what is legal, what the splice actually moves, and what a
kind switch rewrites. Vocabulary is defined in [glossary.md](glossary.md); how *nesting scope*
governs each editing behavior is in [BEHAVIOR_MATRIX.md](BEHAVIOR_MATRIX.md).

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
- **`placeholder` is the fallback, not the first choice.** A bare value pulled out of an array
  into a keyed destination synthesizes `<array's own key>_<index>` (`b_1` out of `b = [x, y]`),
  carried on `Mutation::Insert.suggested_key` and shared by all three backends
  (`model/node.rs::array_element_suggested_key`). It falls back to the generic `placeholder = …`
  only when the array has no key of its own — nested or root-level bare.
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
  copy). An AoT ***entry*** (`product[0]`) move/copy **works**: into a table/root it splits into
  member fragments (`aot_entry_member_fragments`, sub-sections flattened to dotted entries) and
  lands as nodes (dotted re-prefix, per-leaf collision). A **move** (cut→paste or drag-move) into
  another `[A/T]` *group* lands **atomically** instead — its own body lines land directly and
  every nested `[table]` sub-section is reconstructed as a nested section under the new entry
  (`aot_entry_section_body` + `prefix_section_headers`, ADR 0004 §3), not flattened to a dotted
  key. Everything else still flattens: clipboard capture always pre-flattens via
  `aot_entry_member_fragments` (so a **copy** flattens whatever the destination), and a plain
  array's elements are inline values that cannot carry `[table]` headers, so even a move into an
  array packs the flattened fragments into one `{ … }` element (`dest_is_aot` gates the atomic
  path to `ArrayOfTables` destinations only). A nested `[[…]]` sub-group has no dotted/atomic
  form either way: move → `Unsupported`, copy → full-section capture.
- Moving a **`[T/S]` scope table** into another scope re-prefixes every header with the
  destination path (`prefix_section_headers`: `[a]`/`[a.sub]` into `[b]` → `[b.a]`/`[b.a.sub]`);
  capture is scope-relative via `strip_section_header_prefix` (a nested `[a.sub]` cut into `[b]`
  becomes `[b.sub]`). A `[T/D]` table moved into an inline table flattens its members to inline
  dotted keys. **Illegal table moves**: a `[table]` section into an inline table, or nested under
  a *pure* `[T/D]` (both checked in `insert`).

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
atomic** (edited on a `clone_for_update` copy, committed only on success, then semantically
validated). Validation's serialize + re-parse doubles as the normalization back to an immutable
tree, so `apply` returns `(SyntaxNode, String)` and the caller commits both — one serialize and
one parse per mutation, not two. KIND tags are the KIND-column vocabulary.

**Never traverse a mutable tree while a whole-document index is alive.** This is a hard
performance invariant of the TOML backend, not a style preference. `clone_for_update` gives
rowan's *mutable* representation, where a parent finds a child by scanning its **live**
children (a sorted linked list of `NodeData`, `rowan::cursor`), so a `CstIndex` — which holds a
live `SyntaxElement` per node — makes every subsequent traversal quadratic. Measured at 7,001
projected nodes: `walk(tree, "")` costs **5.6 ms** with nothing else alive, and **60-97 ms**
(11-17×) with a previous walk's index still in scope. Repeatable, and not a warm-up effect —
four consecutive walks all cost ~97 ms while one index is held, and 5.6 ms each when it is not.
So `move_nodes` and `delete` `drop` their `(proj, idx)` the moment the owned data they need
(fragments, spans, anchor path) has been extracted, before any splice. `JsonDocument` is immune
by construction — its `project()` keeps only the owned `NodeTree` and `resolve()` discards its
index on return — which is why the same "quadratic move" shape never appeared there.

**The invariant has two more edges, both closed 2026-09-09 (F10).** Dropping the index before
the phases was only half of it; the other half is that a *single* traversal or splice under a
live index is already quadratic on its own:

| under a live 7,001-node index | cost | without it |
|---|---|---|
| `tree.children().count()` over 5,000 ROOT children | 32 ms | **0.09 ms** |
| `tree.splice_children(at..at, els)` at ROOT | ~95 ms | ~7 ms |

So three rules, not one. **(1)** Drop the index before the phases (already above). **(2)** Never
*scan* the tree for something the index already knows: `table_member_spans` gets its section
headers from the index (`Target::Header`/`AotEntry` whose path is prefixed by the table's)
instead of filtering `tree.children()`, and `section_span_text` reads its span off `tree.green()`
— immutable data in the same child order, materializing nothing. **(3)** Drop the index before
the splice: `insert_with` takes its `CstIndex` **by value** and drops it after the position is
resolved, because a splice renumbers every live handle under the spliced parent.

Together these took `Move ×8` at 7,001 nodes from **2.27 s to 158 ms (−93%)**, within 2× of the
YAML backend's 82 ms; `Move ×1` went 296 ms → 33 ms. The remaining gap is the per-source
`walk` each re-insert still does (5.5 ms) plus the shared `apply` envelope.

| Variant | Behaviour |
|---|---|
| **Insert** | Adapts the fragment to the destination — forming/clamp rules are the *Insert / move legality* table. A keyed entry into a `[T/I]` inline table rebuilds the `{ … }` from members' verbatim source with normalized `, ` separators (front/middle/append; dup key = `Collision`; empty `{}` → `{ k = v }`). Keyed fragments into an `[A/T]` *group* synthesize one new `[[…]]` entry at the slot (`aot_group_insert`; multiple pasted nodes join via `joinable_entry` and pack into ONE entry; in-set dup keys follow on-rename/collision; a section fragment = `Illegal`). |
| **Delete** | Removes the node's full extent. An `[A/T]` entry takes its own section **plus** its sub-sections (`aot_entry_end_from`). A `[T/D]` table fans out to every member line. |
| **Replace** | Empty path = **whole-document reparse** (rejects invalid as `Fragment`; doc untouched) — the `$EDITOR`/root rewrite path. An `[A/T]`-entry path (`product[0]`) rewrites only that `[[…]]` entry; sibling entries and between-entry comments stay intact. The fragment must resolve to **exactly one** value node in every backend; surplus text is rejected as `Fragment`, never silently dropped (F14 — it mattered for YAML, whose lenient grammar happily parses a two-node fragment). |
| **Rename** | Swaps **only the key token in place** (position-preserving, collision-checked). Rewrites the **whole** key for a dotted rename (`foo` → `foo.x` converts the scalar into a `[T/D]` table). No separate user action — driven by the UI's rename flow. |
| **Move** | Atomic: delete-before-reinsert on a scratch tree, committed only on success, so a same-scope reposition is a move, not a `Key already exists` collision. An `[A/T]` *entry* moved/copied out of its array splits into member fragments; into a table/root the body lines land as nodes (**sub-sections flattened to dotted**: `[fruit.physical]` `color` → `physical.color`, `aot_entry_member_fragments`); a **move** into another `[A/T]` *group* lands **atomically** instead — nested `[table]` sub-sections are reconstructed as nested sections under the new entry, not flattened (`aot_entry_section_body`, ADR 0004 §3). A **copy** — or a plain-array destination, move or copy — still flattens to dotted fragments: capture always pre-flattens (`aot_entry_member_fragments`), and array elements are inline values that cannot carry `[table]` headers (`dest_is_aot` gates the atomic path to `ArrayOfTables` destinations only). A whole-`[A/T]`-*group* Move degrades to `Unsupported`. A nested `[[…]]` sub-group has no dotted/atomic form either way: Move → `Unsupported`, Copy → full-section capture. |
| **Remark / EditComment / InsertComment** | Comments are first-class — see *Comment* / *Trailing comment*. Remark applies to any node that **occupies its own line(s)** — a keyed member, an **array element**, a whole table/section — in all three formats, and un-remarking restores the source byte-for-byte. It does not apply inside a **single-line** collection (`a = [1, 2]`, `{"x": 1, "y": 2}`, `a: {x: 1}`), where a comment leader would swallow the siblings, nor to a read-only node; every backend reports that case as `Unsupported`. Pinned by `tests/format_parity.rs`; the rule itself is [BEHAVIOR_MATRIX.md](BEHAVIOR_MATRIX.md) §8. |
| **SetTrailingComment** | Sets, changes, or clears the *value-attached* EOL comment of the node at `path` (`host: x  # bind`); `Some(text)` writes it (the text carries its own `#`/`//` prefix), `None` clears it. Deliberately **independent of the value `Replace`**: a plain value edit, a `nudge` and a paste all *preserve* the comment and never issue this variant, and the inline editor issues it only when the comment half of the buffer actually changed. Which parent scopes can carry one at all is *not* decided here — see [BEHAVIOR_MATRIX.md](BEHAVIOR_MATRIX.md) tables A and C. |
| **ConvertKind** | Rewrites a node's kind/notation in place, same-kind only; the legal targets come from `ConfigDocument::kind_options(path)` and the rules are *Kind switch (`K`) rules* below. TOML's four datetime **types** are reachable from the same key but not through this variant — they commit as a value `Replace` (ADR 0012). |
| **SetTrailingBlankLines** | The one variant that is **not** a green-tree splice: a format-neutral text splice (`model/blank_lines.rs`) rewriting the blank run at an offset the backend supplies (`ConfigDocument::trailing_blank_anchor`). That anchor is the node's **contiguous extent end** — the same extent `Delete` covers — so a `[table]`'s run sits after its last member and a sub-table is *inside* it, not after it, and a branch's last child shares the branch's run (both anchors coincide). Two shared normalizations make the anchor trustworthy in every backend: **`anchor_at` retracts back over the run**, because several spans already swallow it (TOML's index-derived section extent, a YAML `MAP_ENTRY` whose value is a block map/seq/scalar, a JSON `//` comment token) and an anchor placed *after* a run makes it invisible — `count_after` reports 0 while the node's own `Replace` still overwrites the lines, silently deleting them; and **`owns_line_tail` rejects a node that doesn't own the end of its line** (only its separator comma, whitespace or a trailing comment may follow), so a member of a single-line `{ … }`/`[ … ]` reports `None` instead of claiming its container's run. A comment **block** anchors past its last line, not its first. Whitespace-only lines are normalized to empty; `n = 0` removes the run entirely; at EOF a terminating newline is emitted before the run. JSON keeps the separator comma *before* the run. `None` (⇒ `Unsupported`) for a YAML opaque node, any inline-collection member, and the whole-document path. |

**Multiline-array layout.** Four separate rules keep an array's authored spelling across an
element edit, none of which is a constant:

- **Insert separator** — `,` + the trivia measured in front of the array's existing elements
  (`array_element_lead`), so a multiline array keeps one element per line at the author's own
  indent (2-space, 4-space, tab) and a single-line one keeps `, `.
- **Close padding** — taplo bakes the padding between the last element and the `]` *into* that
  element's `VALUE` node (`[ 1 ]` ⇒ `VALUE "1 "`), so an append lands *behind* it. The padding is
  detached and re-emitted after the new element (`detach_value_trailing_pad`), keeping
  `[ 1, 9 ]` rather than `[ 1 , 9]`.
- **Line ownership on delete** — a span that owns a whole line takes that line's indent with it.
  `extend_over_newline` covers the tail; `retract_over_line_indent` covers the head, guarded on
  the preceding element being a `NEWLINE`/`[`/start-of-file so a *trailing* comment's separating
  space (preceded by a `,`) is never swallowed. Deleting the **last** element of a trailing-comma
  array retracts the same way (that cut takes the comma *after* the element, so its indent would
  otherwise strand in front of the `]`), but only when nothing else still owns the indent — a
  surviving element, or the deleted element's own EOL comment, keeps its column.
- **First element in a comment-only array** — the no-elements branch splices bare before the `]`,
  which is right for `[]`/`[ ]`/`[`⏎`]` but not for an array already holding comments: that one
  ends in a NEWLINE, so a bare splice lands in column 0. When the element before the `]` is a
  NEWLINE, the value is preceded by `array_comment_indent` — the indent of the array's last
  comment line, so the first element joins that column (empty for a flush `#`).

**Flow fragments.** A YAML flow-seq *element* has no `Target` of its own — the projection
indexes it as `Target::Element(<the whole FLOW_SEQ>)`, since every edit needs the collection plus
an ordinal — so `fragment_of` takes the path's last `Seg::Index` and slices that one item out
(`flow::flow_item_text`, trailing whitespace excluded). Without it a copy, a `Move` capture, or a
multiline edit of one element took the entire `[ … ]` and nested the collection into its own
element. Flow *map members* are `Target::MapEntry(FLOW_ENTRY)` and need no such lookup. The
registration covers a **nested collection** element (`g: [ {x: 1}, 2 ]`) as well; while it was
missing, that element resolved to nothing and every mutation on it returned `NotFound`. Since the
resolved node is then the *collection*, `ConvertKind` rejects an `Element(FLOW_SEQ)` outright
(`resolve_value_node`) — converting would retarget the parent sequence, and an in-flow item has no
block form anyway.

**Nesting cap.** Every backend parses by recursive descent (TOML through taplo), so container
nesting is capped at `model::MAX_NESTING_DEPTH` (256): deeper input is a `ParseError`
("nesting deeper than 256 levels is not supported") at load, and a `MutateError::Fragment` for
a whole-document `Replace`/`Insert` fragment — never a stack overflow. JSON and YAML count depth
inside their own parsers (block *and* flow for YAML); TOML runs `bracket_depth_exceeds`, a
string/comment-aware bracket pre-scan, before handing the text to taplo. Pinned by
`crates/confy-core/tests/hostile_input.rs`.

## Kind switch (`K`) rules

`Mutation::ConvertKind { path, target }` rewrites a node's kind/notation **in place**; targets come
from `ConfigDocument::kind_options(path)` so the UI never hard-codes a backend's notations. KIND tags
are the KIND-column vocabulary.

**TOML.** The full set:

| Node | Convertible between | Rejects as `Illegal` |
|---|---|---|
| **string** | basic / literal / multiline / multiline-literal (content decoded then re-encoded) | a `'` in a literal form; `'''` in a multiline literal; a real newline in a single-line literal (single-line *basic* escapes newlines as `\n`, so mstr→str is lossless) |
| **integer** | dec / hex / oct / bin (`_` separators parse) | negatives have no prefixed form |
| **float** | plain ↔ exponent (exponent re-rendered from the parsed `f64`) | — |
| **bool / `inf` / `nan`** | (one notation — don't convert) | — |
| **datetime** | not a `ConvertKind` at all: `K` on a TOML datetime opens the **value picker** and commits a value `Replace` — see below | — |
| **array** | inline ↔ multiline | collapse rejects held comments or multi-line elements |
| **table** | `[T/I]` / `[T/D]` / `[T/S]` | a `[T/S]` target violating the D5 capture rule (mid-entry `[t]`, or a section preceded by a foreign header); an inline target holding comments. A nested `[s.t]` converts relative to its parent's capture. |
| **`[A/T]` group ↔ array** | group → inline/multiline array of inline tables (`convert_aot_to_array`: contiguous span, plain single-line entry bodies, no sub-sections/comments, replacement `key = […]` not captured by a foreign header); a keyed flat-ROOT array of **all** inline tables → `[[…]]` group (`convert_array_to_aot`) | array→group rejected when an entry follows before the next header (the sections would capture it) |
| **AoT entry / Root / comment** | (don't convert) | — |

**JSON / JSONC.** A much smaller set, because JSON has one string notation and no radixes:
object and array switch **inline ↔ multiline** (`KindTarget::{TableInline, TableMultiline}` /
`{ArrayInline, ArrayMultiline}`) and a float switches **plain ↔ exponent**
(`KindTarget::{FloatPlain, FloatExponent}`). Nothing else is offered; a `//` block comment is a
read-only node and rejects every conversion.

**YAML.** Map and sequence switch **block ↔ flow** (`KindTarget::{Block, Flow}` — the KIND tags
`[T/B]`/`[T/F]` and `[A/B]`/`[A/F]`), a string switches between its **five** styles
(`KindTarget::{StringPlain, StringSingle, StringDouble, StringLiteralBlock, StringFolded}`), an
integer between **dec / hex / oct** (no binary), and a float **plain ↔ exponent**. Two YAML-only
rejections: a **block-producing** target on a member of an inline flow collection is refused
outright (and hidden from the `K` popup), and every conversion on or inside an **opaque**
out-of-subset span returns `Unsupported`.

Scalars switch **within their own type, never across types** — with one deliberate exception,
TOML's four datetime *types* (`[D:odt]`/`[D:ldt]`/`[D:ldat]`/`[D:ltim]`). They are mutually
reachable from `K`, but not through `ConvertKind`: `Session::open_kind_switch` diverts a
datetime node to `Mode::SchemaEnum` (the value picker), whose options read `name  [D:tag]`
like a notation row (`local date      [D:ldat]`) and whose commit path is an ordinary value
`Replace` gated by `PromptKind::TypeChange` — that confirm is where the rewritten literal is
previewed (`1979-05-27T07:32:00Z → 1979-05-27`) and every dropped/auto-filled component
named. `kind_options` therefore
still returns an empty list for a datetime, and `ConvertKind`'s same-kind invariant is intact.
Component surgery lives in `session/datetime.rs`; fill policy and the rejected
`KindTarget::Datetime*` design are in **ADR 0012**.

## Nested behavior matrix

The scope model (`kind × layout`, five columns), tables A/B/C, the design criteria, the facet
layer, and the scope-independent invariants are in [BEHAVIOR_MATRIX.md](BEHAVIOR_MATRIX.md) —
the single, self-contained account. There is deliberately no condensed second copy here: two
copies of one matrix drift.
