# Action menu — centralized node operations across desktop, touch, and TUI

Date: 2026-08-30
Status: approved design, pending implementation plan
ADR: [`../../adr/0009-centralized-action-menu-core-owned.md`](../../adr/0009-centralized-action-menu-core-owned.md)

Revised after a grilling pass that corrected two factual errors in the first draft (§2's
Append-comment claim and §5's desktop reachability claim) and dropped three items from the
item model. Where a claim below cites a file:line, it was verified rather than assumed.

## Problem

Node operations are reachable through **five** unrelated surfaces that disagree with each
other:

- Desktop rows carry a per-row `⋮` button (`web/render.ts:201`) opening a 10-item menu
  (`buildCtxMenu`, `web/ui.ts:1641-1678`) whose labels are **hardcoded English with no
  i18n keys**.
- Touch rows have no menu at all (`web/touch/render.ts:87-136`) — only a grip plus
  swipe-left delete / swipe-right remark. With a multi-node **Locked selection**, touch
  has no way to act on the selection.
- The web detail panel repeats four of those operations as buttons
  (`web/panel.ts:197-204`), which the TUI's detail popup does not have.
- The floating `+` (`web/fab.ts`) performs a context-aware add, and overlaps the status
  bar because `.fab` is `position:fixed; bottom:18px` (`web/style.css:822`) while
  `.footer` occupies the same corner.
- The Tauri **native Edit menu** (`web/menu.ts:333-347`) holds *Copy Node* / *Cut Node* /
  *Paste Node*, dispatching `CopySelected` / `CutSelected` / `Paste` with no eligibility
  at all.

The TUI has no menu surface; every operation is a single keystroke.

Consequence: the same logical operation is implemented three times with three different
eligibility rules, and neither touch multi-selection nor TUI menu discovery is served.

## Solution

One **Action menu**, its item model owned by `confy-core`, rendered three ways. Node rows
keep only a move grip on both web surfaces. The detail panel becomes editing-and-
information only. The TUI gains an equivalent overlay on `m`.

### Decisions taken

| Decision | Choice | Rationale |
|---|---|---|
| Item model ownership | Core-owned items **and** open state (`ModeView` variant) | One eligibility computation, one i18n source, and the web menus gain arrow-key navigation they have never had |
| Terminology | Overflow menu / Action menu / Action button | Distinguishes the RWD-folded toolbar menu from the new node-operations surface |
| Membership | Single core intent over the target set, unless the node already has a dedicated always-visible control | Makes the item list derived rather than inherited from `buildCtxMenu` |
| Desktop right-click | Kept, opens the same Action menu at the pointer | Desktop-idiomatic, already documented in `web/help-content.ts`, near-zero extra code |
| Desktop keyboard | `m`, same as the TUI | The menu is otherwise the one surface a desktop keyboard user cannot reach |
| Button position | Non-scrolling wrapper around the tree scroller | Magic-number-free and resize-aware (ui-design-principles §19) |
| Detail panel | All four action buttons removed; every editing affordance kept | Anatomy parity with the TUI detail popup |
| Native Edit menu | Exempt | OS convention, reached by muscle memory not discovery (ADR 0009) |

## §1 Terminology

Four entries added to `docs/reference/CONTEXT.md`'s Language section (**Overflow menu**,
**Action menu**, **Action button**, **Native menu bar**), plus a line on the **Remark**
entry recording that its user-facing label is "Toggle comment" on every host. Already
written.

Code hygiene that follows: today `data-act="menu"` means *overflow menu* on touch
(`web/touch/app.ts:247`) and *per-row node menu* on desktop (`web/ui.ts:1233`) — the exact
collision this terminology resolves. Touch's becomes `data-act="overflow"`; the Action
button is `data-act="actions"`.

## §2 Core model

New view type and `ModeView` variant beside `KindSwitch`, following its shape
(`crates/confy-core/src/session/view.rs:143`):

```rust
pub enum ActionId {
    Edit, AddChild, AddSibling, Copy, Cut, Remark, Detail, Delete,
}

pub struct ActionItemView {
    pub id: ActionId,
    /// Localized core-side via `tr(self.lang, "core.action.*")`, exactly as
    /// `ModeView::Prompt.question` is — hosts never reconstruct label prose.
    pub label: String,
    pub enabled: bool,
    /// Render a separator above this item. `Delete` only.
    pub separator_before: bool,
    /// `Delete` only — hosts render it as destructive.
    pub danger: bool,
}

ModeView::ActionMenu {
    cursor: usize,
    items: Vec<ActionItemView>,
    /// `selected_paths().len()`.
    target_count: usize,
    /// The node's key when `target_count == 1`, else the localized
    /// "N nodes" — hosts render verbatim, never recompute.
    target_label: String,
}
```

`Session::action_menu()` builds the list from `selected_paths()`
(`crates/confy-core/src/session/session.rs:1502`), the existing universal
Locked-selection-else-Cursor resolver — which returns the cursor as a one-element set when
the selection is empty. Targeting therefore needs **no new logic**: the desktop
right-click path keeps calling today's `selectForMenu` (`web/ui.ts:2062`) to retarget
first, then opens the menu.

`target_label` exists because detaching the trigger from the row is this change's one real
ergonomic loss: the menu no longer opens *at* the node it acts on, so it must name it.

The web wire types mirror this by hand: `web/types.ts` gains the `ActionMenu` arm on its
`ModeView` union (`types.ts:161`) plus an `ActionItemView` interface, and the five new
intents on its `Intent` union. No new FFI entry point is needed — the menu rides the
existing `SessionSnapshot.mode` projection, exactly as `KindSwitch` does.

### Items

Eight, derived from the membership rule (§1 / ADR 0009), in this order:

1. Edit in editor
2. Add child
3. Append sibling
4. Copy
5. Cut
6. Toggle comment
7. Detail
8. — separator — Delete

**Not included, and why:**

- **Paste** — legal only while **Clipboard-armed**, and `OpenActionMenu` is refused in
  that state, so it could never be reached. Today's `⋮` Paste entry (`ui.ts:1641`, item 6)
  is *already* dead code: both triggers bail out while armed (`ui.ts:1234`, `ui.ts:2029`)
  and `.paste-mode .row-actions` hides the `⋮` outright.
- **Kind switch** — the node already carries a dedicated always-visible control: the kind
  badge (`render.ts:90-94`), routed on desktop at `ui.ts:1254` and present in both
  panels as `.kindbtn`. §5 and §8 make preserving it a verified requirement.
- **Append comment** — no single intent exists (`EditField` has only `Value` and `Name`,
  `state.rs:170`); the TUI cannot create a trailing comment at all (`app.rs:589-653`); and
  both web hosts already create/change/clear one via the panel's trailing input
  (`panel.ts:132-143`, committed at `350-352`), which §5 keeps. `ExternalEditKind::Comment`
  (`view.rs:227-232`) edits a **comment node's text** via `ApplyEditComment`, *not* a
  trailing comment — the first draft claimed otherwise and was wrong.

**Delete moves to the end below a separator** rather than sitting beside Cut as it does
today: the same list is now a full-width touch sheet where a mis-tap is cheap. Accepted
cost: existing desktop muscle memory.

### Eligibility

Computed once, in core, and **derived from the type signatures rather than enumerated**:

> An item is single-node-only exactly when the core state behind it carries one `Path`.

| Item | `enabled` when | Multi-select |
|---|---|---|
| Edit in editor | `target_count == 1`, not read-only | dimmed — `ExternalEditKind{path}` is one path |
| Add child | `target_count == 1`, row is a branch | dimmed — one parent needed |
| Append sibling | `target_count == 1`, `path.len() > 0` | dimmed — a normalized selection may span several parents |
| Copy | always | **enabled** |
| Cut | no read-only node in the target set | **enabled** |
| Toggle comment | no read-only node in the target set | **enabled** |
| Detail | `target_count == 1` | dimmed — the panel renders one `ViewRow` |
| Delete | no read-only node in the target set | **enabled** |

So a multi-node selection dims **4 of 8**; a selection containing a read-only node leaves
only Copy enabled — **7 of 8**. That worst case is the reason ineligible items are shown
**disabled rather than hidden**: a menu that silently collapsed to one row explains
nothing, whereas dimming shows four operations that a narrower selection would unlock.

Read-only rejection already exists in core (CONTEXT.md **Read-only node**: rejects edit,
delete, cut, remark); `action_menu()` reuses those predicates rather than restating them.

**Invariant: the menu is never empty and never fully disabled.** Copy is unconditionally
enabled — a **Read-only node** is explicitly copyable — so the cursor always has a
landing row and `action_menu()` needs no "empty menu" refusal path.

`enabled` means "core will attempt it", not "it will succeed". `Remark` on a comment whose
text is not valid TOML fails with a Fragment error (`replace_delete.rs:1081-1153`, and
equivalently in the JSON/YAML backends); predicting that would require a speculative
reparse of every comment node per snapshot, and `ViewRow.comment_advisory` cannot help —
it means something else entirely (comments in a plain `.json` file, `session.rs:316`).
The failure surfaces as an `Error` **Notice**, exactly as pressing `r` does today.

### Intent mapping

| `ActionId` | Effect |
|---|---|
| Edit | `BeginEditExternal` |
| AddChild | `AddChild` |
| AddSibling | `AddSibling` |
| Copy | `CopySelected` |
| Cut | `CutSelected` |
| Remark | `Remark` |
| Detail | `ToggleDetail` |
| Delete | `DeleteSelected` |

Every item is one core intent — no host-mapped exceptions. The `Edit` item is labeled
**"Edit in editor"** because it dispatches `BeginEditExternal` (the TUI's `E`, not `e`);
inline editing is unchanged and stays on click / `e` / the panel.

### Intents

Added to `crates/confy-core/src/session/intent.rs`:

- `OpenActionMenu`
- `ActionMenuMove(i32)` — wraps, and **skips disabled items**
- `ActionMenuCommit` — TUI: applies `items[cursor]`
- `ActionMenuPick(ActionId)` — pointer hosts: applies a directly-chosen id
- `ExitActionMenu`

**`ActionMenuCommit` / `ActionMenuPick` exit to `resting_mode()` first, then dispatch the
mapped intent.** One implementation in core, so no host can forget to close its popup, and
so `Edit` / `AddChild` / `Detail` — each of which sets its own `Mode` — do not have to
overwrite `Mode::ActionMenu` as a side effect. Without this, `Copy` / `Cut` / `Delete`
would leave the menu open holding a stale item list (post-Cut, the clipboard is armed and
every remaining item should have flipped to disabled).

`Escape` peels `Mode::ActionMenu` to `resting_mode()`, inserted into the existing peel
chain in `dispatch.rs`. Commit/Pick on a **disabled** item is a no-op that sets a `Warn`
notice (`core.action.unavailable`) rather than silently ignoring the input — the dimming
and the notice are the same affordance.

`OpenActionMenu` while **Clipboard-armed** refuses and sets the existing
`core.clipboard.action-locked` notice (`session.rs:804-811`, `guard_clipboard_locked`).
This is not a special case bolted on — it is why the Action button flips to Paste while
armed: armed already blocks mutations (ADR 0005 §5), so a menu of blocked operations would
be dishonest.

### i18n keys

Eleven, added to **both** `i18n/en.json` and `i18n/zh-TW.json` (the catalog test at
`crates/confy-core/src/session/i18n.rs:142` panics on a zh-TW key missing from `en`):

`core.action.title`, `core.action.targets`, `core.action.unavailable`,
`core.action.edit`, `core.action.add-child`, `core.action.add-sibling`,
`core.action.copy`, `core.action.cut`, `core.action.remark`, `core.action.detail`,
`core.action.delete`.

`core.action.targets` follows the existing `"…{0} node(s)"` plural style
(`core.clipboard.copied`). No `core.action.section.*` keys — `separator_before` replaced
section headers, which cost six keys per catalog and six non-navigable rows to distinguish
groups that eight self-describing labels already distinguish.

Removed: `web.render.moreActions.title`, the `web.panel.editExternal` usage (§5), and the
`web.fab.*` notice keys `fabAddAction` fed (§3).

Shortcut hints stay **host-side**: the same action is `E` in the TUI and `e` on desktop,
so each host maps `ActionId` to its own key hint. Core supplies no keystrokes.

## §3 Desktop

- `web/render.ts:201` loses the `⋮` button, along with `IC_MORE` (`render.ts:29`) and the
  `web.render.moreActions.title` key. `.row-actions` becomes **grip only**.
  `.paste-mode .row-actions{display:none}` (`web/style.css:527`) is unchanged and now
  hides just the grip. **The kind badge is a sibling in the same flex row and must survive
  untouched** — `render.ts:85` carries an explicit warning that this flexbox area is
  fragile ("push the kind badge off, making it unclickable").
- `buildCtxMenu`, `openCtxMenuAt`, and `ctxMenuPath` (`web/ui.ts:1641`, `1685`, `1552`)
  are deleted. `buildActionMenu()` renders from `snapshot.mode` instead of from a path.
  The separate kind popover (`openKindMenuAt`, `kindMenuPath`, `ui.ts:1586-1614`) is
  **not** touched.
- Existing CSS already covers the item shapes: `.pop`, `.menu-item`,
  `.menu-item:disabled` (opacity .35), `.menu-sep`, `.menu-label`
  (`web/style.css:292-311`). One rule is added: `.menu-item.danger`.
- Open state now lives in core, so `render()` shows/hides the popup; the host retains only
  the anchor coordinates. Anchored **upward** from the Action button
  (`x = rect.right − popWidth`, `y = rect.top − height − 8`); `placePopAt`'s existing
  viewport clamps handle the edges. Right-click anchors at the pointer.
- A second click on the Action button dispatches `ExitActionMenu`
  (ui-design-principles §15, toggle-closed).
- `↑`/`↓` → `ActionMenuMove`, `Enter` → `ActionMenuCommit`, `Esc` → `Escape`. This is
  keyboard navigation the desktop menus do not currently have (`placePopAt` has no arrow
  handling and no focus trap today).
- `m` → `OpenActionMenu` in `web/key-intent.ts`, plus a `help-content.ts` row. Cross-host
  key parity with the TUI, and the only way a desktop keyboard user reaches the menu.
- Action button click: armed → `Paste`; otherwise → `OpenActionMenu`. Because `fab.ts` is
  the **shared** module, this touches both surfaces: `fabHTML`'s `data-act="add"` becomes
  `data-act="actions"`, `syncFab` keeps its paste-copy/paste-cut variants unchanged, and
  the context-aware-add heuristic `fabAddAction` (`fab.ts:50`) is **deleted** — Add child
  and Append sibling are now explicit items, so nothing guesses between them.

Accepted cost: the hover-to-act affordance on each row is gone. Right-click, `m`, and the
Action button all cover it, and rows get visually quieter.

## §4 Touch

- A new mode-driven `.sheet` reusing the existing grab / `sheet-head` / `sheet-body`
  anatomy and the `mi()` item helper (`web/touch/app.ts:677-679, 784-853`), opened from
  `render()` when `mode` is `ActionMenu` — so it inherits the one-sheet-at-a-time rule
  (`openSheet`, `app.ts:356`).
- `dismissSheets()` (`app.ts:1926`) gains an `ActionMenu → ExitActionMenu` arm, matching
  how every other mode peels itself on dismiss.
- `.menu-item` gains disabled and `.danger` styling, plus a `.menu-sep` rule for the
  Delete separator. Touch's `mi()` already renders a shortcut `<span class="sc">`, which
  stays empty on touch.
- The sheet header shows `target_label`, so a multi-node Locked selection finally has a
  visible, operable surface — and a single-node selection names the node.
- Swipe-left delete and swipe-right remark are **unchanged**, as are their armed-clipboard
  guards (`app.ts:1290`). They do not duplicate the menu's Delete/Toggle comment for
  membership-rule purposes: swipe actions are hidden at rest.
- Touch rows are already grip-only, so desktop now matches touch rather than the reverse.
- The touch row kind badge is **not** tappable today (no `data-kind` routing in
  `touch/app.ts`, no `.kind` rule in `touch/style.css`); touch reaches Kind switch through
  the panel's `.kindbtn` → `openKindRow` → `openKindSheet` (`app.ts:178, 603-634`). That
  asymmetry is pre-existing and explicitly **out of scope** here.

## §5 Detail panel

Remove the `.row-btns` action block from both branches (`web/panel.ts:87-91` for comment
nodes, `197-204` for regular nodes) and their handlers (`360-376`), including the `act()`
helper, which becomes orphaned. Dead CSS to remove: `#detailBody .row-btns`,
`#detailBody .row-btns .btn` (`web/style.css:703-704`) and `#detailBody .btn.danger`
(`702`). `#detailBody .btn` itself stays — `.kindbtn` uses it.

**Positional-argument hazard.** `wirePanel(container, row, send, openKind, onError,
afterMutation, batch, schemaEnum)` is positional, and `afterMutation` (`panel.ts:223`)
exists only to confirm-and-dismiss after Delete / Copy / Cut, so it is orphaned too.
Dropping the 6th parameter shifts every trailing argument at all three call sites —
`web/ui.ts:560-568`, and touch at `app.ts:487` (`…, afterPanelMutation, undefined,
schemaEnum`) and `app.ts:584` (`…, afterPanelMutation`). Both trailing parameters are
optional, so a missed call site may not fail to compile; it would fail *silently* as a
dead kind button or a dead schema `<select>`. §8 verifies this explicitly.

`batch`/`run`, `onError`, and `openKind` all stay — the remaining editing handlers use
them, and `openKind` is what keeps Kind switch alive in the panel.

**Every editing affordance stays.** The panel remains fully editable; only actions leave:

| Kept editing affordance | `panel.ts` | Dispatches |
|---|---|---|
| Key / name input (rename) | 161+ | `CommitEdit` |
| Value input `.c-edit` | inline | `CommitEdit` |
| Multi-line value button `editvalue` | 342-348 | `BeginEdit` |
| Schema enum `<select>` | 315-323 | `SchemaEnumMove` + `SchemaEnumCommit` |
| Trailing comment input | 132-143, 349-352 | `SetTrailing` (create, change, and clear) |
| Comment-node input | 353-356 | `ApplyEditComment` |
| **Kind badge `kindswitch`** | 358 | host kind-switch surface — desktop `openKindForRow` (`ui.ts:585`), touch `openKindSheet` (`app.ts:603`) |

| Removed action | `panel.ts` | Now reached via |
|---|---|---|
| `editexternal` | 200 | Action menu → Edit in editor (`BeginEditExternal`) |
| `copy` | 201 | Action menu → Copy |
| `cut` | 202 | Action menu → Cut |
| `del` | 203 | Action menu → Delete |

Note on req 1: `editexternal` existed to "unconditionally force the external popup editor,
bypassing the schema-select branch `BeginEdit` takes for an enum-constrained scalar"
(`panel.ts:331-333`). That escape hatch moves to the Action menu's Edit item, which
dispatches the same `BeginEditExternal` — preserved as a capability, no longer inside the
panel.

`panel-schema.spec.mjs:122-130` asserts the Schema block renders *before* `row-btns`;
after this change Schema (then Comment advisory) is the panel's trailing block, and the
spec must assert that instead.

### The panel closes when the menu opens — on **both** hosts

The first draft claimed desktop kept the panel visible. That is true of pixels and false
of behavior:

- `Mode` is a **single-slot enum** (`state.rs:44-63`) and core has no mode stack or
  return-mode field anywhere.
- **Desktop**: the panel is `Mode::Detail`-driven — `renderDetailPanel` closes it whenever
  `tag !== "Detail"` (`ui.ts:543`). Entering `Mode::ActionMenu` therefore closes it.
- **Touch**: the detail sheet is *host-local* state, not core-mode-backed (`app.ts:1619-
  1620`, explicitly commented "unlike desktop's `Mode::Detail`"), but the
  one-sheet-at-a-time rule closes it just the same.

Accepted, because it is **already the shipped behavior for the panel's own kind badge**:
`openKindForRow` (`ui.ts:585-594`) dispatches `OpenKindSwitch` → `Mode::KindSwitch` → the
panel closes. The Cursor is unchanged, so the menu targets the same node. Rejected
alternatives: a return-mode on `Mode::ActionMenu` (invents core's first mode stack for one
case, and would make the long-shipped kind-badge behavior look like a bug), and an
"Actions" row inside the detail sheet (reintroduces precisely what this change removes).

## §6 TUI

- New `crates/confy-tui/src/tui/overlay_action_menu.rs` drawing `Mode::ActionMenu` in the
  `overlay_kind_switch.rs` shape: `›` cursor marker, `Modifier::REVERSED` cursor row,
  `centered_rect(40, …)`, `Clear` first, `title_bottom` key hints. Header shows
  `target_label`.
- Disabled items render dimmed (`Color::DarkGray`) and are skipped by navigation; the
  Delete separator renders as a plain rule (no `LayoutRow::Header` machinery is needed now
  that section headers are gone).
- Scrolling follows `overlay_schema_enum.rs` (`schema_enum_scroll_offset` analogue) so a
  list taller than the terminal stays navigable (ui-design-principles §11).
- `keys.rs` binds `m` → `KeyAction::ActionMenu`; the keybinding-contract test at
  `keys.rs:156` gains it.
- `mod.rs` gets one key block after the `KindSwitch` block: `↑`/`k` and `↓`/`j` move,
  `Enter` commits, `Esc` peels.
- `ui.rs` inserts the overlay into the existing draw z-order.
- Armed clipboard guards `m` exactly as it guards `lang_picker`
  (`keys.rs:173-220`, `armed_clipboard_guards_lang_picker_and_edit_node`).

The single-key shortcuts (`a`, `d`, `c`, `x`, `v`, `r`, `e`, `E`, `K`) are unchanged; the
menu is a discovery surface layered over them, not a replacement.

## §7 Action button position

The button must clear the status bar without magic numbers and without breaking when
`.footer` wraps at narrow widths (`web/style.css:648`).

`.main` is already `position:relative`, but `.detail` is a flex sibling inside it, so an
absolute bottom-right button in `.main` would sit *on top of* the open detail panel.
`.tree-wrap` cannot host it either: it is `overflow:auto`, so an absolute child scrolls
with the content.

Therefore both hosts get a non-scrolling positioned wrapper around the tree scroller:

```html
<div class="main">
  <div class="pane-wrap">            <!-- flex:1; min-width:0; position:relative -->
    <div class="tree-wrap" id="treeWrap">…</div>
    <button class="fab" data-act="actions">…</button>
    <button class="fab-clear" data-act="pastecancel">…</button>
  </div>
  <aside class="detail" id="detail">…</aside>
</div>
```

`.fab` / `.fab-clear` change from `position:fixed` to `position:absolute`. The result:
above the footer (the wrapper is inside `.main`, which excludes `.footer`), left of the
detail panel (the wrapper shrinks when the panel opens), and non-scrolling — all derived
from layout, none hardcoded. `.tree-wrap`'s existing `padding-bottom:80px` clearance stays
so no row hides beneath the button. `body.raw-view` hiding is unchanged.

Touch applies the same wrapper around `.tree-pane`, keeping
`env(safe-area-inset-bottom)` in the offset since the touch status bar already includes it.

## §8 Verification

Green unit tests are not sufficient; both real binaries are exercised.

**Core (`confy-core`)**
- `action_menu()` item set, order, and `target_label` for: scalar leaf, branch, comment,
  root, read-only node, array element.
- Multi-node Locked selection: exactly Edit / Add child / Append sibling / Detail are
  disabled and the four set-applying items stay enabled; `target_count` matches
  `selected_paths().len()`.
- A multi-node selection **containing a read-only node** leaves only Copy enabled.
- `OpenActionMenu` while Clipboard-armed refuses and sets `core.clipboard.action-locked`.
- `ActionMenuMove` skips disabled items and wraps.
- `ActionMenuPick` on a disabled id is a no-op setting `core.action.unavailable`.
- **After `Pick`/`Commit` on an enabled item, `mode` is no longer `ActionMenu`** — the
  exit-then-dispatch ordering.
- `Escape` peels `Mode::ActionMenu` to `resting_mode()`.
- Every `ActionId` label resolves in both catalogs (extends the existing i18n test).

**TUI (`crates/confy-tui/src/tui/tests.rs`)**
- `m` opens the overlay; `Enter` on each enabled item dispatches the expected mutation
  (asserted through `serialize()`, per the established style).
- `Esc` closes to `resting_mode()`; `m` while armed does not open.
- `K` still opens the kind switch.

**Web**
- New `web/action-menu.spec.mjs` in the existing esbuild-extraction style: desktop upward
  anchoring, toggle-closed on second trigger click, arrow/Enter navigation, `m`, and the
  touch sheet's item / separator / disabled rendering.
- Update `web/panel-schema.spec.mjs` for the new trailing block.
- Extend `web/touch-modal-lock.spec.mjs` with the armed-clipboard `ActionMenu` refusal.

**Kind switch must still work** (explicit, because §3/§5 both edit code adjacent to it)
- Desktop row kind badge still opens the popover and still toggles closed on a second
  click after `.row-actions` loses the `⋮` (`render.ts:85`'s flexbox warning).
- The panel's `.kindbtn` still opens the popover on desktop and the sheet on touch, from
  all three `wirePanel` call sites, **after `afterMutation` is dropped from the signature**
  — i.e. no argument slid into the `batch` or `schemaEnum` slot.
- The panel's schema `<select>` still works, for the same reason.

**Real binaries**
- TUI: press `m` on a scalar, a branch, a comment, a multi-node selection, a selection
  containing a read-only node, and while armed; confirm the overlay, dispatch, and
  refusal.
- Web dev server: desktop click, right-click, and `m`, then touch emulation, each in
  normal and armed states; confirm the button never covers the status bar at wide, narrow,
  and footer-wrapped widths, and that the kind badge and panel kind button both still
  open.
- VS Code webview: confirm right-click reaches the Action menu rather than being swallowed
  by the webview host (`web/vscode.ts` is a message-passing adapter over this same UI, so
  it inherits the change untested otherwise).

**Docs and record**
- CONTEXT.md — the four §1 terms and the Remark label line. **Done.**
- ADR 0009 + index row. **Done.**
- WEBUI.md — row anatomy (`:146-155`), the FAB bullet (`:156-160`), the detail-panel
  buttons, and Overflow-menu naming (`:409-416`).
- TUI.md — key table and the new overlay.
- CHANGELOG.md — one `Unreleased Update` entry.

## Out of scope

- Making touch's row kind badge tappable (pre-existing asymmetry; §4).
- Driving the native Edit menu's items from core eligibility (ADR 0009; a follow-up ADR if
  the read-only inconsistency proves to matter).
- Long-press on the Action button for a direct add — the one-tap add regression is
  accepted rather than mitigated.
- `EditField::Trailing` + a `BeginTrailingEdit` intent, which is what "Append comment"
  would need to become a real cross-host item.
- Making the schema enum `<select>` offer free-text entry (the deeper fix for req 1).
- Any change to the Overflow menu's contents or the swipe gestures.
- `docs/reference/CHROME.md` documents a host that does not exist in the repo (no
  `extension/`, no manifest). Unrelated doc-accuracy cleanup, noted here only so the next
  reader does not treat it as a surface this change missed.
