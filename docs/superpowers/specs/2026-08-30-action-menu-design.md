# Action menu — centralized node operations across desktop, touch, and TUI

Date: 2026-08-30
Status: approved design, pending implementation plan

## Problem

Node operations are reachable through four unrelated surfaces that disagree with each
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

The TUI has no menu surface at all; every operation is a single keystroke.

Consequence: the same logical operation is implemented three times with three different
eligibility rules, and neither touch multi-selection nor TUI menu discovery is served.

## Solution

One **Action menu**, its item model owned by `confy-core`, rendered three ways. Node rows
keep only a move grip on both web surfaces. The detail panel becomes editing-and-
information only. The TUI gains an equivalent overlay on `m`.

### Decisions taken

| Decision | Choice | Rationale |
|---|---|---|
| Item model ownership | Core-owned items **and** open state (`ModeView` variant) | One eligibility computation, one i18n source, and the web menus gain the arrow-key navigation they have never had |
| Terminology | Overflow menu / Action menu / Action button | Distinguishes the RWD-folded toolbar menu from the new node-operations surface |
| Desktop right-click | Kept, opens the same Action menu at the pointer | Desktop-idiomatic, already documented in `web/help-content.ts`, near-zero extra code |
| Button position | Non-scrolling wrapper around the tree scroller | Magic-number-free and resize-aware (ui-design-principles §19) |
| TUI trigger | `m` | Free lowercase key, mnemonic "menu" |
| Detail panel | All four action buttons removed | Anatomy parity with the TUI detail popup; every operation stays reachable via the Action menu |

## §1 Terminology (CONTEXT.md)

Three entries added to CONTEXT.md's Language section, in the style of the existing
cross-platform entries (Cursor, Locked selection, Clipboard-armed).

**Overflow menu**:
Chrome only. Lists exactly the toolbar controls the current viewport width has folded
away (`foldedEntries`, `web/toolbar-fold.ts:24`). Desktop `⋯` popup, touch `⋯` sheet.
Never holds node operations.
_Avoid_: More menu, dynamic menu, menu sheet.

**Action menu**:
The single surface listing every operation available on the current **Cursor** or
**Locked selection**. One item model in core; three renderings — desktop popup, touch
bottom sheet, TUI overlay. Opened by the **Action button**, by desktop right-click, or by
the TUI's `m`.
_Avoid_: context menu (that is one desktop gesture that opens it), node menu, `⋮` menu.

**Action button**:
The floating trigger that opens the **Action menu**. While **Clipboard-armed** it is
instead the **Paste button**, with a ✕ cancel above it.
_Avoid_: FAB, `+` button.

Code hygiene that follows: today `data-act="menu"` means *overflow menu* on touch
(`web/touch/app.ts:247`) and *per-row node menu* on desktop (`web/ui.ts:1233`) — the exact
collision this terminology resolves. Touch's becomes `data-act="overflow"`; the Action
button is `data-act="actions"`.

## §2 Core model

New view type and `ModeView` variant beside `KindSwitch`, following its shape
(`crates/confy-core/src/session/view.rs:143`):

```rust
pub enum ActionId {
    Edit, AddChild, AddSibling, Copy, Cut,
    Remark, AppendComment, Detail, Delete,
}

pub struct ActionItemView {
    pub id: ActionId,
    /// Localized core-side via `tr(self.lang, "core.action.*")`, exactly as
    /// `ModeView::Prompt.question` is — hosts never reconstruct label prose.
    pub label: String,
    /// Localized section header rendered *before* this item, when present.
    pub section: Option<String>,
    pub enabled: bool,
    /// `Delete` only — hosts render it as destructive.
    pub danger: bool,
}

ModeView::ActionMenu {
    cursor: usize,
    items: Vec<ActionItemView>,
    /// `selected_paths().len()` — hosts show "N nodes" rather than recomputing it.
    target_count: usize,
}
```

`Session::action_menu()` builds the list from `selected_paths()`
(`crates/confy-core/src/session/session.rs:1502`), the existing universal
Locked-selection-else-Cursor resolver. Targeting therefore needs **no new logic**: the
desktop right-click path keeps calling today's `selectForMenu` (`web/ui.ts:2062`) to
retarget first, then opens the menu.

The web wire types mirror this by hand: `web/types.ts` gains the
`{ ActionMenu: { cursor: number; items: ActionItemView[]; target_count: number } }`
arm on its `ModeView` union (`types.ts:161`) plus an `ActionItemView` interface, and the
four new intents on its `Intent` union. No new FFI entry point is needed — the menu rides
the existing `SessionSnapshot.mode` projection, exactly as `KindSwitch` does.

### Eligibility

Computed once, in core. This is what gives touch multi-selection a usable surface.

| Item | `enabled` when |
|---|---|
| Edit | `target_count == 1`, not read-only |
| Add child | `target_count == 1`, row is a branch |
| Append sibling | `target_count == 1`, `path.len() > 0` |
| Copy | always |
| Cut | no read-only node in the target set |
| Toggle comment | no read-only node in the target set |
| Append comment | `target_count == 1`, not a comment, no existing trailing comment, parent not inline |
| Detail | `target_count == 1` |
| Delete | no read-only node in the target set |

Read-only rejection already exists in core (CONTEXT.md **Read-only node**: rejects edit,
delete, cut, remark); `action_menu()` reuses those same predicates rather than restating
them.

**Paste is deliberately absent.** Paste is only legal while **Clipboard-armed**, and
`OpenActionMenu` refuses in that state (see Intents below) — so a Paste item could never
be reached. This is not a regression: today's context-menu Paste entry
(`web/ui.ts:1641`, item 6) is *already* dead code, because both of its triggers are
blocked while armed (`ui.ts:1234` and `ui.ts:2029` both bail out with the action-locked
notice, and `.paste-mode .row-actions` hides the `⋮` outright). Pasting stays where it
works: the Action button's armed Paste state, tap/click-to-target, and `v`.

**Invariant: the menu is never empty and never fully disabled.** Copy is unconditionally
enabled — a **Read-only node** is explicitly copyable (CONTEXT.md) — so there is always at
least one enabled item for the cursor to land on, and `action_menu()` never needs an
"empty menu" refusal path.

### Intent mapping

| `ActionId` | Effect |
|---|---|
| Edit | `BeginEditExternal` — same as today's context-menu item 1. Inline editing is unchanged and stays on click / `e` / the detail panel |
| AddChild | `AddChild` |
| AddSibling | `AddSibling` |
| Copy | `CopySelected` |
| Cut | `CutSelected` |
| Remark | `Remark` (labeled "Toggle comment") |
| AppendComment | **host-mapped** — see below |
| Detail | `ToggleDetail` |
| Delete | `DeleteSelected` |

`AppendComment` is the one item core does not dispatch itself. `EditField` has only
`Value` and `Name` (`state.rs:170`), so beginning a trailing-comment edit is a host
affordance today: desktop calls `beginTrailingEdit` (`web/ui.ts:1482`, a DOM-only inline
input that commits through `SetTrailing`), touch routes to the detail panel's trailing
field, and the TUI uses `BeginEditExternal` with `ExternalEditKind::Comment`. Core still
owns **whether the item is offered and what it is called**; only the editor's appearance
is host-side — the same split as the detail panel's `openKind` callback. Unifying it
behind a new `EditField::Trailing` is listed in Out of scope.

### Order

Today's order is preserved and grouped, with one deliberate change: **Delete moves to the
end, below a separator**, so it is not adjacent to Cut on a touch sheet.

1. *(Edit)* — Edit
2. *(Add)* — Add child, Append sibling
3. *(Clipboard)* — Copy, Cut
4. *(Comment)* — Toggle comment, Append comment
5. *(View)* — Detail
6. *(Danger)* — Delete

### Intents

Added to `crates/confy-core/src/session/intent.rs`:

- `OpenActionMenu`
- `ActionMenuMove(i32)` — wraps, and **skips both section headers and disabled items**
  (`type_filter::nav_rows` precedent)
- `ActionMenuCommit` — TUI: applies `items[cursor]`
- `ActionMenuPick(ActionId)` — pointer hosts: applies a directly-chosen id
- `ExitActionMenu`

`Escape` peels `Mode::ActionMenu` to `resting_mode()`, inserted into the existing peel
chain in `dispatch.rs`. `ActionMenuCommit` / `ActionMenuPick` on a disabled item is a
no-op that sets a `Warn` notice (`core.action.unavailable`) rather than silently ignoring
the input.

`OpenActionMenu` while **Clipboard-armed** refuses and sets the existing
`core.clipboard.action-locked` notice. This is not a special case bolted on — it is why
the Action button flips to Paste while armed: armed already blocks mutations (ADR 0005
§5), so a menu of blocked operations would be dishonest.

### i18n keys

Added to **both** `i18n/en.json` and `i18n/zh-TW.json` (the catalog test at
`crates/confy-core/src/session/i18n.rs:142` panics on a zh-TW key missing from `en`):

`core.action.title`, `core.action.targets`, `core.action.unavailable`,
`core.action.edit`, `core.action.add-child`, `core.action.add-sibling`,
`core.action.copy`, `core.action.cut`, `core.action.remark`,
`core.action.append-comment`, `core.action.detail`, `core.action.delete`,
`core.action.section.edit`, `core.action.section.add`,
`core.action.section.clipboard`, `core.action.section.comment`,
`core.action.section.view`, `core.action.section.danger`.

Removed: `web.render.moreActions.title`, and the `web.panel.editExternal` usage in
`panel.ts` (§5).

Shortcut hints stay **host-side**: the same action is `E` in the TUI and `e` on desktop,
so each host maps `ActionId` to its own key hint. Core supplies no keystrokes.

## §3 Desktop

- `web/render.ts:201` loses the `⋮` button, along with `IC_MORE` (`render.ts:29`) and the
  `web.render.moreActions.title` key. `.row-actions` becomes **grip only**.
  `.paste-mode .row-actions{display:none}` (`web/style.css:527`) is unchanged and now
  hides just the grip.
- `buildCtxMenu`, `openCtxMenuAt`, and `ctxMenuPath` (`web/ui.ts:1641`, `1685`, `1552`)
  are deleted. `buildActionMenu()` renders from `snapshot.mode` instead of from a path.
- Existing CSS already covers the new item shapes: `.pop`, `.menu-item`, `.menu-item:disabled`
  (opacity .35), `.menu-sep`, `.menu-label` (`web/style.css:292-311`). One rule is added:
  `.menu-item.danger`.
- Open state now lives in core, so `render()` shows/hides the popup; the host retains only
  the anchor coordinates. Anchored **upward** from the Action button
  (`x = rect.right − popWidth`, `y = rect.top − height − 8`); `placePopAt`'s existing
  viewport clamps handle the edges. Right-click anchors at the pointer.
- A second click on the Action button dispatches `ExitActionMenu`
  (ui-design-principles §15, toggle-closed).
- `↑`/`↓` → `ActionMenuMove`, `Enter` → `ActionMenuCommit`, `Esc` → `Escape`. This is
  keyboard navigation the desktop menus do not currently have (`placePopAt` has no arrow
  handling and no focus trap today).
- Action button click: armed → `Paste`; otherwise → `OpenActionMenu`. Because `fab.ts` is
  the **shared** module, this touches both surfaces: `fabHTML`'s `data-act="add"` becomes
  `data-act="actions"`, `syncFab` keeps its paste-copy/paste-cut variants unchanged, and
  the context-aware-add decision logic `fabAddAction` (`fab.ts:50`) is **deleted** — Add
  child / Append sibling are now explicit menu items, so no host-side heuristic picks
  between them, and the `web.fab.*` notice keys it fed become unused.

Accepted cost: the hover-to-act affordance on each row is gone. Right-click and the Action
button both cover it, and rows get visually quieter.

## §4 Touch

- A new mode-driven `.sheet` reusing the existing grab / `sheet-head` / `sheet-body`
  anatomy and the `mi()` item helper (`web/touch/app.ts:677-679, 784-853`), opened from
  `render()` when `mode` is `ActionMenu` — so it inherits the one-sheet-at-a-time rule
  (`openSheet`, `app.ts:356`).
- `dismissSheets()` (`app.ts:1926`) gains an `ActionMenu → ExitActionMenu` arm, matching
  how every other mode peels itself on dismiss.
- `.menu-item` gains disabled and `.danger` styling; section headers get a `.menu-sec`
  rule. Touch's `mi()` already renders a shortcut `<span class="sc">`, which stays empty
  on touch.
- The sheet header shows `target_count` ("N nodes"), so a multi-node Locked selection
  finally has a visible, operable surface.
- Swipe-left delete and swipe-right remark are **unchanged**, as are their armed-clipboard
  guards (`app.ts:1290`).
- Touch rows are already grip-only, so desktop now matches touch rather than the reverse.

## §5 Detail panel

Remove the `.row-btns` action block from both branches (`web/panel.ts:87-91` for comment
nodes, `197-204` for regular nodes) and their handlers (`360-376`), including the `act()`
helper, which becomes orphaned. Dead CSS to remove: `#detailBody .row-btns`,
`#detailBody .row-btns .btn` (`web/style.css:703-704`) and `#detailBody .btn.danger`
(`702`). `#detailBody .btn` itself stays — `.kindbtn` uses it.

`wirePanel`'s `afterMutation` parameter (`panel.ts:223`) exists only to confirm-and-dismiss
after Delete / Copy / Cut, so it is orphaned too: drop it from the signature and from both
call sites (`web/ui.ts`, `web/touch/app.ts`). `batch`/`run`, `onError`, and `openKind` all
stay — the remaining editing handlers still use them.

**Every editing affordance stays.** The panel remains fully editable; only actions leave:

| Kept editing affordance | `panel.ts` | Dispatches |
|---|---|---|
| Key / name input (rename) | 161+ | `CommitEdit` |
| Value input `.c-edit` | inline | `CommitEdit` |
| Multi-line value button `editvalue` | 342-348 | `BeginEdit` |
| Schema enum `<select>` | 315-323 | `SchemaEnumMove` + `SchemaEnumCommit` |
| Trailing comment input | 349-352 | `SetTrailing` |
| Comment-node input | 353-356 | `ApplyEditComment` |
| Kind badge `kindswitch` | 358 | host kind-switch surface |

| Removed action | `panel.ts` | Now reached via |
|---|---|---|
| `editexternal` | 200 | Action menu → Edit (`BeginEditExternal`) |
| `copy` | 201 | Action menu → Copy |
| `cut` | 202 | Action menu → Cut |
| `del` | 203 | Action menu → Delete |

Note on req 1: `editexternal` existed to "unconditionally force the external popup editor,
bypassing the schema-select branch `BeginEdit` takes for an enum-constrained scalar"
(`panel.ts:331-333`). That escape hatch moves to the Action menu's Edit item, which
dispatches the same `BeginEditExternal`. It is therefore preserved as a capability, but no
longer inside the panel.

`panel-schema.spec.mjs:122-130` asserts the Schema block renders *before* `row-btns`;
after this change Schema (then Comment advisory) is the panel's trailing block, and the
spec must be updated to assert that instead.

### Reachability while the panel is open

Because the panel is now the only surface without its own actions, the Action button's
availability matters:

- **Desktop**: `.detail` is a flex sibling inside `.main` (`web/index.html:127`), so the
  §7 wrapper keeps the Action button over the tree and to the *left* of the open panel —
  visible and clickable throughout.
- **Touch**: the detail is a `.sheet` behind a scrim (`z-index` 45 over the button's 40),
  one sheet at a time, so the button is genuinely unreachable while it is open. Accepted:
  dismiss the sheet, then act — the Cursor is unchanged, so the Action menu targets the
  same node. Explicitly rejected alternative: an "Actions" row inside the detail sheet,
  which would reintroduce precisely what this change removes.

## §6 TUI

- New `crates/confy-tui/src/tui/overlay_action_menu.rs` drawing `Mode::ActionMenu` in the
  `overlay_kind_switch.rs` shape: `›` cursor marker, `Modifier::REVERSED` cursor row,
  `centered_rect(40, …)`, `Clear` first, `title_bottom` key hints.
- Disabled items render dimmed (`Color::DarkGray`) and are skipped by navigation; section
  headers render cyan + bold and are non-navigable (`overlay_type_filter.rs`
  `LayoutRow::Header` precedent).
- Scrolling follows `overlay_schema_enum.rs` (`schema_enum_scroll_offset` analogue) so a
  list taller than the terminal stays navigable (ui-design-principles §11).
- `keys.rs` binds `m` → `KeyAction::ActionMenu`; the keybinding-contract test at
  `keys.rs:156` gains it.
- `mod.rs` gets one key block after the `KindSwitch` block: `↑`/`k` and `↓`/`j` move,
  `Enter` commits, `Esc` peels.
- `ui.rs` inserts the overlay into the existing draw z-order.
- Armed clipboard guards `m` exactly as it guards `lang_picker`
  (`keys.rs:173-220`, `armed_clipboard_guards_lang_picker_and_edit_node`).

The single-key shortcuts (`a`, `d`, `c`, `x`, `v`, `r`, `e`, `E`) are unchanged; the menu
is a discovery surface layered over them, not a replacement.

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
- `action_menu()` item set and order for: scalar leaf, branch, comment, root, read-only
  node, array element.
- Multi-node Locked selection disables the single-node items and keeps the
  selection-aware ones enabled; `target_count` matches `selected_paths().len()`.
- `OpenActionMenu` while Clipboard-armed refuses and sets `core.clipboard.action-locked`.
- `ActionMenuMove` skips headers and disabled items, and wraps.
- `ActionMenuPick` on a disabled id is a no-op setting `core.action.unavailable`.
- `Escape` peels `Mode::ActionMenu` to `resting_mode()`.
- Every `ActionId` label and section resolves in both catalogs (extends the existing
  i18n catalog test).

**TUI (`crates/confy-tui/src/tui/tests.rs`)**
- `m` opens the overlay; `Enter` on each enabled item dispatches the expected mutation
  (asserted through `serialize()`, per the established style).
- `Esc` closes to `resting_mode()`; `m` while armed does not open.

**Web**
- New `web/action-menu.spec.mjs` in the existing esbuild-extraction style: desktop
  upward anchoring, toggle-closed on second trigger click, arrow/Enter navigation, and
  the touch sheet's item/section/disabled rendering.
- Update `web/panel-schema.spec.mjs` for the new trailing block.
- Extend `web/touch-modal-lock.spec.mjs` with the armed-clipboard `ActionMenu` refusal.

**Real binaries**
- TUI: press `m` on a scalar, a branch, a comment, a multi-node selection, and while
  armed; confirm the overlay, dispatch, and refusal.
- Web dev server: desktop click and right-click, then touch emulation, each in normal and
  armed states; confirm the button never covers the status bar at wide, narrow, and
  footer-wrapped widths, and stays visible beside an open detail panel.

**Docs and record**
- CONTEXT.md — the three §1 terms.
- WEBUI.md — row anatomy (`:146-155`), the FAB bullet (`:156-160`), the detail-panel
  buttons, and Overflow-menu naming (`:409-416`).
- TUI.md — key table and the new overlay.
- CHANGELOG.md — one `Unreleased Update` entry.
- A new ADR: this is hard to reverse, surprising without context, and a real trade-off
  (per-row affordance and one-tap add given up for cross-host consistency and core-owned
  eligibility).

## Out of scope

- Kind switch (`K`) as an Action menu item — reachable from the panel's kind badge and the
  TUI key; adding it is a separate decision.
- Long-press on the Action button for a direct add (the one-tap add regression is accepted
  rather than mitigated).
- Making the schema enum `<select>` offer free-text entry (the deeper fix for req 1).
- Any change to the Overflow menu's contents or the swipe gestures.
