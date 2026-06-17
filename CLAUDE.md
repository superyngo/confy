# CLAUDE.md — confy developer guide

## Build & test commands

```bash
cargo build                   # compile
cargo test                    # unit + integration tests
cargo clippy -- -D warnings   # lint (must be clean before commit)
cargo fmt                     # format
cargo fmt --check             # check formatting without modifying
cargo run -- <file.toml>      # run against a TOML file
```

## Architecture

**Lossless CST.** `CstDocument` (`model/cst_doc.rs`) holds a `taplo` parse → `rowan` syntax tree
as the single source of truth. Comments, whitespace and newlines are real tokens with real
positions, so `serialize()` is plain token concatenation and an untouched file round-trips
byte-identically. The Node tree is a *projection* (`cst_project.rs`) rebuilt after every
mutation — it is never mutated directly. `apply` edits a `clone_for_update` copy of the tree and
commits only on success, so **every mutation is atomic** (failure leaves the document untouched).
Every successful mutation is also **semantically validated before commit** (`validate_semantics`:
taplo DOM validation — duplicate sections/keys reject as `Collision`, other semantic errors as
`Illegal`), a backstop for edits the targeted pre-checks can't see (e.g. a whole-document or block
`$EDITOR` rewrite introducing a duplicate `[a]`).

**JSON/JSONC backend.** `JsonDocument` (`model/json/`) is a second concrete `ConfigDocument`
built on a hand-rolled lossless lexer + recursive-descent parser that emits a `rowan` green tree
(the same `rowan` version taplo uses, pinned `=0.15.18`). Load, serialize, and apply are all
atomic-commit; a `validate_semantics` post-check (DOM re-parse for duplicate keys) mirrors the
TOML backstop. JSONC extends `.json` with `//` line comments — which project as first-class
Comment nodes (consecutive lines merge; a blank splits them) or `trailing_comment` — and `/* */`
block comments, which project as **read-only** Comment nodes (new `Node.read_only` flag:
displayed and copyable, but edit/delete/cut/remark reject them). A pure `.json` file whose first
remark is triggered prompts `Mode::Prompt(JsoncUpgrade)`; `y` flips `supports_comments()` true
and `//` is used thereafter (the file extension is never rewritten). Trailing commas are accepted
on parse but never emitted by splices. `K` switch covers object/array Inline↔Multiline and float
Plain↔Exponent; the `f` type-filter shows only JSON-reachable facets (`(Q)`/`(-)` key signs,
no `[A/T]`/`[T/D]`/`[T/S]`, no radix/string-style/datetime rows). JSON omits TOML-only
features: no dotted keys, array-of-tables, datetimes, integer radixes, multiline strings, or
string-notation switching; newlines are `\n`-encoded only. New model atoms added for this
backend: `ScalarType::Null` (KIND tag `[S:null]`), `Format::Exponent` (KIND tag `[F:exp ]`),
`KindTarget::TableMultiline` (KIND tag `[T/M]`), `Node.read_only`.

**YAML subset backend.** `YamlDocument` (`model/yaml/`) is a third concrete `ConfigDocument`, also
a hand-rolled lossless lexer + recursive-descent parser onto the same `rowan` green tree; load,
serialize, and apply are atomic-commit with a `validate_semantics` duplicate-key backstop. The
splice core is a **reindent engine** (`reindent` in `edit.rs`) — YAML's analogue of JSON's
comma/brace normalization — that re-flows a fragment from its source indent to the destination's.
**Subset:** a single document (an optional leading `---` is kept verbatim), block + single-line flow
maps/sequences (**nesting is preserved** — the parser builds nested `FLOW_MAP`/`FLOW_SEQ` child nodes
and a `FLOW_ENTRY` node per flow-map member, so a nested `{…}`/`[…]` value is a real recursing child
and each member is individually addressable/editable; replace/insert/delete/rename on a flow member
rebuild the `{…}` inline, while block-producing converts on an inline member are rejected and the `K`
popup hides them), 5 scalar styles (plain, single-quoted, double-quoted, literal `|`, folded `>` with
chomping), `#` comments, and YAML 1.2 **core-schema typing** with **no datetime** (date-looking
scalars are strings). **Out-of-subset constructs** — `&anchor`, `*alias`, `<<:` merge, `!tag`,
multi-line flow — project as **read-only opaque nodes** (`Node.read_only`, KIND tag `[opaq ]`): they
render and copy, but every mutation on or into them (and on any entry whose *value* is opaque —
`entry_has_opaque_value`) returns `Unsupported`, leaving the document untouched. **Multi-document**
files are rejected at load (a whole-document `E` re-parse rejects them too). The resolver maps a path
to a `Target` (`MapEntry`/`Element`/`Comment`/`Opaque`); `is_opaque` walks ancestors so a path inside
an opaque span is blocked. New model atoms: `Format::{Block, SingleQuoted, DoubleQuoted, LiteralBlock,
Folded}` and `KindTarget::{Flow, Block, StringPlain, StringSingle, StringDouble, StringLiteralBlock,
StringFolded}` — driving KIND tags `[A/B]`/`[A/F]` (block/flow seq), `[T/B]`/`[T/F]` (block/flow map;
`[T/F]` is shared by flow map and inline table), `[S:sq  ]`/`[S:dq  ]`/`[S:lit ]`/`[S:fold]`. `K`
covers map/seq block↔flow, the 5 string styles, integer radix (dec/hex/oct), float plain↔exponent.
`scalar_fragment` wraps `key: value` (or a bare `- ` element); `value_kind` projects the value in YAML
syntax for the type-change check.

**`ConfigDocument` trait** abstracts the storage backend so YAML/JSON can be added later; the
concrete backends are `CstDocument` (TOML), `JsonDocument` (JSON/JSONC), and `YamlDocument`
(YAML subset) (the original `toml_edit`-based `TomlDocument` was retired after reaching parity). The trait exposes `load`, `project`, `serialize`, `serialize_fragment`,
`serialize_fragment_relative`, `is_dirty`, `apply(Mutation)`, and three **format facets** —
`format() -> DocFormat`, `comment_prefix()`, `supports_comments()` — plus `kind_options(path)`,
which serves the `K` popup's per-node convertible-kind list (`(label, KindTarget)` pairs) so the
TUI never hard-codes a backend's notations, and two **fragment facets** the inline editor/`nudge`/`a`
use so they don't hard-code a notation either: `scalar_fragment(key, value)` (wraps a value repr as
`key = value` / `"key": value`, or — `key: None` — the backend's *value-Replace* element form, which
TOML wraps as `__elem__ = value`), `array_element_fragment(value)` (the **bare keyless element** form
`a` seeds into an array/seq — TOML/JSON re-wrap a bare value spliced keyless, YAML's `- value` — so all
three seed array elements uniformly), and `value_kind(value)` (projects
the value in the backend's own syntax for the type-change check). **`AnyDocument`** (`model/any_doc.rs`) is a one-enum
dispatcher wrapping every backend (`Toml(CstDocument)`, `Json(JsonDocument)`, `Yaml(YamlDocument)`)
and implementing `ConfigDocument` by match-delegation; the TUI holds a single `AnyDocument`, and a
new format is one more variant. `detect_format(path)` maps the extension to a `DocFormat`
(`.toml`/`.json`/`.jsonc`/`.yaml`/`.yml`); `load_as(path, format)` dispatches to TOML, JSON/JSONC,
or YAML. `Mutation::Insert`/`Replace` carry a format-neutral `fragment:` field (not `toml:`).
Path→node lookup lives on `NodeTree::node_at(path)` (model layer, reused by `kind_options`).

**Document-level conversion** (`model/convert.rs`, spec §Phase 4). `convert(doc, target) ->
Result<ConvertResult, ConvertAbort>` lowers a loaded document to a **format-neutral `Value`
tree** (`model/value.rs`: `Value::{Null,Bool,Int,Float,Str,Datetime,Seq,Map}`, ordered
`Vec<Item>` where `Item::{Comment, Node{key,value,trailing}}` keeps confy's first-class comments
in document order), then renders it back in the *target's* default style. The lowering is one
generic walk — `tree_to_value(&NodeTree, src)` maps containers by `NodeKind` (Table/InlineTable→
`Map`, Array/ArrayOfTables→`Seq`, the Root sniffs keyed-vs-keyless children, a comment→
`Item::Comment` with markers stripped, `trailing_comment`→`Item.trailing`), and per-format
`decode_*` helpers decode each scalar's raw token text (`node.value`) to typed data (TOML/JSON/
YAML radix, escapes, block scalars, inf/nan). Each backend implements `ConfigDocument::to_value`
as `tree_to_value(&self.project(), <fmt>)`. **Loss policy** (the documented lossy contract):
notation/style that the default render drops is collected as deduplicated **warnings** during the
walk (`style_note`: radix, string style, inline/flow, dotted, AoT, exponent); `analyze` adds the
target-specific rules — `null`→TOML and a YAML opaque node→any target **abort** (no output;
null paths listed), TOML datetime→JSON/YAML and non-finite floats→JSON **warn**. The three
renderers emit default style only (`render_toml` scope tables + bare keys + `#`, two-phase so
keys precede `[sub]`/`[[aot]]` headers; `render_json` 2-space multiline, `//` comments only when
present ⇒ JSONC; `render_yaml` block + plain-where-safe scalars + `#`). A **reparse safety net**
loads the rendered text with the target backend before returning, so invalid output never reaches
disk. The **source document is never modified**. Two surfaces: the `confy convert <in> <out>
[--from --to --yes]` CLI (`cli.rs`) and a TUI Root-node action on `C` (`Mode::Convert`: pick
format → output path → warning/confirm; the open doc is untouched).

**Addressing.** Keyed nodes are addressed by `Seg::Key(name)`; **positional** nodes — comments,
array elements, AoT entries — by `Seg::Index(i)` over the parent's *full child sequence*
(comments share the slot space, so an element after a comment keeps its full-sequence index).
There are no synthetic keys; the TUI identifies a comment by `NodeKind::Comment`, never by
sniffing the path. `cst_edit::walk` builds the same `path → syntax element` index the projection
uses, so resolver and projection cannot drift (a consistency test ties them).

**`Mutation` enum** is the closed set of document operations (Insert, Delete, Replace, Rename,
Move, Remark, EditComment, InsertComment). Each variant is implemented in `cst_edit.rs` as a
rowan green-tree splice (insert/remove/replace of syntax elements with newline/indent
normalization). `Rename` swaps only the key token in place (position-preserving,
collision-checked) — there is no separate user-facing rename action; it is driven from the
inline editor (see below). `Replace` with an **empty path** targets the whole document (external
`E` on the root/file node): it reparses the edited text as a full document, rejecting invalid
TOML as `Fragment` (doc untouched). `Replace` on an AoT-entry path (`product[0]`) rewrites only
that `[[product]]` entry; sibling entries and between-entry comments stay intact. `Insert`
adapts the fragment to the destination (`parse_fragment_adapted`); the forming/clamp rules
(keyless-vs-keyed, `placeholder` synthesis, `[table]`/`[[aot]]`→array rejection, header/leaf
partition clamp) mirror CONTEXT.md's *Insert / move legality* table. Inserting a keyed entry
**into an inline table** routes to `inline_table_insert`,
which rebuilds the `{ … }` from its members' verbatim source with normalized `, ` separators
(taplo bakes the closing brace's leading space into the last entry, so token surgery is brittle) —
the new entry lands at the target slot (front/middle/append), a duplicate key is a `Collision`, and
an empty `{}` becomes `{ k = v }`. **`[A/T]` interactions**: inserting keyed
fragments into an AoT *group* synthesizes a new `[[…]]` entry at the target slot
(`aot_group_insert`; multiple pasted nodes are joined — `joinable_entry` — and pack into ONE
entry; in-set duplicate keys follow o/r/c; a section fragment is `Illegal`). An `[A/T]` group is
**equivalent to an array of inline tables**: moving/copying an AoT *entry* out **splits it into
member fragments** (`aot_entry_member_fragments` — body entry lines verbatim, one fragment each,
**sub-sections flattened to dotted entries**: `[fruit.physical]` `color` → `physical.color`), so
into a table/root the members land as nodes (dotted re-prefix, per-leaf collision) and into
another group / an array they join into ONE `[[entry]]` / `{ … }` element. Deleting an entry
removes its **full extent** (`aot_entry_end`: own section + its sub-sections). A nested `[[…]]`
sub-group has no dotted form — move degrades to `Unsupported`, copy falls back to the full
section capture. Known edges: whole-AoT-*group* Move degrades to a graceful
`Unsupported`, and multiline-array element insert/delete spacing is not yet byte-perfect.

**Projection.** Dotted *keys* (`a.b.c = 1`) **nest** into a chain of synthetic `Table` nodes
(`a → b → c`) with `Format::Dotted` (rendered `[T/D]`) — `project_entry_into`/`ensure_dotted_chain`
in `cst_project.rs`; scattered dotted entries sharing a prefix merge under one table **per scope**,
positioned at the table's **first** definition (matching where a consolidating block-rewrite
lands). The leaf keeps the **full** path for its
`Target::Entry`, so an **untouched file round-trips byte-identically**; the synthetic intermediates
carry no index target (like an implicit header table — the `index_covers_every_projected_path` test
exempts `Table` nodes), and — like every other branch — **start collapsed** (only the root file node
is seeded into `expanded` at load). The whole
decomposed chain (synthetic tables **and** leaf) carries `KeySign::Dotted` (`(D)`) — `(D)` marks
any dotted-key origin, so the `f` filter's `(D)` checkbox matches decomposed dotted entries;
per-segment `Bare`/`Quoted` is no longer surfaced for a decomposed chain. A dotted key **inside an
inline table** (`t = { x.y = 1, x.z = 2 }`) decomposes the same way — members sharing a prefix
merge under one synthetic `[T/D]` chain inside the `[T/I]` node. Ops on such a synthetic table
route through the **inline machinery**, never the flat-ROOT splices (`inline_ancestor_len` guards
the path): insert/add re-prefixes the key scope-relative (`q = 9` into `t.x` → member `x.q = 9`)
and lands via `inline_table_insert` with the projected index translated to a raw member slot
(`inline_raw_member_index`); collision is exact full path (a shared prefix merges); `Delete` and
move/copy fan out over the member entries (`inline_member_entries`; capture drops the segments
between the `{ … }` and the node, keeping its own key); the `e` block edit consolidates at the
first member (`replace_inline_dotted_table`, single-line entries only); comments are rejected
(`{ … }` holds none). **Comments are never inside a `[T/D]` table**: a comment adjacent to a
dotted member is an independent scope-level node (it stays put on table move/copy/delete and
the `e` consolidation), and `InsertComment` targeting a `[T/D]` re-routes to the scope level —
the comment lands directly **above the table's first member** as an independent node, never
rejected, never bound. **Editing a `[T/D]` table**
(`cst_edit.rs`, all keyed off `Format::Dotted` since the table has no own element): a child
insert/add writes a scope-relative dotted entry next to its siblings (`x = v` → `a.b.x = v`,
`prefix_entry_key`); a child `add` seeds a scalar (a dotted table is excluded from the
table-capture **partition split** in `add_node`/`check_partition`, so a following scalar is legal);
`Replace` (the `e` block edit) **consolidates** — `replace_dotted_table` removes every member
(`dotted_member_entries`) and splices the edited block in at the first member's slot; `Delete` fans
out to remove every member (plain cascade). `dotted_member_entries` counts only **flat-ROOT**
entries — an entry nested inside an inline-table/array *value* (`dotted.t = {x=1}`) belongs to that
value, not the table, so its interior is never pulled out as a stray top-level line. `Rename` rewrites the **whole** key (not just the last
segment), so `foo` → `foo.x` turns a scalar into a `[T/D]` table — the inline editor confirms the
type change and defers the whole edit (`PendingCommit::Rename`) so `n` is a no-op. A whole-subtree
*move/copy* of a synthetic `[T/D]` table **fans out to its member entries** (`move_nodes` and the
header-less multi-entry `insert` split), each captured scope-relative and re-prefixed for the
destination — so cut/copy of a `[T/D]` table into a scope / another `[T/D]` / root adjusts the prefix.
Insert **collision is exact full-path** (`target.parent ++ key segments`): a dotted sibling sharing only
a prefix merges into the same table instead of colliding. **Every table is an open set of "member
spans"** (`table_member_spans` in `cst_edit.rs`): its own `[a]` section, every descendant
`[a.sub]`/`[[a.list]]` section wherever it sits, plus flat dotted member lines — serialize/`e`,
delete and move/copy fan out over all of them, so a scattered `[a] … [b] … [a.sub]` is captured,
deleted and moved whole (no orphan `[a.sub]`), the block edit consolidating at the **first
definition** (validated when 2+ spans: headers must stay in-subtree and the block header-led — see
CONTEXT.md's `e` matrix). An **implicit** table (only `[a.sub]` written) gets its `[a]` section
synthesized at first definition when an entry child is inserted; a **mixed** table (dotted members
+ sections, the `fruit.apple` pattern) takes entry children as dotted members (a header would be
spec-illegal while dotted definitions remain), accepts sub-table sections, and `e`-consolidates to
scope form. The headerless-ancestor rule (`is_headerless_table`) replaces the old `Format::Dotted`
checks for prefix strip/add. Moving a **`[T/S]` scope table into another
scope nests it** — every header in the moved section is re-prefixed with the destination path
(`prefix_section_headers`: `[a]`/`[a.sub]` into `[b]` → `[b.a]`/`[b.a.sub]`; capture is
scope-relative via `strip_section_header_prefix`, so a nested `[a.sub]` cut into `[b]` becomes
`[b.sub]`); a `[T/D]` table into an
inline table flattens its members to inline dotted keys. **Illegal table moves report `Illegal`**: a
`[table]` section into an inline table or nested under a *pure* `[T/D]` dotted table (both checked
in `insert`).
Dotted *headers* (`[x.a]` with no `[x]`) still
project as a real nested `Scope` branch. `ScalarType` and a node's
**Format** (writing style) are derived read-only during projection and are orthogonal to each other.
Format covers scalars (hex/oct/bin, basic/literal/multiline string — from the token's syntax kind via
`scalar_kind` — plus `Inf`/`Nan` floats, told apart by token text) *and containers*: an array
is `Inline` or `Multiline`, an inline table `Inline`, a `[table]` scope `Scope`, a dotted-key table
`Dotted`; Root, AoT groups/entries, comments, bools, datetimes and plain floats stay `Plain`. Each node also
carries a **`KeySign`** facet (`Bare | Quoted | Dotted | None`) describing how its own key is
written — `None` for keyless nodes (array elements, comments, AoT entries, Root); taplo lexes
quoted keys as `IDENT` tokens that keep their quotes, so the sign is derived from the token
text. Single-line arrays and inline tables still carry their one-line source repr in `value`
(a multiline array leaves it `None`) — this drives both the VALUE column and the
inline-editability rule below. Golden tests in `cst_project.rs` freeze the projected shape
(snapshotted at toml_edit parity when the legacy backend was retired; regenerated when `sign=`
and container formats landed). The **KIND column** (formerly TYPE/FORMAT; takes 40% of the
terminal width for NAME, kind at the 2/5 mark, value the remainder) renders the type/notation facet as a
**fixed-pitch 8-column tag** (`type_tag` in `app.rs`: the type slot
`[T/S]`, `[A/I]`, `[S:str ]`, …); JSON has no scope table — an inline object is `[T/I]`, a
multiline one `[T/M]` — and adds `[S:null]` (null scalar) and `[F:exp ]` (exponent float); YAML adds `[A/B]`/`[A/F]` (block/flow seq), `[T/B]`/`[T/F]`
(block/flow map), `[S:sq  ]`/`[S:dq  ]`/`[S:lit ]`/`[S:fold]` (string styles), and `[opaq ]`
(out-of-subset read-only). The **key-sign facet** (`(B)/(Q)/(D)/(-)`) is no longer in the column —
it reads as a word on the detail popup's `Sign:` line. `type_tag` (and the type-filter's `classify`) take `(doc: DocFormat,
read_only)` so the rendered slot is backend-aware — the YAML opaque gate (`read_only && doc==Yaml`)
tags `[opaq ]` whatever the underlying kind. The detail popup keeps word labels (its `Path:` line
includes positional indices, e.g. `a.b[2].c`), and `node_type_label`
still drives the inline editor's type-change comparison.

**Editing.** `e` dispatches via `edit_target_kind`. The **inline-vs-`$EDITOR` boundary** is
governed by BEHAVIOR_MATRIX §6 (universal single-line-scalar inline editing across all scopes;
single-line arrays/inline tables/JSON objects edited as their one-line repr, EOL comment
preserved via `entry_trailing_comment`; the YAML array-ancestor lift where `plugins[1].name` /
`plugins[3]` edit inline and `edit_node` skips array truncation; literal `|`/folded `>` and
everything multiline → `$EDITOR`). The inline editor edits one field at a time: **`Tab` toggles
between Value (default) and Name**; committing a changed Name applies `Mutation::Rename` first,
then the value `Replace` (Tab is disabled for array elements and comments, which have no key).
Commit detects a **type change** via the backend's `value_kind(value)` (which parses+projects the
value in the doc's own syntax) fed to `node_type_label`, prompting y/n when the label differs; the
fragment it applies comes from `scalar_fragment` (so TOML and JSON each get their own notation). The
TOML-only dotted-key→table rename prompt (a Name edit such as `foo` → `foo.x`) is gated to TOML. Both columns share one
horizontal-scroll/overflow treatment (`edit_field_spans`, also reused to render the `/` filter
input); editor and filter input are caret-based fields (`←/→/Home/End` move the caret,
`Backspace`/`Del` erase before/at it). The `←/→` **value nudge** re-applies underscore digit
grouping when the original had it. `edit_node` truncates the path only at the first `Index`
whose container is a real `Array` (editing the whole array there); AoT-entry indices and the
keys below them are kept and addressed directly. A `$EDITOR` fragment starts at the node's own
header/value line — an adjacent standalone comment is an independent node and is never part of
the fragment. TOML has no null, so there is no clear-value operation. **`a` (add)** adds a
**next sibling of the cursor's own kind** in the cursor's scope — a scalar (empty string, opened
in the inline editor) beside a scalar, an empty container beside a container (`[]`/`{}`, or a TOML
`[table]`/`[[aot]]` header, named `placeholder`), and another standalone comment beside a comment;
the **root or an expanded branch** appends an empty scalar as its last child. Container/scalar seeds
go through the backend's `scalar_fragment` (no hard-coded notation), **except an array/seq element
seed**, which uses `array_element_fragment` so it is a **bare keyless** element in every backend
(TOML included — previously TOML seeded a `{ __elem__ = "" }` inline table). A scalar appended into a
branch is still clamped to the leading region (before any `[table]`/`[[aot]]`) so it stays legal (D5).
A scalar add opens the inline editor on the seed; pressing **Esc** there (`edit_cancel` with
`EditState.created_on_add`) rolls the insert back via `History::cancel_last` — no node, no undo/redo
crumb — so a mistaken `a` is undone in one keystroke.

**Kind switch (`K`).** `Mutation::ConvertKind { path, target: KindTarget }` (`convert_kind` in
`cst_edit.rs`) rewrites a node's kind/notation in place; the TUI side is `Mode::KindSwitch` —
`open_kind_switch` builds the per-node option list (current kind excluded), a small single-select
popup applies on Enter (`k` remains vim cursor-up, so the binding is capital `K`). **Scalars switch
between notations of their own type**, never across types: strings between
basic/literal/multiline/multiline-literal (content decoded then re-encoded; a `'` in a literal
form, `'''` in a multiline literal, or a real newline in a single-line literal is `Illegal` —
single-line *basic* escapes newlines as `\n`, so mstr→str is lossless), integers between
dec/hex/oct/bin radices (`_` separators parse; negatives have no prefixed form), floats between
plain ↔ exponent (exponent detected from the value text — `Format` has no variant for it; re-rendered
from the parsed `f64`); bools, datetimes and `inf`/`nan` have one notation and don't convert.
Arrays toggle inline ↔ multiline
(collapse rejects comments / multi-line elements); tables convert between `[T/I]`/`[T/D]`/`[T/S]`
with `[T/S]` targets checked against the D5 capture rule (mid-entry `[t]`, or a section preceded by
a foreign header, is `Illegal`; a nested `[s.t]` converts relative to its parent's capture) and
inline targets rejecting held comments. **`[A/T]` ↔ arrays**: a group converts to an
inline/multiline array of inline tables (`convert_aot_to_array`: contiguous span, plain
single-line entry bodies only — no sub-sections/comments — and the replacement `key = […]` entry
must not be captured by a foreign preceding header), and a keyed flat-ROOT array whose elements
are **all inline tables** converts to an `[[…]]` group (`convert_array_to_aot`,
`KindTarget::ArrayOfTables`; rejected when an entry follows before the next header — the
sections would capture it). AoT entries, Root and comments don't convert.

**Comments are first-class nodes** (concepts in CONTEXT.md: *Comment*, *Trailing comment* —
standalone `#` lines merge into one node and are never dragged by an adjacent node's move; a
trailing comment is value-attached decoration). A comment node carries its text as its `value`,
so the VALUE column and detail popup show it; multi-line cell values (merged comments, multiline
strings) are collapsed to a one-line preview (first line + ` …`) by `cell_preview` in `ui.rs`.
A trailing comment
is **shown in-row** (dimmed, after the value, in the VALUE cell — `value_cell` in `ui.rs`) and is
**edited inline together with the value**: `begin_inline_edit` seeds the Value buffer as
`value  # comment`, and `edit_commit` splits it back via `ConfigDocument::split_value_comment`
(which lexes through the backend so a `#`/`//` *inside a string* is not the comment). A change
from the baseline (`EditState.orig_trailing`) is staged in `App.pending_trailing` and applied by
`apply_replace` as a `Mutation::SetTrailingComment { path, comment: Option<String> }` right after
the value `Replace` (one undo step); `edit_cancel` clears the stage so it can't leak onto a later
nudge. `SetTrailingComment` is a uniform text-splice in each backend's `edit.rs` (replace the span
from the value's content end — past a separator comma for a multiline-array element — to the next
newline), `comment: None` clears, and it handles both keyed entries and **array elements**
(`Target::Element`/`ArrayElement`). **Array elements** carry an editable trailing comment too: a
**multiline-array** element gains `1,  # x`; an element (or member) inside an **inline** array /
flow collection is rejected cleanly in `edit_commit` ("switch to multiline (K) first") so the edit
stays atomic. Most backends' value `Replace` preserves an unchanged comment, but YAML's whole-entry
swap drops it; `ConfigDocument::replace_preserves_trailing_comment()` (default `true`, YAML `false`)
makes the editor re-assert an existing comment after a YAML value edit. The `←/→` value nudge
goes through the same value `Replace`, so it stages the same re-assert (a YAML nudge keeps its
trailing comment; TOML/JSON preserve it natively). `e` on a
**single-line** comment edits inline (`Mode::Edit` with `is_comment`: the raw `#`-prefixed text
is the sole field — no name, `Tab` is a no-op — and `edit_commit` routes to
`Mutation::EditComment`, staying in the editor on a non-`#` validation error); `E`, a merged
multi-line comment, or one with an `Array` ancestor open `$EDITOR` with the raw text. Deleting a
comment (`d`) is a plain token removal at its `Seg::Index` slot.

**Navigation.** Expand/collapse state is an `App.expanded: HashSet<Path>` of open branch paths. The
**root/file node has the empty path** and is collapsible like any branch — `flatten` treats it
uniformly; the App seeds `[]` into `expanded` so it starts open, and `collapse_all` (`0`) re-inserts
`[]` so it keeps the file node open (only an explicit toggle on the root row hides everything).
Beyond the all-at-once `9`/`0`, **`1`/`2` work one level at a time**: `expand_level` (`1`) inserts
the shallowest not-yet-expanded depth of the cursor branch's subtree per press; `collapse_level`
(`2`) collapses an open branch in place, else moves the cursor up to its parent branch and collapses
that (repeated presses ascend). Both re-find the cursor by path after `rebuild_rows`.

**Filter.** `/` is a three-state flow: `Mode::Filter` (the inline `/` input field) → **Enter** →
`Mode::FilterResults` (browse/select/edit the locked-in filtered list, status shows `[filter: …]`),
or **Esc** clears the filter back to `Mode::Normal`. `App.last_filter` remembers the last committed
query so `/` (`enter_filter`) prefills it and re-applies the live filter. `FilterResults` reuses the
Normal key dispatch (no early-return block); its only differences are mode-aware `escape`
(`exit_filter_results`, keeps `last_filter`) and `/` (`enter_filter`, to refine). Esc peels **one**
filter layer (`exit_filter_results`; the text layer when only `/` is active) — `last_filter` is pure
memory, never a persisted filter. The fuzzy query
matches a node's **key/path** plus a **Comment node's own text** (`recompute_filter` builds the haystack
from the path's `Seg::Key` segments — positional nodes contribute none — and appends the comment text
for a Comment node); a scalar's **value is never matched** — this keeps a loose query from fuzzily hitting unrelated
values while leaving comments searchable as standalone nodes. While a filter is active the matched chars are
highlighted in the **NAME cell** (`search::fuzzy_indices` → `ui::highlight_spans`; gated on a non-empty
query, not the mode, so the highlight survives an inline edit / detail popup; a Comment node's NAME
shows its text, so its match highlights there too). Transient overlays (detail popup,
inline editor) close back into the filtered selection via `App::resting_mode` (`FilterResults` when
`filtered_paths.is_some()`, else `Normal`) — `exit_detail`/`edit_cancel`/`edit_commit` use it.

**Type filter.** `f` opens `Mode::TypeFilter`, a modal checkbox popup (`tui/type_filter.rs`) that
filters by a node's **type facets** — the same `KeySign`/`NodeKind`/`Format` the KIND column shows.
`TypeToken` enumerates one leaf atom per KIND slot and `classify(kind, format, doc, read_only)` is
the arm-for-arm inverse of `type_tag` (so popup and column can't drift; `layout(doc)` shows only the
loaded backend's reachable facets — JSON/YAML omit TOML-only rows, YAML adds block/flow + opaque). The popup has two halves — **key sign**
(`(B)/(Q)/(D)/(-)`) and **type** (root/comment + array/table/string/integer/float/bool/date groups,
`[A/T]` grouped under tables) — each multi-format group carrying an **`all`** quick-toggle row that
is **tristate** (`group_state`: `[x]` all / `[~]` some / `[ ]` none; Space selects-or-clears the
whole group). `TypeFilter::matches` ANDs the two halves and unions within each; an empty half is no
constraint (`is_active` gates the whole filter). `layout()` is the single source of truth for both
render and nav; `nav_rows()` drops headers so the `(row,col)` cursor only lands on cells. The popup
filters **live** (every `type_filter_toggle` recomputes), Enter (`commit_type_filter`) closes into
`resting_mode`, Esc (`exit_type_filter`) clears the type selections. `recompute_filter` now builds
`filtered_paths` as the **AND intersection** of the `/` text match and the type match (matched nodes
keep ancestors). When both filters are active, Esc in `FilterResults` peels **one layer at a time**
via `App.last_filter_applied: Option<FilterLayer>` (most-recently-applied first); the status bar
shows `[filter: …]` and/or `[type: N]`.

**Multi-select.** `Selection` holds `committed` (finalized rows + `s` toggles) and an in-progress
`round` (`anchor..=cursor`); the live set is their union. A Shift+Arrow run extends `round`; the next
Shift+Arrow after any non-shift key (tracked by `App.last_action_was_shift_select`, reset in the event
loop) starts a fresh round, folding the old one into `committed` — so runs union (separate or
overlapping) rather than re-extending the first anchor.

**Clipboard / paste mode.** `copy_selected` (`c`) and `cut_selected` (`x`) load `App.clipboard`
(`Option<Clipboard>`) from `selected_paths()` (the selection, or the cursor row when none). Both
capture **scope-relative** fragments: a node copied/cut out of a `[T/D]` table drops its leading
dotted-ancestor key segments (`serialize_fragment_relative` for copy; `Mutation::Move` strips at
capture for cut — `dotted_ancestor_prefix_len` + `strip_key_prefix`), so `dotted.test.bool_true`
becomes `bool_true` and a paste re-prefixes only for the **destination** (`prefix_entry_key`) instead
of stacking the source prefix. (The `$EDITOR` block edit still uses the full-key `serialize_fragment`.)
Cut defers deletion until a successful paste. A loaded clipboard *is* "paste mode" and is kept distinct from
selection mode: while `clipboard.is_some()`, the three selection mutators (`toggle_select`,
`extend_select_up`/`down`) early-return, so selection is frozen; pressing `c`/`x` again **toggles** the
existing clipboard's mode (copy ↔ cut) instead of re-capturing. Render cues (`draw_tree`): cursor row
green (paste-ready), source rows blue, selected rows grey — and since selection is frozen during paste
mode, blue vs grey never collide. `Esc` in `Mode::Normal` peels one layer per press: clipboard first
(keeping any live selection, status "clipboard cleared"), then selection. Paste (`v`) resolves the
insertion `Target` with `resolve_target` over `true_sibling_index` (position in the *full* tree, so
FilterResults' hidden siblings don't skew it — the same helper is used by `add_node` and the
collision-retry path). `do_paste` pairs each fragment with its source path and splits **node** vs
**comment** entries (identified by `NodeKind::Comment`, not by the path). Nodes: **cut** routes
through the atomic `Mutation::Move` (delete-before-reinsert on a scratch tree, committed only on
success) so a same-scope reposition is a move, not a `Key already exists` collision; **copy** uses the
per-fragment `Mutation::Insert` loop. **Moving or copying an array element out**, and **multiple
keyed nodes joined into one array/`[A/T]` element**, follow the forming rules in CONTEXT.md's
*Insert / move legality* table (helpers: `unpack_inline_table`/`wrap_keyed_as_inline_element`,
`joinable_entry`, in `move_nodes`/`do_paste`/`insert`). Comments: a Comment node's fragment is its raw `# …` text, pasted
via `Mutation::InsertComment` (validates every line starts with `#`, splices the block in at the target
child index, never collides); a cut deletes the source comment first, then inserts. A comment into a
**single-line array** is no longer rejected: `InsertComment` upgrades the array to multiline (one
element per line, exact element reprs kept) and then inserts — the TUI asks first via
`Mode::Prompt(ArrayUpgrade)` (`y` re-issues `do_paste` with the upgrade allowed, `n` cancels keeping
the clipboard); the inverse collapse back to inline is deliberately not built. `do_paste` takes the
`Clipboard` by value and **restores it on every failure** (collision → `Mode::Prompt(Collision)` with
the remaining entries — comment entries are preserved so they run on retry; any other error → restores
the rest + `paste error: …`), so a failed paste is never destructive; only `Esc`/`c` at the collision
prompt discards it. Because comments are independent nodes, a moved or copied node never carries an
upper-adjacent comment with it — the comment simply stays where it is.

## Module map

```
src/
  main.rs          CLI entry: parse args, load CstDocument, run TUI
  lib.rs           module declarations + re-exports (enables integration tests)
  cli.rs           clap args: default `confy <file> [--format]` (TUI) + `confy convert <in> <out>` subcommand
  model/
    mod.rs         re-exports
    node.rs        Seg, ScalarType, Format, NodeKind, Node, NodeTree (+ node_at lookup)
    document.rs    ConfigDocument trait (+ to_value), DocFormat, Mutation, Target, OnCollision, ConvertAbort, errors
    value.rs       format-neutral Value/Item tree for conversion (has_null/has_datetime)
    convert.rs     document-level conversion: tree_to_value walk + per-format scalar decoders + default-style renderers + loss policy
    any_doc.rs     AnyDocument enum: per-format dispatch + detect_format/load_as (TOML/JSON/YAML)
    cst_doc.rs     CstDocument holding the taplo/rowan tree: load/serialize/apply (atomic commit)
    cst_project.rs CST → NodeTree projection (comments as real nodes; golden tests)
    cst_edit.rs    rowan splice helpers: one fn per Mutation variant + the path→element walk index
    json/
      mod.rs       re-exports for the JSON/JSONC backend
      syntax.rs    SyntaxKind enum + rowan Language impl (hand-rolled JSON token/node kinds)
      parse.rs     lossless lexer + recursive-descent parser → rowan GreenTree (JSONC-aware)
      doc.rs       JsonDocument: load/serialize/apply (atomic commit + validate_semantics)
      project.rs   GreenTree → NodeTree projection (// comments as real nodes; golden tests)
      edit.rs      rowan splice helpers: one fn per Mutation variant for JSON/JSONC
    yaml/
      mod.rs       re-exports for the YAML-subset backend
      syntax.rs    SyntaxKind enum + rowan Language impl (hand-rolled YAML token/node kinds)
      parse.rs     lossless lexer + recursive-descent parser → rowan GreenTree (subset; multi-doc reject)
      doc.rs       YamlDocument: load/serialize/apply (atomic commit + validate_semantics)
      project.rs   GreenTree → NodeTree projection (# comments real nodes; opaque read-only nodes; golden tests)
      edit.rs      rowan splice helpers: reindent engine + one fn per Mutation variant; opaque guard
  tui/
    mod.rs         re-exports; run() entry point + event loop (run_event_loop)
    app.rs         App state + operation handlers (the event loop dispatches keys to these)
    state.rs       Mode (incl. Edit), Clipboard, EditState, undo/redo stacks
    keys.rs        KeyAction mapping + help text
    insertion.rs   §6.1 insertion-target resolution from cursor
    selection.rs   multi-select + range select + §6.2 normalization
    search.rs      fuzzy filter state + haystack builder
    type_filter.rs type-filter (`f`) facets: TypeToken/classify, TypeFilter predicate, popup layout+nav
    editor.rs      $EDITOR integration (external edit for nested array/table)
    ui.rs          ratatui rendering: title bar + NAME/TYPE/VALUE column header + tree Table, detail popup, help, prompts
tests/
  roundtrip.rs     integration: open/edit/save, diff fixture
  convert_cli.rs   integration: `confy convert` happy/lossy/abort paths, source-unchanged
  fixtures/        sample .toml files
```

`model/` is pure (no TUI deps) and fully unit-testable in isolation.

## Terminology

See **`CONTEXT.md`** for the canonical glossary. Key rule: use **Node** (not "Entry"). Subtypes
are **Root**, **Branch node**, **Leaf node**, **Scalar**, and **Comment**. The operation that
toggles a live Node to/from a Comment is **Remark** (key `r`).
