# Mobile M1 plan — Android sideload APK (Tauri v2)

**Date:** 2026-07-13 · **Status:** planned, approved, not started
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

### Task 3 — File association + open-intent

1. `tauri.conf.json` `bundle.fileAssociations` for `.toml/.json/.jsonc/.yaml/.yml`; apply the
   Task 0 finding (manual intent-filter in `gen/android` manifest if needed — document it in
   the plan file if so, since `gen/` edits must survive regeneration).
2. Listen for the open event (deep-link / `RunEvent::Opened` — whichever the spike showed
   works for content URIs) and feed it through the same `openTauriPath`-style entry `fs.ts`
   already has. Desktop keeps `startup_file`.

*Verify:* device test — from the Files app, "Open with" → confy opens with the file loaded.

### Task 4 — APK build + manual test matrix

Icons (`cargo tauri icon` — already-generated set may suffice; Android needs the mipmap set
`android init`/`icon` produces), debug-keystore signing (sideload needs no release key),
`cargo tauri android build` → APK.

*Verify (user, real device):* pick → edit → save → kill app → reopen same file via "Open
with" → prior edit present; language/theme/help all work; desktop macOS build unaffected.

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
