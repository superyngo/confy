# Mobile M1 plan — Android sideload APK (Tauri v2)

**Date:** 2026-07-13 · **Status:** SHIPPED — 2026-07-15. All tasks complete; acceptance criteria
verified on real hardware (user-confirmed pick → edit → save → kill app → reopen via "Open with").
**Spec:** `docs/superpowers/plans/2026-07-12-mobile-tauri-spec.md` (M1 phase; reviewed 2026-07-13,
this plan folds in the 5 review supplements)
**Execution:** new session, **executing-plans** skill, task-by-task with checkpoints.
**Scope:** `web/fs.ts` + `web/menu.ts` + touch-UI gating, `crates/confy-tauri/` (commands,
config, capabilities, Android gen project). **Zero changes to `confy-core`, `confy-ffi`, or the
`Intent`/`SessionSnapshot` contract — no wasm rebuild should be needed.** No new npm Tauri
dependencies (global-binding pattern, see Governing facts).

## Goal

A sideloadable Android APK (no store) that can open a config file from the system document
picker, edit it in the existing wasm `Session` + touch UI, save in place, and appear in
Android's "Open with" sheet for `.toml/.json/.jsonc/.yaml/.yml`. The desktop (macOS) app keeps
working identically after the shared I/O refactor — that regression gate is part of this plan,
not a separate task.

## Governing facts (read these files first)

- **Spec** (above) — especially §"Design: opaque-string handle, plugin-backed I/O" and
  §"Risks / research items".
- `web/fs.ts`: the **`FsHandle` contract** (l.13, `getFile`/`createWritable`) is the invariant.
  `tauriHandle(path, name)` (l.98) wraps an opaque path string and never inspects it —
  swapping the string for a `content://` URI must require no `ui.ts`/`touch/app.ts` changes.
  Current Tauri plumbing: `tauriCore()` (l.81), `isTauri()` (l.88), `tauriStartupFile()`
  (l.118), `openTauriPath` (~l.130), `pickOpenFile` (~l.160), `pickSaveFile` (~l.181).
- `crates/confy-tauri/src/main.rs`: the 5 commands to replace/keep — `open_dialog`,
  `save_dialog`, `read_file_text`, `write_file`, `startup_file`; `tauri_plugin_dialog` already
  a dependency (Rust-side).
- `crates/confy-tauri/tauri.conf.json`: `withGlobalTauri: true` — **plugin JS APIs are reached
  via `window.__TAURI__.fs` / `window.__TAURI__.dialog` globals, no npm dependency**; write
  minimal ambient types exactly like `web/menu.ts` does for `__TAURI__.menu` and `fs.ts:79`
  for `__TAURI__.core`.
- `crates/confy-tauri/capabilities/default.json`: current permissions `core:default`,
  `dialog:default`, `core:webview:allow-set-webview-zoom`. **Lesson from the desktop-menu
  work: `core:default`'s bundled sub-sets are curated subsets, not "everything in the
  namespace"** — expect to add explicit `fs:allow-read-text-file`/`fs:allow-write-text-file`
  (+ scope for picked paths/URIs) rather than assuming a `:default` covers it. Mobile
  capability files can be platform-scoped (`"platforms": ["android"]`).
- `web/menu.ts`: `setupAppMenu` (l.184) currently gates only on `isTauri()` (l.186) — true on
  mobile too, so it needs a platform guard.
- Save-destination paths that assume a save dialog (all need `canSaveAs()` gating on mobile):
  Save As (`ui.ts` `pickSaveFile` call ~l.745 and the touch equivalent), **File > New followed
  by first Save** (no opened URI to write back to), and **Convert output**
  (`doConvertWrite`, `ui.ts:268` + `convert-dialog.ts`).
- **Verified non-issue:** the service worker registers only when `location.protocol ===
  "https:"` (`index.html`/`touch.html`); Tauri webview origins are `tauri://` (macOS) and
  `http://tauri.localhost` (Android/Windows), so the PWA SW never activates inside the native
  shells. Don't "fix" this.
- **Workflow gotchas:** esbuild deadlocks under `/Volumes/Home` — bundle from a scratchpad
  copy (including `web/node_modules`) and copy artifacts back. Verify web changes with
  `npx tsc --noEmit` (in `web/`) + the scratchpad esbuild. `functional_smoke.mjs` runs from
  `crates/confy-ffi/` cwd. The workspace release profile is slow — use `--debug` bundles for
  local checks. TUI/GUI cannot be driven from the session — the user tests desktop and device
  manually; prepare exact instructions and wait for their report.

## Tasks

### Task 0 — Day-1 spike: Android toolchain + persistable-write verification (DECISION GATE)

1. Toolchain: Android Studio SDK + NDK, `rustup target add aarch64-linux-android
   x86_64-linux-android` (x86_64 for emulator), set `ANDROID_HOME`/`NDK_HOME`, then
   `cargo tauri android init` in `crates/confy-tauri/` (inspect what lands in `gen/android/`;
   decide gitignore vs commit — Tauri docs recommend committing `gen/android` minus build
   outputs).
2. Minimal spike build (can be a scratch branch): add `tauri-plugin-fs`, wire a throwaway
   button or auto-run that (a) opens the dialog-plugin picker, (b) writes a marker line back
   to the picked `content://` URI via plugin-fs, (c) persists the URI string, then **fully
   kill the app**, relaunch, and write again using the stored URI without re-picking.
3. Same session, investigate: does `fileAssociations` in `tauri.conf.json` generate the
   Android intent-filter, or does `gen/android/app/src/main/AndroidManifest.xml` need manual
   edits? Record the answer for Task 3.

**Gate:** if (2c) succeeds → proceed with the plan as written. If it fails → M1 falls back to
**session-scoped save**: the handle is valid only while the app lives; after restart the UI
prompts re-pick before saving ("re-pick to save" UX). Record the outcome in the plan file and
tell the user before continuing.

*Verify:* real device (or emulator for the toolchain part; the permission-persistence check
should run on a real device) — user performs the on-device steps and reports.

### Task 0 — OUTCOME (recorded 2026-07-13)

**Toolchain:** installed CLI-only (no Android Studio GUI) via Homebrew: `temurin` (JDK 26,
default `JAVA_HOME`) + `temurin@21` (JDK 21, **pinned for Android/Gradle builds** — Gradle
8.14.3 cannot load class files compiled by JDK 26, "Unsupported class file major version 70")
+ `android-commandlinetools` (`sdkmanager` → `platform-tools`, `platforms;android-34`,
`build-tools;34.0.0`, `ndk;27.0.12077973`) + `rustup target add aarch64/x86_64/armv7/i686
-linux-android*`. `ANDROID_HOME`/`NDK_HOME`/`JAVA_HOME` (→ JDK 21) exported from `~/.zshrc`
(backed up as `~/.zshrc.bak-mobile-m1` first). `cargo tauri android init` required restructuring
`confy-tauri` into a `[lib] confy_tauri_lib` (staticlib/cdylib/rlib, `#[cfg_attr(mobile,
tauri::mobile_entry_point)] pub fn run()`) + a thin `main.rs` calling `confy_tauri_lib::run()`
— the plan's "5 commands" logic all moved into `lib.rs` unchanged. `gen/android/` committed
(its own generated `.gitignore` already excludes `build/`, `.gradle`, `local.properties`,
`.cxx`, keystores — matches the plan's "commit minus build outputs" guidance, no edits needed).

**Gate result: PASS — but only with a custom picker plugin, not stock `tauri-plugin-dialog`.**
First spike attempt (using `tauri-plugin-dialog`'s `dialog.open()` + `tauri-plugin-fs`) failed
on the **very first write**, before any restart was involved:
`Permission Denial: ... requires android.permission.MANAGE_DOCUMENTS, or grantUriPermission()`.
Root cause, confirmed against the plugin's own Android source
(`tauri-plugin-dialog` v2.7.1, `android/src/main/java/DialogPlugin.kt:62-63`):

```kotlin
// TODO: ACTION_OPEN_DOCUMENT ??
val intent = Intent(Intent.ACTION_GET_CONTENT)
```

`ACTION_GET_CONTENT` is a read-oriented, single-shot content grab — it never requests/receives
a write-capable or persistable URI grant, for any session length. This is a **current,
unresolved upstream gap**: [tauri-apps/plugins-workspace PR #2871](https://github.com/tauri-apps/plugins-workspace/pull/2871)
(merged 2025-07-28, closing #2423/#2826) explicitly replaced `ACTION_PICK` with `ACTION_GET_CONTENT`
and left the `ACTION_OPEN_DOCUMENT` TODO in place rather than fixing it. So the spec's own
fallback ("session-scoped save") would **also** have failed — the problem isn't permission
*persistence* across restart, it's that the picker never grants write access at all.

**Fix:** a new workspace crate, `crates/tauri-plugin-confy-picker` (scaffolded via
`cargo tauri plugin new confy-picker --android --no-example -d crates/tauri-plugin-confy-picker`;
non-interactive shells need `script -q /tmp/x.log cargo tauri plugin new ...` — the CLI's
dialoguer prompts require a real tty). One command, `pick_writable`, implemented in
`android/src/main/java/net/turkeyang/confy/picker/ConfyPickerPlugin.kt`: `Intent.ACTION_OPEN_DOCUMENT`
+ `FLAG_GRANT_{READ,WRITE,PERSISTABLE_URI_PERMISSION}` on the launch intent, then
`contentResolver.takePersistableUriPermission(uri, READ|WRITE)` on the result before resolving.
Desktop's `desktop.rs` stub returns `Error::Unsupported` (never called there — desktop keeps
`tauri-plugin-dialog`'s `dialog.open()`, which has no such problem on macOS). Registered in
`confy-tauri`'s `lib.rs` behind `#[cfg(target_os = "android")]` only. Capability grant:
`confy-picker:default` (the auto-generated `allow-pick-writable` permission).

Retested with the fix (device: real Android hardware, file picked from Downloads):
`Pick & Write` → `picked+wrote: content://com.android.providers.downloads.documents/document/31`
→ **full app kill** → relaunch → `Write Again (no repick)` →
`wrote again (no repick) to: content://.../document/32` (success, no re-pick) → `Read Current` →
`"confy spike marker A @ ...\nconfy spike marker B (no repick) @ ...\n"` (both markers present,
confirming the write actually landed). **Gate passes.**

**Consequence for Task 1:** the Android open-file path must call
`window.__TAURI__.core.invoke('plugin:confy-picker|pick_writable')` instead of
`window.__TAURI__.dialog.open()`; desktop is unaffected (keeps `dialog.open()`). `web/fs.ts`'s
`FsHandle` contract still holds — the returned string is still an opaque handle `fs.ts` never
inspects, it's just sourced from a different invoke on Android.

**Step 3 finding (fileAssociations → Android intent-filter):** confirmed via Tauri v2 docs
(`https://v2.tauri.app/learn/mobile-file-associations/`) — `bundle.fileAssociations` in
`tauri.conf.json` (`ext`, `mimeType` — **required on Android** for intent-filter matching —
`role`, `androidIntentActionFilters`) is enough; the Tauri build system **generates the Android
intent-filter automatically**, no manual `AndroidManifest.xml` edit needed. The open event
arrives as `RunEvent::Opened { urls }` (Rust) for a running app, and cold-start URLs are
retrieved via JS `invoke('opened_urls')`; a warm-running app also gets a JS `listen('opened',
…)` event. Feeds Task 3 directly — no manifest surgery required there either.

**Cleanup done:** the throwaway spike UI (debug bar in `touch.html`) and its
`capabilities/spike-android.json` were removed after recording the outcome above; the
`tauri-plugin-confy-picker` crate, the `[lib]` restructuring, and the `tauri-plugin-fs`
dependency are **not** throwaway — they carry forward into Task 1.

### Task 1 — Desktop I/O refactor to plugin-backed I/O (before any Android app work)

Land the shared refactor on the desktop first; macOS must keep working (regression gate).

1. Rust: add `tauri_plugin_fs::init()` to the builder. Delete the custom `read_file_text` /
   `write_file` / `open_dialog` / `save_dialog` commands; keep `startup_file` (desktop CLI-arg
   open stays a custom command).
2. `web/fs.ts`: `tauriHandle` reads/writes via `window.__TAURI__.fs.readTextFile` /
   `writeTextFile`; `pickOpenFile`/`pickSaveFile` call `window.__TAURI__.dialog.open()` /
   `save()` directly (keep the extension filter on desktop; on mobile pass no filter — same
   iOS-UTI lesson as the web `fileInput`). Ambient types inline, no npm dep. The `FsHandle`
   shape and every `ui.ts`/`touch/app.ts` call site stay untouched.
3. Capabilities: add the explicit fs permissions + scope needed for arbitrary picked paths;
   keep `dialog:default`. Expect iteration — permission errors surface as rejected invokes
   (wrap in the existing try/catch → status-line pattern).
4. `web/menu.ts` recent-files reopen (`openTauriPath`) switches to plugin-fs read too.

*Verify (regression gate):* `cargo build -p confy-tauri` (compiles without the deleted
commands), `npx tsc --noEmit`, scratchpad esbuild, then the user manually tests a
`--debug` bundle on macOS: open via dialog, edit, save in place, Save As, recent-files reopen,
CLI-arg open (`confy-desktop some.toml`). `cd crates/confy-ffi && node functional_smoke.mjs`
still passes (proves no core/ffi drift).

**Task 1 — OUTCOME (2026-07-13):** all 6 manual checks passed on a `--debug` macOS bundle
(open via dialog, edit, save in place, Save As, recent-files reopen, CLI-arg open). Regression
gate cleared.

**Deferred follow-up (user-noted, out of M1 scope):** after Save As or Convert writes a new
file, confy doesn't switch the open document to that new file/path — it stays pointed at the
original. Arguably it should re-point (and reload) at the freshly written file, matching what
most editors do after "Save As". Not implemented now; revisit in a later UX pass.

### Task 2 — Frontend mobile guards

1. `web/menu.ts`: platform guard — on `android`/`ios` (detect via
   `window.__TAURI__.os.platform()` global, or UA fallback) `setupAppMenu` no-ops exactly like
   the pure-web build.
2. `web/fs.ts`: export `canSaveAs(): boolean` (desktop Tauri or browser FS-Access/download →
   true; mobile Tauri → false in M1). Gate all three save-destination paths: Save-As
   action(s), first Save after File > New (no handle), Convert's write-output step — the touch
   UI hides or disables them with a translated hint (new `web.*` i18n keys in both catalogs).

*Verify:* `tsc` + bundle; on desktop nothing visibly changes; grep confirms every
`pickSaveFile` call site is behind the probe.

**Task 2 — OUTCOME (2026-07-13):** implemented with one deliberate expansion beyond the plan
text, decided with the user mid-task. Gating all three save-destination paths as literally
written would leave touch UI with **no way to save at all** on mobile (its Save button only
ever opened the Save/Convert picker sheet — no in-place quick save existed, unlike desktop's
⌘S). That would contradict M1's own acceptance criterion (edit → save → kill app → reopen →
edit present). Resolution (user chose "add quick-save now" after a design menu — see
`CHANGELOG.md`): a shared `host-io.ts::doQuickSave` writes in place to an already-open handle
unconditionally (no `canSaveAs` gate — that flag only blocks picking a *new* destination); a
new touch kebab button opens the existing Save/Convert sheet, mirroring desktop's
button-opens-panel / instant-key-saves-in-place split exactly. `ui.ts`'s `doSave()` now
delegates to the same shared function (no behavior change, verified via desktop `--debug`
bundle + Playwright smoke of both `index.html` and `touch.html` against the local dev server).
`web/menu.ts`'s platform guard uses `fs.ts`'s UA-sniffed `isTauriMobile()` (no
`tauri-plugin-os` dependency added) rather than a separate detection path.

### Task 3 — File association + open-intent

1. `tauri.conf.json` `bundle.fileAssociations` for `.toml/.json/.jsonc/.yaml/.yml`; apply the
   Task 0 finding (manual intent-filter in `gen/android` manifest if needed — document it in
   the plan file if so, since `gen/` edits must survive regeneration).
2. Listen for the open event (deep-link / `RunEvent::Opened` — whichever the spike showed
   works for content URIs) and feed it through the same `openTauriPath`-style entry `fs.ts`
   already has. Desktop keeps `startup_file`.

*Verify:* device test — from the Files app, "Open with" → confy opens with the file loaded.

**Task 3 — OUTCOME (2026-07-13):** implemented as planned, one addition beyond the literal text:
`bundle.fileAssociations` was put in a new `tauri.android.conf.json` platform-merge override
rather than the shared `tauri.conf.json`. Reason found while implementing — the shared file's
`bundle` section also governs the macOS `dmg` bundle (the only other target configured), and the
frontend wiring below is touch-only (`web/touch/app.ts`); `web/ui.ts` (desktop) deliberately
keeps only `startup_file`, per this task's own text. Registering the association in the shared
file would have made macOS Finder's "Open With confy" appear and silently do nothing (load the
sample) instead of opening the file — a real regression, not a hypothetical one, since Finder
registers file associations from `Info.plist` at install/build time regardless of whether the
Rust/JS side "wires it up". The Android-only override avoids that while keeping macOS unaffected
(precedent: `tauri.windows.conf.json` already does the same platform-merge pattern for the NSIS
target).

No new plugin was needed for the read/write side (unlike Task 0's picker case): a file-association
launch intent (`ACTION_VIEW`/`ACTION_EDIT` with `FLAG_GRANT_{READ,WRITE}_URI_PERMISSION`, which
Android's "Open with" chooser sets) grants access for the life of the receiving activity — no
`takePersistableUriPermission` call is needed since each "Open with" is a fresh intent with its
own fresh grant (the M1 acceptance test explicitly re-triggers "Open with" after killing the app,
never relying on a permission surviving without a new intent). `tauri-plugin-fs`'s
`readTextFile`/`writeTextFile` handle the granted `content://` URI directly, same as they already
do for `tauri-plugin-confy-picker`'s URIs.

Verified so far (macOS): `cargo build`/`clippy -D warnings`/`fmt --check` clean, `tsc` clean,
`functional_smoke.mjs` (47/47) unaffected (no `confy-core`/`confy-ffi` changes), `cargo tauri
build --debug` produces a working `.dmg`/`.app` with no regression to desktop's existing
`startup_file` path. The actual "Open with" flow can only be verified on a real Android device
(Task 4).

### Task 4 — APK build + manual test matrix

Icons (`cargo tauri icon` — already-generated set may suffice; Android needs the mipmap set
`android init`/`icon` produces), debug-keystore signing (sideload needs no release key),
`cargo tauri android build` → APK.

*Verify (user, real device):* pick → edit → save → kill app → reopen same file via "Open
with" → prior edit present; language/theme/help all work; desktop macOS build unaffected.

**Task 4 — DONE. All 12 fixes resolved and verified on-device; full acceptance flow (pick → edit
→ save → kill app → reopen via "Open with" → prior edit present) user-confirmed on real hardware
2026-07-15.**
Debug universal APK builds clean (`cargo tauri android
build --debug --apk`, no keystore setup needed — debug builds auto-sign). Real-device testing
(user, on-device only) surfaced several real bugs across two rounds, all now fixed in the working
tree (uncommitted, `mobile-m1-android` branch):

1. **Crash: double-free on a failed parse.** `host-io.ts::replaceSession` froze the *old*
   session before attempting to parse the new text, contradicting its own doc comment ("a parse
   failure... the host keeps its state"). Opening an invalid file left the caller's `session`
   variable dangling on a freed wasm object; the next touch double-freed it
   (`Error: null pointer passed to rust`). Fixed: `prev` is now freed only after the replacement
   parses successfully. Root-caused from a real repro — the user's `sample.toml` in Downloads
   still had Task 0's leftover spike-marker text, not valid TOML, which is what triggered the
   parse failure that exposed this.
2. **Cold-start double delivery.** The Rust side both emits an `"opened"` event and pushes to
   the `opened_urls()` drain state unconditionally, so a cold-start file can be delivered via
   both paths. Fixed with a frontend `Set<string>` dedupe in `touch/app.ts::openOpenedUrl`
   (harmless if the same URL is opened again later in the same session — treated as a no-op).
3. **Picker-opened files failed to save ("Permission").** Found a real Task 1 gap: `fs.ts`'s
   `pickOpenFile()` was never actually wired to call `tauri-plugin-confy-picker` on Android — it
   still called stock `tauri-plugin-dialog`'s `dialog.open()` (`ACTION_GET_CONTENT`, confirmed in
   Task 0 to never grant write access). The plugin's own capability grant
   (`confy-picker:default`) was also missing from `capabilities/default.json` — likely dropped
   when the throwaway `capabilities/spike-android.json` was removed after Task 0. Both fixed:
   `pickOpenFile()` now calls `plugin:confy-picker|pick_writable` on Android (detected via new
   `fs.ts::isTauriAndroid()`), and the capability is granted in the real `default.json`.
4. **"Open with" chooser inconsistency across file managers.** MaterialFiles only offered confy
   for `.toml`; the stock Files app only offered it for `.json`; neither offered it for `.yaml`.
   Root cause: `.toml`/`.yaml` have no IANA-registered MIME type, so different apps guess
   differently when resolving the file's MIME for the intent (Android intent-filter matching
   requires a mimeType match; the auto-generated `pathPattern` has no effect without a `scheme`,
   which Tauri's `bundle.fileAssociations` schema doesn't expose). Mitigated (not fully fixable
   without a stock Tauri limitation) by declaring extra `fileAssociations` entries per extension
   for the common fallback guesses (`text/plain` for all three; `application/x-yaml`/`text/yaml`/
   `text/x-yaml` in addition to `application/yaml` for YAML) — broadens the matching net across
   file managers' differing heuristics, doesn't guarantee every one.
5. **Wrong app icon.** `gen/android`'s mipmap set was still Tauri's generic placeholder logo
   (never regenerated from `icons/icon.png` after Task 0's `android init`). Fixed: reran
   `cargo tauri icon icons/icon.png`, which regenerates every platform's icon set including
   Android's mipmaps (and, as a side effect, an unused `icons/ios/` set — harmless, iOS isn't
   targeted).
6. **Status bar overlap.** `gen/android`'s generated project targets SDK 36, which forces
   edge-to-edge by default (content draws under the system status bar unless the app opts out).
   Fixed with `android:windowOptOutEdgeToEdgeEnforcement` in both `values/themes.xml` and
   `values-night/themes.xml` (a `gen/android` edit — noted here per the Task 0 precedent, since
   `gen/` edits must survive regeneration), plus a `web/style.css` `env(safe-area-inset-top)`
   fallback on `.toolbar`.
7. **Touch UX**: the Save/Save-As kebab was two visually separate buttons (per the Task 2
   AskUserQuestion decision) — merged into one split-button pill (`.split-btn` in `style.css`,
   `touch/app.ts`'s toolbar markup): tap the wide left side (Save) for instant in-place save, tap
   the narrow chevron on the right for the Save/Convert sheet. Also: the disabled-kebab-plus-title
   approach for `!canSaveAs` communicated nothing on touch (no hover) — now a tap always fires,
   showing the translated hint via toast instead of doing nothing when disabled.
8. After fix #7, the user reported the Save/kebab pair still rendered as two buttons stacked
   top-to-bottom rather than one horizontal pill. Root-caused live on-device (see OUTCOME below)
   and then superseded by a design reversal — the split-button pill was dropped altogether in
   favor of a plain Save-button-opens-a-sheet design.

**Re-verified on-device (2026-07-15, desktop-handoff session):** fixes 1-7 all retested on the
real device and **passed**: invalid-file open no longer crashes (double-free gone), cold-start
"Open with" delivers the file exactly once, a picker-opened file now saves successfully
(`confy-picker:default` capability + `pickOpenFile()` wiring confirmed live), the "Open with"
chooser now offers confy for `.toml`/`.json`/`.yaml` across both MaterialFiles and the stock
Files app, the app icon is correct, and the status bar no longer overlaps toolbar content.
`tsc`, `cargo build`/`clippy -D warnings`/`fmt --check`, and a `cargo tauri build --debug` macOS
regression check all continued to pass through this round too.

**Finding #8 — root-caused and resolved (2026-07-15, same session, device connected over
`adb tcpip`).** Diagnosed live by forwarding the app's WebView devtools socket (`adb forward
tcp:9223 localabstract:webview_devtools_remote_<pid>`) and driving the Chrome DevTools Protocol
directly over that WebSocket (`Runtime.evaluate` with `suppress_origin`, since the devtools
server 403s a mismatched `Origin` header) — no `chrome://inspect` UI needed, and Playwright's
MCP tool couldn't reach it (it only drives its own local Chromium, no CDP-attach parameter).
`getComputedStyle('.split-btn')` on the real device returned **`display: block`**, not `flex` —
confirming the stack was real, not a screenshot artifact. Querying `document.styleSheets` showed
why: the touch build loads its **own separate stylesheet**, `http://tauri.localhost/touch/style.css`
(`web/touch/style.css`, not `web/style.css`) — and it had **zero `.split-btn` rules at all**. The
`.split-btn` CSS added for fix #7 only ever landed in the shared `web/style.css` (desktop, where
the class isn't even referenced by `ui.ts`) — never in the touch-only stylesheet the touch build
actually serves. So the two buttons fell back to the browser's default block-level stacking.
Confirmed the fix live by injecting the missing rule via CDP into the running page before
touching any file — `.split-btn` computed `display` flipped to `flex` and the two buttons laid
out side-by-side (82×44 pill) immediately.

**Design reversal, not just a bug fix.** Once fixed, the user reviewed the resulting horizontal
pill live on-device and rejected the visual — asked to drop the split-button merge entirely and
go back to a plain sheet-based chooser. Resolved via `AskUserQuestion`: single Save button, tap
always opens a small action sheet with "Save" / "Save As / Convert…" choices (no more one-tap
instant quick-save from the toolbar on touch). Implemented: `touch/app.ts`'s toolbar markup drops
the `.split-btn` wrapper back to one plain `<button data-act="save">`; a new `openSaveSheet()`
(same anatomy as `openLangSheet`/`openMenuSheet` — `.sheet` div + `.menu-item` rows) replaces the
`"save"` click handler and the now-deleted `"saveas"` case; the dead `.split-btn` CSS added for
fix #7 was removed from `web/touch/style.css` (the also-unused copy in `web/style.css` predates
this session and was left alone, per the minimal-diff/don't-touch-unrelated-dead-code rule).
Verified end-to-end on the real device over the same CDP connection: tapping Save opens the
sheet showing both translated rows ("儲存" / "另存新檔／轉換格式…"); tapping "Save" invokes
`doQuickSave` (correctly hit the `canSaveAs` mobile-unavailable message, since the fresh launch
had no file open yet); tapping "Save As / Convert…" correctly hit the same gate rather than
opening the convert sheet (expected — same `canSaveAs` check as before). `tsc --noEmit` clean,
`functional_smoke.mjs` (unaffected, no core/ffi change) still 100% passing, `cargo tauri android
build --debug --apk` rebuilt and reinstalled clean.

**Round 3 (2026-07-15, same session, device still on `adb tcpip`) — 4 more user-reported issues:**

9. **Picker-opened files misdetect format, defaulting to TOML.** Opening a `.yaml` file via
   "Open with" worked, but via the in-app picker (the `tauri-plugin-confy-picker` flow from Task
   0/fix #3) failed with a raw parse error ("parsing TOML: unexpected…"). Initial hypothesis —
   `content://` URIs are opaque by design (the Downloads provider hands out `.../document/msf:NNN`
   with no filename anywhere in the URI), so `fs.ts`'s old `uri.split(/[\\/]/).pop()` name-guess
   silently lost the extension — was correct but the first fix attempt (having
   `ConfyPickerPlugin.kt` query `ContentResolver` for the real `DISPLAY_NAME` and pass it back)
   *still* didn't work on retest. Root-caused by driving the actual on-device repro end-to-end
   over the same live CDP connection used for finding #8 (`window.__TAURI__.core.invoke(...)`
   plus `adb shell input tap` to drive the real system document-picker UI, screenshotting each
   step) and logging every stage: the Kotlin query itself was correct all along (logcat confirmed
   `_display_name=sample_test.yaml` at the right cursor column) — the name was being read
   correctly and put into the Kotlin response object, but never reached JS. The actual bug was one
   layer up, on the **Rust** side: `tauri-plugin-confy-picker/src/models.rs`'s
   `PickWritableResponse` struct only declared a `uri` field. Every mobile-plugin response is
   deserialized from the JNI/Kotlin JSON into this typed Rust struct before being re-serialized
   back to the JS caller — serde silently drops any field not declared on the struct, so the
   Kotlin plugin's `name` key was being read correctly and then thrown away one hop before
   reaching JS, no matter how the Kotlin side computed it. Fixed by adding `pub name:
   Option<String>` to `PickWritableResponse`; verified via the same live CDP-invoke + UI-automation
   repro — `window.__TAURI__.core.invoke('plugin:confy-picker|pick_writable')` now returns
   `{"uri":"…","name":"sample_test.yaml"}`, and opening the file through the app's real Open →
   Browse-local-files flow now correctly shows the YAML format pill and parses the content. *Separately
   noted, out of scope:* the raw parse-error text itself ("parsing TOML:
   unexpected…") isn't translated — it's the underlying parser library's own diagnostic
   (`cst_doc.rs`/`json/doc.rs`/`yaml/doc.rs` wrap it with `anyhow!("parsing {FMT}: {e}")`), not a
   catalog string, and true on desktop too; not a mobile-specific or new regression, left as-is.
10. **"Open with" chooser support across file managers is still inconsistent** (MaterialFiles:
    `.toml`/`.yaml` but not `.json`; stock Files: only `.toml`) despite the fallback MIME entries
    added in Task 3 fix #4. Re-confirmed this is the **same already-documented limitation**, not a
    regression: `AndroidManifest.xml`'s generated intent-filters are correct and symmetric across
    all three formats (verified — every format has an IANA/registered mimeType entry *and* a
    `text/plain` fallback), but Tauri's `fileAssociations` schema has no `android:scheme` field, so
    the auto-generated `pathPattern`s never actually match content URIs — matching depends entirely
    on whatever MIME type each third-party file manager's own heuristics guess for a given
    extension, which is inherently inconsistent across apps and not something confy's manifest can
    force. No further code change made; left as documented in Task 3/fix #4.
11. **Status bar overlap regression.** Traced to the exact same class of bug as fix #7 (the
    split-button): the `env(safe-area-inset-top)` toolbar padding fix from fix #6 was added only to
    the shared `web/style.css` (desktop), never mirrored into the **separate** `web/touch/style.css`
    the touch build actually loads. Fixed by adding the identical `padding-top:max(8px,
    env(safe-area-inset-top))` rule to `.toolbar` in `web/touch/style.css`. Verified via an on-device
    screenshot (`adb shell screencap`) after reinstall — clean gap between the status bar and the
    toolbar.
12. **Launcher icon appears as a solid black/near-black block.** Root-caused by inspecting
    `icons/icon.png`'s actual pixel alpha values (all 255 — fully opaque, no transparency
    anywhere, including the corners) and the generated adaptive-icon foreground PNG (also alpha=255
    edge-to-edge, no safe-zone margin). Android's adaptive-icon system expects the foreground layer
    to have a transparent margin so the background color layer shows through; with none, the
    foreground fills the entire icon and — combined with the OS's automatic zoom/crop of adaptive
    icon layers — reads as an undifferentiated dark block. Per the user's choice (asked via
    `AskUserQuestion`: redesign a proper transparent asset vs. disable adaptive icons vs. defer —
    chose **disable**), removed the adaptive-icon definition (`gen/android`'s
    `mipmap-anydpi-v26/ic_launcher.xml`, plus the now-orphaned `drawable-v24/ic_launcher_foreground.xml`
    and `values/ic_launcher_background.xml`) so Android falls back to the plain per-density
    `ic_launcher.png` mipmaps `cargo tauri icon` already generates. A `gen/android` edit — per the
    Task 0/fix #6 precedent, must be reapplied if `cargo tauri icon`/`android init` regenerates this
    directory. Verified via an on-device home-screen screenshot: the icon now shows its intended
    design (a rounded navy badge with a document glyph) instead of a flat block.

All four confirmed end-to-end on the real device this round: #9 root-caused and fixed after an
initial wrong-layer diagnosis, then verified live by driving the actual system document-picker UI
via `adb shell input tap` and reading the resulting app state over the same CDP connection; #10
re-confirmed as the already-documented, unfixable-from-confy's-side limitation; #11/#12 confirmed
via on-device screenshots. `tsc --noEmit` clean, `cargo tauri android build --debug --apk` rebuilt
and reinstalled clean on every round.

**Technique note for future sessions:** driving the system Android UI (document pickers, "Open
with" choosers) via `adb shell input tap`/`screencap`, combined with the app's own WebView
inspected live over CDP (`adb forward tcp:PORT localabstract:webview_devtools_remote_<pid>` +
raw WebSocket `Runtime.evaluate`, `suppress_origin=True` to dodge the devtools server's Origin
check), turned out to be a reliable way to reproduce and verify device-only bugs end-to-end
without the user in the loop for every iteration — worth reaching for again before assuming a fix
needs user hands-on retesting.

### Task 5 — Docs + memory

CHANGELOG Unreleased (`feat(mobile)` + the Task 1 `refactor(desktop)` note), CLAUDE.md module
map + desktop-host paragraph (5 commands → plugin-based), WEBUI.md desktop/mobile section,
spec file status flip, `confy-mobile-plan` memory update (M1 outcome + spike verdict).

## Acceptance criteria (from the spec)

- `cargo tauri build` on macOS still produces a working desktop app after the I/O refactor.
- On a real Android device: pick → edit → save → kill app → reopen the same file via
  "Open with" → prior edit is present.
- No change to `confy-core`, `confy-ffi`, or the `Intent`/snapshot contract.

## Handoff prompt (paste into the new session)

```
請用 executing-plans skill 執行 docs/superpowers/plans/2026-07-13-mobile-m1-android-plan.md
（confy Mobile M1：Android sideload APK）。

背景：
- Spec 在 docs/superpowers/plans/2026-07-12-mobile-tauri-spec.md，已 review 通過；M0（PWA）
  已完成並出貨。計畫文件的 Governing facts 一節列了所有該先讀的檔案，照著讀，不要重新推導。
- Task 0 是決策閘門（Android picked content:// URI 重啟後能否持久寫回）：先做、回報結果、
  依閘門結論決定走原計畫或 session-scoped-save 降級分支，再繼續。

硬性約束：
- confy-core / confy-ffi / Intent-SessionSnapshot 合約零改動（理論上不需 wasm rebuild）。
- Task 1 的 macOS 桌面回歸閘門必須通過才能進入 Android 工作。
- 一切工作在本地 feature branch，未經我明示不 push、不 merge（keep-branches-local）。
- 桌面 GUI 與 Android 真機測試由我手動執行：把要測的步驟列清楚給我，等我回報結果。
- esbuild 必須從 scratchpad 副本跑（/Volumes/Home 下會 deadlock），連 node_modules 一起複製。
- 每個 task 完成後更新 CHANGELOG Unreleased；全部完成前不標記計畫為 done。
```
