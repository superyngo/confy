# TUI layer — confy ratatui frontend

TUI-specific mechanics for the ratatui frontend (`src/tui/`). These are **not** shared
with the model layer; see `WEBUI.md` for the parallel web-UI mechanics. For model
semantics (Mutation variants, kind-switch rules, insert/move legality) see `CONTEXT.md`.
For the inline-vs-`$EDITOR` boundary see `BEHAVIOR_MATRIX.md §6`. The keyboard bindings
themselves are **not** documented here: `KEYMAP.md` is the TUI ↔ Web single source of
truth for them, and its table is machine-checked against `map_key` (`src/tui/keys.rs`).
The TUI calling `Session` methods directly rather than routing every mutation through
`dispatch(Intent)` (as the other hosts do) is a deliberate exception — ADR 0003.

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
tags `[opaq ]` whatever the underlying kind. The detail popup keeps word labels; its `Path:` line
is `ViewRow.path_display` (built by `Session::human_path`), which includes positional indices and
re-wraps a quoted-YAML key segment in its authored flanks — `a.b[2].c`, but `servers."web 1".port`
for a key written with quotes, so the displayed path matches the file (CONTEXT.md *Path line*).
`node_type_label` still drives the inline editor's type-change comparison.

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
grouping when the original had it. Its **step is the schema's**: with a `multipleOf` on the node,
the nudge walks that grid (an off-grid value aligns in the nudge's direction on the first press),
`minimum`/`maximum` clamp inward to the nearest in-range grid point, and without a schema
constraint the step stays ±1 (±1 at the displayed precision for a float). `edit_node` truncates the path only at the first `Index`
whose container is a real `Array` (editing the whole array there); AoT-entry indices and the
keys below them are kept and addressed directly. A `$EDITOR` fragment starts at the node's own
header/value line — an adjacent standalone comment is an independent node and is never part of
the fragment. The editor command comes from `$EDITOR`, then `$VISUAL`, then `vi` (`notepad` on
Windows); it is shell-split (`tui/editor.rs`, `shell-words`) so `EDITOR="code --wait"` works, and
the scratch file carries the document's own extension (`.toml`/`.json`/`.yaml`) so the editor
picks the right syntax mode. On return the event loop repaints via `full_redraw` (a query-free
`Terminal::resize`, not `Terminal::clear`, which since ratatui 0.30 issues a cursor-position query
that non-answering PTYs time out on). TOML has no null, so there is no clear-value operation. **`a` (add)** adds a
**next sibling of the cursor's own kind** in the cursor's scope — a scalar (empty string, opened
in the inline editor) beside a scalar, an empty container beside a container (`[]`/`{}`, or a TOML
`[table]`/`[[aot]]` header, named `placeholder`), and another standalone comment beside a comment
(blank-line separated so it stays a **distinct** single-line node instead of merging into the
neighbour, and opened in the inline editor — same as a scalar);
the **root or an expanded branch** appends an empty scalar as its last child. Container/scalar seeds
go through the backend's `scalar_fragment` (no hard-coded notation), **except an array/seq element
seed**, which uses `array_element_fragment` so it is a **bare keyless** element in every backend
(TOML included — previously TOML seeded a `{ __elem__ = "" }` inline table). A scalar appended into a
branch is still clamped to the leading region (before any `[table]`/`[[aot]]`) so it stays legal (D5).
A scalar **or comment** add opens the inline editor on the seed; pressing **Esc** there
(`edit_cancel` with `EditState.created_on_add`) rolls the insert back via `History::cancel_last`
— no node (for a comment, the blank separator goes too), no undo/redo crumb — so a mistaken `a`
is undone in one keystroke.

**`e` on a `bool`** does not open the text editor at all: it opens the two-option `true`/`false`
picker popup (`overlay_schema_enum.rs`, `Mode::SchemaEnum` with `from_schema: false`, so the popup
is titled ` Value ` rather than ` Schema value `) — same widget and same keys as a schema
`enum`/`const` constraint (`↑↓`/`j`/`k`, `Home`/`End`, `PgUp`/`PgDn`, `Enter` apply, `Esc` cancel),
because a bool's value domain is closed at two members. Options carry the node's **authored
casing** (YAML's `True`/`TRUE` stay uppercase — committing lowercase would silently re-case the
document), a schema `enum` on the same node **outranks** this fallback, and `E` (force `$EDITOR`)
is unaffected: it stays the way to type a bool's line free-form (including its trailing comment).

## Comments (TUI)

A comment node carries its text as its `value`,
so the VALUE column and detail popup show it; multi-line cell values (merged comments, multiline
strings) are collapsed to a one-line preview (first line + ` …`) by `cell_preview` in `ui.rs`.
A trailing comment
is **shown in-row** (dimmed, after the value, in the VALUE cell — `value_cell` in `ui.rs`) and is
**edited inline together with the value**: `begin_inline_edit` seeds the Value buffer as
`value  # comment`, and `edit_commit` splits it back via `ConfigDocument::split_value_comment`
(which lexes through the backend so a `#`/`//` *inside a string* is not the comment). A change
from the baseline (`EditState.orig_trailing`) is staged in `Session.pending_trailing` and applied by
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

Expand/collapse state is a `Session.expanded: HashSet<Path>` of open branch paths. The
**root/file node has the empty path** and is collapsible like any branch — `flatten` treats it
uniformly; the Session seeds `[]` into `expanded` so it starts open, and `collapse_all` (`0`) re-inserts
`[]` so it keeps the file node open (only an explicit toggle on the root row hides everything).
Beyond the all-at-once `9`/`0`, **`1`/`2` work one level at a time**: `expand_level` (`1`) inserts
the shallowest not-yet-expanded depth of the cursor branch's subtree per press; `collapse_level`
(`2`) collapses an open branch in place, else moves the cursor up to its parent branch and collapses
that (repeated presses ascend). Both re-find the cursor by path after `rebuild_rows`.

## Filter

`/` is a three-state flow: `Mode::Filter` (the inline `/` input field) → **Enter** →
`Mode::FilterResults` (browse/select/edit the locked-in filtered list, status shows `[filter: …]`),
or **Esc** clears the filter back to `Mode::Normal`. `Session.last_filter` remembers the last committed
query so `/` (`enter_filter`) prefills it and re-applies the live filter. `FilterResults` reuses the
Normal key dispatch (no early-return block); its only differences are mode-aware `escape`
(`exit_filter_results`, keeps `last_filter`) and `/` (`enter_filter`, to refine). Esc peels **one**
filter layer (`exit_filter_results`; the text layer when only `/` is active) — `last_filter` is pure
memory, never a persisted filter. The fuzzy query
matches a node's **key/path** plus a **Comment node's own text** (`recompute_filter` builds the haystack
from the path's `Seg::Key` segments — positional nodes contribute none — and appends the comment text
for a Comment node, plus a scalar leaf's **own value**), so a query matches keys, paths, comments, and
values alike. While a filter is active the matched chars are
highlighted in **both the NAME and the VALUE cell** (`search::fuzzy_indices` →
`ui::highlight_spans`/`highlight_spans_styled`; gated on a non-empty
query, not the mode, so the highlight survives an inline edit / detail popup; a Comment node's NAME
shows its text, so its match highlights there too). Each cell runs the matcher against **its own
text**, not the haystack, so the marks line up with what's drawn — a row matched only via its path
shows no marks in VALUE, and vice versa. A row's **trailing comment** is never highlighted; it keeps
its dim/advisory styling as annotation. `highlight_spans_styled` layers the highlight over a base
style so a `comment_advisory` value stays underlined underneath. Transient overlays (detail popup,
inline editor) close back into the filtered selection via `Session::resting_mode` (`FilterResults` when
`filtered_paths.is_some()`, else `Normal`) — `exit_detail`/`edit_cancel`/`edit_commit` use it.

## Type filter

`f` opens `Mode::TypeFilter`, a modal checkbox popup (`tui/type_filter.rs`) that
filters by a node's **type facets** — the same `KeySign`/`NodeKind`/`Format` the KIND column shows.
`TypeToken` enumerates one leaf atom per KIND slot and `classify(kind, format, doc, read_only)` is
the arm-for-arm inverse of `type_tag` (so popup and column can't drift; `layout(doc)` shows only the
loaded backend's reachable facets — JSON/YAML omit TOML-only rows, YAML adds block/flow + opaque). The popup groups three facet sets —
**key sign**
(`(B)/(Q)/(D)/(-)`), **type** (root/comment + array/table/string/integer/float/bool/date groups,
`[A/T]` grouped under tables), and **Flags** (`(!) has warning` / `has comment`) — each multi-format group carrying an **`all`** quick-toggle row that
is **tristate** (`group_state`: `[x]` all / `[~]` some / `[ ]` none; Space selects-or-clears the
whole group). `TypeFilter::matches` ANDs the three facet sets and unions within each; an empty set is no
constraint (`is_active` gates the whole filter). `layout()` is the single source of truth for both
render and nav; `nav_rows()` drops headers so the `(row,col)` cursor only lands on cells. A
`Reverse` header/cell sits first in every `layout()` (row 0, so opening the popup starts there) —
toggling it inverts `matches`' combined result (`base = sign_ok && type_ok`), but only once
`is_active()` is true; with nothing else selected `reverse` is a deliberate no-op, not a "hide
everything" trap. `clear()` resets it alongside the three facet sets. Cursor movement isn't limited to
arrows: Home/End jump to the first/last nav row (col clamps into the new row's width, same as any
vertical move); PageUp/PageDown jump by `ui::type_filter_page_step` — how many nav rows fit in the
popup's visible height, *not* the raw line count (headers don't count as cursor stops, so using line
count would roughly double-jump). The popup
filters **live** (every `type_filter_toggle` recomputes), Enter (`commit_type_filter`) closes into
`resting_mode`, Esc (`exit_type_filter`) clears the type selections. `recompute_filter` now builds
`filtered_paths` as the **AND intersection** of the `/` text match and the type match (matched nodes
keep ancestors — *except* a node that's a deliberate `reverse` exclusion target: `TypeFilter::is_reverse_excluded`
is true when the node's own sign/type facet was positively selected, so `reverse` hid it on purpose;
`recompute_filter` prunes that node's whole subtree instead of just dropping it from the match set,
otherwise a descendant that legitimately passes the reversed filter would drag the excluded container
back in via ancestor-context — the bug that made `reverse` look like a no-op on Table/Array while
working fine on Scalar/Comment, which have no children to trigger the resurrection). When both
filters are active, Esc in `FilterResults` peels **one layer at a time**
via `Session.last_filter_applied: Option<FilterLayer>` (most-recently-applied first); the status bar
shows `[filter: …]` and/or `[type: N]` (`N` counts only `key_signs`/`types`, never `reverse`).

## Multi-select

`Selection` holds `committed` (finalized rows + `s` toggles) and an in-progress
`round` (`anchor..=cursor`); the live set is their union. A Shift+Arrow run extends `round`; the next
Shift+Arrow after any non-shift key (tracked by `Session.last_action_was_shift_select`, reset in the event
loop) starts a fresh round, folding the old one into `committed` — so runs union (separate or
overlapping) rather than re-extending the first anchor.

## Action menu

`m` opens `Mode::ActionMenu { cursor }`, a modal popup (`overlay_action_menu.rs`, same
shape as the `K` kind-switch popup) listing the eight core-owned Action menu items
(design doc `docs/superpowers/specs/2026-08-30-action-menu-design.md` §2, ADR 0009):
Edit in editor, Add child, Append sibling, Copy, Cut, Toggle comment, Detail, Delete
(separated by a rule and shown in red). `Session::action_menu_items()` derives each
item's `enabled` flag fresh from `selected_paths()` every frame — a single-path item
(Edit in editor / Add child / Append sibling / Detail) dims on a multi-node selection;
the four set-applying items (Copy / Cut / Toggle comment / Delete) dim only if any
targeted node is read-only. Disabled items stay visible (dimmed), never hidden, so
cursor position is stable. Up/Down (or j/k) move the cursor, skipping disabled items;
Home/End jump to the first/last enabled item (`App::action_menu_jump_edge` — core's
stride-by-delta move means the host sends the exact `target − cursor` offset), and
PageUp/PageDown page by `ACTION_MENU_PAGE_STEP` (5); all wrap like the arrows. Web's
popup/sheet mirrors this in `web/key-intent.ts` (`actionMenuEdgeDelta` /
`ACTION_MENU_PAGE_STEP`), so external keyboards on touch behave identically.
Enter (`action_menu_commit`) always exits the menu first (`resting_mode()`), then
dispatches the picked item's intent if it was enabled, else surfaces
`core.action.unavailable`; Esc cancels without dispatching. Opening the menu while the
clipboard is armed is refused (`core.clipboard.action-locked`), the same modal lock
every other popup uses (ADR 0005 §5). Kind switch (`K`) is deliberately not in this
list — the row's KIND badge is already a dedicated, always-visible control for it.

## Language / i18n (TUI)

Language is a host-owned preference layered on top of `confy-core`'s catalog (see root
`CLAUDE.md` §Architecture *i18n*). Resolution order: `--lang <code>` CLI flag (session-only,
never written back) > `~/.config/confy/config.toml`'s `lang = "…"` (`crates/confy-tui/src/
config.rs`; `$XDG_CONFIG_HOME/confy/config.toml` else `~/.config/confy/config.toml` on
macOS/Linux, `%APPDATA%\confy\config.toml` on Windows via `dirs::config_dir()`) > default `en`.
A missing/unparsable config file is never an error — it just falls back to defaults.

`l` opens a small host-side popup (`App::open_lang_picker`, same pattern as the kind-switch
popup) listing the available languages; selecting one dispatches `Intent::SetLang`, calls
`save_config` (best-effort — a write failure surfaces as a status message via
`tui.lang.save-failed`, never a crash), and confirms via `tui.lang.saved`. The About screen (`?` →
About tab) appends two host-only lines after the core's translated `about_text(lang)` body:
`Config: <path>` (the resolved path, shown even before the file exists) and `Language: <code>` —
these can't live in the core catalog since the config path is filesystem-specific to this host.
`tui/keys.rs::help_text(format, lang)` and every prompt/status string in `tui/ui.rs` route through
the same `tui.*` catalog keys as the rest of the TUI; CJK lines in the `?` cheatsheet and detail
popup were manually eyeballed for the double-width alignment risk noted in the i18n plan.

## Clipboard / paste
See ADR 0004 for the cross-platform `PasteSlot` targeting model these mechanics implement,
and `ROW_STATE_MODEL.md` for the full cross-platform row cursor/selection/clipboard state model.

`copy_selected` (`c`) and `cut_selected` (`x`) load `Session.clipboard`
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
blue, cut source green, copy source magenta (three mutually exclusive full-row fills), and a locked
selection paints no fill at all — only its `●` marker — so it composes with any of them. `Esc` in
`Mode::Normal` peels one layer per press: clipboard first
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

## Status & diagnostics (TUI)

See `MESSAGES.md` for the full cross-platform message-system reference (severity
table, host comparison, unified design). This section covers TUI-specific
rendering mechanics only.

The status bar (`draw_status` in `ui.rs`) renders the Session's single-slot `Notice` (`Severity` +
localized text) alongside mode hints, in priority order: **(1) Error notice** — red background,
` ✗ ` prefix, shown outside `Mode::Edit` regardless of any other state (clipboard armed, filter
active) — the "errors never hidden" invariant; **(2) active input** — `Mode::Filter`'s inline `/`
field, or `Mode::Edit`'s value/name editor (inside `Mode::Edit`, a pending notice *overrides* the
edit hints — shown in red with an `(Esc:cancel)` cue — rather than being hidden by them);
**(3) Warn/Success/Info notice** — the status bar's default dark-gray slot, which also wins over
the clipboard-armed sticky hint and the `FilterResults` tag/count line (both used to render first
and silently swallow a pending non-Error notice — fixed 2026-08-22); **(4) mode/hint fallback** —
clipboard-armed sticky hint, `FilterResults` tag line, or the default `pos/total` status with a
dynamic schema `edit_hint` tooltip and aggregate violation count (`core.schema.count`), only once
no notice is pending.

The `i` Detail popup's `Schema:` section (`overlay_detail.rs`) is a separate, persistent
per-node reference surface — not part of the Notice/diagnostics system above. It combines three
independent sources, any subset of which may be present: `Session::schema_info(path)` (non-widget
`description`/`type`/`format`/`pattern` info pulled straight from the resolved subschema — the
common plain-typed case `EditHint` doesn't model, e.g. a bare `{"type":"string"}` field), the
cursor row's `edit_hint` constraint description (e.g. "Valid values: …", only for `enum`/`const`/
numeric-bounds constraints), and its current violation message(s) — omitted entirely only when
none of the three apply. Mirrors `web/panel.ts`'s Schema field exactly (§ Shared edit/detail
panel, `WEBUI.md`), so the same schema information is available in both UIs.

`~` opens a read-only diagnostics overlay (`overlay_diag.rs`, `draw_diag_overlay`), a centered
popup displaying the Session's bounded 256-event diagnostic ring (`session.diag`), newest last,
with per-level coloring (`DiagLevel` Error red / Warn yellow / Info cyan / Debug dark gray). Like
the `l` language picker, this is host-owned UI state (`App.diag_overlay_open`), not a core `Mode`:
`~` or `Esc` closes the overlay, other keys are swallowed while open, and opening is mutually
exclusive with the language picker (`app.lang_picker.is_some()`).
