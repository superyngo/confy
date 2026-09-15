# Block-edit implementation audit — the landed code against its design record
Status: Resolved (2026-09-15)

The Block-edit feature ([design record](../spec/2026-09-15-block-edit-whole-file-reparse-design.md),
[plan](../plan/2026-09-15-block-edit-whole-file-reparse-plan.md)) was audited immediately after
it shipped: spec section by spec section against the tree at `25d3f21`, by four read-only scouts
(spans/model, hosts + messages, tests + docs, dead code + drift) plus first-hand probes on the
real API and the real binary.

**Verdict.** Every capability the record specifies is present and works — the whole §1 defect
table is closed on all three backends. One **blocking performance defect** was introduced by a
§5 requirement's implementation; everything else was documentation drift, dead leftovers, and
test holes. All of it is fixed; this record is the frozen trace.

| # | Finding | Severity | Fixed in |
|---|---|---|---|
| A1 | Cursor re-anchor after a Block commit was quadratic — a 37 KB rename took 20.9 s | **Blocking** | `0175aab` |
| B1 | `MUTATIONS.md` said `replace_table_spans`' two checks were retired; the code still enforces them | Doc false | `78a49a4` |
| B2 | Four stale statements: `TUI.md` on `serialize_fragment`, `ARCHITECTURE.md` on two deleted fns, `WEBUI.md` on the touch Comment Apply, `BEHAVIOR_MATRIX.md` + `document.rs` on "external-edit wrap" | Doc false | `78a49a4` |
| B3 | The `ARCHITECTURE.md` module map omitted all four new span modules | Doc gap | `78a49a4` |
| B4 | The glossary called the Block buffer a "fragment" — the word its own **Block** entry forbids | Doc self-contradiction | `78a49a4` |
| C1 | `web/types.ts` kept a dead `ExternalEditKind::Comment` member the Rust enum cannot produce | Dead code | `78a49a4` |
| C2 | `multiline_edit_initial`'s non-empty-path branch was unreachable | Dead code | `78a49a4` |
| C3 | Test holes: scattered `[[a.b]]` groups, the comment-exclusion rule pinned for TOML only, the §8 parity matrix a 26-case sample | Coverage | `f566861` |
| C4 | `end_of(&Target::AotGroup)` returned `0`, which `single_span`'s `Err` arm would feed to `&full[start..0]` — unreachable, but a trap | Latent panic | `f566861` |
| D1 | A YAML comment block's extent stopped after its **first** line, so a multi-line `#` run lost its trailing blank count | Defect (found while fixing C3/C4) | `c724ea8` |

## A1 — the one blocking defect

Spec §5 requires the cursor to re-anchor on "the **first Node of the edited span**, resolved
from the span's start offset". `reanchor_cursor_after_block` did that by asking **every node in
the tree** for its span, and each `node_text_spans` call re-serialized and re-projected the
whole document: one commit = N serializes + N projections. It fired only when the pre-edit path
had vanished — i.e. exactly on the **key rename** the feature exists to enable.

Measured before the fix (release, TOML, mid-document leaf):

| sections | bytes | same-key commit | rename commit |
|---|---|---|---|
| 50 | 1.7 KB | 4.10 ms | 68 ms |
| 200 | 7.2 KB | 3.78 ms | 653 ms |
| 500 | 18 KB | 10.5 ms | **4.49 s** |
| 1000 | 37 KB | 26.1 ms | **20.87 s** |

2× size → 4.6× time. The same-key column isolates the cause: it takes the early return and
stays flat. The fix reads the projected `Node::text_range` the reparse already carries instead
of re-querying spans per node — one downward walk. After `0175aab`, the 1000-section rename is
**25.2 ms** (828×), pinned by a regression test in `tests/block_edit_parity.rs`.

## What the audit confirmed fulfilled

- §1's defect table, probed on the real API: leaf rename, leaf + sibling, branch rename, branch
  + sibling all **applied** on TOML/JSON/YAML (the record's table had "silently ignored",
  "silently dropped" and "rejected" in those cells); a comment block gaining a line applied;
  duplicate key, empty buffer and unparsable buffer all **rejected** with a `core.block.*`
  notice and a byte-identical document.
- §3 spans: EOL comment and trailing blank run included, a preceding standalone Comment
  excluded, flow spans trimmed with the separator outside, verbatim YAML indentation, and the
  reverse-cut multi-span splice order (`model/block_splice.rs`).
- §5 host contract: all three hosts keep the editor open on rejection and detect it via
  `doc_revision`; no host injects error text into the buffer; no `e` on the Root; YAML opaque
  nodes text-editable while structurally refused.
- §6: both notice keys `Warn` in `severity_of`, in the catalog test, in `MESSAGES.md` and in
  both i18n catalogs. §7: one `on_mutation_success(None, …)`.
- §9 step 4: every retired symbol is gone (`wrap_element`, `split_packaged_blank`,
  `last_group_path`, `blank_lines::split_trailing_run`, `blank_lines::blank_separated_groups`),
  and the transitional `HOST_PARITY.md` rows with them.
- No scope creep: all 51 files in `git diff 75ab9cc..25d3f21` trace to the record, the test
  cutover, or repo doc conduct.

## Corrections to the scouts' reports

Two scout claims were wrong and are recorded so the pattern is visible: the §5 re-anchor was
reported **untested** (it is pinned in `crates/confy-tui/src/tui/tests.rs` —
`e_on_a_leaf_can_rename_its_key` asserts the cursor lands on `renamed`; the scout grepped
`ApplyBlockText` in the TUI tests, but the TUI drives `app.edit_node()`), and C4's `AotGroup`
slice was reported as a **live** panic (`member_spans` intercepts AoT groups at every nesting
depth — probed on four shapes). A scout's grep is evidence of a grep, not of behavior.

## Follow-on sweep, same day

With the audit closed, the four remaining rows of the living backlog
([`../plan/2026-09-09-open-follow-ups.md`](../plan/2026-09-09-open-follow-ups.md)) were
evaluated the same way — four read-only scouts plus first-hand measurement. Two shipped
(convert-warning i18n `d9b7771`, the Raw caret → cursor inverse `3f2b4d1`), one closed as
**stale** (the trailing-comment double pass: the claimed 14× does not reproduce; the measured
numbers are in that row), one moved to **Watching** (a rejected Block's buffer-relative offset
needs spans the JSON/YAML parsers do not keep). Eleven orphaned i18n keys were deleted in
`adfc3b1`. The backlog's Open section is empty as of 2026-09-15.
