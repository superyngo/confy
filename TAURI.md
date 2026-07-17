# TAURI.md — confy desktop + mobile app shell (`confy-tauri`)

`crates/confy-tauri` is a Tauri v2 shell over the `web/` bundle — desktop (macOS/Windows)
and, since Mobile M1, Android — adding only native file I/O and a menu bar on top of the
same Session/webview contract `WEBUI.md` documents. This file covers what's specific to
the Tauri host; the web bundle itself (render/pointer internals, touch UI, i18n, PWA,
deployment) stays in `WEBUI.md`. See CLAUDE.md's module map for the crate's file layout
and build commands (`cargo tauri build` / `cargo tauri android build`), and
`confy-tauri-lessons` (memory) for durable architecture lessons (B-lite pattern,
`window.__TAURI__` globals, capability sub-sets, RGBA icons, the slow release profile).

## Desktop menu (Tauri)

`web/menu.ts` builds a native File/Edit/View/Help menu bar for the Tauri desktop shell via
`window.__TAURI__.menu`/`window.__TAURI__.webview` (`withGlobalTauri: true` in
`tauri.conf.json`, so no `@tauri-apps/api` npm dependency — minimal ambient types follow the
`fs.ts` `TauriCore` pattern). `setupAppMenu(deps)` is a no-op on the pure web build
(`isTauri()` guard) and is called from the top of `ui.ts`'s `main()`, **before** `await
load(wasmUrl)` and **not awaited** — menu construction is several async IPC round-trips and
must not delay the wasm boot; this also means the menu is visible during the startup gap, and
Quit/About use `PredefinedMenuItem` so they still work if wasm init fails. `rebuildMenu()`
rebuilds and reinstalls it (`setAsAppMenu()`) on language change and after every recent-files
mutation, re-reading labels via `t()`, the recent list, and `getLang()` each time; an in-flight
flag drops concurrent rebuilds.

**Structure:** File (New `CmdOrCtrl+N` — discards the current doc and loads the default toml
sample, i.e. `loadSample("toml", openSample)`, the same fallback `main()` takes with no
startup file/URL; no confirmation, matching a browser refresh / Open `CmdOrCtrl+O` / Open
Recent ▸ dynamic submenu / Save `CmdOrCtrl+S`),
Edit (native `Predefined` Cut/Copy/Paste/Undo/Redo/SelectAll acting on focused text fields,
plus node-op items Undo/Redo/Copy/Cut/Paste Node), View (Toggle Theme / Zoom In-Out-Reset /
Language ▸ one `CheckMenuItem` per `availableLangs()`, checked = `getLang()`), Help (Help /
About — both send `EnterHelp`, About additionally sends `ToggleHelpTab` to flip onto the About
tab, mirroring `enter_help`/`toggle_help_tab` in `session.rs`). macOS gets a rebuilt app
submenu ("About confy"/Hide/HideOthers/ShowAll/Quit) since `setAsAppMenu()` replaces the
entire default menu bar including Cmd+Q; "About confy" is a custom `MenuItem` (not
`Predefined`) using the same `EnterHelp`+`ToggleHelpTab` handler as the Help menu's About, so
it opens the in-app About overlay instead of macOS's native About panel — one consistent
About surface across platforms. Windows has no app submenu, so a `Predefined` Quit sits at
the bottom of File instead (`navigator.platform`/`userAgentData` check).

**`PredefinedMenuItem.item` gotcha:** every predefined kind is a plain Rust unit variant
serialized as a bare string (`"Quit"`, `"Hide"`, …) — **except** `About`, which the Rust side
models as a newtype variant carrying `Option<AboutMetadata>` and must be sent as
`{ item: { About: null } }`; a bare `"About"` string fails IPC deserialization
(`invalid type: unit variant, expected newtype variant`). This is moot now that the app
submenu's About is a custom item rather than `Predefined`, but the gotcha applies to any
future `PredefinedMenuItem.new({ item: "About" })` call.

**Accelerator policy** (the one dangerous design point): node-op items get **no accelerator
at all** — the plain-key hint (`c`/`x`/`v`/`z`/`y`) is a label suffix only, e.g. `Copy Node
(c)`; actual handling stays in `ui.ts`'s `onKey`. Binding `CmdOrCtrl+C/X/V/Z/Y` to a menu item
would intercept the key **before** the webview sees it, breaking native copy/cut/paste/undo
inside every text input (inline edit, panel fields, search box). Zoom items also get no
accelerator — `zoomHotkeysEnabled` (`tauri.conf.json`) already owns Cmd+/−/0; the JS-tracked
zoom factor (`menu.ts`'s module-local `zoom`, `±0.1` steps clamped to `[0.3, 3]`) is a known,
accepted, not-synced duplicate of that built-in path. `getCurrentWebview().setZoom()` needs
`core:webview:allow-set-webview-zoom` explicitly in `capabilities/default.json` —
`core:webview:default` does not include it.

**GC-retention gotcha:** `buildAndSet()` keeps the built root `Menu` in the module-level
`installedMenu` variable and never lets it go out of scope. Every `Menu`/`Submenu`/`MenuItem`
JS wrapper is backed by a Tauri resource (including the click-action channel); if nothing in
JS references the tree after `setAsAppMenu()` returns, V8 is free to garbage-collect it at any
later point, tearing down those resources while the native OS menu bar keeps showing the —
now silently unresponsive — items. A large allocation spike (e.g. opening a file and swapping
in a fresh wasm `Session`) is a classic GC trigger, which is how this first surfaced. Children
don't need their own persistent JS references — they stay alive via the Rust-side tree the
root `Menu` resource owns.

**Recent files:** `localStorage["confy-recent"]` (Tauri-only — paths are only meaningful
there), most-recent-first, cap 8, deduped by path. `fs.ts`'s `OpenedFile`/`FsHandle` both grew
an optional `path` field (populated only on the Tauri branches of `tauriStartupFile`,
`pickOpenFile`, and `tauriHandle` — so `pickSaveFile`'s returned handle carries it too);
`ui.ts` calls `recentAdd` + `rebuildMenu()` wherever a Tauri path becomes newly known (startup
file, Open, Save As), and `openTauriPath(path)` (new `fs.ts` export, `read_file_text` via
`invoke`) backs the menu's `openRecentPath` handler — a missing/unreadable file calls
`recentRemove` + `rebuildMenu()` + an error status instead of opening.

## Mobile (Tauri Android)

Android reuses the touch UI verbatim (same `web/touch/` module, same `confy.ts`/`Intent`
contract) — the mobile-specific surface is entirely in host I/O (`web/fs.ts`) and a couple of
platform guards, not a separate UI.

**Picker + file-association I/O.** `fs.ts::isTauriAndroid()` (UA-sniffed, no `tauri-plugin-os`
dependency) forks `pickOpenFile()` to call the first-party `plugin:confy-picker|pick_writable`
command instead of `dialog.open()` — stock `tauri-plugin-dialog`'s Android picker uses
`ACTION_GET_CONTENT`, which never grants write access at all. Opening a file via the OS's "Open
with" chooser instead arrives through `tauri.android.conf.json`'s `fileAssociations` (Rust-side
`opened_urls`/`"opened"` event) and reads through the same `openTauriPath`-style path — no plugin
needed there, since a file-association launch intent's own grant covers the receiving activity's
lifetime. `menu.ts`'s native menu bar no-ops on Tauri mobile (same `isTauriMobile()` guard as
`canSaveAs()`) — there's no menu bar on Android.

**`canSaveAs()` gating.** False on Tauri mobile: picking a *new* save destination (Save As, first
Save after File-New-equivalent, Convert's output path) isn't supported in M1, so those paths show
a translated hint (`web.mobile.saveAsUnavailable`) instead of opening a picker. Writing in place
to an already-open handle is unaffected by this flag — `doQuickSave` only consults it on the
no-handle-yet (first save) branch.

**The split-button lesson (why Save is one plain button, not a pill).** An earlier iteration
tried merging the Save button and a "Save As / Convert…" chevron into one visually-glued
`.split-btn` pill. It rendered as two buttons stacked top-to-bottom on a real device with no
visible CSS explanation — root cause: **`web/touch/` has its own separate stylesheet
(`touch/style.css`), not the shared desktop `web/style.css`**, and the `.split-btn` CSS rule (and,
separately, the `env(safe-area-inset-top)` toolbar padding fix) had only been added to the
desktop file. Any style fix aimed at touch must land in `touch/style.css`, not `style.css` — the
two are not the same cascade and nothing here shares rules between them by default. Once fixed and
seen live, the pill design itself was dropped in favor of the plain single-button-opens-a-sheet
design described in `WEBUI.md`'s Touch UI section — simpler, and immune to this whole class of bug.

**Debugging technique — live CDP against the on-device WebView.** Android's WebView exposes a
Chrome DevTools Protocol endpoint when the app is debuggable: `adb forward tcp:PORT
localabstract:webview_devtools_remote_<pid>` (find `<pid>` via `adb shell ps -A | grep
<package>`), then `curl http://localhost:PORT/json` for the page's `webSocketDebuggerUrl`. A
plain WebSocket client can then send `Runtime.evaluate` (and other CDP methods) directly — no
`chrome://inspect` UI needed. One gotcha: the devtools server 403s a connection whose `Origin`
header doesn't match an allowlist, so connect with `suppress_origin=True` (Python
`websocket-client`) or an equivalent that omits the header. Combined with `adb shell input
tap`/`screencap` to drive the actual system UI (document pickers, "Open with" choosers, the home
screen), this lets bugs get root-caused and fixes verified end-to-end on real hardware without a
human re-testing every iteration.
