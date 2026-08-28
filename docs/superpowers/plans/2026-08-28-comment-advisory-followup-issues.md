# Comment-advisory follow-up issues (memo, not yet investigated)

Status: **note only** — captured for a future session, no root-cause analysis
done yet. Do not assume any of the hypotheses below are confirmed.

Context: after landing the JSONC-schema-parsing fix + comment-advisory UI
(commits around 2026-08-27, see CHANGELOG "Unreleased Update" entries for
`fix(schema)` / `feat(core)` / `feat(host)` / `feat(ui)`), manual testing
surfaced three behavioral gaps.

## 1. Editing in a comment on a previously-clean `.json` doesn't trigger the advisory

Repro: open a valid (comment-free) `.json` file, then use the editor to add a
comment via a mutation. Expected: `comment_advisory` should light up (red
underline / message) same as if the file had been opened with the comment
already present. Actual: no warning appears.

Likely area: `Session::comment_advisory_for` (`session/session.rs` ~L219-226)
is only ever invoked from the per-row `to_view_row` projection, driven by
`self.strict_json`. `strict_json` itself is set once at `Session::new`/
`from_tree` and "never toggled by mutations" (see its doc comment at
`session/session.rs` L58-63). That should still be fine — `strict_json`
being sticky is correct, and `to_view_row` is supposed to recompute on every
row rebuild. So the bug is probably NOT in `comment_advisory_for` itself, but
in whichever mutation path adds the comment: does `Mutation::InsertComment`
(or a trailing-comment attach) actually route through the normal
rebuild-rows / `to_view_row` refresh, or does it use a fast/partial-update
path that skips recomputing `comment_advisory` for the touched row? Compare
against how `violations`/`has_descendant_violation` get refreshed after a
mutation (`revalidate_schema` is called explicitly post-mutation — is there
an equivalent recompute step missing for comment_advisory, or does it rely
purely on `to_view_row` always re-running per row and something upstream is
memoizing/short-circuiting that row).

## 2. Comment advisory and schema hint detection seem to fight each other on load

Repro: open a `.json` file that already contains a comment (and would also
carry a `$schema` hint or an externally-supplied schema). Observed: the
comment-advisory notice fires, but the schema hint/validation appears to be
suppressed — until *any* edit is made, at which point schema validation
suddenly kicks in.

Likely area: `detect_and_request_schema` / `apply_schema_text` in
`session/session.rs` (~L1495-1560) vs. the strict_json/comment-advisory
wiring done at host load time (confy-tui `load_document`,
confy-tauri/web equivalent — see `apply_schema_text` dispatch note in
CHANGELOG "Host端佈線"). Hypothesis: the host's one-shot `SetHostNotice` for
the comment-advisory toast and the schema-fetch request are both dispatched
around document load, and one may be clobbering or delaying the other's
`Notice` slot (there is only one active per-host notice slot — see
`Notice`/`NoticeSource` in `session/notice.rs`), or the schema hint's
detection order runs before the JSONC-tolerant re-parse fix lands, so it
still reads the pre-fix cached `raw` doc text. Needs a repro test that opens
a JSON file with both a comment AND a `$schema` hint, and traces whether
`detect_and_request_schema` gets called / returns `Some` at all before the
first mutation, vs. after.

## 3. Add-new-node on a trailing-comment-bearing node inserts between value and its trailing comment

Repro: pick a node that has a trailing comment (`key: value # comment`,
JSON/YAML/TOML equivalent), then "Add" a new sibling node. Expected: the new
node should land after the trailing comment (i.e. after the whole existing
line incl. its comment) — trailing comment stays attached to its original
value. Actual: the new node is inserted between the value and its trailing
comment, which detaches the comment from its owner and turns it into an
independent standalone comment node.

Likely area: whichever per-format `insert`/`edit.rs`/`mutations.rs` cursor
resolution converts a "trailing-comment-bearing node" row index into an
insertion anchor point — e.g. `crates/confy-core/src/model/json/edit.rs`
`fn insert` (~L401), `crates/confy-core/src/model/toml/move_paste.rs::insert`
(~L36), `crates/confy-core/src/model/yaml/edit/block.rs::insert` (~L780).
Compare against `insert_node_below_a_comment` (TOML test, ~L1036) which
covers inserting after a *standalone* comment row correctly — the trailing
(same-line) comment case looks like it isn't given the same "skip past this
comment" treatment when computing the anchor/target index. Also check the
`Item::Node { trailing: Option<...> }` model (`model/value.rs`) — the anchor
resolution presumably needs to treat a node-with-trailing-comment as one
atomic unit for insertion-point purposes, the same way multi-line leading
comments are already treated as "ONE slot" per the JSON edit.rs comment at
L1697-1699 (`insert_after_multiline_comment_no_offset`).

## Suggested approach for the follow-up session

1. Write three failing regression tests first (one per issue) at the
   relevant seam identified above, confirm they reproduce.
2. Fix #3 first — it's a pure document-model/CST-edit bug, no host UI
   involved, likely the most isolated and safest fix.
3. For #1 and #2, trace the exact call sequence at document load and at
   post-mutation rebuild (add temporary `eprintln!`/tracing if needed) to
   nail down whether it's a Notice-slot collision, a stale-cache issue, or a
   missing recompute call — before writing the fix.
