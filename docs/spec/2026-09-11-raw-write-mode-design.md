# The Raw pane gains a write mode — whole-document editing in place, not in a popup
Status: Shipped (2026-09-14)

Note: Q1 was RS0's measurement; it is answered in the Evidence section below.

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
| R4 | **Apply** (`⌘/Ctrl+Enter`, or the pane's ✓ button) commits `ApplyReplace { path: [], text }` and **stays in write mode**; leaving write mode is a separate, explicit toggle *(Amended 2026-09-15: this is now the **keyboard's** rule only — `⌘↩` is the checkpoint you press while still typing. The band's 套用 button applies **and** exits, mirroring 取消's discard-and-exit; a failed Apply still stays put, which is what makes the rule safe.)* | A whole-file apply can fail (the buffer must parse *and* pass `validate_semantics`). Staying put means a failed apply is a notice, not lost work |
| R5 | **Failure is detected by `doc_revision` (settled 2026-09-14, replaces the `history_len` detector).** Core gains a monotonic `doc_revision: u64` on `SessionSnapshot`, incremented once in `on_mutation_success`. Unchanged after an Apply ⇒ nothing committed ⇒ keep the buffer verbatim and let core's notice speak; changed ⇒ re-seed the textarea from `session.serialize()` | `history_len` is **provably wrong twice**: `History::push` returns early when the new snapshot equals `current` (`state.rs:307-310`, test `push_of_identical_snapshot_is_not_an_undo_step`), so a successful no-change Apply looks like a failure; and `depth()` is `past.len()` under ring eviction at `MAX_HISTORY` plus a byte cap (`state.rs:317-321`), so at the cap a successful Apply also leaves it unchanged — and whole-file snapshots hit the byte cap early. A revision counter is a document-level fact every host and the `?diag=1` trace can use |
| R6 | **`⌘/Ctrl+S` in write mode = apply-if-dirty, then Save** (settled 2026-09-14): `if (dirty) { apply(); if (!committed) return; } save()`. A clean buffer saves directly, without an Apply | ⌘S must always mean "save what I see". The earlier "always Apply first, only save if it committed" wording combined with the broken detector to **refuse saving an unmodified buffer**; apply-if-dirty also avoids a pointless `doc_revision` bump and its diag noise |
| R7 | `Escape` in write mode exits to Raw **view** and sends `"Escape"` to peel core's pending edit — after a confirm prompt **iff** the buffer differs from the last committed serialization | Matches the modal's cancel path; the confirm exists because the payload is the whole file, not one value |
| R8 | While write mode is active, `renderRawOrTree()` **must not** write to the textarea (the render loop owns the `<pre>`, the user owns the textarea) | This is the clobber class (`wens-dev-principles ui 19`): the existing Raw pane is re-seeded from `session.serialize()` on *every* render, which would eat keystrokes |
| R9 | **TUI: no Raw view, no write mode.** `$EDITOR` *is* the TUI's raw editor, and the Action item keeps spawning it (`BEHAVIOR_MATRIX.md`'s inline-vs-`$EDITOR` boundary). One row in `HOST_PARITY.md`, not a ratatui multi-line editor | The TUI has no multi-line text editor and building one to duplicate `$EDITOR` is the opposite of minimal |
| R10 | **VS Code: no write mode.** The workbench's own text editor is the raw editor; the item and the pane's Edit button are suppressed under `VSHOST` | VS Code owns the buffer, dirty state and undo (ADR 0007 / `VSCODE.md`); a second editable text surface over the same `TextDocument` is two owners of one buffer |
| R11 | The pane never becomes editable while the **clipboard is armed** — `begin_external_edit_document` already returns early on `guard_clipboard_locked()` | ADR 0005 §5; no new rule, just don't fake the mode host-side |
| R12 | **Raw keeps the breadcrumb.** `ui.ts:518`'s `crumbsEl.classList.toggle("hidden", rawView)` is deleted: the crumbs row stays visible and live in both Raw states, driven by the same `snap.cursor` + `children(path)` it already uses — no Raw-specific rendering | The cursor still exists in Raw (the Session is unchanged by the view toggle); hiding the one widget that says *where you are* was the least defensible part of Raw view. Desktop/VS Code only — the touch host has no breadcrumb at all |
| R13 | **Raw-specific controls live at the right end of the crumbs row**, not in the header: the `檢視 \| 編輯` pair, **套用** (`⌘↩`). *(Amended 2026-09-14: the **存檔** button is removed — it duplicated the header's Save and was never a requirement; `⌘S` keeps its apply-then-save meaning. The three remaining controls are always rendered at the same width, with the inapplicable one `disabled`, so the band never changes geometry under the pointer. Amended again the same day: the `檢視 \| 編輯` pair collapses into ONE **toggle** whose label is the CURRENT state (`.active`/blue while editing, like the header's Tree/Raw button), and a **取消** control joins 套用 under the identical enable rule — commit the changes or discard them, the two halves of one decision; 取消 re-seeds the pane from the last-applied text and stays in write mode.)* *(Amended 2026-09-15: the band is **two** controls, and the toggle becomes an ACTION whose label is the effect of pressing it — 編輯 in Raw view (accent-filled), 套用 in Raw write (no fill, level with 取消). 套用 now commits **and** leaves write mode, 取消 discards **and** leaves it, so both are symmetric exits and both are enabled by the mode alone, not by a dirty buffer — a clean buffer still needs a way out. A failed Apply is the one case that stays (R4). 取消 asks no confirm; the press is the answer.)* They render only in Raw, and register as `toolbar-fold.ts` entries like every other foldable control | The header is global chrome (open/save/undo/theme); a mode-scoped control set next to the mode-scoped navigator reads as one band. `CHROME.md` owns the inventory, so it gets a Raw row |
| R14 | **Breadcrumb jump in Raw = select the node's source span.** A segment/mini-tree pick still sends `RevealPath`, and the host additionally resolves that path's `text_range` from `session.outline()` (already exported over wasm, `OutlineNode.text_range`, `ffi/src/lib.rs:175`) and sets the pane's `selectionStart`/`End` to it, scrolling the span's line a third of the pane down. *(Amended 2026-09-14: one code path, not two — the pane is a single `<textarea>` in both Raw states, so the DOM `Range`/`Selection` branch is gone; and the scroll is computed explicitly, because `setSelectionRange` does not scroll and the original `<pre>` was not even a scroll container, so as shipped neither state scrolled at all. And the span must come from a per-path core query — `Session::span_of`, added 2026-09-14 — not from `outline()`, which omits `Comment` nodes by design and so made a jump to a comment row a silent no-op)* | Reuses the existing outline transport and the existing Reveal intent; no new core query, and "跳選" is literally a text selection in both states |
| R15 | **`text_range` is UTF-8 bytes; JS offsets are UTF-16 code units.** The conversion is one shared helper (byte offset → code-unit offset over the serialized text), used by both branches of R14 and unit-tested against a CJK + emoji fixture | The silent-corruption trap: a document with any non-ASCII byte before the target makes a naive `slice(byteOffset)` land mid-character. Cheap to get right once, invisible when wrong |
| R16 | **The mapping is one-way only (path → span).** Moving the caret in write mode does **not** move the tree cursor or the breadcrumb. **Superseded 2026-09-15** — Q4 shipped: the inverse now exists (`Session::node_at_offset`) and the binding is two-way, with a latch/debounce/identity guard trio | The inverse (offset → path) has no core query today, and inventing a host-side one over raw text is exactly the kind of second source of truth this record exists to avoid. Recorded as Q4, not silently assumed |
| R17 | **Jump is gated on a clean buffer (settled 2026-09-11, was Q5).** The breadcrumb's *display* stays live in both Raw states. Its *jump* selects source text only while the buffer equals the last committed `serialize()`; while the buffer is **dirty**, a pick still sends `RevealPath` (tree cursor + expansion move) but moves no caret/selection, and the status line says so (`web.raw.jump-needs-apply`) | `text_range`s come from the last commit, so on a dirty buffer they point into text that may no longer exist there. Worse, the host cannot know whether the buffer even *parses* until an Apply is attempted — "dirty" is the only honest observable proxy. An offset that silently lands 40 lines off is a worse failure than a disabled jump with a reason |
| R18 | **Touch keeps the popup (settled 2026-09-11).** On touch, an empty-path external edit routes to the **existing** external-edit bottom sheet (`touch/app.ts:666`), unchanged — no Raw write mode, no crumbs band. R1's predicate is therefore desktop-only | Touch has no breadcrumb, and its Raw pane is a read-only `<pre>` inside a scroll container with no room for a control band; the sheet is already the touch host's multi-line text surface (values *and* comments), so the whole file is just its largest payload. This **removes** the RS3 slice rather than adding work |
| R19 | The touch sheet's commit path gains **one** empty-path rule: a failed Apply keeps the sheet open with the buffer intact (R4/R5's semantics), instead of closing like a per-node edit does | Without it the touch host silently discards the entire file's text on an unparsable Apply. This is the one line R18 costs, and the reason R18 is cheaper than a touch write mode rather than merely smaller |
| R20 | **A dedicated core entry point: `apply_document_text(text)`** — `Mutation::Replace { path: [], fragment: text }` + `on_mutation_success`, and nothing else. The empty path does **not** go through `apply_external_replace` | That function is per-node machinery: it calls `split_packaged_blank` (which consults `trailing_blank_anchor(&[])`, so a backend returning `Some` for the root would strip the file's trailing newlines and re-apply them as a `SetTrailingBlankLines` mutation), reads the root's trailing comment, and offers `wrap_element`. A whole-file buffer's trailing newlines *are* the file's end, not a node's blank run |
| R21 | **The empty-path pending edit is modal for mutations** (settled 2026-09-14): while it is open, core refuses mutating intents with a notice, exactly like `guard_clipboard_locked` does. Per-node external edits keep today's non-modal behavior; VS Code is exempt (R10 suppresses the mode there) | A pending external edit deliberately lives outside `Mode` (`session.rs:2087-2090`), so today a tree edit made while the buffer is open would be **silently overwritten by the whole file** on Apply. A per-node buffer can only clobber its own node; a whole-file buffer clobbers everything |
| R22 | **Vocabulary** (settled 2026-09-14, lands in `glossary.md` with slice RD): **Raw pane** is the surface, with two states **Raw view** and **Raw write**; the unapplied text the user holds is the **document buffer** (contrast core's *fragment*); **Apply** is "send the document buffer into the Session", distinct from core's **commit** (a mutation's atomic success) — one Apply can fail to commit. This record is reworded to use only these terms | The record had been mixing *Raw view*/*Raw pane*/*buffer*/*整檔* against core's *document-scoped*/*external edit*/*fragment* |
| R23 | An identity Apply that is **not** byte-identical is a **ship blocker** for that format (Q1), fixed in the backend before RS2, scoped to the identity case only | "Open the whole-file editor, change nothing, press Apply" is the likeliest first action; a byte change there violates the repo's byte-fidelity invariant. Not a licence to renormalize whole-file Replace in general |
| R24 | **R21's lock is passive, and it covers undo/redo** (settled 2026-09-14): no chrome is pre-disabled — a refused intent answers with a notice. But `Undo`/`Redo` are inside the lock, not outside it | The desktop user cannot see the tree while the document buffer is open (touch's sheet covers it), so a pre-emptive dimming has nothing to dim. Undo/redo are *not* harmless here: they swap the whole document text, which is precisely the stale-buffer overwrite R21 exists to prevent |
| R25 | **`doc_revision` and `history_len` coexist** (settled 2026-09-14), and `history_len`'s doc-comment gains an explicit warning: it is *not* a commit counter — identical snapshots are deduped and old entries are evicted by the undo cap; use `doc_revision` for commit facts. Replacing `history_len` with `can_undo`/`can_redo` is a follow-up, not this feature's business | Keeps the change additive. The trap that produced the broken R5 detector was the field's *name* reading like a commit count; a warning at the definition is where the next reader will be |
| R26 | **One new message key, `core.document.apply-failed`**, with the backend's error string as a `tr_args` argument — covering all three failure classes (fragment parse, `validate_semantics`, YAML multi-doc reject). **Amended by T2's F4 (2026-09-14): it is raised at `Severity::Error` regardless of the inner cause**, because a parse failure notices as `warn` today and a rejected whole buffer is not a "proceeded, with a caveat" event | The user needs "the whole file was not applied, because X". Three keys would overlap semantically; and the framing goes through `tr_args` rather than surfacing raw English, which is the mistake `MESSAGES.md` already records for convert warnings |
| R27 | **The host state becomes a three-way `rawState: "off" \| "view" \| "write"`**, replacing `rawView: boolean`; `body.raw-view`/`body.raw-write` derive from it in one place | A boolean pair (`rawView` + `rawWrite`) admits the impossible `off + write` state — exactly the incoherence that breeds R8's clobber class. Three lines in RS2, and the CSS classes gain a single source |
| R28 | **ADR 0014's claim is the wider one**: *whole-document editing is carried by each host's existing multi-line text surface — the desktop's Raw pane in write mode, touch's external-edit sheet, the TUI's `$EDITOR`, and VS Code's own editor — not by one uniform new surface.* `doc_revision` (R5) is **not** ADR material: it is an additive snapshot field, reversible, and lives here | The question a future reader actually asks is "why are these three different?", not "why isn't it a popup on the desktop". An ADR needs all three of hard-to-reverse, surprising, and a real trade-off; the field fails the first |
| R29 | **The jump selects the node's whole member, key included** (F5, 2026-09-14): `outline()`'s `text_range` spans `target = "needle"`, not `"needle"`; a value-only range does not exist in the wire contract. T8 promises row-text selection, not value selection | Discovered by measurement rather than assumed. Adding a value-only range would be new FFI surface for a cosmetic difference, and selecting the member is also the more useful target for "show me where this is" |

### Switching has to be seamless — the concrete rules

**Amended 2026-09-14 (post-ship, measured in a real browser).** The two-element design below
did not deliver a seamless switch and was replaced by a **single-element pane**: one
`<textarea id="rawEdit">`, `readonly` in Raw view and writable in Raw write, absolutely
positioned over the whole `.tree-wrap` box. What the shipped two-element version actually did:
the `<pre>` never scrolled (`.tree-wrap` owned the scroll, `overflow: visible` on the `<pre>`),
so the textarea's `min-height: 100%` gave write mode a *second*, 86px shorter scroll container
(539px against the `<pre>`'s 625px, the difference being the wrap's `padding-bottom: 80px` FAB
reserve), the "copy `scrollTop` across the swap" rule copied a value that was always 0 — so
entering write mode after reading to line 100 jumped back to the file head — and the text
shifted ~10px vertically. With one element there is nothing to copy and nothing to keep in
sync; R13's Save button was dropped in the same change and the band's three controls became
static and same-size (see `CHROME.md`).

| Rule | Implementation (2026-09-14) |
|---|---|
| Identical box | Not "two rule blocks kept in sync" but **one element**: `#rawEdit` carries the only metrics block (`padding: 8px 10px`, `font-size: 13px`, `line-height: 1.55`, `tab-size: 2`, `white-space: pre`, `resize: none`, transparent background), and `border-left` is 2px transparent in view mode so write mode's accent changes only the border color, never the text's position |
| One scroll container | `#rawEdit { position: absolute; inset: 0; overflow: auto }` + `body.raw-view .tree-wrap { overflow: hidden; padding: 0 }` — the pane owns the only scrollbar, at the full pane height, in both states |
| No scroll jump | Structural: the element that scrolls is the element that stays. The scroll is explicitly preserved only where a `.value` assignment would reset it (entering write mode, a committed Apply, and a view-mode re-seed — which is itself skipped when the text is unchanged) |
| No layout animation | Transition **only** `background-color`, `border-color`, `box-shadow` (~120 ms, compositor-cheap). Never height/padding/font-size |
| Caret continuity | Entering write mode seats the caret at offset 0 and restores the scroll around that call, so the reading position is exactly where view mode had it |
| Keyboard | `document.body`'s key delegation skips only a **writable** textarea, so the readonly view-mode pane (which the breadcrumb jump focuses to show its selection) never swallows a shortcut |

### State legibility — the cues

Four independent signals, so no single missed cue leaves the state ambiguous:

1. **The primary 編輯 → 套用 control** at the right end of the crumbs row (R13): in Raw view it
   is the accent-filled 編輯 button; in Raw write its label is 套用 and the accent moves to the
   pane itself (cue 2) — next to the navigator rather than buried in the header.
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
- **Q4 — caret → cursor (the inverse of R14).** **Settled 2026-09-11: not now; shipped 2026-09-15.** Ship one-way;
  the inverse needs a core `node_at_offset(offset) -> Path` query (the CST walk is
  straightforward, but it is new API surface) and no usage has asked for it. RS4 logs it as a
  row in `docs/plan/2026-09-09-open-follow-ups.md` so it is recorded rather than remembered. That row closed 2026-09-15: `Session::node_at_offset` (innermost containing Node) plus `codeUnitToByte` and a guarded `selectionchange`-equivalent listener; WEBUI.md §Tree | Raw view | Raw write owns the behavior.
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

### T2 — failure, the `history_len` detector, byte offsets (measured 2026-09-14)

Same harness, section *RS0/T2*, 18 further checks.

**T2.1 — a broken Apply never commits.** Five fixtures across the three backends —
TOML unbalanced `[server`, TOML duplicate key, JSON unbalanced brace, JSON duplicate key,
YAML multi-document — all leave `serialize()` byte-unchanged with `history_len === 0`, and
all raise a non-empty notice. R4's "a failed Apply is a notice, not lost work" holds at the
backend level.

**F4 (new finding, contradicts R26's assumption of one failure class).** The notice
**severity is not uniform**: a *parse* failure is `warn`, a *semantic* failure is `error`.

| Fixture | severity | text |
|---|---|---|
| TOML unbalanced | `warn` | `invalid TOML: expected "]" (7..8)` |
| TOML duplicate key | `error` | `error: key collision: port` |
| JSON unbalanced | `warn` | `invalid JSON: expected R_BRACE, found None` |
| JSON duplicate key | `error` | `error: key collision: a` |
| YAML multi-doc | `warn` | `invalid YAML: multi-document YAML is not supported` |

Consequence for R26: a whole-file Apply that **rejected the user's entire buffer** can
surface as a *warning*, which `MESSAGES.md`'s severity table reserves for "proceeded, with a
caveat". T4 must therefore wrap the whole-file failure in `core.document.apply-failed` at
**`Severity::Error` regardless of the inner cause**, carrying the backend text as the
`tr_args` argument — the inner severities stay as they are for every other call site. This is
a refinement of R26, not a new decision; recorded here because the flat "one key" wording did
not anticipate a severity mismatch.

**T2.2 — `history_len` is provably not a commit counter (the R5 failing-before evidence).**
Two cases, both measured:

- a no-change Apply commits and leaves `history_len === 0` (`History::push`'s dedup);
- at the undo cap, `history_len` is pinned at **200** across a further successful commit that
  demonstrably changed `serialize()`.

Both are exactly the false "Apply failed" readings R5 rejects. T3's `doc_revision` must move
in both.

**F5 (new finding, narrows R14/R15).** `outline()`'s `text_range` for a leaf spans the
**whole member** — for `target = "needle"` the range slices `target = "needle"`, not
`"needle"`. The value-only range does not exist in the wire contract; `key_text_range` slices
`target` exactly. So R15's jump selects **the node's row text**, and the design record's
"select that node's text" wording means the member, key included. T8 must not promise a
value-only selection.

**T2.3 — the byte→code-unit drift is real and quantified.** Fixture
`note = "設定檔 🎉 comment"\ntarget = "needle"\n`: the target member's byte offset runs **8
ahead** of its JS code-unit offset (3 CJK × 3 bytes = 9 bytes / 3 units, plus an astral emoji
at 4 bytes / 2 units → +6 +2 = 8). Slicing `serialize()` by the raw byte numbers in JS
returns the wrong substring, so T8's `byteToCodeUnit` helper has its failing-before case:
`drift === 8` on this fixture.
