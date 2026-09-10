# Host parity — the deliberate TUI ↔ Web divergences

One headless core (`confy-core`), five hosts: the **TUI**, **web desktop**, **web touch**, the
**Tauri** shell (desktop + Android) and the **VS Code** webview. Everything they share is
specified elsewhere; this file is the index of the places where they **deliberately differ**.

**What this file is.** An index, not a specification. Each row states the divergence in one
line and names the doc (or, where no doc owns it, the code) that is its authority. Read the
authority before changing behavior — the row is a pointer, never the full rule.

**What a row means.** Every divergence listed here is **decided, not accidental**. Making two
hosts agree on any of these is a behavior change that needs its own decision, not a "fix".
Conversely, an *undocumented* difference is a bug: core owns the model, so two hosts diverge
only for a host-shaped reason (no terminal hover, no browser subprocess, no touch keyboard,
a row the host doesn't draw…).

**Scope note.** Keyboard bindings have their own single source of truth in
[`KEYMAP.md`](KEYMAP.md) — including its machine-checked table and its own
"Deliberate divergences" / "Implementation differences" / "Help overlay parity" /
"Editor parity" sections. Rows below that concern keys are **summaries with a pointer there**;
`KEYMAP.md` stays the authority and the thing tests check.

## 1. Input

| Divergence | TUI | Web | Why | Authority |
|---|---|---|---|---|
| `w` save alias | bound (vim `:w`) | unbound; `Ctrl+S` is the cross-surface binding | terminal muscle memory | [KEYMAP.md](KEYMAP.md) §Deliberate divergences |
| `g` / `G` first/last row | unbound (`Home`/`End`) | bound | vim list nav on a full keyboard | ditto |
| `+` / `-` nudge | unbound (`←`/`→`) | bound | non-arrow stepping | ditto |
| `Ctrl+O` open | unbound — the path is a CLI argument | bound (desktop); suppressed in VS Code | a terminal process is launched *on* a file | ditto §Present on one surface only |
| `l` language picker | modal picker (`Mode`-less, TUI-local) | toolbar dropdown; touch folds it into "⋯ More" | no persistent toolbar in a terminal | ditto §Same capability, different affordance |
| `~` diag overlay | read-only ring overlay | none — `?diag=1` drains to `console.debug` | DevTools is the browser's overlay | [MESSAGES.md](MESSAGES.md) §4 |
| `q` quit | quits | quits on web desktop; **suppressed** under VS Code and touch (`vshost`) | an editor tab / PWA has nothing to quit | [KEYMAP.md](KEYMAP.md) §Same capability… |
| `.json` ↔ `.jsonc` on Convert | `Tab` toggles the extension on the Path step | a `Jsonc` pseudo-tag in the format `<select>` | `Tab` can't collide with typing a path | ditto |
| Page step | `terminal_height / 2` | derived from the scroll container's row ratio, then halved | no fixed row height in the DOM | ditto §Implementation differences |
| `Escape` order | reaches core immediately (filter → locked selection → clipboard) | dismisses the topmost **host-local** sheet/modal first, then core | DOM overlays live outside core's `Mode` | ditto |
| Row actions | keys only (`d`, `r`) | touch adds swipe-left Delete / swipe-right Remark | no keys on a phone | [WEBUI.md](WEBUI.md) §Swipe actions |
| Multi-select gestures | `s`, ⇧↑↓ | desktop adds ⌘/⇧-click **and marquee**; touch uses tap / modifier-tap (no marquee) | marquee is pointer-only and fights list scrolling | [ROW_STATE_MODEL.md](ROW_STATE_MODEL.md) §1 |

## 2. Root visibility — root-visible vs root-hidden hosts

Worth its own section because it is a **core mode**, not a rendering choice
([ADR 0013](../adr/0013-root-visibility-is-core-state.md)): `Session.root_visible`, set by the
host at load time via `Intent::SetRootVisible`, decides whether the document **Root** exists
as a row *inside core* at all.

| Host | Mode | Why |
|---|---|---|
| TUI | **root-visible** | a file row is the terminal's only place to hang whole-document state (name, format, dirty) |
| web desktop | **root-visible** | wide viewport; the row is also the drop target for "append at the document end" and the reachable home of document-level actions |
| web touch | **root-hidden** | a phone's tree pane cannot spend a row and an indent level on chrome the header already shows |
| VS Code | **root-hidden** | the editor tab already names the file, and its own text editor owns whole-file editing |

What each mode means for the snapshot every host renders:

| | root-visible | root-hidden |
|---|---|---|
| Row for the document | drawn, `path == []`, `depth == 0` | **absent** — no row addresses the document |
| Top-level Node depth | `1` (one indent step under the Root) | `0` |
| Cursor | may sit on the Root (`g`/Home) | never `[]` |
| Paste slots | includes `Into([])` / `After([])` | neither is emitted |
| Type filter | includes the `[G] root` facet | facet absent |
| Collapsed Root | legal — the Root row stays, so it can be re-expanded | Root is treated as unconditionally expanded |
| Empty document | the Root row is still there | **zero rows** — the host owns the empty state (D11) |

Because the mode lives in core, **no host carries a root stand-in**: the web's seven of them
(row filter, `depth - 1`, `drawnCursorFallback`, `overshotUndrawnRootSlot`, `rootSlotLine`,
the `select.ts` filter, the pre-`OpenConvert` `SetCursor: []`) were deleted rather than made
conditional. Two per-host rules remain, and they are *behavioral*, not compensating:

| Rule | Where | Authority |
|---|---|---|
| The Root takes the cursor but is **never selected** — selecting it reports Info `core.selection.root-excluded`; Copy/Cut/Remark/Delete are dimmed on it | all hosts, both modes | [ROW_STATE_MODEL.md](ROW_STATE_MODEL.md) §6a; [TUI.md](TUI.md) §Multi-select |
| The drawn Root row has **no drag grip** and an **inert kind badge** (a document has no kind; `Move` on it is `Unsupported`) | web desktop | `web/render.ts`; ADR 0013 D12 |

Core's slot order is **unchanged** by any of this: `paste_slots()` still emits each row's
`Into` before its `After`.

## 3. Row / cursor / clipboard state

| Divergence | TUI | Web | Why | Authority |
|---|---|---|---|---|
| Post-paste highlight | selection cleared | a follow-up client-side `SetSelection` highlights the landed nodes | TUI arrow keys don't collapse `Selection`, so re-selecting would resurrect the stale-selection bug | [ROW_STATE_MODEL.md](ROW_STATE_MODEL.md) §6d; [ADR 0004](../adr/0004-unified-clipboard-move-targeting.md) |
| `Escape` presses to leave cut/copy mode | 1 (a bare cursor leaves no selection) | 2 on desktop (a click always writes a 1-path `Selection`) | pointer selection is a real state the keyboard doesn't create | [ROW_STATE_MODEL.md](ROW_STATE_MODEL.md) §1b, §2; [ADR 0005](../adr/0005-row-cursor-selection-clipboard-state-model.md) |
| Aiming the paste target | arrow keys step the flattened `PasteSlot` list | desktop: hover preview + click to commit the slot; touch: row-body drag repositions it continuously, FAB commits | pointer-first and touch-first affordances over one core algorithm (`pointer_slot`) | [ROW_STATE_MODEL.md](ROW_STATE_MODEL.md) §6a, §6b; [ADR 0010](../adr/0010-pointer-drops-resolve-through-pasteslot.md) |
| Selection marker | `●` glyph in the NAME cell (leaves cell backgrounds free) | 3px left accent bar (`::before`) | terminal cell fills are already spent on cursor/cut/copy | [ROW_STATE_MODEL.md](ROW_STATE_MODEL.md) §3 |
| Edge auto-scroll while dragging | n/a | desktop inherits native HTML5 drag scroll; touch hand-rolls a RAF edge loop | custom pointer gestures get no native container scroll | [ROW_STATE_MODEL.md](ROW_STATE_MODEL.md) §6c |

## 4. Editing

| Divergence | TUI | Web | Why | Authority |
|---|---|---|---|---|
| Inline editor | drives core's edit buffer keystroke by keystroke (`EditChar`/`EditCursor*`/`EditDelete`) | a real `<input>`; the browser owns cursor/selection/delete, commit is one `CommitEdit` | native text editing, IME, selection handles | [KEYMAP.md](KEYMAP.md) §Implementation differences + §Editor parity |
| Trailing comment while editing a value | **bundled** into the same buffer (`value␠␠# comment`), split on commit | stripped from the value `<input>` and edited in its own cell; re-appended on commit so a value edit never drops it | a GUI can afford a second field; typing over a value must not eat the comment | [WEBUI.md](WEBUI.md) §Shared edit/detail panel; `web/render.ts`, `web/ui.ts` |
| Multiline / forced editor (`e`, `E`) | suspends the alternate screen and spawns `$EDITOR` | desktop `#ext-modal`; touch `.ext-sheet` | a browser can't spawn a subprocess — one `snap.external_edit` handshake, three presentations | [KEYMAP.md](KEYMAP.md) §Editor parity |
| Detail view (`i`/`Enter`) | centered floating popup (`Mode::Detail`) | desktop `<aside>` pane; touch bottom sheet (or a split side pane when wide) | width budget | ditto; [WEBUI.md](WEBUI.md) |
| Core sub-modes the host doesn't render | renders `Mode::Edit`/`Detail`/`KindSwitch` | **touch renders none of the three** — it backs out to its own sheets | nested modals are unusable on a phone | [WEBUI.md](WEBUI.md) |
| Bool toggle | `←`/`→` | keyboard `←`/`→`/`+`/`-` only — **never** the wheel or a swipe | a scroll over a row must not flip a flag | [ADR 0011](../adr/0011-bool-toggle-on-the-intent-nudge-path-only.md) |
| Datetime type switch | the enum-picker popup (`Mode::SchemaEnum`) | the Kind popover, with the in-row `<select>` suppressed | one widget per concept: kind switches are popovers | [ADR 0012](../adr/0012-datetime-cross-type-switch-is-a-value-replace.md) |
| Trailing-comment creation | can only edit one that already exists | can create / change / clear it on any node | falls out of the bundled-buffer editor above | [ADR 0009](../adr/0009-centralized-action-menu-core-owned.md) |
| Core call boundary | mutations go through `Session::apply(Intent)`, inspection reads `Session` directly (no snapshot on pure navigation) | everything goes through `Session::dispatch(Intent) -> SessionSnapshot` | avoids an O(visible rows) snapshot per arrow key in the hot TUI loop | [TUI.md](TUI.md); [ADR 0003](../adr/0003-audit-remediation-undo-cap-and-tui-dispatch-boundary.md) |

## 5. Rendering

| Divergence | TUI | Web | Why | Authority |
|---|---|---|---|---|
| KIND badge vocabulary | monospace bracket tags (`[T/S]`, `[S:mstr]`, `[D:odt]`) | a `label·notation` pill (`{}·scope`, `[]·multi`, `str·"…"`) | two notations for two widgets — deliberately **not** unified, incl. in the Help legend | [KEYMAP.md](KEYMAP.md) §Help overlay parity; [TUI.md](TUI.md); [WEBUI.md](WEBUI.md) |
| Fuzzy-filter match marks | repaints the foreground (reverse/bold) | a translucent wash (`<mark class="fz">`) so the value's own type color still reads through | a terminal cell can't layer alpha | `web/style.css` (`.fz`); `crates/confy-tui/src/tui/ui.rs` |
| Comment advisory (non-standard JSON comment) | underlined warn style; the full note lives in the `i` Detail popup | wavy underline + native hover tooltip (touch: the detail sheet) | terminals have no hover | `crates/confy-tui/src/tui/ui.rs`; `web/style.css` (`.comment-advisory`) |

## 6. Chrome, messages, capabilities

| Divergence | TUI | Web | Why | Authority |
|---|---|---|---|---|
| Notice surfaces | one status-bar slot for every severity | desktop: Success → toast, Info/Warn → status line, Error → a click-to-clear `#error` box; touch: **one toast for everything**, Warn/Error held longer | each host's spare screen real estate differs | [MESSAGES.md](MESSAGES.md) §5.1–5.3 |
| Host-level dialogs | none | the VS Code *extension host* uses native `showErrorMessage` — English, outside i18n, never in `Session.notice` or the diag ring | it runs outside the webview | [MESSAGES.md](MESSAGES.md) §5.4 |
| Non-interactive output | `confy convert` prints to stdout/stderr with exit codes — no Notice slot at all | n/a | Unix CLI convention | [MESSAGES.md](MESSAGES.md) §5.5 |
| Chrome trimming | status bar + column header | VS Code and Tauri hide header row 1 / Undo-Redo and relocate the Raw-Tree toggle, deferring to the native menu & workbench | never duplicate a native control | [CHROME.md](CHROME.md) §Per-host trimming; [TAURI.md](TAURI.md); [VSCODE.md](VSCODE.md) |
| Save affordance | keys only (`Ctrl+S`, `C` for Convert) | desktop split button (Save ∕ Save As…); touch one Save button opening an action sheet | the touch split pill regressed on mobile CSS | [CHROME.md](CHROME.md) |
| Breadcrumb bar | none (title/status carry the path) | desktop, Tauri, VS Code only — deliberately **absent on touch** | touch is sheet-driven with a weak cursor concept | [WEBUI.md](WEBUI.md) §Breadcrumb bar |
| Native menu accelerators | n/a | Tauri's Edit-menu node items carry **no** OS accelerators, and no predefined clipboard items | a window-menu accelerator steals the keystroke before the webview sees it, breaking text inputs | [TAURI.md](TAURI.md) §Desktop menu |
| File I/O | direct fs + atomic temp-rename + BOM re-emit | File System Access API (+ download fallback, `?url=`); Tauri desktop `tauri_plugin_fs`; Android SAF via `tauri-plugin-confy-picker`; VS Code `TextDocument` | core is filesystem-free; every host brings its own sandbox | [CLAUDE.md](../../CLAUDE.md); [TAURI.md](TAURI.md); [VSCODE.md](VSCODE.md) |
| Save As / Convert to a **new** file | always available | unavailable on Tauri **Android** (M1) — writing in place is unaffected | no new-destination picker on mobile yet | [TAURI.md](TAURI.md) |
| Local `$schema` sibling file | resolved against the open file's directory | web resolves URLs only; Android can't reach a sibling file (SAF grants one file) | per-file URI grants | [TAURI.md](TAURI.md); [WEBUI.md](WEBUI.md) |
| Preferences | `~/.config/confy/config.toml` (`%APPDATA%` on Windows) — deliberately the terminal-tool path on macOS, **not** `~/Library/Application Support` | `localStorage` (`confy-lang`, `confy-theme`) | platform convention vs browser sandbox | [TUI.md](TUI.md); `crates/confy-tui/src/config.rs` |

## Not divergences

Called out because they look like ones: the **model, mutation legality, notice catalog,
severity table, action menu, add picker, filter/type-filter semantics, undo/redo, schema
validation and the clipboard's modal lock** are all core-owned and identical on every host —
see [MUTATIONS.md](MUTATIONS.md), [BEHAVIOR_MATRIX.md](BEHAVIOR_MATRIX.md),
[MESSAGES.md](MESSAGES.md), [ROW_STATE_MODEL.md](ROW_STATE_MODEL.md). A host that "fixes" one
of those locally is the bug.

## Maintenance

- Adding a host-specific behavior means **adding a row here in the same commit**, pointing at
  the doc that owns the detail. If no doc owns it, that is the signal the behavior needs one.
- Keep the detail in the authority doc; this file must stay a one-line-per-divergence index,
  so it can be read end-to-end before touching host code.
- Keyboard rows are mirrors of [`KEYMAP.md`](KEYMAP.md); change that file first.
