# Block edit via whole-file reparse — Design
Status: Draft

## Summary

Change what the multi-line editor (the web/touch pop-up, the TUI `$EDITOR` spawn) *means* on
save. Today the returned buffer is a **fragment bound to one node**, spliced back into the green
tree 1:1 — so it cannot rename the node's own key, cannot add a sibling, and cannot turn a
Comment into a live Node. After this change the buffer is **the text of the byte span(s) the
node owns**: it is spliced into the document *text* and committed through the whole-document
`Replace{path: []}` route that every backend already implements. Any edit that is legal for the
format is therefore legal in the buffer, and anything else is rejected atomically.

This is a capability increase, a code decrease, and a measured speed-up at the same time. The
reason all three land together: **every backend's `apply` already serializes + reparses +
re-validates the whole document on every single mutation** (`cst_edit/mod.rs:145-148`,
`json/edit/mod.rs:36-56`, `yaml/edit/mod.rs:144`). The per-node route pays that **plus**
`clone_for_update`, a full `walk`, and green-tree surgery. The whole-document route pays only
the half both share.

Vocabulary: this document uses **Node / Root / Branch / Leaf / Scalar / Comment** as
[`../reference/glossary.md`](../reference/glossary.md) defines them. One new term is introduced,
**Block** — the span-set a multi-line edit owns (§3) — and it gets its glossary entry in the
same commit as the implementation.

Evidence, probes and the full decision log are in
`docs/tmp/claude-scratch/block-edit-eval/` (`EVALUATION.md`, `DECISIONS.md`, three probe
binaries). That folder is scratch and gitignored; everything load-bearing is restated here.

## 1. Problem — measured, not assumed

Driving the hosts' own path (`multiline_edit_initial(path)` → user edits →
`Intent::ApplyReplace` / `ApplyEditComment`) on all three backends:

| buffer edit | node | TOML | JSON | YAML |
|---|---|---|---|---|
| change the key | leaf | **silently ignored, no notice** | applied | applied |
| add a sibling line | leaf | **silently dropped, no notice** | rejected | rejected |
| change the key | branch | applied (`[a]`→`[c]`) | — | applied |
| add a sibling section/map | branch | applied | — | rejected |
| add a child | branch | applied | applied | applied |
| comment block → +1 comment line | comment | applied | — | — |
| comment block → live entry | comment | rejected | — | — |

Three separate defects hide in that table.

1. **TOML silently discards.** `replace_value` locates `frag.descendants().find(VALUE)` and
   swaps only the VALUE's content (`cst_edit/replace_delete.rs:449-494`), so the fragment's key
   token and every surplus node are dropped without a notice. JSON and YAML have single-node
   guards (`json/edit/fragment.rs:9`, `yaml/edit/block.rs:393`); TOML has none — which makes
   [`../reference/MUTATIONS.md`](../reference/MUTATIONS.md):135's promise ("surplus text is
   rejected as `Fragment`, never silently dropped (F14)") **false for the TOML backend**.
2. **The three backends disagree** about the same gesture, and the divergence is nowhere
   documented — unlike every deliberate one, which
   [`../reference/HOST_PARITY.md`](../reference/HOST_PARITY.md) records.
3. **A trailing comment costs 14×.** When the returned fragment carries an EOL comment,
   `replace_value` reports it back (`replace_delete.rs:488-494`) and `apply` runs a **second**
   full pass — `set_trailing_comment`, a textual splice plus a whole-document reparse
   (`cst_edit/mod.rs:81`). Because `multiline_edit_initial` packages the node's trailing comment
   **into the buffer**, any node with an EOL comment takes that branch even when the user
   changed nothing.

Only (1) is a bug in the usual sense; (2) and (3) are consequences of the 1:1 fragment design.

## 2. Measurements

5000 sections, ~525 KB TOML, mid-document leaf, median of 5, `--release`
(`docs/tmp/claude-scratch/block-edit-eval/probe_trailing_comment.rs`):

| | fragment `k1 = 1` | fragment `k1 = 1  # note` |
|---|---|---|
| flat sections, leaf `Replace` | 289 ms | **4103 ms** |
| with `[secN.sub]` sub-sections | 454 ms | **5164 ms** |
| whole-document `Replace{path: []}` | 396 ms | 364 ms |

Scaling, same comparison without a trailing comment (`probe_timing.rs`):

| doc | (a) `Replace` @ node path | (b) splice + `Replace` @ `[]` | (c) `project()` (both pay) |
|---|---|---|---|
| 1.7 KB | 1.38 ms | **0.51 ms** | 0.27 ms |
| 18 KB | 8.86 ms | **2.63 ms** | 1.33 ms |
| 492 KB | 5031 ms¹ | **355 ms** | 57.6 ms |

¹ that row's fragment carried a trailing comment — the 14× branch above. The comment-free
figure is 289-454 ms.

Reference bench (`cargo bench -p confy-core --bench perf -- --nodes 5000`; 1.1 MB, 70,001
nodes): parse 13.0 ms, serialize 11.8 ms, `project()` 89.0 ms. Parsing is not the bottleneck;
projection is, and projection is unchanged by this design.

**Two hypotheses were tested and rejected**, and are recorded so they are not re-filed: the
5031 ms outlier is *not* the documented live-index quadratic (`MUTATIONS.md:97-128`) and *not*
sub-section nesting (nesting alone is worth 289 → 454 ms).

## 3. The model

A Node's **Block** is an ordered, non-overlapping set of byte spans in the document text.

**Span boundaries.** A span covers the Node's body, its **trailing EOL comment**, and its
**trailing blank-line run**. A *preceding* standalone Comment node is excluded — it is an
independent Node with its own row and its own `e` (pinned today by
`crates/confy-tui/src/tui/tests.rs:404`). Inside a flow collection the span is the Node's own
token range **trimmed of surrounding whitespace**, and the separating `,` is **outside** it: a
comma belongs to the container, not to any member. Trimming also keeps taplo's baked padding
(`[ 1 ]` ⇒ `VALUE "1 "`, `cst_edit/move_paste.rs:782`) out of the buffer, so
`detach_value_trailing_pad` has no role on this route.

**Multi-span Nodes** are in scope from v1: a scattered `[T/S]`, a `[T/D]` dotted table, a
scattered `[A/T]` group. Commit order is: delete the later spans in **reverse document order**
(so earlier offsets stay valid), then splice the buffer at the **first** span.

**The contract** (this is the sentence that goes into the reference docs):

> A multi-line edit is accepted if and only if its buffer parses, on its own, as a legal Node
> sequence at the edited Node's container level. Otherwise the whole commit is rejected and the
> document is untouched. Backends differ only where the format's grammar differs.

So a buffer may rename the Node's key, emit zero-plus-one… N Nodes, convert a Comment into live
content or the reverse — anything the format permits. An **empty buffer is rejected**: clearing
a buffer is more plausibly a slip than an intent, and deletion already has `d` with its own
confirmation and multi-select semantics. Rejecting it also keeps the dangling-separator case
(`{ , "b": 2 }`) unreachable, which is why §3's "commas stay outside the span" is safe.

YAML buffers are **verbatim, indentation included** — no dedent on open, no `reindent` on
commit. The alternative trades a byte-identity invariant that random testing can sweep for a
transformation that must be its own exact inverse across tabs, spaces and `- key: val` entries.

## 4. Architecture

The mechanism lives in the **session layer**; the `Mutation` set stays closed.

```
Session::apply_block_text(path, text)
  spans   = doc.node_text_spans(path)        // NEW: read-only, per backend
  doc_txt = doc.serialize()
  new_txt = splice(doc_txt, spans, text)     // shared, one implementation
  doc.apply(Mutation::Replace { path: [], fragment: new_txt })   // existing route
```

Backends gain exactly one read-only query:

```rust
fn node_text_spans(&self, path: &[Seg]) -> Vec<(usize, usize)>;   // empty = unsupported
```

Start offsets come free from rowan (`text_range().start()`); ends mostly exist already —
`extent_end_offset` (TOML `cst_edit/replace_delete.rs:826`, JSON `json/edit/mutations.rs:320`,
YAML `yaml/edit/resolve.rs:72`), `section_end_from`, `aot_group_span`, `aot_entry_end_from`,
`comment_block_range`, `table_member_spans`, `dotted_member_entries`,
`inline_member_entries`, plus `blank_lines::measure` for the trailing run. The genuinely new
work is (i) converting the TOML member-span child indices into byte ranges, (ii) the flow-level
shapes where `extent_end_offset` answers `Unsupported` and the range is the trimmed
`text_range()`, and (iii) YAML opaque nodes.

Everything else is inherited rather than written:

- **Atomicity** — the backends' `apply` is already commit-on-success.
- **Semantic rejection** — taplo DOM validation / the JSON duplicate-key scan / YAML's
  validator run on the reparsed text.
- **Undo/redo** — history already stores whole-document text snapshots
  (`session/session.rs:2192-2196`), so a Block commit is already exactly one undo step.
- **Dirty tracking, BOM, atomic file rename** — host-owned, untouched.

`replace_table_spans`' two bespoke checks ("every header stays inside the table's subtree", "the
block must start with a `[header]` line", `MUTATIONS.md:84-86`) are **retired**. They existed
because the old mechanism could only consolidate in place, so a header leaving the subtree was
necessarily an accident; with a Block it is an intent. Legality is whatever the reparse and the
DOM validation accept.

Deleted on this route: `wrap_element` and `split_packaged_blank` (`session/inline_edit.rs`).
`apply_packaged_blank` survives as an inert branch of `apply_replace`, which the *inline* editor
still uses.

## 5. Host contract

A new intent, additive:

```
Intent::ApplyBlockText { path, text }
```

`ApplyReplace` and `ApplyEditComment` stay exactly as they are — `ApplyReplace{path: []}` is
also the VS Code schema session's reparse channel (`editors/vscode/src/schemaSessionManager.ts:60`)
and has nothing to do with this design. Hosts migrate one at a time; dropping
`ApplyEditComment` is a later cleanup commit, not part of this one.

**On rejection the editor stays open, holding the user's text**, on every host — the behavior
web raw-write already has (`web/raw-write.spec.mjs:202`,
`web/touch-ext-apply.spec.mjs:141`). Losing a whole buffer of typing to a rejected commit is
data loss, which `wens-dev-principles ui` grades `MUST`. The TUI, whose `$EDITOR` is
synchronous, implements this by **re-spawning `$EDITOR` seeded with the returned text**. Errors
travel only on the notice channel and are **never** injected into the buffer; a comment line
prepended to the buffer would be saved back as content on the retry.

**Cursor re-anchor** after the reparse, in order: the pre-edit path if it still resolves → the
**first Node of the edited span**, resolved from the span's start offset → the first row. The
span-based fallback is what makes a key rename land on the renamed Node; a visible-row-index
fallback (wenv's rule) only coincides with it while the row count is unchanged.

**No `e` on Root.** Root deliberately has no visible row, and `E` carries a whole-document lock
(`guard_document_edit_locked`, R21/R24) that `e` must not inherit.

**YAML opaque spans become editable** through `e` (raw text, no confirmation prompt). This is a
policy change, not a new capability: the whole-file `E` route already rewrites them, because the
opaque guard is written `if !path.is_empty() && is_opaque(…)` (`yaml/edit/mod.rs:108`), and
opaque fencing is recomputed from scratch on every parse (`yaml/parse.rs:20-63`). The accepted
cost is that `read_only` narrows from "not writable" to "not **structurally** editable" (rename,
`K`, remark and paste-into stay refused), which must be restated in
[`../reference/glossary.md`](../reference/glossary.md):169-175 and
[`../reference/BEHAVIOR_MATRIX.md`](../reference/BEHAVIOR_MATRIX.md):214-215, and two tests
rewritten (`yaml/edit/tests.rs:795`, `crates/confy-core/tests/session_headless.rs:3746`).

## 6. Messages

Two new keys, both `Severity::Warn`:

| key | when |
|---|---|
| `core.block.invalid` | the buffer does not parse, or the result is semantically illegal |
| `core.block.empty` | the buffer is empty — points the user at `d` |

`Warn`, not `Error`, precisely because §5 keeps the buffer open: nothing the user typed is lost.
(`core.document.apply-failed` is `Error` because that route closes its surface.) Both keys are
registered in `severity_of` (`session/notice.rs:57`), in
`severity_of_covers_the_full_catalog_table`, in
[`../reference/MESSAGES.md`](../reference/MESSAGES.md) §2.2, and in both the `en` and `zh-TW`
tables. Per MESSAGES.md §5.5 any CLI test asserting this text pins `--lang en`.

## 7. Schema revalidation

`on_mutation_success` is called with `touched: None`, so the schema is **always** revalidated in
full. The one-path fast skip (`session/session.rs:2178-2188`, `schema/dirty_check.rs:118`) is
unsound here: a Block commit can add or rename arbitrary paths, so a violation introduced by a
newly written key would never be reported — and a soft constraint that is not shown is a feature
that does not exist. The cost is bounded: one full validation at the moment the editor closes,
never on a keystroke path.

## 8. Verification

A new data-driven `crates/confy-core/tests/block_edit_parity.rs` holds the matrix:

- **shapes** — leaf, branch section, scattered table, dotted table, array element, flow member,
  comment block, AoT entry, YAML opaque
- **formats** — TOML, JSON/JSONC, YAML
- **outcomes** — unchanged buffer ⇒ byte-identical; key rename ⇒ applied; +N siblings ⇒
  applied; Comment ↔ live ⇒ applied; duplicate key ⇒ rejected and document untouched; empty
  buffer ⇒ rejected

Plus a proptest (alongside `tests/roundtrip_proptest.rs`): random document, random Node, buffer
returned unmodified ⇒ the document is byte-identical. Under this design that invariant is a pure
string equality, which is exactly why it can be swept randomly — and it is the invariant a
mis-computed span breaks first.

Real-binary verification, per repo conduct: the TUI ships in the same commit as the core, so
every behavior above — including the re-spawn on rejection — is reproduced on the actual `confy`
binary, not only under `cargo test`.

## 9. Sequencing

| # | Lands | Contents |
|---|---|---|
| 0 | now, independent | TOML leaf `Replace` rejects a fragment whose key ≠ the path's key; `MUTATIONS.md:135` corrected. Deleted again by step 1. |
| 1 | core + TUI, one commit | `node_text_spans` ×3, `apply_block_text`, `Intent::ApplyBlockText`, the parity matrix + proptest, TUI migration incl. re-spawn |
| 2 | web + touch | pop-up / ext-sheet migrated to `ApplyBlockText` |
| 3 | VS Code | no-op (it only uses the empty path) |
| 4 | cleanup | remove `ApplyEditComment`, `wrap_element`, `split_packaged_blank`; drop the transitional `HOST_PARITY.md` rows |

Step 0 ships first on its own because "I changed the key and nothing happened" is a silent
data-loss users hit today, and a reference doc currently promises the opposite of what the code
does. Neither should wait for a multi-week change. Steps 1-3 leave transitional host
differences, which are recorded in `HOST_PARITY.md` **with their removal condition** — the file
exists for deliberate divergence, and a temporary one still has to be visible.

## 10. Rejected alternatives

**1:N fragment splice per backend** (what `~/repos/wenv` actually does — `apply_edited_value`
parses only the snippet and splices the resulting `Vec<Entry>` at the same index,
`wenv/src/tui/app.rs:2167-2204`). It would need every container shape in every backend — root,
section, inline table, array, flow map/seq, AoT entry — generalized to accept N nodes: roughly
3× the code and 3× the edge cases, and still slower, since it keeps `clone_for_update` + `walk`
+ green-tree surgery and then pays the same serialize/reparse. Its only advantage is
fragment-scoped error text. wenv can afford its version because it has no key-path identity, no
nesting, no semantic validation and a 6-line serializer (`wenv/src/tui/operations.rs:175-180`);
confy has all four.

**Widening the buffer's scope** (a leaf's `e` opening its whole parent scope). Rejected as a
separate axis: it breaks cursor identity and would open a 5,000-line buffer inside a large
section. It remains a pure addition on top of this design if ever wanted.

**Delete-on-empty-buffer** (wenv's rule). Rejected: it would drag comma surgery back into a
model whose whole point is that a splice is plain text.

**Keeping the 1:1 contract and only making the silent drops loud.** That code would be deleted
entirely by step 1, and it answers a user who wanted the edit to *succeed* with a better error
message. Only its smallest piece survives, as step 0.

## 11. Follow-ups this design does not cover

- Translating a rejected commit's document-space parse offset back into a buffer offset, so the
  host could place the caret on the offending line.
- Retiring `ApplyEditComment` (step 4) and the `Block` glossary entry's interaction with the
  `Remark` operation.
