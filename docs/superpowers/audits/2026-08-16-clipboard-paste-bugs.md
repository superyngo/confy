# Clipboard/paste bugs found while grilling ADR 0004

✅ **Resolved — historical reference.** All findings below were addressed; see `CHANGELOG.md`. Kept for the development record, not as an open action list.

Found during the `grilling` session that produced
`docs/adr/0004-unified-clipboard-move-targeting.md`. None of these are design tradeoffs — plain
correctness bugs — so they're tracked here, not as ADRs, and don't gate ADR 0004's implementation.
All three findings are root-caused and fixed (via `systematic-debugging` passes), each with a
headless regression test and, for findings 1 and 3, confirmed against the real `confy` TUI binary.
Not yet triaged into GitHub issues (repo has none open at time of writing); do that if/when picked
up.

## 1. FIXED — crash appending to a `[T/D]` table with a nested-inline-table member

Originally reported as "self-paste of a `[T/D]` table into its own scope panics." Root-caused via
`systematic-debugging` (instrumented `insert()`/`resolve_insert_at`, ran the exact repro headlessly)
and found to be **broader than self-paste**: *any* insert that appends a new member to a `[T/D]`
dotted table whose existing member's value contains 2+ levels of nested inline tables (e.g.
`t.a = { b = { x = 1 } }`) triggered it — confirmed with a non-self-paste repro (`add newkey = 1`
under `t`) that panicked identically.

**Root cause**: `project_inline` (`cst_project.rs:680`) indexes an inline table's own members as
`Target::Entry` too, but their `SyntaxNode::index()` is relative to their *immediate* CST parent
(the inline table), not the flat ROOT. `node_last_root_index` (`tree_nav.rs`) recursed past a
member's own backing ROOT entry into those nested, container-relative indices and treated them as
if they were ROOT-child positions — producing a splice index past the ROOT's actual child count,
which panicked `rowan`'s `splice_children`. Its sibling `node_start_root_index` already
short-circuited on a node's own backing element instead of recursing past it; `node_last_root_index`
didn't, which was the actual asymmetry/bug.

**Fix**: `crates/confy-core/src/model/cst_edit/tree_nav.rs`, `node_last_root_index` — short-circuit
on `Target::Entry` exactly like `node_start_root_index` does (an Entry's own physical span already
contains everything nested in its value; only `Header`/`AotEntry`/headerless-container members are
genuinely separate ROOT-level elements worth descending into). Regression tests:
`crates/confy-core/tests/session_headless.rs`,
`append_new_key_into_dotted_table_with_nested_inline_member` and
`copy_paste_dotted_table_into_its_own_scope_does_not_panic`. Full suite (632 tests) green after the
fix.

## 2. FIXED (related, but distinct bug) — stale `self.tree` after a partial multi-fragment paste failure

Root-caused a **second, real** bug via the same `systematic-debugging` pass, related to but distinct
from the originally-suspected mechanism. `do_paste`'s NODE-PHASE loop (copy branch,
`clipboard.rs`, grouped-fragment insert) held one `doc` borrow across every loop iteration and never
called `self.on_mutation_success(None)` on its `Collision`/generic-`Err` early-returns — even
though an *earlier* iteration in the same loop may have already mutated `self.doc`. Confirmed with
a deterministic repro: paste two nodes into a table where the first has no collision and the
second does — the first's insert commits to the real document, but `self.tree` (and therefore
`visible_rows`, `cursor_row`, `selected_paths`) kept the **pre-paste snapshot**, silently diverged
from the document. The comment phase a few lines below already re-borrows `doc` per iteration and
calls `on_mutation_success` on its own error paths — the node phase was the asymmetric, unfixed
twin. **Fixed** to match (re-borrow `doc` inside the loop, call `on_mutation_success(None)` on both
error arms). Regression test: `crates/confy-core/tests/session_headless.rs`,
`paste_partial_failure_reprojects_tree_before_returning`.

**`MutateError::NotFound`'s `Display` is real** ("path not found",
`crates/confy-core/src/model/document.rs:278`) — not a UI cosmetic issue, and this bug's stale-tree
mechanism is a genuine, independently-fixed cause of it. The user's specific "delete fails until
Esc" symptom, though, turned out to be a *different* mechanism — see finding 3b's stale-selection
bug, which fully explains it (no lingering unconfirmed link to this one).

## 3. FIXED — two compounding bugs: `do_paste` didn't expand its `Into` target, and rename never remapped `self.selection`

Root-caused the user's exact repro — in two passes. The first pass (below) found and fixed a real
bug but **mis-attributed** the two reported error strings to it; the user pushed back ("問題還是存在
... source錨點狀態" — the problem still reproduces; the cause is likely that a paste doesn't clear its
source-anchor state) and was right that something was still wrong, though the actual second bug is
more specific than "clears after paste" — see below.

**Reproduction** (all three earlier headless-`Session` attempts had skipped clearing the pre-seeded
value-buffer text — `""` for a fresh JSON empty string — before typing; `edit_input_char` inserts at
the buffer's *end*, so typing over an uncleared seed silently fails differently. Redone with the
buffer cleared first, mirroring a real user backspacing before typing):

1. New JSON file (`{}`), `a` to add a field, clear the seeded `""`, type `{"a":1}`, confirm the
   string→table prompt (`y`) — `new_field = {"a": 1}`.
2. Copy `new_field` (`c`), step the paste slot back to `Into(new_field)` (`k`), paste (`v`) —
   succeeds, producing `new_field = {"a":1, "new_field": {"a":1}}`.
3. Rename the nested copy to `inner` (`F2`), copy it (`c`), navigate the paste slot to root
   (`Home`), paste (`v`) — reported: `paste error: invalid fragment: fragment is not a value`.
4. Delete the same node — reported: `delete error: path not found`.

### Bug 3a — `do_paste` didn't expand its `Into` target

`do_paste` (`clipboard.rs`) points `self.cursor` at the freshly-pasted child of an `Into` target
without expanding that target first, so the target stays visually **collapsed** (confirmed
byte-for-byte on the real `confy` TUI binary) and `F2` on the nested copy **silently does nothing**
— `Session::cursor_row`/`view_row_at` return `None` for any path hidden by a collapsed ancestor.
`add_node_impl` (`inline_edit.rs:849`) and `reveal_path` already know this and call
`self.expanded.insert(target.parent.clone())`; `do_paste` was the missing third case of the same
idiom. **Fixed.**

This fix alone let `F2` actually engage — but continuing the repro with *only* this fix applied
still reproduced both reported error strings verbatim. **Retracted**: the write-up at this point
claimed the second error was root's empty path colliding with `NotFound`; that was wrong — see
below for what was actually happening.

### Bug 3b — rename updated the cursor but never remapped `self.selection`

The real second bug, found by instrumenting `capture_selected`/`paste`/`delete_selected` with
temporary `eprintln!` debug output and re-running the exact sequence on the real TUI binary (per
the user's correct pushback that something was still stale). `do_paste`'s success path selects the
freshly-pasted node (`self.selection.set_all(...)`) so the paste destination is visibly highlighted
— correct on its own. But `F2`-renaming that *same* node only remapped `self.cursor` to the new
path (`edit_commit`'s rename-success block, and the parallel `apply_deferred_rename` for a
TypeChange-confirmed dotted rename); neither touched `self.selection`, which kept the stale
pre-rename path. `Session::selected_paths()` prefers a non-empty `self.selection` over the cursor,
so the very next `copy_selected` silently captured a fragment from the stale, now-nonexistent path
instead of the renamed node — the wrong (empty/garbage) fragment, with no error at copy time.
Pasting that fragment to root then failed with exactly `"fragment is not a value"`, and deleting the
(still-stale) selected path failed with exactly `"path not found"`. Both verbatim.

**Fixed** with `Selection::remap_prefix(&old_prefix, &new_prefix)` (`selection.rs`) — rewrites every
selected path (and the in-progress round's anchor) whose prefix is the renamed node's old path,
preserving any suffix beneath it (covers a selected descendant of the renamed node, not just an
exact match). Called alongside the existing cursor remap at both rename call sites. With *both*
3a and 3b fixed, the full real-binary repro now succeeds end-to-end: paste-to-root and the
following delete both succeed on the **first** try — no `Esc` even needed, which also resolves the
"Esc then delete succeeds" detail the first pass couldn't explain.

`"fragment is not a value"` is otherwise a correct, if tersely-worded, JSON-backend rejection
(`model/json/edit.rs:144`) of pasting a keyed table-entry fragment directly at root (root's members
need a *bare* value fragment, not `"key": value`); it was never a bug on its own, only a correct
error surfaced against the wrong (corrupted) fragment.

Regression tests: `selection.rs` (`remap_prefix_rewrites_exact_and_descendant_matches`,
`remap_prefix_leaves_unrelated_paths_untouched`, `remap_prefix_updates_the_round_anchor`) and
`session_headless.rs` (`paste_into_slot_expands_target_so_pasted_child_is_visible`,
`rename_remaps_stale_selection_so_the_next_copy_targets_the_right_node` — the latter scripts the
full sequence above end-to-end and asserts the correct fragment is copied and both paste-to-root
and delete succeed on the first try).

<details>
<summary>Earlier (superseded) headless-repro attempts and their false-negative cause</summary>

Scripted this exact sequence three ways against the headless `Session` API — compact single-line
JSON, pretty-printed multi-line JSON, and the raw end-state document skipping straight to the
final copy/paste — all three completed with **no error**, because all three used a one-shot
`commit_edit`-style value overwrite that happened to replace the seeded buffer wholesale, or (in
the raw-end-state variant) skipped the add/type-change step entirely — none drove the
`Into`-paste + rename-immediately-after sequence closely enough to land the cursor on an invisible
row (bug 3a) or to leave a stale selection behind a rename (bug 3b). A follow-up interactive-TUI
attempt (PTY) hit an unrelated input-transmission issue sending raw bytes through the harness's
`hub` tool (its `text` field doesn't interpret `\u` escapes; needed real control bytes sent via
`eval`/Python instead) and was abandoned at the time rather than chased further — resolved in this
pass.

</details>
