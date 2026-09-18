# Evaluation of the four remaining Open follow-ups
Status: Resolved (2026-09-15)

Tip at evaluation time: `adfc3b1`.
Method: four read-only scouts (one row each), then first-hand verification of every
claim that would change a decision. Transcripts: `history://ConvertWarningsI18n`,
`history://RawCaretToCursor`, `history://RejectedBlockOffset`,
`history://TrailingCommentDoublePass`.

Rows evaluated: the four `## Open` rows of [`../plan/BACKLOG.md`](../plan/BACKLOG.md) as of
`adfc3b1` — the four `history://` measurements named above, one per row.
Outcome recorded in the backlog's `Done` and `Watching` sections and in the
[block-edit implementation audit](2026-09-15-block-edit-implementation-audit.md)'s
"Follow-on sweep" section; this record keeps the full measurement detail behind those.

---

## Verdict table

| Row | Row's premise holds? | Real effort | Verdict |
|---|---|---|---|
| Convert warnings bypass i18n | Yes (its *examples* are invented) | S | **Do it** |
| Q4 caret -> cursor | Yes | S-M | Do it after the above |
| Rejected Block offset | **No** - two of three backends have no offsets at all | M | **Move to Watching** |
| Trailing-comment double pass | **No** - 14x does not reproduce; no host takes the path | XS (close) | **Close as stale** |

---

## 1. Convert warnings bypass i18n - DO IT (S)

Row premise correct, its parenthetical examples are not. `convert.rs` never drops
comments and never merges duplicate keys. The real inventory is **13 static
warnings**, all argument-free:

- 3 schema-hint drops (`convert.rs:149`, `:156`, `:162`)
- 8 style normalizations (`convert.rs:326`-`:342`)
- 2 semantic-loss normalizations (`convert.rs:599`, `:602`)

So 13 `tr` keys, zero `tr_args`. No production host matches on the English text
(CLI `cli.rs:305`, TUI `overlay_convert.rs:47`, web `convert-dialog.ts:83` all just
bullet-print); **tests do** (`convert.rs:1415,1518,1525`,
`confy-tui/src/tui/tests.rs:575`, `tests/convert_cli.rs:118`) - that is the one
breakage surface.

`ConvertResult` is not on the FFI surface; only `ModeView::Convert(ConvertView)` is.
`model/` has no `Lang` and must not gain one.

**Chosen shape: structured enum in core, translated at projection.**
`enum ConvertWarning` with `catalog_key() -> &'static str`; `ConvertView.warnings`
stays `Vec<String>`, translated in `dispatch.rs` via `tr(self.lang, ...)`. This is
exactly how `Notice.text`, `Prompt.question`, `ActionItemView.label` and
`comment_advisory` already work, so **wasm wire churn = 0, web/VS Code churn = 0**.
Rejected: enum crossing the wire (breaks `SessionSnapshot`, duplicates translation
into TS).

Acceptance: `tests/convert_cli.rs` with `--lang zh-TW` on a `0xFF` source shows the
translated warning; `MESSAGES.md` §7.2 loses the "bypasses i18n" caveat.

## 2. Q4 caret -> cursor - DO IT, but not free (S-M)

Row premise correct. The existing `Session::path_at_offset`
(`session/inline_edit.rs:852`, private) is **not reusable**: its predicate is
`start >= offset` (first Node at or after the anchor), designed for post-splice
re-anchoring. A caret inside a Node's span (`10..20`, caret 15) misses and returns
the *next* Node. A real query needs containment + innermost descent.

Raw pane is one `<textarea id="rawEdit">` (`web/index.html:127`) in both states
(read-only view; write mode). TUI/touch/VS Code: **zero** work - TUI has no Raw pane
or breadcrumb, touch has no breadcrumb and a static `<pre>`, VS Code keeps Raw view
(write mode suppressed by R10/ADR 0014) and would inherit the web behavior.

Two hazards named by the scout and worth pinning in a design note before coding:
1. **Feedback loop** - `jumpSelectRawSpan` (path -> offset) has no re-entry guard
   today because nothing listens to selection. Adding offset -> path needs a latch
   plus debounce plus a cursor-identity short-circuit.
2. **UTF-8 vs UTF-16** - `web/text-offset.ts` has `byteToCodeUnit`; the inverse
   `codeUnitToByte` does not exist yet and must be written and spec'd.

Cost: new core query + `Session`/FFI export + `codeUnitToByte` + listener with
guards. Honest size is the top of S, arguably M.

## 3. Rejected Block offset - MOVE TO WATCHING (M, blocked upstream of itself)

Row premise **does not hold**. It assumed a document-space offset exists and only
needed `offset - start` arithmetic. Measured reality, per backend:

- **TOML**: offset exists upstream (`taplo::parser::Error { range: TextRange, .. }`)
  but `reparse_document` (`cst_edit/replace_delete.rs:51-55`) throws it away via
  `e.to_string()`.
- **JSON/JSONC**: `json/parse.rs:11` is `parse(&str) -> Result<GreenNode, String>`;
  the lexer emits `(SyntaxKind, String)` - **no offsets at all**.
- **YAML**: `yaml/parse.rs:23` identical - **no offsets at all**.

So the honest scope is "teach two hand-rolled lossless parsers to track spans", plus
a new `SessionSnapshot` field (cross-host), plus per-host caret placement - and the
error can legally land *outside* the spliced region (unclosed delimiter, or a
duplicate key 50 lines away), which needs its own `None` rule. A TOML-only fix buys
caret placement for one of three formats and books parity debt.

Against that: a Block is typically 1-10 lines and every host already keeps the user's
text (TUI re-spawn loop; web/touch modal stays up). Watching, with the parser-span
prerequisite recorded, is the right shelf.

## 4. Trailing-comment double pass - CLOSE AS STALE (XS)

Row premise **does not hold**, and I measured it rather than taking the scout's word.

Source facts: `edit_commit` splits the comment off (`split_value_comment`) before
building the fragment, so `scalar_fragment` emits `key = value` with no comment,
`replace_value` returns `None`, and `cst_edit/mod.rs:81-84` takes the `None => tree`
arm. `set_trailing_comment` therefore never runs on a plain inline value edit. No
host commits per keystroke either (TUI on `Enter`, web on `Enter`/`blur`).

Measured on 5000 sections, release, median of 9:

| route | TOML | YAML |
|---|---|---|
| A `Replace`, node has no comment | 227 ms | 61 ms |
| B `Replace`, node **has** a trailing comment | 227 ms | 59 ms |
| C `SetTrailingComment` | 615 ms | 59 ms |
| D `Replace` with a comment-bearing fragment | 664 ms | 59 ms |

B == A confirms the bypass first-hand. The **14x from the spec §1 does not
reproduce** - today it is 2.7-2.9x, and only on C/D, i.e. the user explicitly
changing a comment. YAML shows no penalty at all on any route.

One nuance neither the row nor the scout noticed: `YamlDocument::
replace_preserves_trailing_comment()` is `false` (`yaml/doc.rs:108`), so
`inline_edit.rs:449`'s `reassert` makes **every** YAML value edit on a commented Node
issue `SetTrailingComment`. Measured cost of that extra mutation: 59 ms on a 5000-key
document, i.e. the same as the edit itself. Not a defect.

Close the row citing this table. If it is ever reopened, the trigger is a measurement,
not the stale 14x.

---

## Recommended order

1. Convert warnings i18n (S, zero wire churn, closes a visible zh-TW gap).
2. Q4 caret -> cursor (S-M, needs a short design note for the loop guard +
   `codeUnitToByte`).
3. Close the trailing-comment row as stale with the measured table (XS).
4. Move the rejected-Block-offset row to Watching with the parser-span prerequisite.

3 and 4 are doc-only and can ride along with 1.
