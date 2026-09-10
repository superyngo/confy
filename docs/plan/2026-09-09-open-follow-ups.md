# Open engineering follow-ups
Status: In progress

The **single live backlog** for confy. Every recorded-but-unfixed item lives here with its
evidence, its verdict date, and an acceptance criterion — so no defect survives only inside a
frozen audit or a doc footnote.

Scope and lifecycle:

- This is a *working* record, and the one exception to the "frozen once it lands" rule that
  governs the rest of `docs/plan/`. Rows move to **Done** with the commit that closed them;
  the row is never deleted, so the history of what was open stays readable.
- A finding enters here only after being **verified against current code** — file and symbol
  named, never a line number. The source records
  ([2026-08-29 audit](../audit/2026-08-29-code-audit.md),
  [2026-09-09 re-verification](../audit/2026-09-09-open-findings-reverification.md),
  [2026-09-09 documentation audit](../audit/2026-09-09-documentation-audit.md)) stay frozen;
  this file is where their open items are tracked. `MESSAGES.md` §8 used to be a fourth
  source; its items are all in *Done* below and the section was removed from the reference
  doc on 2026-09-09, since `docs/reference/` carries current behavior only.
- When the last row reaches Done, this record's `Status:` becomes `Resolved (date)` and it
  joins the frozen set.

Effort is XS (< 1 h) / S (a session) / M (multi-session).

---

## Open

| Opened | Item | Evidence | Effort | Acceptance |
|---|---|---|---|---|
| 2026-09-09 | **Convert warnings bypass i18n.** `ConvertResult.warnings` is a `Vec<String>` of raw English ("comments will be dropped", "duplicate key merged"), rendered ad hoc by every convert surface — CLI stderr, the TUI's `overlay_convert`, the web convert dialog — instead of going through `tr`/`tr_args` like every other user-facing string. | `model/convert.rs` (`ConvertResult.warnings`); consumers `crates/confy-tui/src/tui/overlay_convert.rs`, `web/convert-dialog.ts`. Recorded in `MESSAGES.md` §7.2 as out-of-scope since the message-system work, never filed as a row. | S | Each warning is a catalog key with args; `MESSAGES.md` §7.2 drops the "bypasses i18n" caveat; a zh-TW convert shows translated warnings. |
| 2026-09-10 | **Web: collapsing the Root blanks the tree.** The cursor starts on the Root (no web host moves it at open), so `Space` → `toggleExpandSelected` → `ToggleExpand` collapses the file; core then returns exactly one row (the Root), which both web renderers filter out — an empty tree with no cursor, recoverable only via `9`. The TUI shows the single file row instead. | Measured through the wasm channel: 1 row returned, 0 drawable (`docs/spec/2026-09-10-web-root-row-alignment.md` E4). Sites: `web/render.ts` (`treeHTML` root filter), `web/touch/render.ts`, `web/ui.ts` (`toggleExpandSelected`), `crates/confy-tui/src/tui/tests.rs` (TUI oracle). | S | Unreachable in **both** modes of ADR 0013: root-visible collapses to the single file row (TUI parity), root-hidden has no Root row to collapse. No host guard, no placeholder — if a guard is needed, the mode is wrong. |
| 2026-09-10 | **Web: the `[G] root` type-filter facet blanks the tree.** Same cause, pointer-only path: `f` → click the `[G] root` cell filters to the Root alone, core returns one row, both web renderers drop it. | Measured through the wasm channel: 1 row returned, 0 drawable (design record E5). Facet defined in `session/type_filter.rs` (`TypeToken::Root`, all three format layouts). | S | Per ADR 0013: root-visible renders the facet's one row; root-hidden omits the `root` facet from the type-filter layout entirely. Separately, an empty document in root-hidden mode is a legal zero-row state and the host must render an empty state whose button dispatches `AddChild([])`. |
| 2026-09-10 | **Core: the action menu offers three impossible operations on the Root.** `action_menu_items` enables `Cut`, `Remark` and `Delete` whenever the single target is not read-only, so with the Root as target `Delete` reports "operation not supported here", `Remark` reports the misleading "path not found", and `Cut` **succeeds** — arming the clipboard with the Root, which can never paste (self-subtree reject), leaving the modal lock a dead end until `Esc`. Reachable in the TUI today (`m` with the cursor on the file row); drawing the web Root row would add a hover `⋮` to it. | Measured through the wasm channel (design record E7/E8). Sites: `session/action_menu.rs` (`action_menu_items`), `session/clipboard.rs` (`cut_selected`, `move_selection_to` self-subtree reject), backends' `Delete`/`Remark` root rejects. | S | With the Root as the single target, Cut/Remark/Delete are disabled items; no dispatch on the Root can arm the clipboard; a core test pins all three. |

---

## Watching (no action planned)

- **`taplo` is unmaintained upstream.** Recorded as a known risk in `CLAUDE.md` with the
  ~1,240-LOC vendoring scope, and `rust-ci.yml` has an active `cargo audit` step — the agreed
  trigger. Do not migrate pre-emptively; `tombi` is still not a usable dependency.
- **`JsonDocument` does not override `rename_key_segs`.** TOML and YAML both do, to decode a
  rename's literal with their own key lexer. Not a live defect — JSON keys are always quoted, so
  the trait default coincides — but it is the last asymmetry in that area, and it would become a
  defect if JSON ever grew unquoted keys.
- **The YAML subset lexer accepts unterminated scalars.** `x: "unclosed`, `x: [1, ` and
  `x: {a: ` all lex as plain scalars. This was filed as half of F14 and is **not** a defect:
  `AnyDocument::from_str_as` accepts the same text from disk, so `Replace` and load agree.
  Tightening one without the other would mean a value you can open but cannot retype. Only
  fix these together, and only if the subset parser is made strict on purpose. Asserted as
  the documented exception in `tests/format_parity.rs` (F8), so the day it changes, a test says so.
- **The two web palettes are not the same set.** `web/style.css` defines `--drop` (the
  drop-indicator green, also the desktop status line's Success hue); `web/touch/style.css`
  does **not** define it at all, so `var(--drop)` silently falls back to `currentColor` on
  touch — caught 2026-09-09 only because a computed-style check contradicted the screenshot.
  The touch severity toasts use `--t-string` for Success instead, with the reason in the CSS
  comment. A shared token file would remove the class of bug; not worth it for one token.

---

## Done

| Closed | Item | Commit |
|---|---|---|
| 2026-09-09 | Double serialize per mutation — `sync_schema_hint` now takes the text | `57630e4` |
| 2026-09-09 | TOML `Move` quadratic — live-index release in `move_nodes`/`delete`, −48% | `57630e4` |
| 2026-09-09 | JSON/JSONC parser-simplification plan — premise refuted, both halves already done | (record closed) |
| 2026-09-09 | **F1** Remark semantics unified — own-line rule, uniform `Unsupported`, array elements now remarkable in TOML+JSON | `72805c0` |
| 2026-09-09 | **F2** 3-format parity suite — `tests/format_parity.rs`, 9 behaviors, exhaustive-`match` fixtures | `72805c0` |
| 2026-09-09 | **F14** YAML `Replace` silently dropped everything past the first node — now `Fragment("fragment must be a single value")`; two of the four filed rows were misdiagnosed and moved to *Watching* | `08fc59c` |
| 2026-09-09 | **F3** TUI `~` overlay windowed the ring's head — now the tail, with a `last N of M` title and an empty-ring line | `0db6b79` |
| 2026-09-09 | **F4** Web `?diag=1` cursor now resets on session swap — the record's "8 replacement sites" was one (`openText`) | `ccb0999` |
| 2026-09-09 | **F5** Touch `sev-*` toasts styled — border/left-bar tint per severity, matching the desktop status hues | `4379723` |
| 2026-09-09 | **F6** `json/edit.rs` split — 8 production files (max 377 lines, was 1,774 in one) + sibling `tests.rs`; all 67 tests preserved | `739132e` |
| 2026-09-09 | **F7** Diag taps moved `dispatch()` → `apply()` — the TUI's `~` ring went 0 → 32 events on the same 16 keystrokes; the filed "~15-20 bypass sites" was 5, of which 4 now go through Intents (`ConvertWriteDone`, `SetStrictJson`, `SetPasteSlot`) | `8cc0ccb` |
| 2026-09-09 | **F8** `MutateError` taxonomy documented + enforced — Root delete `NotFound`→`Unsupported` (3 backends), JSON unterminated string `Illegal`→`Fragment` (lexer emits ERROR), 2 new parity tests | `8cc0ccb` |
| 2026-09-09 | **F15** `?diag=1` drain extracted to shared `web/diag.ts`; touch went 0 → 12 logged lines on 3 keystrokes, desktop unaffected | `5612849` |
| 2026-09-09 | **F13** `.cargo/config.toml` with `incremental = false` — `target/debug/incremental` was 23 GB / 120k files (47% of `target/`) for ~1.6 s per edit rebuild | `1959801` |
| 2026-09-09 | **F11** `jsonschema 0.30 → 0.55` (fields became methods) and `fuzzy-matcher 0.3 → nucleo-matcher 0.3` (unmaintained → maintained); `ureq 2` still deliberately deferred | `1959801` |
| 2026-09-09 | **F9** — premise refuted by measurement (undo is 15 ms at 1 MB; a green tree costs ~70× its text). Fixed the real axis instead: a 16 MiB byte cap beside the 200-entry cap, ADR 0003 amended | `88215e1` |
| 2026-09-09 | **F10** — three live-index rules, not one: spans from the index, section text off the green tree, `insert_with` owns and drops its index before the splice. `Move ×8` 2.27 s → 158 ms (−93%) | `5691819` |
| 2026-09-09 | **F12** — `CHANGELOG.md` split by series: root keeps `[Unreleased]` + v1.x (534 KB → 119 KB), v0.x archived verbatim under `docs/reference/changelog/` | `9f7c58e` |
