# Evaluation — multi-line editor (popup / `$EDITOR`) save-parse model
Status: Resolved (2026-09-15)

Date evaluated: 2026-09-15. Produced
[`../spec/2026-09-15-block-edit-whole-file-reparse-design.md`](../spec/2026-09-15-block-edit-whole-file-reparse-design.md)
(Shipped) and the settled decisions in
[`2026-09-15-block-edit-design-decisions.md`](2026-09-15-block-edit-design-decisions.md).

Probe sources, now in
[`2026-09-15-block-edit-design-evaluation/`](2026-09-15-block-edit-design-evaluation/)
(run from a dir **outside** the workspace, e.g. `/tmp/confy_probe`, with `probe_Cargo.toml`
renamed to `Cargo.toml`):
- `probe_limits.rs` → `src/main.rs`: what the current commit path accepts/silently drops.
- `probe_timing.rs` → `src/bin/timing.rs`: node-splice vs whole-file-reparse commit cost.
- `probe_trailing_comment.rs`: the trailing-comment second-pass cost (§9).

---

## 1. Measured current behavior (`probe_limits.rs`)

Host round trip simulated exactly as the hosts do it: `multiline_edit_initial(path)` →
user edits → `Intent::ApplyReplace{path, text}` / `ApplyEditComment{path, text}`.

| Buffer edit | Node | TOML | JSON | YAML |
|---|---|---|---|---|
| change the key | leaf | **silently ignored, no notice** | applied | applied |
| add a sibling line | leaf | **silently dropped, no notice** | rejected (`unexpected ':' after document`) | rejected (`fragment must be a single value`) |
| change the key | branch | applied (`[a]`→`[c]`) | — | applied (`srv:`→`renamed:`) |
| add a sibling section/map | branch | applied (`[z]` landed) | — | rejected (`unconsumed INDENT`) |
| add a child | branch | applied | applied | applied |
| comment block → +1 comment line | comment | applied | — | — |
| comment block → live entry | comment | rejected (`comment lines must start with #`) | — | — |

Mechanism, not guesswork:
- TOML leaf: `replace_value` takes `frag.descendants().find(VALUE)` and splices **only the
  VALUE content**, so the fragment's key token and every surplus node are discarded
  (`crates/confy-core/src/model/cst_edit/replace_delete.rs:449-494`).
- JSON/YAML reject surplus (`json/edit/fragment.rs:9`, `yaml/edit/block.rs:393`), TOML has no
  such guard — `docs/reference/MUTATIONS.md:135`'s "surplus text is rejected as `Fragment`,
  never silently dropped (F14)" is **not true of the TOML backend**. Standalone defect.
- A branch is a whole-span splice (`replace_value`'s `Target::Header` arm, `:429-447`), which
  is why a header rename and a sibling section already work in TOML — the "one node in, one
  node out" rule only binds the *leaf/element* path. The three backends therefore disagree
  about the same gesture (HOST_PARITY-class divergence, undocumented).

## 2. Reference model (`~/repos/wenv`, scouted)

Not a whole-file reparse. `apply_edited_value` (`src/tui/app.rs:2167-2204`) parses **only the
edited snippet** and `replace_entry_with_parsed` (`src/tui/operations.rs:193-209`) splices the
resulting `Vec<Entry>` into `file.entries` at the same index: 0 entries = delete, 1 = replace,
N = replace + insert siblings. Save = concatenating `entry.value` verbatim
(`operations.rs:175-180`). Cursor/selection survive by **numeric visible index**, clamped
(`app.rs:1434-1437`). Nothing is ever rejected — unparsable text falls back to
`EntryType::Code` (`parser/bash/mod.rs:67`). Whole-file reparse happens only for an explicit
file-header edit (`run_edit_file`, `app.rs:2099-2127`).

So wenv's real rule is **1:N fragment splice**, affordable because it has no key-path identity,
no nesting, no semantic validation, and a 6-line serializer.

## 3. Measured cost of the two candidate mechanisms (`probe_timing.rs`)

Same document, same mid-document leaf, median of 7, `--release`.

| doc size | (a) today: `Replace` @ node path | (b) text-splice + `Replace` @ `[]` | (c) `project()` (both pay) |
|---|---|---|---|
| 1.7 KB / 20 sections | 1.38 ms | **0.51 ms** | 0.27 ms |
| 18 KB / 200 sections | 8.86 ms | **2.63 ms** | 1.33 ms |
| 492 KB / 5000 sections | 5031 ms | **355 ms** | 57.6 ms |

Whole-file reparse is **3.4×–14× cheaper** than the current node splice, at every size. Reason:
every backend's `apply` already serializes + reparses + re-validates the whole document on
*every* mutation (`cst_edit/mod.rs:145-148`, `json/edit/mod.rs:36-56`,
`yaml/edit/mod.rs:144`), so the node route pays that *plus* `clone_for_update` + a full `walk`
+ green-tree surgery. The whole-document route pays only the part both share.
Reference bench (`cargo bench -p confy-core --bench perf -- --nodes 5000`, 1.1 MB, 70,001
nodes): parse 13.0 ms, serialize 11.8 ms, `project()` 89.0 ms — parsing is not the bottleneck,
projection is, and projection is unchanged.

## 4. What already exists and is reusable

- Whole-document commit is a shipped, tested route in all three backends:
  `Replace{path: []}` → `Session::apply_document_text` (`session/inline_edit.rs:696-711`),
  used by TUI `E`, desktop Raw write mode, touch's external-edit sheet (ADR 0014), and the
  VS Code schema session (`editors/vscode/src/schemaSessionManager.ts:60`).
- Atomicity + semantic rejection are inherited for free (doc untouched on `Err`).
- Undo/redo already stores **full document text** snapshots (`on_mutation_success` →
  `history.push(text)`, `session/session.rs:2192-2196`), so a whole-file commit is already
  exactly one undo step.
- Byte-extent machinery exists: `extent_end_offset` (TOML `replace_delete.rs:826`, JSON
  `json/edit/mutations.rs`), `table_member_spans`, `aot_group_span`, `trailing_blank_anchor`.
  Missing piece is only the **start** offset (trivial: `node.text_range()`).
- Cursor re-anchor primitives exist: `cursor_row_index()` (`session.rs:456`) and the
  "snap to first row if the path vanished" fallback in `compute_rows` (`session.rs:425-427`).

Machinery that would become **dead** under the whole-file route (for the external editor
only): `apply_external_replace`'s trailing-comment force-clear, `split_packaged_blank` /
`apply_packaged_blank`, and the `wrap_element` re-wrap (`inline_edit.rs:713-854`).

## 5. Host surface to migrate

Small and enumerable: `Intent::{ApplyReplace, ApplyEditComment}` + `ExternalEditKind::{Value,
Comment}` (`session/view.rs:284-299`, `web/types.ts:245-252,367-369`). Commit sites:
`web/ui.ts:1417-1419`, `web/touch/app.ts:1104-1106`, `crates/confy-tui/src/tui/app.rs:700-760`.
VS Code only uses the empty-path form already.

## 6. Options

- **A. Document-text splice + whole-file reparse.** Buffer replaces the node's byte span in the
  document text; existing `Replace{path: []}` commits. Lifts all four limits at once, deletes
  four special-case mechanisms, measured faster. New code: `node_text_spans(path)` per backend
  + one session entry point + cursor re-anchor.
- **B. 1:N fragment splice at the node's slot** (wenv's actual mechanism). Generalize each
  backend's replace to accept N nodes, per container shape (root, section, inline table,
  array, flow map/seq, AoT entry). ~3× the code and edge cases, and still slower than A.
- **C. Keep 1:1; make the silent failures loud.** Reject a TOML key change / surplus node with
  a notice. Fixes the documented-vs-actual defect, adds no capability.

## 7. Open decisions (need the user's call)

1. Semantic conflict (duplicate key, TOML header capture): reject atomically + keep the buffer
   open with the user's text (what web raw-write already does — `raw-write.spec.mjs:202`)?
2. Cursor re-anchor: same path → same visible index → first row?
3. Empty buffer = delete the node (wenv's rule), or reject?
4. YAML opaque spans: still refuse, or allow editing their text now that a reparse re-fences?
5. Wire contract: add `Intent::ApplyBlockText` and leave the old intents (no breaking change)?

## 8. Recommendation

C now (real defect, small), then A. Reject B: it buys nothing A doesn't, and costs 3× more.

## 9. Follow-up measurement (2026-09-15, `probe_timing.rs` variant `t2.rs`)

The 5031 ms outlier in §3 is **not** the documented live-index quadratic and **not** nesting.
It is the fragment's **trailing comment**. When the returned fragment carries an EOL comment,
`replace_value` reports it back (`replace_delete.rs:488-494`) and `apply` runs a **second**
full pass — `set_trailing_comment`, a textual splice + whole-document reparse
(`cst_edit/mod.rs:81`, `replace_delete.rs:502-557`).

| 5000 sections, ~525 KB, leaf `Replace`, median of 5 | fragment `k1 = 1` | fragment `k1 = 1  # note` |
|---|---|---|
| flat sections | 289 ms | **4103 ms** (14.2×) |
| with `[secN.sub]` sub-sections | 454 ms | **5164 ms** (11.4×) |
| whole-document `Replace{path: []}` on the same doc | 396 ms | 364 ms |

Why it matters here: `multiline_edit_initial` packages the node's trailing comment **into the
buffer**, so an unchanged popup/`$EDITOR` commit on any node that has an EOL comment takes the
expensive branch. Corrected comparison for §3: the node route costs 289-454 ms without a
comment and 4.1-5.2 s with one; the whole-file route costs ~360-400 ms **either way** (it has
no second pass — the comment is just text).

So this is a separate, independently shippable perf defect of the *current* design, and option
A deletes it by construction rather than fixing it.
