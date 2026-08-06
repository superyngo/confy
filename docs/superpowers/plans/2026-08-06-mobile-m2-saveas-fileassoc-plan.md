# Mobile M2 plan — Android Save As + "Open with" chooser visibility

**Date:** 2026-08-06 · **Status:** DRAFT — decisions grilled and locked 2026-08-06 (see
`docs/adr/0001-android-save-as-persistable-grant.md`); ready for a new session to execute.
**Spec:** none separate — this plan's own "Governing facts" section is the spec, distilled from
`docs/superpowers/plans/2026-07-13-mobile-m1-android-plan.md` (M1, shipped 2026-07-15) and a
2026-08-06 investigation + grilling session (Android build/sign/install verification,
root-causing both gaps below, and locking the five design decisions this plan embodies).
**Execution:** new session, **executing-plans** skill, task-by-task with checkpoints (same
convention M1 used).
**Scope:** `web/fs.ts`, `web/host-io.ts`, `web/touch/app.ts`, `web/ui.ts` (gate flip only),
`i18n/en.json` + `i18n/zh-TW.json` (catalog cleanup), `crates/tauri-plugin-confy-picker/`
(new `create_writable` command — mandatory, see Task 0),
`crates/confy-tauri/gen/android/app/src/main/AndroidManifest.xml` (hand-authored
intent-filters). **Zero changes to `confy-core`, `confy-ffi`, or the
`Intent`/`SessionSnapshot` contract** — same invariant M1 held.

## Goal

Two independent, bundled fixes for Android's known M1 gaps:

**(A) Save As.** `canSaveAs()` is hardcoded `false` on Tauri mobile (`web/fs.ts:135`) — a
deliberate M1 scope cut, not a bug. Users can only save in place to an already-open file; picking
a *new* destination (Save As, first Save after File > New, Convert's output) shows a "not
available on mobile yet" hint instead. This plan enables it for real.

**(B) "Open with" visibility.** confy does not appear in at least one real device's file-manager
"Open with" list for `.toml`/`.json`/`.yaml` (user-reported 2026-08-06). M1 already investigated
this twice (fix #4, fix #10 in the M1 plan) and concluded it was "mitigated, not fully fixable"
because Tauri's `fileAssociations` config schema has no `android:scheme` field. This plan
supersedes that conclusion with a manifest-level hand-edit Tauri's schema can't express but
Android itself fully supports.

## Global constraints

- No `confy-core`/`confy-ffi`/`Intent`/`SessionSnapshot` changes (host-I/O + manifest only).
- Desktop (macOS) regression gate: `cargo tauri build --debug` must keep working identically.
- Every device step needs real evidence (screenshot / CDP read-back), not "seemed to work" —
  same bar M1 set. Use the M1 "Debugging technique — live CDP" (`TAURI.md` § Mobile) to drive and
  verify without a human in the loop wherever possible; reserve human-in-the-loop for final
  sign-off.
- `adb -s realme:5555` is the established device channel for this project (Android 12, RMX2202).
  This machine's shell has `CI=1` exported globally, which `cargo tauri`'s `--ci` flag parser
  rejects (`error: invalid value '1' for '--ci' [possible values: true, false]`) — always run
  `cargo tauri android build …` as `CI=true cargo tauri android build …`.
- `gen/android` is committed to git (M1 Task 0 decision) — hand-edits there are normal and must
  be documented so they survive a future `cargo tauri android init` regeneration, same precedent
  as M1's icon/theme/status-bar edits (M1 plan fix #5/#6/#12).

---

## Governing facts (read these before writing code)

- **M1 plan** `docs/superpowers/plans/2026-07-13-mobile-m1-android-plan.md` — full history.
  Especially: Task 0 (M1's own persistable-write-permission decision-gate methodology — this
  plan's Task 0 deliberately skips the spike-first step, see
  `docs/adr/0001-android-save-as-persistable-grant.md`, since the stock-plugin gap is already
  source-confirmed, not an open question worth a separate build cycle), Task 4 fix #3
  (`PickWritableResponse` serde field-drop bug — a reminder to double-check every new
  Kotlin↔Rust↔JS field round-trips), fix #4/#10 (the "Open with" limitation this plan's Task 2
  supersedes), the "Debugging technique" CDP paragraph (Task 4, ~l.440).
- **`TAURI.md`** § Mobile (Tauri Android) — current state as of 2026-08-06: `canSaveAs()` gating
  paragraph (to be removed by Task 2), the Google Play + "Build/sign/install verified" paragraphs
  (2026-08-06 session — unrelated to this plan, don't re-verify build/sign, only re-verify the
  on-device behavior this plan actually changes).
- **`web/fs.ts`**: `canSaveAs()` (l.135, currently `!isTauriMobile()`); `pickSaveFile(docFormat,
  suggestedName)` (l.283) — the Tauri branch calls `g.dialog.save({ defaultPath: suggestedName })`
  (**stock** `tauri-plugin-dialog`, NOT the custom `confy-picker` plugin `pickOpenFile()` already
  forks to on Android); `pickOpenFile()` (l.240) is the existing Android-fork template — mirror
  its `isTauriAndroid()` branching shape for Task 0's new command; `isTauriAndroid()` (l.122),
  `isTauriMobile()` (l.116).
- **`web/host-io.ts`**: three `if (!io.canSaveAs)` early-returns show
  `t("web.mobile.saveAsUnavailable")` and bail — l.177 (first save after File > New / no handle
  yet, inside the shared `doQuickSave`-adjacent save flow), l.230 (`openSaveConvert`'s Save-As
  branch), l.265 (Convert's output-path branch). All three become live, reachable code paths once
  the gate flips — read the surrounding function bodies in full before touching them, the gate
  check is not the only logic in each function.
- **`web/touch/app.ts:593-604`**: the Save button's tap handler — `id === "save"` → `doQuickSave`;
  else `!io.canSaveAs` → `toast(t("web.mobile.saveAsUnavailable"))`; else → `openSaveConvert(io)`.
  The sheet UI itself (`.sheet`/`.menu-item` rows, `openSaveSheet`-style anatomy) already exists
  from M1 Task 2/finding #8 — this plan doesn't touch sheet markup, only which branch fires.
- **`web/ui.ts:109-137,1567-1682`**: desktop's parallel Save/Save-As chevron button + menu —
  `canSaveAs: canSaveAs() && !VSHOST` (l.135). `VSHOST` (VS Code webview host) forces `false`
  regardless of this plan's changes — **do not touch that `&& !VSHOST` term**, it's an unrelated
  third shell, not part of the mobile gate.
- **`i18n/en.json:86` / `i18n/zh-TW.json:87`**: `"web.mobile.saveAsUnavailable"` catalog entries —
  the only consumers are the three `host-io.ts` call sites + the one `touch/app.ts` call site
  above. If Task 2 removes every call site, remove these two catalog lines too (clean cutover,
  matches repo convention — no orphaned i18n keys).
- **`tauri-plugin-dialog` v2.7.1 Android source**
  (`~/.cargo/registry/src/index.crates.io-*/tauri-plugin-dialog-2.7.1/android/src/main/java/DialogPlugin.kt:199-245`):
  `saveFileDialog` already uses `Intent.ACTION_CREATE_DOCUMENT` — the **correct** SAF action for
  save-as (unlike `open()`'s `ACTION_GET_CONTENT`, which is what forced M1's custom-plugin
  workaround for the *open* flow). **But it never calls `takePersistableUriPermission`**, unlike
  `tauri-plugin-confy-picker`'s `pickWritable` (M1 fix). This is the exact fact behind
  `docs/adr/0001-android-save-as-persistable-grant.md`: build `create_writable` unconditionally
  rather than spiking first — decisions 1+2 were grilled and locked 2026-08-06, since this gap is
  a `grep`-level fact, not an open question worth a separate spike-and-decide cycle the way M1's
  Task 0 was (M1's own open question — whether an `ACTION_OPEN_DOCUMENT` grant survives restart
  at all — was genuinely unknown until tested; here we already know the specific stock code path
  never even calls the persistence API). Task 0 below still empirically *verifies* the built fix
  on real hardware — it just doesn't gate *whether* to build it.
- **`crates/tauri-plugin-confy-picker/`** (the exact template Task 0 clones — building
  `create_writable` is unconditional, see `docs/adr/0001-android-save-as-persistable-grant.md`):
  - `src/models.rs` — `PickWritableResponse { uri: Option<String>, name: Option<String> }`.
  - `src/mobile.rs` — `pick_writable()` calls `run_mobile_plugin("pickWritable", ())`.
  - `src/desktop.rs` — stub returns `Err(Error::Unsupported)` (never called there).
  - `src/commands.rs` — `#[command] pick_writable` thin wrapper.
  - `src/lib.rs` — registers `commands::pick_writable` in `generate_handler!`.
  - `permissions/default.toml` — `permissions = ["allow-pick-writable"]`;
    `permissions/autogenerated/commands/pick_writable.toml` exists (auto-generated by Tauri's
    permission macro from the command name — a `create_writable` command will need its own
    generated file the same way; regenerate via a normal `cargo build`, don't hand-write it).
  - `android/src/main/java/net/turkeyang/confy/picker/ConfyPickerPlugin.kt` —
    `pickWritable`/`pickWritableResult`/`queryDisplayName` (the last is directly reusable
    verbatim for a new command).
  - `crates/confy-tauri/capabilities/default.json:17` — `"confy-picker:default"` grant; confirm
    (after adding a new command) whether the `default` permission set auto-includes it or needs an
    explicit second grant.
- **`crates/confy-tauri/gen/android/app/src/main/AndroidManifest.xml`**: the file-association
  intent-filters (currently ~l.25-115) are wrapped in
  `<!-- tauri-file-associations. AUTO-GENERATED. DO NOT REMOVE. -->` markers and **rewritten
  wholesale by every `cargo tauri android build`/`dev`** from `crates/confy-tauri/tauri.android.conf.json`'s
  `bundle.fileAssociations`. Any edit *inside* those markers is lost on the next build. New,
  hand-authored `<intent-filter>` blocks must live *outside* the marker pair (still inside
  `<activity>...</activity>`) to survive.
- **Root cause, confirmed 2026-08-06** by reading the generated manifest: each format's
  `<data android:mimeType="…"/>` entries have no `android:scheme` — Android's
  `IntentFilter.addDataType()` implicitly assumes `content:`/`file:` schemes, so these entries
  *can* match, but only when the firing file manager's own MIME-type guess for that extension
  happens to equal one of confy's declared mimeTypes (inconsistent across apps — already
  documented in M1 fix #4/#10). Separately, the `<data android:pathPattern="…"/>`-only entries
  (no scheme, no host) are **inert** — Android requires a scheme (and, in every working
  real-world example of this pattern, a host/authority, even a wildcard one) to be present in the
  filter before path-pattern matching is evaluated at all; these entries currently do nothing.
- **Fix design** (a well-established Android pattern — not novel, don't over-engineer it): one
  hand-authored `<intent-filter>` per extension group using
  `<data android:scheme="content"/>`, `<data android:scheme="file"/>`, `<data android:host="*"/>`,
  `<data android:mimeType="*/*"/>`, and one or more `<data android:pathPattern="…"/>` entries. The
  wildcard `mimeType="*/*"` makes the filter accept *any* MIME type the file manager guesses
  (sidestepping the inconsistent-guessing problem entirely); `pathPattern` still restricts matches
  to files actually ending in the target extension — Android's `IntentFilter` ANDs across the
  scheme/type/path categories, so this can't false-positive on unrelated file types (verify this
  explicitly on-device in Task 3, don't just trust the docs).

## Non-goals

- iOS (not targeted per `RELEASES.md`).
- Re-running the full M1 manual acceptance matrix (pick→edit→save→kill→reopen for the *original*
  open-file flow) — unrelated to this plan's two changes, already build-verified 2026-08-06 (see
  `TAURI.md`).
- Fixing every third-party file manager's UI — the manifest fix maximizes match probability
  across SAF-compliant apps; some very old/niche apps may still not surface confy.
- Play Store publishing / tester enrollment — separate, user-owned track (per 2026-08-06 session).
- The pre-existing, unrelated `functional_smoke.mjs` failure (`grid active after toggle`,
  TypeFilter grid) flagged 2026-08-06 — do not fix it here, do not let it block this plan.

---

## Tasks

### Task 0 — `create_writable` command in `tauri-plugin-confy-picker`, wired to Save As

Per `docs/adr/0001-android-save-as-persistable-grant.md` (decisions 1+2, grilled and locked
2026-08-06): build this unconditionally, no spike first — stock `tauri-plugin-dialog`'s Android
`saveFileDialog` never calling `takePersistableUriPermission` is a fact already confirmed by
reading its source
(`~/.cargo/registry/src/index.crates.io-*/tauri-plugin-dialog-2.7.1/android/src/main/java/DialogPlugin.kt:199-245`),
not an open question worth a separate spike-and-decide build cycle the way M1's Task 0 was.

1. **Rust** (`crates/tauri-plugin-confy-picker/`):
   - `src/models.rs`: add
     ```rust
     #[derive(Debug, Clone, Deserialize, Serialize)]
     #[serde(rename_all = "camelCase")]
     pub struct CreateWritableRequest {
         pub suggested_name: String,
     }
     ```
     Reuse the existing `PickWritableResponse { uri, name }` as the return type — same shape.
   - `src/mobile.rs`: add
     ```rust
     pub fn create_writable(&self, suggested_name: &str) -> crate::Result<PickWritableResponse> {
         self.0
             .run_mobile_plugin(
                 "createWritable",
                 CreateWritableRequest { suggested_name: suggested_name.to_string() },
             )
             .map_err(Into::into)
     }
     ```
   - `src/desktop.rs`: add
     ```rust
     pub fn create_writable(&self, _suggested_name: &str) -> crate::Result<PickWritableResponse> {
         Err(crate::Error::Unsupported)
     }
     ```
     (never called there — desktop's `pickSaveFile` keeps using `g.dialog.save()` directly, same
     as it already does).
   - `src/commands.rs`: add
     ```rust
     #[command]
     pub(crate) async fn create_writable<R: Runtime>(
         app: AppHandle<R>,
         suggested_name: String,
     ) -> Result<PickWritableResponse> {
         app.confy_picker().create_writable(&suggested_name)
     }
     ```
   - `src/lib.rs`: add `commands::create_writable` to the `generate_handler!` list.
   - Run `cargo build -p tauri-plugin-confy-picker` once — this regenerates
     `permissions/autogenerated/commands/create_writable.toml` and the reference doc. Confirm it
     appeared; if the `default` permission set (`permissions/default.toml`) doesn't automatically
     include the new command, add `"allow-create-writable"` to its `permissions` array explicitly.

2. **Kotlin** (`crates/tauri-plugin-confy-picker/android/src/main/java/net/turkeyang/confy/picker/ConfyPickerPlugin.kt`):
   add, mirroring `pickWritable`/`pickWritableResult` exactly:
   ```kotlin
   @Command
   fun createWritable(invoke: Invoke) {
       val args = invoke.parseArgs(CreateWritableArgs::class.java)
       val intent = Intent(Intent.ACTION_CREATE_DOCUMENT)
       intent.addCategory(Intent.CATEGORY_OPENABLE)
       intent.type = "*/*"
       intent.putExtra(Intent.EXTRA_TITLE, args.suggestedName)
       intent.addFlags(
           Intent.FLAG_GRANT_READ_URI_PERMISSION or
               Intent.FLAG_GRANT_WRITE_URI_PERMISSION or
               Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION,
       )
       startActivityForResult(invoke, intent, "createWritableResult")
   }

   @ActivityCallback
   fun createWritableResult(invoke: Invoke, result: ActivityResult) {
       val ret = JSObject()
       val uri = if (result.resultCode == Activity.RESULT_OK) result.data?.data else null
       if (uri != null) {
           activity.contentResolver.takePersistableUriPermission(
               uri,
               Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
           )
           ret.put("uri", uri.toString())
           ret.put("name", queryDisplayName(uri))
       } else {
           ret.put("uri", null)
           ret.put("name", null)
       }
       invoke.resolve(ret)
   }
   ```
   Add the `CreateWritableArgs` data class (`data class CreateWritableArgs(val suggestedName: String)`)
   next to whatever existing args class pattern the file uses (check `PickWritableResponse`'s
   Kotlin-side counterpart, if any, for the exact JSON-arg-parsing convention — `pickWritable`
   currently takes no args, so this may be the first args class in this file; match Tauri's
   plugin-arg parsing convention used elsewhere in `tauri-plugin-dialog`'s
   `SaveFileDialogOptions`/`invoke.parseArgs(...)` if `ConfyPickerPlugin.kt` doesn't already show
   one).

3. `crates/confy-tauri/capabilities/default.json:17` — confirm `"confy-picker:default"` now covers
   `allow-create-writable` too (it should, since `default.toml`'s `default` permission set was
   updated in step 1); if not, add `"confy-picker:allow-create-writable"` explicitly.

4. `web/fs.ts`: `pickSaveFile()` — add an Android branch mirroring `pickOpenFile()`'s shape:
   ```ts
   if (g?.core && g.fs && isTauriAndroid()) {
     const res = await g.core.invoke<PickWritableResponse>(
       "plugin:confy-picker|create_writable",
       { suggestedName },
     );
     if (!res.uri) return null;
     return tauriHandle(res.uri, res.name ?? suggestedName);
   }
   ```
   placed before the existing `if (g?.dialog && g.fs)` branch (desktop/iOS keep using
   `g.dialog.save()` unchanged).

5. Build and verify end-to-end on real hardware. This step also temporarily needs
   `web/fs.ts`'s `canSaveAs()` flipped to `return true;` so the UI path is reachable — Task 1
   makes that permanent.
   a. Rebuild the debug APK: `cd crates/confy-tauri && CI=true cargo tauri android build --debug --apk`.
   b. Install: `adb -s realme:5555 install -r crates/confy-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk`;
      launch: `adb -s realme:5555 shell am start -n net.turkeyang.confy/.MainActivity`.
   c. Drive "Save As" end-to-end: open the sample doc, tap Save → tap the chevron/"另存新檔／轉換格式…"
      row → confirm the SAF create-document picker appears → pick a destination + name → confirm
      the file lands there with content. Use
      `adb forward tcp:PORT localabstract:webview_devtools_remote_<pid>` (find `<pid>` via
      `adb shell ps -A | grep net.turkeyang.confy`) + a raw WebSocket client sending
      `Runtime.evaluate` (`suppress_origin=True` — the devtools server 403s on a mismatched
      `Origin` header) to read back app state without a human driving every tap, same technique
      M1 used (`TAURI.md` § Mobile, "Debugging technique" paragraph); combine with
      `adb shell input tap` to drive the actual system SAF picker UI (not reachable via CDP, it's
      outside the WebView).
   d. Edit the newly-created file again, tap Save (quick save, in place) — confirm the write
      lands (re-read via CDP or a follow-up file open).
   e. **Kill the app fully**: `adb shell am force-stop net.turkeyang.confy`. Relaunch. Reopen the
      SAME file (via whatever path the app currently offers). Edit. Save (quick save to the same
      handle). Confirm this write also lands — this is the acceptance bar locked by decision 1
      (grilled 2026-08-06): kill+relaunch survival, not just session-scoped.

*Verify:* screenshot + CDP read-back evidence at steps 5c-5e, not "seemed to work." Step 5e
passing is the concrete proof the persistable grant works. There is no fallback path if it
doesn't — decisions 1+2 already ruled out relying on the stock plugin — so a failure here means
debugging steps 1-4's implementation, not reconsidering the approach.

### Task 1 — Flip `canSaveAs()` for real + remove the now-dead gate

1. `web/fs.ts:135`: change `canSaveAs()` to `return true;` permanently (or remove the function and
   inline `true` at call sites — check both `ui.ts:135` and `touch/app.ts:148` call sites first;
   keep the named function if it still reads naturally as a documented "capability" concept, drop
   it if it doesn't). Update/remove the doc comment above it (currently describes the M1
   mobile-unavailable behavior — now stale).
2. `web/host-io.ts`: the three `if (!io.canSaveAs) { io.err(t(...)); return; }` blocks (l.177,
   230, 265) become unreachable on every current platform once `canSaveAs()` is always `true`
   (VSHOST is a separate, `ui.ts`-only gate — see below). Read each surrounding function fully,
   then delete these three blocks (clean cutover — don't leave provably-dead branches; this
   directly matches the repo's minimal-diff convention once the flag they guard no longer varies
   on any path these functions run on).
3. `web/touch/app.ts:600-602`: the `else if (!io.canSaveAs) toast(...)` branch becomes dead by the
   same logic — collapse to just `if (id === "save") void doQuickSave(io); else
   openSaveConvert(io);` (confirm no other `id` values reach this handler first — read the
   surrounding `querySelectorAll(".menu-item")` loop in full before editing).
4. `web/ui.ts:135`: **do not touch** `canSaveAs: canSaveAs() && !VSHOST` — `VSHOST` (VS Code
   webview host) is an unrelated third shell that must keep forcing `false` regardless of this
   plan. `canSaveAs()` itself now always returning `true` just means this expression simplifies to
   `!VSHOST` in practice — leave the expression as-is (documents intent, harmless).
5. `i18n/en.json:86` and `i18n/zh-TW.json:87`: after step 2/3 remove every `t("web.mobile.saveAsUnavailable")`
   call site, delete both catalog lines (`grep -rn "saveAsUnavailable" web/ i18n/` should return
   nothing when done).
6. `web/menu.ts` (desktop app-menu `isTauriMobile()` no-op) is unrelated — don't touch.

*Verify:* `grep -rn "canSaveAs\|saveAsUnavailable" web/ i18n/` shows only `ui.ts:135`'s
`VSHOST`-gated line and the `canSaveAs` function definition itself (if kept). `npx tsc --noEmit`
clean (run from `web/`). On-device: Save As reachable with no "unavailable" hint anywhere on
Android; VS Code extension webview (separate manual/regression check, not a new device — just
confirm `VSHOST` still gates it) still shows the old hint/disabled state if that's its existing
UX (don't change VS Code behavior, only confirm no regression).

### Task 2 — Fix "Open with" chooser visibility (manifest-level, independent of Task 0/1)

1. Read the CURRENT generated `crates/confy-tauri/gen/android/app/src/main/AndroidManifest.xml`
   fresh — it's rewritten by every `cargo tauri android build`/`dev`, so don't trust this plan's
   line numbers by execution time. Locate the
   `<!-- tauri-file-associations. AUTO-GENERATED. DO NOT REMOVE. -->` **closing** marker
   (immediately before `</activity>`).
2. Insert new, hand-authored `<intent-filter>` blocks immediately after that closing marker (still
   inside `<activity>...</activity>`, outside the marker pair so a rebuild won't erase them) — one
   filter per extension group:
   ```xml
   <!-- MANUAL, confy-specific: survives `cargo tauri android build` regeneration because it's
        outside the tauri-file-associations markers above. Re-add if a full `cargo tauri android
        init` ever regenerates this file from scratch. See TAURI.md § Mobile > Open-with
        visibility, and docs/superpowers/plans/2026-08-06-mobile-m2-saveas-fileassoc-plan.md.
        Action set (VIEW + SEND + SEND_MULTIPLE) deliberately mirrors the auto-generated block
        above (decision 3, grilled 2026-08-06) — the same scheme-less-pathPattern root cause
        equally breaks the Share sheet, not just "Open with". -->
   <intent-filter>
       <action android:name="android.intent.action.VIEW" />
       <action android:name="android.intent.action.SEND" />
       <action android:name="android.intent.action.SEND_MULTIPLE" />
       <category android:name="android.intent.category.DEFAULT" />
       <category android:name="android.intent.category.BROWSABLE" />
       <data android:scheme="content" />
       <data android:scheme="file" />
       <data android:host="*" />
       <data android:mimeType="*/*" />
       <data android:pathPattern=".*\\.toml" />
   </intent-filter>
   <intent-filter>
       <action android:name="android.intent.action.VIEW" />
       <action android:name="android.intent.action.SEND" />
       <action android:name="android.intent.action.SEND_MULTIPLE" />
       <category android:name="android.intent.category.DEFAULT" />
       <category android:name="android.intent.category.BROWSABLE" />
       <data android:scheme="content" />
       <data android:scheme="file" />
       <data android:host="*" />
       <data android:mimeType="*/*" />
       <data android:pathPattern=".*\\.json" />
       <data android:pathPattern=".*\\.jsonc" />
   </intent-filter>
   <intent-filter>
       <action android:name="android.intent.action.VIEW" />
       <action android:name="android.intent.action.SEND" />
       <action android:name="android.intent.action.SEND_MULTIPLE" />
       <category android:name="android.intent.category.DEFAULT" />
       <category android:name="android.intent.category.BROWSABLE" />
       <data android:scheme="content" />
       <data android:scheme="file" />
       <data android:host="*" />
       <data android:mimeType="*/*" />
       <data android:pathPattern=".*\\.yaml" />
       <data android:pathPattern=".*\\.yml" />
   </intent-filter>
   ```
3. Rebuild (`CI=true cargo tauri android build --debug --apk`), install
   (`adb -s realme:5555 install -r <path>`).
4. Identify the device's stock file manager: `adb -s realme:5555 shell pm list packages | grep -i
   filemanager` (this is an OPLUS/ColorOS device — the stock app package name will contain
   `oplus`/`coloros`/`filemanager`). Also check whether MaterialFiles (M1's second tester) is
   still installed: `adb -s realme:5555 shell pm list packages | grep -i materialfiles`.
5. In each installed file manager: navigate to a `.toml`, `.json`, and `.yaml` file, long-press or
   tap → "Open with"/share → confirm confy now appears in the chooser for **all three** formats.
   Screenshot each (`adb shell screencap`), before this task's fix would have shown confy missing
   (M1's already-documented state) → after showing it present.
6. **Negative check**: "Open with" on an unrelated file already on the device (e.g. a `.png` or
   `.pdf`) — confirm confy does **not** appear. This proves the `mimeType="*/*"` wildcard is
   safely constrained by `pathPattern`, not accidentally catching every file type on the device.
7. Also sanity-check the *existing* M1 flow still works: cold-start "Open with" on a `.toml` from
   the stock Files app should still open confy directly (not just list it) with the file loaded —
   the new manual filters are additive, shouldn't change file managers that already worked.

*Verify:* on-device screenshots for all three formats × at least the stock file manager (plus
MaterialFiles if present) showing confy in the chooser; one negative-check screenshot; one
cold-start-open screenshot confirming no regression to the already-working M1 path.

### Task 3 — Regression gate + docs

1. `cargo build -p confy-tauri -p tauri-plugin-confy-picker`/`clippy -p confy-tauri
   -p tauri-plugin-confy-picker -D warnings`/`fmt --check -p confy-tauri -p tauri-plugin-confy-picker`
   (Task 0 always builds the plugin now, so this is unconditional — no "if Task 1 ran" branch).
2. `cd web && npx tsc --noEmit`.
3. `cd crates/confy-ffi && node functional_smoke.mjs` — expect the SAME pre-existing
   `grid active after toggle` failure noted 2026-08-06 (unrelated TypeFilter grid bug, already on
   `main` before this plan). Do not fix it here; do not let it block this plan's own tasks.
4. `cd crates/confy-tauri && cargo tauri build --debug` on macOS — desktop regression check:
   manually confirm Save/Save-As/Convert/first-save-after-New still work identically (the
   `canSaveAs()` function body changed, even though desktop's `VSHOST`-independent branch was
   already `true` before this plan — verify no accidental behavior change).
5. `CHANGELOG.md` `[Unreleased]`: new `feat(android)` entry — Save As enabled + the new
   `create_writable` plugin command (cross-reference `docs/adr/0001-android-save-as-persistable-grant.md`);
   new `fix(android)` entry — "Open with"/Share chooser visibility (manifest hand-edit + why,
   cross-reference this plan file).
6. `TAURI.md` § Mobile (Tauri Android): remove/update the `canSaveAs()` gating paragraph (no
   longer false on mobile); add a paragraph on the manifest hand-edit (mirroring how M1's
   icon/theme/status-bar edits are documented) with a pointer to this plan file for the full
   rationale.
7. `RELEASES.md`: Android Google Play row — drop "另存新檔/Save-As still unimplemented" from the
   2026-08-06 status note (added in the prior session); the row's remaining blockers stay (no Play
   Console account, no testers, `publish-play.yml` CI not built).

## Acceptance criteria

- On a real Android device: Save As picks a NEW destination via the SAF create-document picker,
  the file is created with the chosen name, content is written, and — critically — the SAME file
  can be reopened and re-saved after a **full app kill and relaunch** (not just within the
  original session).
- On the same real device (and a second file manager if available): confy appears in the "Open
  with" chooser for `.toml`, `.json`, and `.yaml`/`.yml` files; does NOT appear for an unrelated
  file type; the already-working M1 cold-start-open flow is unaffected.
- `cargo tauri build --debug` on macOS unaffected (Save/Save-As/Convert/first-save-after-New all
  still work).
- No change to `confy-core`, `confy-ffi`, or the `Intent`/`SessionSnapshot` contract.
- Regression gate (build/clippy/fmt/tsc/`functional_smoke.mjs`) clean except the pre-existing,
  already-flagged `grid active after toggle` failure.

## Handoff prompt (paste into the new session)

```
請用 executing-plans skill 執行 docs/superpowers/plans/2026-08-06-mobile-m2-saveas-fileassoc-plan.md
（confy Mobile M2：Android 另存新檔 + 檔案總管「開啟方式」清單不出現 confy 的問題）。

背景：
- M1（sideload debug APK）已於 2026-07-15 出貨，另存新檔是當時刻意排除的範圍（不是 bug）。
- 這份計畫的五個關鍵設計決策已在 2026-08-06 的 grilling session 逐一問過使用者、鎖定，其中
  「另存新檔要用 custom SAF plugin，不用 stock tauri-plugin-dialog」那條記錄成
  docs/adr/0001-android-save-as-persistable-grant.md——照著做，不要重新論證或重新開一輪
  spike/decision gate（stock plugin 缺 takePersistableUriPermission 是讀原始碼就能確認的
  事實，不是要在真機上賭一把才知道的未知數）。
- Governing facts 一節列了所有該先讀的檔案與已確認的根因，照著讀，不要重新推導或重新調查
  已經查清楚的部分（例如「開啟方式」清單問題的根因——manifest 的 pathPattern 需要 scheme
  才會生效，Tauri 的 fileAssociations schema 沒有 scheme 欄位——已在 2026-08-06 session
  確認過，直接照 Task 2 的設計做，記得手動 intent-filter 要涵蓋 VIEW 也要涵蓋
  SEND/SEND_MULTIPLE，跟自動產生那段的 action 集合對齊）。

硬性約束：
- confy-core / confy-ffi / Intent-SessionSnapshot 合約零改動。
- 另存新檔的驗收標準是「app 被砍掉重開後還能對同一個檔案再寫一次」，不是只在同一個
  session 內能用（session-scoped 不算過）。
- 桌面（macOS）的 Save/Save-As/Convert 回歸檢查必須通過才算完成。
- adb 用 realme:5555（已知這台機器 shell 裡 CI=1 會讓 cargo tauri 的 --ci 解析失敗，
  跑 android build 一定要 CI=true 開頭）。
- gen/android 的 hand-edit（AndroidManifest.xml 新 intent-filter）要寫在
  tauri-file-associations 的 AUTO-GENERATED 標記「外面」，否則下次 build 會被沖掉——這是
  刻意維持跟 M1 一樣的手動維護模式，不要另外寫自動化腳本去重新注入它（已在 grilling
  session 決定過，理由見 Task 2 開頭的說明）。
- 每個 task 完成後更新 CHANGELOG Unreleased；全部完成前不標記計畫為 done。
- 裝置端驗證盡量用 CDP + adb shell input/screencap 自動化（M1 的 Debugging technique），
  不要每一步都等人工回報；最終驗收截圖仍要附上證據。
```
