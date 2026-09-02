# TAURI.md — confy desktop + mobile app shell (`confy-tauri`)

`crates/confy-tauri` is a Tauri v2 shell over the `web/` bundle — desktop (macOS/Windows)
and, since Mobile M1, Android — adding only native file I/O and a menu bar on top of the
same Session/webview contract `WEBUI.md` documents. This file covers what's specific to
the Tauri host; the web bundle itself (render/pointer internals, touch UI, i18n, PWA,
deployment) stays in `WEBUI.md`. See CLAUDE.md's module map for the crate's file layout
and build commands (`cargo tauri build` / `cargo tauri android build`), and
`confy-tauri-lessons` (memory) for durable architecture lessons (B-lite pattern,
`window.__TAURI__` globals, capability sub-sets, RGBA icons, the slow release profile).

## Content Security Policy

`tauri.conf.json`'s `app.security.csp` is **set**, not `null` (Tauri's default, which ships no
CSP at all):

```
default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline';
img-src 'self' data:; connect-src 'self' https: http: ipc: http://ipc.localhost;
object-src 'none'; base-uri 'self'; frame-ancestors 'none'
```

Why each non-obvious entry is needed: `'wasm-unsafe-eval'` for the wasm core (`confy-ffi`);
`'unsafe-inline'` in `style-src` for the `style="…"` attributes `render.ts`/`panel.ts` emit
(row indents, edit-field widths); `https:`/`http:` in `connect-src` for remote `$schema` hints
and Open-from-URL; `ipc:`/`http://ipc.localhost` for Tauri's own IPC transport.

**Consequence for the web bundle: no inline `<script>` anywhere.** `script-src` deliberately
omits `'unsafe-inline'`, so the two boot scripts every HTML entry needs live in external files —
`entry-desktop.js` / `entry-touch.js` (the pointer-based desktop↔touch router, which must stay
the first element in `<head>`) and `register-sw.js` (https-only service-worker registration).
Inlining either one back into `index.html`/`touch.html` silently breaks the touch redirect and
the PWA on the desktop app while the tree itself still renders — the failure is invisible
without checking the webview console. Any new file must also be added to `assemble-dist.mjs`'s
copy list or it won't reach `web/dist` (and therefore not the app bundle).

## Desktop menu (Tauri)

`web/menu.ts` builds a native File/Edit/View/Help menu bar for the Tauri desktop shell via
`window.__TAURI__.menu`/`window.__TAURI__.webview` (`withGlobalTauri: true` in
`tauri.conf.json`, so no `@tauri-apps/api` npm dependency — minimal ambient types follow the
`fs.ts` `TauriCore` pattern). `setupAppMenu(deps)` is a no-op on the pure web build
(`isTauri()` guard) and is called from the top of `ui.ts`'s `main()`, **before** `await
load(wasmUrl)` and **not awaited** — menu construction is several async IPC round-trips and
must not delay the wasm boot; this also means the menu is visible during the startup gap, and
Quit uses `PredefinedMenuItem` so it still works if wasm init fails (About is a custom item that
needs the wasm Session — see below). `rebuildMenu()`
rebuilds and reinstalls it (`setAsAppMenu()`) on language change and after every recent-files
mutation, re-reading labels via `t()`, the recent list, and `getLang()` each time; an in-flight
flag drops concurrent rebuilds.

**Structure:** File (New ▸ TOML `CmdOrCtrl+N`/JSON/YAML — discards the current doc and loads
the built-in sample in the chosen format, i.e. `loadSample(format, openSample)`, the same
fallback `main()` takes with no startup file/URL; no confirmation, matching a browser refresh /
Open ▸ Browse Local File `CmdOrCtrl+O` (native picker, unchanged `doOpen`) / Open from URL…
(the existing combined open modal, `openUrlModal()` focuses the URL field directly instead of
the Browse button) / Open Recent ▸ dynamic submenu / Save `CmdOrCtrl+S`),
Edit (node-op items only — Undo/Redo/Copy/Cut/Paste Node as custom `MenuItem`s dispatching
Session intents; deliberately **no** native `Predefined` Cut/Copy/Paste/SelectAll, which would
compete with the webview's own focused-text-field handling), View (Toggle Theme / Zoom In-Out-Reset /
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
file, Open, Save As), and `openTauriPath(path)` (`fs.ts` export, `fs.readTextFile` via
`tauri-plugin-fs`) backs the menu's `openRecentPath` handler — a missing/unreadable file calls
`recentRemove` + `rebuildMenu()` + an error status instead of opening.

## Chrome trimming (Desktop)

`document.body.classList.add("host-tauri-desktop")` (`ui.ts`'s `main()`, guarded by the
module-level `TAURI_DESKTOP = isTauri() && !isTauriMobile()` flag). The full header/
filter-row trimming and relocation rules for this host are documented once, alongside
every other host, in **`CHROME.md`** — not restated here. In short: the whole
`header.toolbar` is hidden, the same trim VS Code's `host-vscode` class applies
(`CHROME.md`): Open/Save/Save-As/Convert/Undo/Redo/theme/language/Help-About all live in
the native menu bar above instead. The filter row (search/type-filter/Expand-Collapse,
plus the Raw/Tree toggle relocated in from the header) stays.

The header also carried two pure status displays with no menu equivalent — the format pill and
the dirty-dot — so those move to the native OS window title instead: `menu.ts`'s
`setWindowTitle(fileName, format, dirty)` (`window.__TAURI__.window.getCurrentWindow().setTitle`,
needing `core:window:allow-set-title` explicitly in `capabilities/default.json` — not covered by
`core:default`) renders `"● name · FORMAT — confy"` (dirty prefix omitted when clean), called
from `render()` every snapshot, deduped against the last-set string so an unchanged render is a
no-op IPC-wise.

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
lifetime. `menu.ts`'s native menu bar no-ops on Tauri mobile (its own `isTauriMobile()` guard at
`setupAppMenu`/`rebuildMenu`) — there's no menu bar on Android.

**Save As (M2, 2026-08-06).** `canSaveAs()` now returns `true` on every platform — picking a *new*
save destination (Save As, first Save after File-New-equivalent, Convert's output path) was
hardcoded `false` on Tauri mobile in M1 (stock `tauri-plugin-dialog`'s Android `saveFileDialog`
never took a persistable write grant). Fixed with a new `create_writable` command in
`tauri-plugin-confy-picker` (`ACTION_CREATE_DOCUMENT` + `takePersistableUriPermission`, the same
shape as `pick_writable` above); `fs.ts::pickSaveFile()` forks to it on `isTauriAndroid()`. Writing
in place to an already-open handle was always unaffected by this flag — `doQuickSave` only
consulted it on the no-handle-yet (first save) branch, which is now unconditional everywhere except
the VS Code webview host (`ui.ts`'s separate `VSHOST` gate, unrelated to mobile). See
`docs/adr/0001-android-save-as-persistable-grant.md` and
`docs/superpowers/plans/2026-08-06-mobile-m2-saveas-fileassoc-plan.md` for the full rationale and
the kill+relaunch persistable-grant verification.

### JSON Schema on Android

A local/relative-path schema hint (`#:schema ./s.json` or a bare relative-path
`$schema` value) cannot resolve on Android: `tauri-plugin-confy-picker`'s only commands
(`pick_writable`, `create_writable`) grant a persistable SAF URI to exactly the *document
being opened*, not a directory — there is no way to read a second file relative to it. This
degrades soft (`SchemaStatus.load_error`, editing unaffected) — see ADR 0001 for why
`pick_writable` exists (a durability gap, not a read/write capability gap) and
`docs/superpowers/specs/2026-08-10-json-schema-support-design.md`'s Tauri/Android section
for the full reasoning. **URL-based hints work identically to desktop** — no new capability
needed (plain `fetch()`, already used by "Open from URL…"). Schema attachment is
detection-only (in-document `$schema`/`#:schema`/yaml-language-server-modeline annotations
via `Session::detect_and_request_schema`) on every host, Android included — there is no
manual "attach a schema file" action anywhere in the UI.

**Open-with / share chooser visibility (M2 manifest hand-edit).** Confy didn't reliably appear
when opening/sharing `.toml`/`.json`/`.yaml` from a file manager — root cause: the auto-generated
`AndroidManifest.xml` intent-filters (from `tauri.android.conf.json`'s `bundle.fileAssociations`)
declare `android:mimeType` with no `android:scheme`, so Android's `pathPattern` matching (which
requires a scheme) never activates, leaving a match dependent on the firing file manager's own
MIME-type guess. Fixed with hand-authored `<intent-filter>` blocks (one per extension group;
VIEW + SEND + SEND_MULTIPLE, `scheme=content`/`file`, `host=*`, wildcard `mimeType="*/*"`
constrained by a real `pathPattern`) placed in
`crates/confy-tauri/gen/android/app/src/main/AndroidManifest.xml` immediately after the
`<!-- tauri-file-associations. AUTO-GENERATED. DO NOT REMOVE. -->` **closing** marker, still inside
`<activity>...</activity>` — same manual-maintenance pattern M1 used for the icon/theme/status-bar
edits (this file is committed to git; hand-edits *outside* the marker pair survive every
`cargo tauri android build`/`dev` regeneration, edits *inside* them don't). Re-add if a full
`cargo tauri android init` ever regenerates this file from scratch.

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

**Google Play (in progress, blocked on account creation).** `gen/android/app/build.gradle.kts`
has a conditional release `signingConfig` (reads a gitignored, per-machine
`keystore.properties` — see `keystore.properties.example`; falls back to an unsigned release
build when absent, so this doesn't affect the M1 debug-sideload flow above) and a
`CONFY_VERSION_CODE`-env-var-driven `versionCode` for future tag-derived CI builds (deliberately
not Tauri's `autoIncrementVersionCode`, whose counter lives in a gitignored file that resets on
every fresh CI clone). No Play Console account exists yet (needs the human: $25 one-time,
plus Google's ≥12-tester/14-consecutive-day closed-testing gate for personal accounts created
after 2023-11-13 before production access), so there's no `publish-play.yml` CI. Status tracked
in `RELEASES.md`'s "Android Google Play" row. See also `crates/confy-tauri/play/` (draft Play
Store feature graphic) and `PRIVACY.md` / `web/privacy.html` (the privacy-policy URL every store
listing points to).

**Build/sign/install verified end-to-end (2026-08-06).** The M1 toolchain setup
(`docs/superpowers/plans/2026-07-13-mobile-m1-android-plan.md`) is CLI-only and still current —
no Android Studio installed or needed (`android-commandlinetools` under
`ANDROID_HOME=/opt/homebrew/share/android-commandlinetools`: build-tools 34/35, platforms 34/36,
NDK 27, plus JDK 21 and the `rustup` android targets). `cargo tauri android build --debug --apk`
and `--apk` (release) both ran clean on the current tree; `apksigner verify`/`aapt` confirmed the
release APK is `confy-release`-signed (not debug), `usesCleartextTraffic=false` (vs `true` in
debug) resolves correctly per build type, and proguard/minify shrinks the universal release APK
585MB→25.6MB. Both variants installed and launched on a real device without crashing, and a
tap-to-select interaction confirmed the wasm↔JS bridge survives release minification. One real
bug found and fixed along the way: `keystore.properties`' `keyPassword` didn't match
`confy-release.keystore` — it's a PKCS12 keystore, which uses one password for both the store and
the key (`keytool -keypasswd` refuses PKCS12 outright), so the separately-recorded key password
was simply never validated; release builds failed with `KeytoolException: ... Given final block
not properly padded` until `keyPassword` was corrected to match `storePassword`. The 7/15 M1
manual acceptance pass was left unfinished (work moved to VS Code); at the time of this
verification pass "另存新檔"/Save As was still the known, deliberate M1 gap — since resolved by
the M2 work above.
