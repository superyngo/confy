# Mobile extension spec — Tauri iOS/Android shell over the confy web UI

**Status: M1 (Android) SHIPPED — 2026-07-15.** Written 2026-07-12, after the PWA step landed
(manifest + service worker; see CHANGELOG Unreleased). The PWA demand gate was met — the Android
sideload APK described below (Phase M1) is built, tested end-to-end on real hardware, and its
implementation plan (`docs/superpowers/plans/2026-07-13-mobile-m1-android-plan.md`) is complete.
iOS (Phase M2+) remains not started.

## Goal

Ship confy as a native mobile app (Android first, iOS later) reusing the
existing stack unchanged: the wasm `Session` + `web/touch/` UI run in the
mobile webview exactly as they do in the desktop Tauri shell; the Rust side
owns only file I/O. The deliverable of Phase M1 is a sideloadable Android APK
that can open, edit, and save-in-place a config file picked from the system
document picker, and appears in Android's "Open with" sheet for supported
extensions.

## What carries over as-is (no work)

- **Editing core**: `confy-core` via wasm; WKWebView and Android System
  WebView both run wasm. `dispatch` stays synchronous in the webview — same
  B-lite rationale as desktop.
- **UI**: mobile webviews report `pointer: coarse`, so the existing
  `index.html` entry router already lands on `touch.html`. No UI work.
- **i18n, themes, convert dialog, help**: all in-webview, format-neutral.

## The real work: file I/O is path-shaped, mobile is URI-shaped

The desktop shell's 5 commands (`crates/confy-tauri/src/main.rs`) all take/
return **filesystem path strings** and use `std::fs` directly. Mobile breaks
both halves:

| Desktop assumption | Android reality | iOS reality |
|---|---|---|
| picker returns a path | returns a `content://` URI | returns `file://` path, sandbox-scoped |
| `std::fs::read/write(path)` works | cannot open `content://` | needs security-scoped access for cross-app files |
| path is a durable save handle | URI write access expires unless persisted | bookmark needed for durable access |
| CLI arg = startup file | no CLI; intent/VIEW action | no CLI; document interaction / URL scheme |

### Design: opaque-string handle, plugin-backed I/O

`web/fs.ts` already treats the Tauri handle as an opaque string wrapped in an
`FsHandle` (getFile/createWritable → `invoke`). Keep that contract; only the
backing changes:

1. **Replace the custom `read_file_text`/`write_file` commands with
   `tauri-plugin-fs`** (`readTextFile`/`writeTextFile` from
   `@tauri-apps/plugin-fs`) on all platforms. The plugin resolves `content://`
   URIs on Android and plain paths on desktop, so one code path serves both.
   The handle string is whatever the dialog plugin returned (path or URI) —
   `fs.ts` never inspects it.
2. **Open**: `tauri-plugin-dialog`'s `open()` works on mobile (native document
   picker; `content://` on Android, `file://` on iOS). Keep the existing
   extension filter on desktop; on mobile pass no filter (same iOS-UTI lesson
   as the web `fileInput` — `.toml`/`.yaml` grey out).
3. **Save**: `save_dialog` (blocking save panel) has no mobile equivalent in
   the dialog plugin. Mobile flow: "Save" writes in place to the opened URI;
   "Save As" is desktop-only in M1 (Android CREATE_DOCUMENT support is a
   research item, below).
4. **Startup file → file association**: replace `startup_file` with
   `fileAssociations` in `tauri.conf.json` + the deep-link/intent event
   (`onOpenUrl` / RunEvent::Opened). Register `.toml/.json/.jsonc/.yaml/.yml`.
   Desktop keeps the CLI-arg command; mobile listens for the open event and
   feeds the same `openTauriPath`-style entry in `fs.ts`.

### Frontend guards

- **`web/menu.ts`**: `window.__TAURI__` exists on mobile but there is no menu
  bar. Gate menu construction on platform (`@tauri-apps/plugin-os` `type()`
  or `platform()` — `android`/`ios` → no-op, like the pure-web build).
- **`fs.ts` capability probe**: expose `canSaveAs()` so the touch UI hides
  Save-As on mobile M1 instead of failing.

## Phases

### M0 — PWA demand gate (DONE 2026-07-12)
Manifest + network-first service worker; installable, offline-capable.
Gate: does anyone (including Wen) actually reach for confy on a phone?

### M1 — Android APK (sideload, no store)
1. Toolchain: Android Studio SDK + NDK, `rustup target add aarch64-linux-android`
   (+ x86_64 for emulator), `cargo tauri android init` in `crates/confy-tauri`.
2. Swap I/O to plugin-fs/dialog as designed above (desktop must keep working —
   this lands as a refactor of the desktop shell first, verified on macOS).
3. Persistable URI permission on Android (**top risk**, see below).
4. `menu.ts` mobile guard; `canSaveAs()` probe.
5. File associations + open-intent handling.
6. Icons (`cargo tauri icon` from the existing 512px source), signing config,
   `cargo tauri android build` → APK. Manual test matrix: open from picker,
   edit, save in place, reopen after app restart (permission persistence),
   open from Files "Open with".

### M2 — iOS (only after M1 proves out)
Apple Developer account ($99/yr), `cargo tauri ios init`, security-scoped
bookmark handling, TestFlight. Same frontend; the I/O layer from M1 should
already be URI-agnostic. Distribution review risk is Apple-side only.

## Risks / research items

- **Android persistable write permission** (M1 blocker-class): Tauri's dialog
  plugin takes `takePersistableUriPermission` for picked files, but coverage
  across Android versions/providers needs a spike — day-1 item: verify a
  picked `content://` file can be rewritten after full app restart. If it
  can't, M1 falls back to session-scoped save + "re-pick to save" UX.
- **Save As on Android** needs `ACTION_CREATE_DOCUMENT`; check whether the
  dialog plugin's mobile `save()` exists by implementation time, else defer.
- **WebView variance**: Android System WebView is user-updatable; floor is
  wasm + ES2022 support (WebView ≥ ~100, several years old — likely fine, but
  state a minSdk accordingly).
- **Plugin-fs scope/capabilities**: mobile capabilities file must grant fs
  read/write for picked URIs; get the ACL right early (this replaced the
  desktop `dialog:default`-only capability set).
- **Not targeted**: folder picking (known Android gap in the dialog plugin —
  confy never needs it), multi-window, Linux mobile.

## Acceptance criteria (M1)

- `cargo tauri build` on macOS still produces a working desktop app after the
  I/O refactor (regression gate — the refactor is shared code).
- On a real Android device: pick → edit → save → kill app → reopen the same
  file via "Open with" → prior edit is present.
- No change to `confy-core`, `confy-ffi`, or the `Intent`/snapshot contract.
