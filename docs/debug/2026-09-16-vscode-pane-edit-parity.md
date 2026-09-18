# VS Code pane-edit (Raw write mode) parity — investigation findings
Status: In progress

Date: 2026-09-16 (diagnosis done, no code changed yet).
Trigger: user report — "vscode 支援 pane edit 實作不完整，請把行為對齊 web/app
(action menu 選項用詞不同、相關按鈕交互行為)". Prior commit under review: `2458a5a`
(feat(vscode): enable Raw pane write mode), ADR 0015.

## 1. What is provably NOT divergent (ruled out by reading)

confy's **in-webview Action menu** and the crumbs-row **Raw band** cannot differ in wording
between VS Code and web/app:

- Item labels are core-owned (`crates/confy-core/src/session/action_menu.rs` →
  `core.action.edit-document`) and band labels come from the shared catalog
  (`web.raw.controls.{edit,apply,cancel}`, `web/ui.ts:645`).
- Core's language is synced on every session swap: `openText` dispatches
  `{ SetLang: getLang() }` (`web/ui.ts:324`) and `handleHostMsg("init")` calls `setLang(...)`
  *before* `openText`; `web/i18n.ts` caches the choice in memory, so a webview whose
  `localStorage` throws still keeps it.
- All four `VSHOST` gates removed by `2458a5a` are gone; `web/key-intent.ts` special-cases
  `VSHOST` for `q` only; `web/style.css`'s `host-vscode` rules only hide `header.toolbar`
  and `#histGroup`; `editors/vscode/src/editorProvider.ts`'s `html()` serves `index.html`
  verbatim except CSP/asset rewrites. Same markup, same catalog, same code path.

## 2. Divergence A — the VS Code "…" More Actions menu (host-owned wording + missing options)

`editors/vscode/package.json` hardcodes English titles and has **no** `package.nls.json` /
`package.nls.zh-tw.json`:

| VS Code "…" menu (hardcoded) | web/app equivalent (catalog key) |
|---|---|
| `confy: Save As / Convert…` | `web.menu.saveAs` / `web.toolbar.saveAs.title` (zh: 另存 / 轉換…) |
| `confy: Help` | `web.help.tab.help` (zh: 說明) |
| `confy: About` | `web.help.tab.about` (zh: 關於) |
| submenu `confy: Theme`, `Auto (Follow VS Code)` / `Light` / `Dark` | `web.menu.toggleTheme` etc. |
| submenu `confy: Language / 語言` | `web.menu.language` |
| — nothing for whole-file edit — | web/app ⋯ menu rows `btnRawEdit` (Edit→Apply), `btnRawCancel`, `btnViewToggle` |

The Tauri app's native menu (`web/menu.ts`) takes every one of these strings from the shared
catalog, so **web and app agree and only VS Code's wording differs** — matching the report.
Note `#btnMore` lives inside `header.toolbar`, which `host-vscode` hides, so the webview's own
⋯ overflow menu is unreachable in VS Code; the crumbs-row band (Edit/Apply/Cancel) is visible.

## 3. Divergence B — `confirm()` is blocked in a VS Code webview (hard bug, silent trap)

`web/ui.ts:454` (`exitRawWrite`) is the **only** modal dialog call in the whole web codebase:

```ts
if (editEl.value !== rawWriteBaseline && !confirm(t("web.raw.discard-confirm"))) return;
```

Evidence:

1. VS Code 1.135 (already downloaded at `editors/vscode/.vscode-test/vscode-darwin-arm64-1.135.0`)
   builds its webview iframe with
   `sandbox.add("allow-scripts","allow-same-origin","allow-forms","allow-pointer-lock","allow-downloads")`
   — grepped from its own bundle; the string `allow-modals` appears **nowhere** in it.
2. Reproduced in Chromium with those exact flags: `iframe.contentWindow.confirm("…")` returns
   **`false`**, no dialog, no throw (`alert()` → ignored).

Consequence, VS Code only, with a dirty Raw buffer: `exitRawWrite()` always returns early, so
**Esc does nothing and the Tree/Raw toggle does nothing**, with zero feedback. Apply and Cancel
still work because both clean the buffer before calling `exitRawWrite`. Web/app prompt, then
exit. `web/raw-write.spec.mjs:49` stubs `confirm`, so the suite can never observe this.

## 4. Divergence C — `⌘S` in write mode: `edit` / `request-save` ordering

`rawEditSave()` posts `edit` (via `applyRawEdit` → `notifyHost`) and then `request-save` in the
same tick. Host side (`editorProvider.ts:125-138`): `edit` runs `void applyWebviewEdit(...)`
which `await`s `workspace.applyEdit`, while `request-save` immediately runs
`workbench.action.files.save`. Nothing serializes them → the save can be issued before the
WorkspaceEdit lands (saves pre-Apply text, document stays dirty). Same hazard for
`convert-save`.

## 5. Divergence D — `staleTree` + write mode: an Apply that silently goes nowhere

`notifyHost()` advances `lastNotifyText` and *then* returns without posting when `staleTree`
(`web/ui.ts:1256-1267`). So while the side-by-side text doesn't parse: Apply bumps
`doc_revision`, re-seeds the buffer, shows no notice — but never reaches the `TextDocument`,
and the next successful `text-changed` drops it. The Raw pane also shows the last-good
serialization rather than the file's real text. Nothing disables Edit/Apply then, although the
host-side `exec` path already refuses Save-As while stale (`web/ui.ts:1364`).

## 6. Divergence E — `text-changed` arriving while in write mode

`reloadFromHost` swaps in a new `Session` (so core's `pending_external_edit` and its
document-edit lock are gone) while `rawState` stays `"write"` and `rawWriteBaseline` still
holds the *old* text. Escape then no-ops on the new session, and an Apply overwrites the newer
side-by-side text with a buffer derived from the older one. VSCODE.md claims a reload discards
"an in-flight inline edit, modal, selection, or filter" — the Raw write buffer is neither
discarded nor reconciled.

## 7. Fix plan

**Status 2026-09-18: P1-P4 shipped in `3573085`** (v1.3.1, 2026-09-16 — i.e. before the
2026-09-17 refile sweep filed this plan as an open backlog row; the row is narrowed
accordingly). Divergences B-E are closed and documented in `docs/reference/VSCODE.md`
§Webview constraints / §Write serialization / §Stale tree. A gap this plan missed — ⌘Z/⌘Y
doing nothing in write mode, because the webview preload forwards both chords to the
workbench — shipped separately in `41eb0fb`. **Only P5 is open** (divergence A, §2):
`editors/vscode/package.json` still hardcodes English titles and there is still no
`package.nls*.json`. Re-verified 2026-09-18: `web` typecheck + `npm test` exit 0,
`editors/vscode` `npm run check` exits 0.

- P1 `web/ui.ts` + `web/index.html` + `web/style.css`: replace the native `confirm()` with one
  in-page confirmation used by every host (single code path, no `VSHOST` branch); update
  `raw-write.spec.mjs`; add a guard spec asserting `web/*.ts` contains no `confirm(`/`alert(`.
- P2 `editors/vscode/src/editorProvider.ts`: chain webview writes — `request-save` /
  `convert-save` await the in-flight `applyWebviewEdit`.
- P3 `web/ui.ts`: while `staleTree`, refuse entry into write mode (band control disabled +
  existing `web.vscode.staleTree` notice) instead of accepting an Apply that goes nowhere.
- P4 `web/ui.ts`: on a successful `reloadFromHost` while `rawState === "write"`, re-arm
  `BeginEditDocument` and re-seed the baseline from the new text so Esc/Apply/Cancel stay honest.
- P5 `editors/vscode/package.json` (+ `package.nls*.json`): localize the "…" menu with the
  catalog's own wording; decide whether the whole-file-edit entries join it.
- Docs: `CHANGELOG.md`, `docs/reference/VSCODE.md` (no-modals constraint + write serialization),
  `HOST_PARITY.md`, `WEBUI.md` (in-page confirm), plan row in
  `docs/plan/2026-09-09-open-follow-ups.md`.
- Verification: `web` typecheck + `npm test`; `editors/vscode` `npm run check`/`build`/
  `integration-test` (new host-ordering test on the real VS Code 1.135 already in
  `.vscode-test/`); manual real-extension check that Esc/Tree exit write mode.
