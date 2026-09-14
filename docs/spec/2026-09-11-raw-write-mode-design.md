# The Raw pane gains a write mode — whole-document editing in place, not in a popup
Status: Approved (2026-09-14) — Q1 is a measurement RS0 owns, not an open approval item

Sibling record: [`2026-09-11-root-hidden-alignment-design.md`](2026-09-11-root-hidden-alignment-design.md)
(its **P3** — "the web has no entry point for whole-document operations" — is what this record
closes; its Action-menu item D8/D9 becomes this feature's *quick entry*).
ADR to record with slice 1: `0014-whole-file-editing-is-the-raw-pane-in-write-mode.md`
(refines ADR 0013's D8 consequence, which assumed the external-edit **modal**).

Ships **before** the root-hidden work: it is independent of row shape, and it is the
user-visible half of P3.

Every file:line below was read on `main` at `db3f700`.

## What exists today

| Piece | Where | State |
|---|---|---|
| Raw view (desktop) | `web/index.html:108` `<pre id="raw" class="raw-view mono hidden" aria-readonly="true">`, `web/ui.ts:114-116,326-350` (`rawView` flag, `setRawView`, `renderRawOrTree`), `body.raw-view` chrome trimming (`web/style.css:873`, `WEBUI.md:200`) | **Read-only**, re-rendered from `session.serialize()` on every render |
| Raw view (touch) | `web/touch/app.ts:105,275,389-392,568-577` — same flag, same `<pre class="raw-view">`, same toggle button | Read-only |
| Raw view (TUI) | — | **Does not exist** |
| "Pop editor" | core `pending_external_edit` → `SessionSnapshot.external_edit` (`ExternalEdit { initial, kind }`, `session/view.rs:279-293`, produced by `external_edit_view`, `dispatch.rs:578-597`); desktop renders `#ext-modal` (`ui.ts:1108-1142`), touch a bottom sheet (`touch/app.ts:1065-1069`), the TUI spawns `$EDITOR` (`tui/app.rs:722-726`) | Commits via `ApplyReplace { path, text }` / `ApplyEditComment` |
| Whole-file edit in core | `begin_external_edit` on the empty path → `external_edit_path(&[])` returns `([], false)` (`session.rs:1738-1755`); `multiline_edit_initial(&[])` = `doc.serialize_fragment(&[])`; `apply_external_replace` already documents the empty path as "the whole-document edit" (`inline_edit.rs:726-733`) | **Already works**; only reachable by putting the cursor on the TUI's Root row |

So the mechanism is complete and shipped — three hosts already render a pending external edit
and commit it. What is missing is (a) an entry point that does not require the Root row, and
(b) a *surface* better than a modal for a payload that is the entire file.

## Decision

**The Raw pane is the whole-file editor.** A pending external edit whose path is empty is
rendered **in place of the Raw pane's `<pre>`**, in a `<textarea>` that occupies the same box —
not in `#ext-modal` / the touch sheet. Commit is the same `ApplyReplace { path: [], text }` both
hosts already send; nothing about the handshake changes.

Raw therefore has two states, **view** and **write**, and exactly one mechanism behind write:
core's external-edit handshake. There is no second buffer, no host-side document model, and no
new mutation.

### Decisions taken

| # | Decision | Rationale |
|---|---|---|
| R1 | **On the desktop host**, a pending `external_edit` with an **empty path** routes to the Raw pane's write mode; every non-empty path keeps the existing modal. Touch routes *every* path to its sheet (R18) | One predicate (`path.length === 0`), one place: the `openExternalEdit` call site in `ui.ts:543`. The popup keeps owning per-node fragments, where a full-pane editor would be absurd |
| R2 | Entering write mode **dispatches an Intent**, it does not flip a host flag: new `Intent::BeginEditDocument` (the cursor-independent sibling of `BeginEditExternal`, calling branch `d945721`'s `begin_external_edit_document`) | Keeps core the owner of "an edit is in flight" (so `Escape`, the modal lock, and the `?diag=1` trace all keep working) and gives the pane's own **Edit** button and the Action menu's *Edit whole file* the same single route |
| R3 | The Action-menu item (root-hidden record D8/D9, above `Delete`) is a **quick entry**: it switches the host to Raw + write mode in one step from anywhere, including from the tree | This is what makes it "編輯全檔" rather than "open a modal". On the TUI it keeps meaning `$EDITOR` (R9) |
| R4 | **Apply** (`⌘/Ctrl+Enter`, or the pane's ✓ button) commits `ApplyReplace { path: [], text }` and **stays in write mode**; leaving write mode is a separate, explicit toggle | A whole-file apply can fail (the buffer must parse *and* pass `validate_semantics`). Staying put means a failed apply is a notice, not lost work |
| R5 | **Failure is detected by `doc_revision` (settled 2026-09-14, replaces the `history_len` detector).** Core gains a monotonic `doc_revision: u64` on `SessionSnapshot`, incremented once in `on_mutation_success`. Unchanged after an Apply ⇒ nothing committed ⇒ keep the buffer verbatim and let core's notice speak; changed ⇒ re-seed the textarea from `session.serialize()` | `history_len` is **provably wrong twice**: `History::push` returns early when the new snapshot equals `current` (`state.rs:307-310`, test `push_of_identical_snapshot_is_not_an_undo_step`), so a successful no-change Apply looks like a failure; and `depth()` is `past.len()` under ring eviction at `MAX_HISTORY` plus a byte cap (`state.rs:317-321`), so at the cap a successful Apply also leaves it unchanged — and whole-file snapshots hit the byte cap early. A revision counter is a document-level fact every host and the `?diag=1` trace can use |
| R6 | **`⌘/Ctrl+S` in write mode = apply-if-dirty, then Save** (settled 2026-09-14): `if (dirty) { apply(); if (!committed) return; } save()`. A clean buffer saves directly, without an Apply | ⌘S must always mean "save what I see". The earlier "always Apply first, only save if it committed" wording combined with the broken detector to **refuse saving an unmodified buffer**; apply-if-dirty also avoids a pointless `doc_revision` bump and its diag noise |
| R7 | `Escape` in write mode exits to Raw **view** and sends `"Escape"` to peel core's pending edit — after a confirm prompt **iff** the buffer differs from the last committed serialization | Matches the modal's cancel path; the confirm exists because the payload is the whole file, not one value |
| R8 | While write mode is active, `renderRawOrTree()` **must not** write to the textarea (the render loop owns the `<pre>`, the user owns the textarea) | This is the clobber class (`wens-dev-principles ui 19`): the existing Raw pane is re-seeded from `session.serialize()` on *every* render, which would eat keystrokes |
| R9 | **TUI: no Raw view, no write mode.** `$EDITOR` *is* the TUI's raw editor, and the Action item keeps spawning it (`BEHAVIOR_MATRIX.md`'s inline-vs-`$EDITOR` boundary). One row in `HOST_PARITY.md`, not a ratatui multi-line editor | The TUI has no multi-line text editor and building one to duplicate `$EDITOR` is the opposite of minimal |
| R10 | **VS Code: no write mode.** The workbench's own text editor is the raw editor; the item and the pane's Edit button are suppressed under `VSHOST` | VS Code owns the buffer, dirty state and undo (ADR 0007 / `VSCODE.md`); a second editable text surface over the same `TextDocument` is two owners of one buffer |
| R11 | The pane never becomes editable while the **clipboard is armed** — `begin_external_edit_document` already returns early on `guard_clipboard_locked()` | ADR 0005 §5; no new rule, just don't fake the mode host-side |
| R12 | **Raw keeps the breadcrumb.** `ui.ts:518`'s `crumbsEl.classList.toggle("hidden", rawView)` is deleted: the crumbs row stays visible and live in both Raw states, driven by the same `snap.cursor` + `children(path)` it already uses — no Raw-specific rendering | The cursor still exists in Raw (the Session is unchanged by the view toggle); hiding the one widget that says *where you are* was the least defensible part of Raw view. Desktop/VS Code only — the touch host has no breadcrumb at all |
| R13 | **Raw-specific controls live at the right end of the crumbs row**, not in the header: the `檢視 \| 編輯` pair, **套用** (`⌘↩`), and **存檔**. They render only in Raw, and register as `toolbar-fold.ts` entries like every other foldable control | The header is global chrome (open/save/undo/theme); a mode-scoped control set next to the mode-scoped navigator reads as one band. `CHROME.md` owns the inventory, so it gets a Raw row |
| R14 | **Breadcrumb jump in Raw = select the node's source span.** A segment/mini-tree pick still sends `RevealPath`, and the host additionally resolves that path's `text_range` from `session.outline()` (already exported over wasm, `OutlineNode.text_range`, `ffi/src/lib.rs:175`) and (a) in **view** mode selects that span in the `<pre>`'s single text node via the DOM `Range`/`Selection` API, (b) in **write** mode sets the textarea's `selectionStart`/`End` to it — in both cases scrolling it into view | Reuses the existing outline transport and the existing Reveal intent; no new core query, and "跳選" is literally a text selection in both states |
| R15 | **`text_range` is UTF-8 bytes; JS offsets are UTF-16 code units.** The conversion is one shared helper (byte offset → code-unit offset over the serialized text), used by both branches of R14 and unit-tested against a CJK + emoji fixture | The silent-corruption trap: a document with any non-ASCII byte before the target makes a naive `slice(byteOffset)` land mid-character. Cheap to get right once, invisible when wrong |
| R16 | **The mapping is one-way only (path → span).** Moving the caret in write mode does **not** move the tree cursor or the breadcrumb | The inverse (offset → path) has no core query today, and inventing a host-side one over raw text is exactly the kind of second source of truth this record exists to avoid. Recorded as Q4, not silently assumed |
| R17 | **Jump is gated on a clean buffer (settled 2026-09-11, was Q5).** The breadcrumb's *display* stays live in both Raw states. Its *jump* selects source text only while the buffer equals the last committed `serialize()`; while the buffer is **dirty**, a pick still sends `RevealPath` (tree cursor + expansion move) but moves no caret/selection, and the status line says so (`web.raw.jump-needs-apply`) | `text_range`s come from the last commit, so on a dirty buffer they point into text that may no longer exist there. Worse, the host cannot know whether the buffer even *parses* until an Apply is attempted — "dirty" is the only honest observable proxy. An offset that silently lands 40 lines off is a worse failure than a disabled jump with a reason |
| R18 | **Touch keeps the popup (settled 2026-09-11).** On touch, an empty-path external edit routes to the **existing** external-edit bottom sheet (`touch/app.ts:666`), unchanged — no Raw write mode, no crumbs band. R1's predicate is therefore desktop-only | Touch has no breadcrumb, and its Raw pane is a read-only `<pre>` inside a scroll container with no room for a control band; the sheet is already the touch host's multi-line text surface (values *and* comments), so the whole file is just its largest payload. This **removes** the RS3 slice rather than adding work |
| R19 | The touch sheet's commit path gains **one** empty-path rule: a failed Apply keeps the sheet open with the buffer intact (R4/R5's semantics), instead of closing like a per-node edit does | Without it the touch host silently discards the entire file's text on an unparsable Apply. This is the one line R18 costs, and the reason R18 is cheaper than a touch write mode rather than merely smaller |
| R20 | **A dedicated core entry point: `apply_document_text(text)`** — `Mutation::Replace { path: [], fragment: text }` + `on_mutation_success`, and nothing else. The empty path does **not** go through `apply_external_replace` | That function is per-node machinery: it calls `split_packaged_blank` (which consults `trailing_blank_anchor(&[])`, so a backend returning `Some` for the root would strip the file's trailing newlines and re-apply them as a `SetTrailingBlankLines` mutation), reads the root's trailing comment, and offers `wrap_element`. A whole-file buffer's trailing newlines *are* the file's end, not a node's blank run |
| R21 | **The empty-path pending edit is modal for mutations** (settled 2026-09-14): while it is open, core refuses mutating intents with a notice, exactly like `guard_clipboard_locked` does. Per-node external edits keep today's non-modal behavior; VS Code is exempt (R10 suppresses the mode there) | A pending external edit deliberately lives outside `Mode` (`session.rs:2087-2090`), so today a tree edit made while the buffer is open would be **silently overwritten by the whole file** on Apply. A per-node buffer can only clobber its own node; a whole-file buffer clobbers everything |
| R22 | **Vocabulary** (settled 2026-09-14, lands in `glossary.md` with slice RD): **Raw pane** is the surface, with two states **Raw view** and **Raw write**; the unapplied text the user holds is the **document buffer** (contrast core's *fragment*); **Apply** is "send the document buffer into the Session", distinct from core's **commit** (a mutation's atomic success) — one Apply can fail to commit. This record is reworded to use only these terms | The record had been mixing *Raw view*/*Raw pane*/*buffer*/*整檔* against core's *document-scoped*/*external edit*/*fragment* |
| R23 | An identity Apply that is **not** byte-identical is a **ship blocker** for that format (Q1), fixed in the backend before RS2, scoped to the identity case only | "Open the whole-file editor, change nothing, press Apply" is the likeliest first action; a byte change there violates the repo's byte-fidelity invariant. Not a licence to renormalize whole-file Replace in general |
| R24 | **R21's lock is passive, and it covers undo/redo** (settled 2026-09-14): no chrome is pre-disabled — a refused intent answers with a notice. But `Undo`/`Redo` are inside the lock, not outside it | The desktop user cannot see the tree while the document buffer is open (touch's sheet covers it), so a pre-emptive dimming has nothing to dim. Undo/redo are *not* harmless here: they swap the whole document text, which is precisely the stale-buffer overwrite R21 exists to prevent |
| R25 | **`doc_revision` and `history_len` coexist** (settled 2026-09-14), and `history_len`'s doc-comment gains an explicit warning: it is *not* a commit counter — identical snapshots are deduped and old entries are evicted by the undo cap; use `doc_revision` for commit facts. Replacing `history_len` with `can_undo`/`can_redo` is a follow-up, not this feature's business | Keeps the change additive. The trap that produced the broken R5 detector was the field's *name* reading like a commit count; a warning at the definition is where the next reader will be |
| R26 | **One new message key, `core.document.apply-failed`**, with the backend's error string as a `tr_args` argument — covering all three failure classes (fragment parse, `validate_semantics`, YAML multi-doc reject) | The user needs "the whole file was not applied, because X". Three keys would overlap semantically; and the framing goes through `tr_args` rather than surfacing raw English, which is the mistake `MESSAGES.md` already records for convert warnings |
| R27 | **The host state becomes a three-way `rawState: "off" \| "view" \| "write"`**, replacing `rawView: boolean`; `body.raw-view`/`body.raw-write` derive from it in one place | A boolean pair (`rawView` + `rawWrite`) admits the impossible `off + write` state — exactly the incoherence that breeds R8's clobber class. Three lines in RS2, and the CSS classes gain a single source |
| R28 | **ADR 0014's claim is the wider one**: *whole-document editing is carried by each host's existing multi-line text surface — the desktop's Raw pane in write mode, touch's external-edit sheet, the TUI's `$EDITOR`, and VS Code's own editor — not by one uniform new surface.* `doc_revision` (R5) is **not** ADR material: it is an additive snapshot field, reversible, and lives here | The question a future reader actually asks is "why are these three different?", not "why isn't it a popup on the desktop". An ADR needs all three of hard-to-reverse, surprising, and a real trade-off; the field fails the first |

### Switching has to be seamless — the concrete rules

`<pre>` → `<textarea>` is the one place a naive implementation jumps: different UA defaults for
font, line-height, padding, and box-sizing, plus a lost scroll position.

| Rule | Implementation |
|---|---|
| Identical box | `#rawEdit` (the textarea) inherits the exact `#raw.raw-view` metrics — `padding: 8px 10px`, `font-size: 13px`, `line-height: 1.55`, `tab-size: 2`, `white-space: pre`, `font-family: inherit` from `.mono`, `border: 0`, `resize: none`, `background: transparent` — declared as **one shared selector list** (`#raw.raw-view, #rawEdit`), never two drifting rule blocks |
| No scroll jump | Copy `scrollTop`/`scrollLeft` across the swap in both directions; focus the textarea **after** the copy |
| No layout animation | Transition **only** `background-color`, `border-color`, `box-shadow` (~120 ms, compositor-cheap). Never height/padding/font-size |
| Caret continuity | Entering write mode places the caret at offset 0 and does not scroll; the user's reading position stays where the `<pre>` had it |

### State legibility — the cues

Four independent signals, so no single missed cue leaves the state ambiguous:

1. **The `檢視 | 編輯` segmented pair** at the right end of the crumbs row (R13), `.active` on
   the live state — the affordance and the state indicator are the same control, next to the
   navigator rather than buried in the header.
2. **Pane accent.** Write mode adds `body.raw-write`: a 2 px accent left border on the pane, a
   faint `--accent`-tinted background wash, and a visible focus ring. View mode has neither.
3. **Footer hint line** (the existing status/footer, translated): `⌘↩ 套用 · ⌘S 套用並存檔 ·
   Esc 離開編輯`. It appears only in write mode, so its presence *is* a cue.
4. **Chrome trimming.** `body.raw-view` keeps hiding the FAB and making the filter row and
   paste cues inert; it no longer hides the breadcrumb (R12).

The dirty dot is untouched — it means "the document differs from disk", and an un-applied
buffer is not the document. That distinction is exactly why cue 3 names `⌘↩`.

### Questions (Q1 narrowed to one measurement; Q2–Q6 settled)

- **Q1 — is an identity Apply byte-identical, per format?** The *route* is no longer in
  question: all three backends already return `self.serialize()` for `serialize_fragment(&[])`
  (`cst_doc.rs:50-53`, `json/doc.rs:39-41`, `yaml/doc.rs:34-36`) and already implement an
  empty-path `Replace` — TOML `reparse_document` (`cst_edit/mod.rs:77-78`), JSON's new-ROOT-
  children splice (`json/edit/replace_delete.rs:6-14`), YAML's whole-document replace with a
  multi-doc reject (`yaml/edit/block.rs:30-38`). What remains measurable is whether applying an
  **untouched** document buffer round-trips byte-identically, and it must be measured **per
  format** (TOML / JSON / JSONC-with-comments / YAML), because JSON's child splice and YAML's
  path are not identity functions by construction. A non-identical format is a ship blocker
  (R23).
- **Q2 — write mode when the document failed to parse.** **Settled 2026-09-14: out of scope.**
  A parse failure is a host-level error state (`staleTree`, `web/ui.ts:148-151`) with no
  `Session` to serialize, so this feature has no text source there. "Repair an unparsable file
  as text" is a different feature over a different source of truth (the bytes on disk) and
  needs its own record.
- **Q3 — schema validation on Apply.** **Settled 2026-09-14: behavior unchanged.** A whole-file
  Replace re-validates the document and a violation stays a **warning, not a rejection**
  (`2026-08-10-json-schema-support-design.md`); the Apply commits whenever the buffer parses and
  passes `validate_semantics`, and the existing notice speaks. No schema special case on the
  whole-file path.
- **Q4 — caret → cursor (the inverse of R14).** **Settled 2026-09-11: not now.** Ship one-way;
  the inverse needs a core `node_at_offset(offset) -> Path` query (the CST walk is
  straightforward, but it is new API surface) and no usage has asked for it. RS4 logs it as a
  row in `docs/plan/2026-09-09-open-follow-ups.md` so it is recorded rather than remembered.
- **Q5 — breadcrumb liveness in write mode.** **Settled 2026-09-11 as R17:** display live,
  jump gated on a clean buffer.

## Slices

**RD — documentation (this record + ADR 0014).** No product code.
*Acceptance:* ADR 0014 recorded and indexed; this record `Approved`; a plan under `docs/plan/`
enumerating RS0–RS4.

**RS0 — evidence.** Through the real wasm channel (`crates/confy-ffi/functional_smoke.mjs`):
(1) **Q1's identity Apply, once per format** — TOML, JSON, JSONC with comments, YAML: open the
document buffer, Apply it untouched, compare `serialize()` byte-for-byte with the pre-Apply
text; (2) Apply of deliberately broken text → no commit, Error notice, document unchanged;
(3) `doc_revision` (R5) moves on a *no-change* successful Apply and at the undo cap, where
`history_len` does not; (4) R14/R15 — for a fixture with CJK **and** an emoji before the target,
`outline()`'s byte `text_range`, after the conversion helper, selects exactly that node's text
in `serialize()`.
*Acceptance:* all four recorded in this record's Evidence section; Q1 answered per format, and
the byte→code-unit helper has a failing-before/passing-after case to test against.

**RS1 — core.** `Intent::BeginEditDocument` + `begin_external_edit_document` (cherry-picked
from branch `root-row-alignment` `d945721`) + `ActionId::EditDocument` wired to it,
`apply_document_text` (R20), `doc_revision` on `SessionSnapshot` (R5) with `history_len`'s
warning comment (R25), the empty-path mutation guard including undo/redo (R21/R24), and the
`core.document.apply-failed` key in both catalogs (R26).
*Acceptance:* `cargo test -p confy-core`; headless tests for the intent, the armed-clipboard
refusal (R11), an Apply at the empty path committing exactly once, a no-change Apply still
bumping `doc_revision`, and both a mutating intent **and** `Undo` refused while the document
buffer is open (R21/R24).

**RS2 — desktop web.** R1 routing, the three-way `rawState` (R27), the `#rawEdit` textarea,
R4–R8 behavior, the cue set, the crumbs-row control band (R13, registered in
`toolbar-fold.ts`), R12's un-hiding, R14/R15's jump (one shared `byteToCodeUnit` helper + the
two selection branches), i18n keys in `i18n/en.json` + `i18n/zh-TW.json`.
*Acceptance:* `npm run typecheck`, `npm test` (a new `raw-write.spec.mjs` covering: render never
clobbers the textarea; Apply failure keeps the buffer; `⌘S` does not save when the Apply
failed; scroll position survives both swaps; `byteToCodeUnit` on the CJK+emoji fixture; a
breadcrumb pick selects the right span in each of the two states; a pick on a **dirty** buffer
moves the cursor but not the caret, and reports (R17)); manual pass in a real browser for the
switch being jump-free.

**RS3 — touch (R18/R19).** No write mode and no new surface: only the sheet's empty-path
failed-Apply rule (keep it open, keep the buffer) plus the Action item reaching that sheet.
*Acceptance:* `npm test` (touch spec: an unparsable whole-file Apply leaves the sheet open with
the text intact); a real-device/emulator pass on the Action-item → sheet route.

**RS4 — VS Code + docs.** R10 suppression; `WEBUI.md` (Raw pane section + the Raw breadcrumb
and jump), `CHROME.md` (the crumbs-row Raw control band), `KEYMAP.md` (`⌘↩`, `⌘S`, `Esc` in
Raw write), `HOST_PARITY.md` (R9/R10/R12/R18 rows), `MESSAGES.md` for
`core.document.apply-failed` + `web.raw.jump-needs-apply`, `glossary.md` (R22's five terms),
`CHANGELOG.md`; root-hidden record's P3 row and the new Q4 row in
`docs/plan/2026-09-09-open-follow-ups.md`.
*Acceptance:* `npm test` + `node functional_smoke.mjs`; no reference doc still calls Raw
read-only.

## Evidence

### Q1 — identity Apply, per format (T1, measured 2026-09-14)

**Answer: yes, on all four formats. R23 is not triggered; no backend fix is needed before
RS2.** Measured through the real wasm channel (`crates/confy-ffi/functional_smoke.mjs`,
section *RS0/T1*, 18 new checks, `node functional_smoke.mjs` → `ALL FUNCTIONAL CHECKS
PASSED`), driving `ApplyReplace { path: [], text }` — the route `Intent::BeginEditDocument`
will reuse.

| Fixture | `serialize()` == input | Identity Apply byte-identical | Same route commits a real edit |
|---|---|---|---|
| TOML (leading comment, blank line, trailing comment, `[[tasks]]`) | yes | yes | yes |
| JSON (nested object + array, multiline) | yes | yes | yes |
| JSONC (leading + inner + trailing `//` comments, inline object) | yes | yes | yes |
| YAML (leading comment, trailing comment, block sequence) | yes | yes | yes |
| TOML with **no final newline** | yes | yes | yes |
| YAML with **CRLF** line endings | yes | yes | yes |

Three notes the measurement settled beyond Q1's wording:

1. **The no-final-newline and CRLF edge cases hold too.** These were the two most likely
   normalization leaks (the trailing-newline question F3 raised, and line-ending rewriting);
   neither occurs.
2. **Each row's third column is a false-positive guard.** An identity Apply that silently
   did nothing also looks byte-identical, so every fixture additionally applies a *modified*
   whole-file text and asserts both the new bytes and `history_len === 1`. The route is live,
   so column 2 is a real identity result and not a no-op.
3. **This measurement ran through `apply_external_replace`** (the current `ApplyReplace`
   wiring), and was still byte-identical — so F3's per-node blank/trailing-comment packaging
   happens to be inert at the empty path *today*. R20's dedicated `apply_document_text` stays
   as decided: inertness by coincidence at one call site is not a contract, and T4 must not
   depend on it.
