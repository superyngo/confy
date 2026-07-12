# Desktop (Tauri) native menu plan — File/Edit/View/Help

**Date:** 2026-07-12 · **Status:** planned, approved, not started
**Scope:** `web/` only (new `web/menu.ts` + wiring in `ui.ts`/`fs.ts` + i18n keys). Zero Rust
changes — `read_file_text` already supports recent-files reopen. Zero `confy-core` changes,
so **no wasm rebuild** is needed.

## Goal

The Tauri desktop shell (`confy-desktop`) gets a native system menu bar mapped to existing app
functions. Menu logic lives in JS (i18n catalogs, lang preference, recent files, and every
Intent already live there). The menu is built as early as possible in `main()` — before the
wasm load — so it appears during the startup gap; Quit/About use `PredefinedMenuItem` so they
work even if wasm init fails. The pure-web build is completely unaffected (runtime `isTauri()`
guard).

## Governing facts (read these files first)

- `crates/confy-tauri/tauri.conf.json`: `withGlobalTauri: true` — the full Tauri JS API is on
  `window.__TAURI__` (`.menu`, `.webview`). **Do not add an npm dependency**; write minimal
  ambient types exactly like `web/fs.ts:74-80` does for `__TAURI__.core`.
- `crates/confy-tauri/capabilities/default.json`: `core:default` already includes
  `core:menu:default` — no capability changes.
- `web/fs.ts`: `isTauri()` (l.83), `tauriCore()` (l.77), `tauriHandle(path, name)` (l.94,
  private), `OpenedFile { handle, name, text }` (l.62), `pickOpenFile` (l.139), `pickSaveFile`
  (l.158), `tauriStartupFile` (l.113).
- `web/ui.ts`: `main()` (l.131) — menu setup goes at the top, **before** `await load(wasmUrl)`;
  `doOpen` (l.739), `doSave` (l.682), `chooseLang` (l.124), `send(intent)` (l.585), `io: HostIo`
  (l.106, has `ok`/`err` → status line), `openText` (l.156).
- `web/host-io.ts`: `toggleTheme` (l.192).
- `web/i18n.ts`: `t()`, `getLang()`, `availableLangs`, `LANG_DISPLAY_NAMES`.
- Intents (`web/types.ts`): `"Undo"`, `"Redo"`, `"CopySelected"`, `"CutSelected"`, `"Paste"`,
  `"EnterHelp"` (opens Help overlay on the Help tab), `"ToggleHelpTab"` (switches Help↔About).
- Tauri v2 zoom: `window.__TAURI__.webview.getCurrentWebview().setZoom(factor)`.
  `zoomHotkeysEnabled: true` (tauri.conf.json) already provides Cmd+/−/0 hotkeys.

## Menu structure (final)

```
[macOS app submenu]  About confy (Predefined) | Separator | Hide / HideOthers / ShowAll
                     (Predefined) | Separator | Quit (Predefined)
File   Open… (CmdOrCtrl+O) | Open Recent ▸ (dynamic; entries + Separator + Clear Recent)
       | Save (CmdOrCtrl+S)     [Windows only: Separator + Quit (Predefined) at the bottom]
Edit   Undo/Redo/Separator/Cut/Copy/Paste/SelectAll (all Predefined — they target focused
       TEXT FIELDS via native selectors; macOS-only effect, harmless on Windows)
       Separator
       Undo (z) | Redo (y) | Copy Node (c) | Cut Node (x) | Paste Node (v)
       ↑ node ops: NO accelerator (see "Accelerator policy"); the key shown in the label
       is a hint only — actual keyboard handling stays in ui.ts onKey.
View   Toggle Theme | Separator | Zoom In / Zoom Out / Reset Zoom | Separator | Language ▸
       (one CheckMenuItem per availableLangs, checked = getLang())
Help   Help | About    (both open the in-app overlay: EnterHelp, then ToggleHelpTab for About)
```

### Accelerator policy (the one dangerous design point)

Menu accelerators intercept keys **before** the webview sees them. confy's node operations use
unmodified keys (`c`/`x`/`v`/`z`/`y`), and text inputs (inline edit, panel fields, search box)
need native Cmd+C/X/V/Z/A. Therefore:

- **Never** bind `CmdOrCtrl+C/X/V/Z/Y/A` to node-op menu items — that would break text editing
  inside every input.
- Node-op items get **no accelerator at all**; append the plain-key hint to the label text
  (e.g. `Copy Node (c)`).
- `CmdOrCtrl+O` / `CmdOrCtrl+S` are unused by the app — safe for Open/Save.
- Zoom items get **no accelerators** (`zoomHotkeysEnabled` already owns Cmd+/−/0; registering
  them again on menu items would shadow the built-in path). Known accepted quirk: the JS-tracked
  zoom factor and the built-in hotkey zoom can diverge; do not try to sync them.

### macOS `setAsAppMenu` gotcha

`setAsAppMenu()` **replaces the entire default menu bar**, including the app submenu with
Quit/Hide. The first submenu on macOS must be the app submenu rebuilt from PredefinedMenuItems
(structure above) or Cmd+Q disappears. On Windows the menu renders as an in-window menubar and
there is no app submenu — put a Predefined Quit at the bottom of File instead (platform check:
`navigator.platform` / `userAgent`; keep it simple).

## Tasks

### Task 1 — `web/menu.ts` (new module)

- Ambient types for `window.__TAURI__.menu` (`Menu`, `Submenu`, `MenuItem`, `CheckMenuItem`,
  `PredefinedMenuItem` — each with a static `new(opts)` returning a Promise) and
  `window.__TAURI__.webview.getCurrentWebview()`, following the `fs.ts` TauriCore pattern.
- `export interface MenuDeps { doOpen, doSave, send, toggleTheme, chooseLang, openRecentPath,
  err }` — plain function fields passed from `ui.ts`; no imports from `ui.ts` (avoid cycles).
- `menuAction(fn)` wrapper: every handler goes through it — `try { await fn() } catch (e) {
  deps.err(String(e)) }`. Menu-event errors are silently-swallowed unhandled rejections
  otherwise. Handlers that need the Session no-op gracefully while wasm isn't loaded yet
  (deps close over `ui.ts` state; `send` already guards on `session`).
- `export async function setupAppMenu(deps)`: `if (!isTauri()) return;` then build the full
  menu and `setAsAppMenu()`. Wrap the whole body in try/catch → `deps.err` (a menu failure must
  never block app boot).
- `export async function rebuildMenu()`: rebuild + `setAsAppMenu()` again (labels re-read via
  `t()`, recent list re-read from storage, language check states re-read from `getLang()`).
  Called on language change and after every recent-files mutation. Guard against concurrent
  rebuilds (a simple in-flight flag is enough).
- Zoom: module-local `let zoom = 1`; In/Out step ±0.1 clamped to [0.3, 3]; Reset → 1;
  `getCurrentWebview().setZoom(zoom)`.

**Verify:** `npx tsc --noEmit` clean; browser (non-Tauri) build behaves identically (menu code
is a no-op there).

### Task 2 — Recent files

- Storage: `localStorage["confy-recent"]` = JSON `[{path, name}]`, most-recent first, cap 8,
  dedupe by path. Tauri-only (paths are only meaningful there). Small helpers in `menu.ts`
  (or a tiny `web/recent.ts` if `menu.ts` gets crowded): `recentList()`, `recentAdd(path,
  name)`, `recentClear()`, `recentRemove(path)`.
- `web/fs.ts`: add optional `path?: string` to `OpenedFile`, populated in the two Tauri
  branches (`pickOpenFile`, `tauriStartupFile`). Add
  `export async function openTauriPath(path): Promise<OpenedFile | null>` —
  `read_file_text` via invoke → wrap with the existing `tauriHandle`; return `null` on error.
- `web/ui.ts`: record recents wherever a Tauri path becomes known — after `pickOpenFile`/
  `tauriStartupFile` succeed in `doOpen`/`main`, and after a Tauri save-as in `doSave`
  (`pickSaveFile` returns a handle; capture the path there — extend its Tauri branch to also
  expose the path, mirroring the `OpenedFile.path` approach). Then `rebuildMenu()`.
- Menu handler `openRecentPath(path)`: `openTauriPath` → found: `openText(text,
  formatFromName(name), handle, name)` + `recentAdd`; not found: `recentRemove(path)` +
  `rebuildMenu()` + error status ("file no longer exists" — i18n key).
- "Clear Recent" item → `recentClear()` + `rebuildMenu()`.

**Verify:** open a file → appears in Open Recent; reopen from menu works; delete the file on
disk, reopen from menu → entry disappears + visible error; list caps at 8 and dedupes.

### Task 3 — `ui.ts` wiring

- Top of `main()` (before `await load(wasmUrl)`): `void setupAppMenu({...})` — **not awaited**
  (menu building is multiple async IPC round-trips; don't delay wasm load on it).
- `chooseLang` (l.124): append `void rebuildMenu()`.
- Help/About handlers: `send("EnterHelp")`; About additionally `send("ToggleHelpTab")` when the
  Help overlay opens on the Help tab (check `snap.mode` after the first send, or send both —
  match whatever a quick read of the dispatch code shows is idempotent).

**Verify:** in `cargo tauri dev` (or a `--debug` build) the menu is visible before the tree
renders; every item drives the same behavior as its toolbar/keyboard equivalent.

### Task 4 — i18n keys

- Add `web.menu.*` keys to `i18n/en.json` **and** `i18n/zh-TW.json` (file, open, openRecent,
  clearRecent, save, edit, undo, redo, copyNode, cutNode, pasteNode, view, toggleTheme, zoomIn,
  zoomOut, zoomReset, language, help, helpItem, about, recentGone — adjust as needed). The
  existing catalog test asserts en/zh-TW key parity — run core tests to confirm.
- Menu labels always render through `t()` at (re)build time; language switch → `rebuildMenu()`
  relabels everything.

### Task 5 — docs + build verification

1. `CHANGELOG.md`: `Unreleased` entry (feat(desktop): native menu bar …).
2. `WEBUI.md`: new short section on the desktop menu (module, structure, accelerator policy);
   `CLAUDE.md` module map: add `menu.ts` line under `web/`.
3. Build checks (all must pass):
   - `npx tsc --noEmit` in `web/` (or the repo's existing tsc invocation — see
     `web/package.json` scripts).
   - esbuild bundle — **must run from a scratchpad copy of the repo, not from
     `/Volumes/Home/...`** (esbuild deadlocks on that volume path); copy `web/` out, build,
     copy `dist/` back. See `web/build.mjs` / `cf-build.sh`.
   - `node web/…functional_smoke` equivalent if the repo's completion checklist asks for it
     (core unchanged, so `crates/confy-ffi/functional_smoke.mjs` should pass untouched).
   - `cargo build -p confy-tauri` (compile check only; no `--release`, it's very slow).
   - `cargo test -p confy-core` (catalog parity test).
4. **Do NOT attempt to drive the GUI programmatically** (no pty, no long-lived background
   processes). Manual GUI verification (menu clicks, text-field shortcuts not stolen, wasm-
   failure Quit still working) is done by the user — list what to check in the final report.

## Explicitly out of scope (do not build)

- `Cmd+C/X/V/Z` bound to node operations (breaks text inputs — see accelerator policy).
- Rust-side menu construction; dock/jump-list recent files; Save item enabled/disabled
  tracking `is_dirty`; syncing the zoom factor with `zoomHotkeysEnabled`'s built-in zoom.
- Any `confy-core` / wasm change.

## Working agreement

- Branch off `main`; **never push or merge without being asked**.
- Minimal, surgical diffs; match existing code style (see `web/fs.ts` ambient-type pattern).
- Follow the repo's CHANGELOG/docs habit (Task 5).
