# Help/About panel, header info button, overlay z-index fix, unified Open popup

Date: 2026-07-09

## Context

Four related UI gaps across the TUI and Web UI:

1. No About panel (author/version/GitHub/license) exists anywhere.
2. Web UI's `#overlay` (Help/Prompt/KindSwitch fallback chrome) can render with its top
   clipped so it looks covered by the header.
3. No dedicated header button opens Help; only the `?` key does.
4. The Web UI's "Open from URL" modal only offers a URL field — no way to also browse a
   local file from the same popup, and the header's "Open" button bypasses it entirely.

## 1. Shared Help/About panel

**Single source of truth in `confy-core`**, so the TUI and Web UI stay in lockstep and
neither reimplements tab state:

- `crates/confy-core/src/session/state.rs`: `Mode::Help` becomes `Mode::Help(HelpTab)`.
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
  pub enum HelpTab { Help, About }
  ```
  `enter_help()` always resets to `HelpTab::Help`.
- New `toggle_help_tab()` method on `Session` flips `Help <-> About` while in `Mode::Help(_)`
  (no-op otherwise).
- New static About text (e.g. in `session/state.rs` or a small `about.rs`), built from
  crate metadata:
  ```rust
  pub const ABOUT_TEXT: &str = concat!(
      "confy ", env!("CARGO_PKG_VERSION"), "\n",
      "A cross-platform TUI/Web UI for editing structured configuration files.\n",
      "\n",
      "Author:    wen\n",
      "License:   MIT\n",
      "Copyright: (c) 2026 wen\n",
      "GitHub:    https://github.com/superyngo/confy\n",
  );
  ```
- `dispatch.rs`: new `Intent::ToggleHelpTab`, routed only while `Mode::Help(_)`.
  `ModeView::Help` gains the active tab so `SessionSnapshot` carries it to the Web UI.

**TUI** (`crates/confy-tui/src/tui/`):
- `keys.rs`: `?` still maps to `KeyAction::Help` (enters/exits as today).
- `mod.rs`'s existing `Mode::Help` key-handling branch gets a `Tab`/`BackTab` arm calling
  `app.toggle_help_tab()` (dispatches `ToggleHelpTab`); all other keys (scroll, Esc) unchanged.
- `ui.rs::draw_help_overlay`: renders `keys::help_text(...)` for `HelpTab::Help` or
  `model::ABOUT_TEXT` for `HelpTab::About`; title bar reads
  `" Help | About (Tab to switch · ? or Esc) "` with the active tab visually marked
  (e.g. bracketed or bold via style, matching existing title-string convention).

**Web UI** (`web/ui.ts`):
- `renderOverlay()`'s `"Help"` branch renders two tab buttons (`Help` / `About`) above the
  content pane; the active tab (from `snap.mode.Help.tab`) gets a `.sel` class. Clicking a
  tab button sends `ToggleHelpTab`. Content below renders `HELP_TEXT + KIND_LEGEND[...]` or
  the About text (plus a real `<a href>` for the GitHub URL) depending on the active tab.
- No separate touch implementation is added unless touch currently renders its own Help
  overlay (verify at implementation time — if it does, mirror the same tab markup there).

## 2. Fix `#overlay` clipping/z-index bug

Root cause: `#overlay` (`position:absolute`) lives inside `.main`, which is
`position:relative; overflow:hidden` (`web/style.css:137`). `.main`'s box starts just below
the header/filterbar. When the popup's rendered height (up to `max-height:70vh`) pushes its
top edge above `.main`'s own top boundary — e.g. long Help text on a short viewport — the
`overflow:hidden` clips that portion, which reads as "the header is covering the panel."

Fix:
- Move `<aside id="overlay">` in `web/index.html` out of `.main` to be a sibling (grouped
  with the other top-level modals like `#url-modal`, `#ext-modal`).
- `web/style.css`: change `#overlay` to `position:fixed; top:50%; left:50%;
  transform:translate(-50%,-50%);` (drop the old `position:absolute; left:50%; top:40%`),
  keeping `max-width`/`max-height`/`z-index:40`. No longer inside any `overflow:hidden`
  ancestor, so it can never be clipped by `.main` again. This also benefits Prompt and
  KindSwitch, which share the same element.

## 3. Header info button

`web/index.html`: add an icon button `#btnInfo` in `.toolbar` (placed with the
theme/undo/redo `tgroup`, before `#btnMore`), SVG "info circle" glyph, `title="Help / About (?)"`.
`web/ui.ts`: wire `btnInfo.addEventListener("click", () => send("EnterHelp"))` — identical
entry point to the `?` keyboard shortcut.

## 4. Unified Open popup

Replace the URL-only `#url-modal` with a combined "Open" popup:

- `web/index.html`: `#url-modal`'s `<h3>` becomes `"Open"`; add a
  `<button id="url-browse">Browse local file…</button>` above a `<hr>`/divider label
  ("or open from URL"), keeping the existing `#url-input` + Open/Cancel buttons below.
- `web/ui.ts`:
  - Rename the opener (kept as `openUrlModal`/exposed as e.g. `openOpenModal`) so both the
    header `#btnOpen` click handler and (if kept) any leftover reference open this modal
    instead of calling `doOpen()` directly.
  - `#url-browse` click handler calls the existing `doOpen()` (FS Access API / hidden
    `#fileInput` fallback) and closes the modal on success/selection.
  - Remove the `"Open from URL…"` entry from `#moreMenu` (now redundant — the header Open
    button covers both paths).
- `web/touch/app.ts`: `openUrlSheet()` gains the same "Browse local file" button (calling its
  local `doOpen()`), with the sheet title changed to "Open"; the touch entry point that
  currently opens `doOpen()` directly (if any, e.g. a FAB "Open" action) is repointed to open
  this sheet instead, and the More-menu "Open from URL" item is removed there too. Full
  code-sharing between desktop and touch (like `panel.ts`) is a stretch goal, not required —
  keep the two implementations if factoring turns out to be more churn than value, and note
  that decision in the PR.

## Out of scope

- No changes to the underlying `doOpen()` / URL-fetch mechanics themselves.
- No new About content beyond version/author/license/GitHub/tagline (no dependency credits).
- No Tauri-desktop-specific Open-popup changes beyond what `web/fs.ts`'s existing Tauri
  detection already routes transparently.
