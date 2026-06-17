# TUI layer — confy ratatui frontend

TUI-specific mechanics for the ratatui frontend (`src/tui/`). These are **not** shared
with the model layer and will have a parallel `WEBUI.md` when the web UI lands. For model
semantics (Mutation variants, kind-switch rules, insert/move legality) see `CONTEXT.md`.
For the inline-vs-`$EDITOR` boundary see `BEHAVIOR_MATRIX.md §6`.

## Rendering

`ScalarType` and a node's **Format** (writing style) are derived read-only during projection
and are orthogonal to each other.
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

## Editing

The inline editor edits one field at a time: **`Tab` toggles
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

## Comments (TUI)

A comment node carries its text as its `value`,
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

## Navigation

Expand/collapse state is an `App.expanded: HashSet<Path>` of open branch paths. The
**root/file node has the empty path** and is collapsible like any branch — `flatten` treats it
uniformly; the App seeds `[]` into `expanded` so it starts open, and `collapse_all` (`0`) re-inserts
`[]` so it keeps the file node open (only an explicit toggle on the root row hides everything).
Beyond the all-at-once `9`/`0`, **`1`/`2` work one level at a time**: `expand_level` (`1`) inserts
the shallowest not-yet-expanded depth of the cursor branch's subtree per press; `collapse_level`
(`2`) collapses an open branch in place, else moves the cursor up to its parent branch and collapses
that (repeated presses ascend). Both re-find the cursor by path after `rebuild_rows`.

## Filter

`/` is a three-state flow: `Mode::Filter` (the inline `/` input field) → **Enter** →
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

## Type filter

`f` opens `Mode::TypeFilter`, a modal checkbox popup (`tui/type_filter.rs`) that
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

## Multi-select

`Selection` holds `committed` (finalized rows + `s` toggles) and an in-progress
`round` (`anchor..=cursor`); the live set is their union. A Shift+Arrow run extends `round`; the next
Shift+Arrow after any non-shift key (tracked by `App.last_action_was_shift_select`, reset in the event
loop) starts a fresh round, folding the old one into `committed` — so runs union (separate or
overlapping) rather than re-extending the first anchor.

## Clipboard / paste

`copy_selected` (`c`) and `cut_selected` (`x`) load `App.clipboard`
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
