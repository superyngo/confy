# VSCODE.md — confy VS Code extension host (`editors/vscode`)

`editors/vscode/` is a third host shell (M1.5, sideload `.vsix` only — no Marketplace
listing) over the same `web/` bundle `WEBUI.md` documents. A `CustomTextEditorProvider`
makes VS Code's own `TextDocument` the single source of truth for content, dirty state,
undo stack, save, revert, and hot exit; the webview runs the unmodified `web/dist` bundle
plus `web/vscode.ts`'s adapter, and the Session there is a *view* over that document.
Every behavior difference from the browser/Tauri hosts is gated in `ui.ts` on `VSHOST`
(`isVsCode()` — true only when `acquireVsCodeApi` exists), so the pure-browser and Tauri
builds are byte-identical when it is absent. See `editors/vscode/README.md` for
build/install/use, and CLAUDE.md's module map for the extension-host-side file layout.

Design record: `docs/superpowers/specs/2026-07-15-vscode-extension-design.md`. M1.5
rebased the provider from `CustomEditorProvider` onto `CustomTextEditorProvider`
(plan: `docs/superpowers/plans/2026-07-16-vscode-m1_5-shared-dirty-state.md`); 0.2.1
fixed the title-bar toggle to truly swap the tab in place and promoted "Open Text Editor
to the Side" to an `editor/title` icon button.

## Chrome trimming

`document.body.classList.add("host-vscode")` on boot; `style.css` hides
`#btnOpen`/`#btnSaveAs`/`#btnTheme` under `body.host-vscode` — the document is tab-bound
(VS Code owns Open), destination picks are native save dialogs, and the theme follows VS
Code's own theme instead of confy's toggle.

## Theme

No `theme` protocol message — `web/vscode.ts`'s `trackVsCodeTheme()` instead runs a
`MutationObserver` on `document.body`'s class list, mapping VS Code's
`vscode-dark`/`vscode-light`/`vscode-high-contrast(-light)` stamps onto confy's own
`:root[data-theme]`. Same visible behavior as a message would give, no protocol needed
(a documented refinement over the original spec).

## Message protocol

`web/vscode-protocol.ts`, single source of truth for both sides:

| Direction | Message | Purpose |
|---|---|---|
| host→webview | `init { text, name, format, lang, dirty }` | Initial state; `dirty` rides along because the TextDocument may already be dirty when the confy editor opens (toggle from an unsaved text editor). VS Code's display language is authoritative here (same principle as theme) |
| host→webview | `text-changed { text, dirty }` | The document changed under us — side-by-side typing (150ms debounce), undo/redo, revert, git. Echoes of the webview's own `edit` are filtered host-side (via `webviewText`) and never arrive here |
| host→webview | `saved` | The document was saved (any save path) — webview clears its dirty pill |
| webview→host | `ready` | Boot handshake |
| webview→host | `edit { text }` | A Session mutation happened: `text` is `session.serialize()`. The host applies it as a minimal-span `WorkspaceEdit` (common prefix/suffix trim) — VS Code's dirty/undo/save machinery takes over from there |
| webview→host | `request-undo` / `request-redo` | Webview keyboard/toolbar undo/redo forward to the workbench, which owns the text document's stacks |
| webview→host | `request-save` | Webview Save / ⌘S → workbench save |
| webview→host | `convert-save { suggestedName, text }` | Convert (or same-format save-a-copy) output: host shows a native save dialog |
| webview→host | `parse-error { message }` | Initial text failed to parse: host offers the default text editor instead of a white screen |

**Echo suppression.** The host tracks `webviewText` (last text the webview is known to
hold — set on `ready`'s `init` reply, on every received `edit`, and on every posted
`text-changed`). An `onDidChangeTextDocument` whose result equals `webviewText` is the
echo of the host's own `applyEdit` and is not posted back — this is what lets a shared
`TextDocument` avoid an infinite edit↔text-changed loop.

**Edit-mode gating eliminates the M1 add→Esc wart.** The webview's `notifyHost` defers
posting `edit` while `Mode::Edit` is active: an `a`-add's immediate Insert never reaches
the host; Esc rolls the Session back to `lastNotifyText` and nothing is posted (no dirty,
no undo entry), while a commit posts one single `edit` for the whole add. A side-by-side
text editor doesn't see in-flight inline-edit/nudge churn until commit; a save/hot-exit
during an in-flight edit stores the text *without* the transient placeholder.

**Stale-tree pause.** While side-by-side text doesn't parse, `reloadFromHost` leaves the
last-good Session in place, sets `staleTree`, and the webview dims the tree
(`body.stale-tree` CSS — browsable/copyable but visibly paused) and shows a status
message (`web.vscode.staleTree`), and stops posting `edit` (so a stale tree can never
clobber newer raw text). Tree edits made during the stale window are dropped on the next
successful reload — a rare, accepted wart. The pause clears the moment a later
`text-changed` parses.

**Expansion + cursor restore on `text-changed`.** A successful reload captures the
expanded-branch set and cursor path before rebuilding the Session, then replays them by
path afterward (`captureTreeState`/`restoreTreeState`) — parents precede children in row
order, so expanding in order always finds the child row once its parent is open. An
in-flight inline edit, modal, selection, or filter is discarded by the reload; this is
accepted (it matches revert semantics).

## Title-bar tab swap (0.2.1)

The **Open with confy** / **Reopen as Text Editor** title-bar buttons
(`confy.openWithConfy`/`confy.reopenAsText`) must truly replace the active tab, not
stack a second one beside it. VS Code tracks tabs by `(uri, viewType)` identity, so a
plain `vscode.openWith` call for a different viewType leaves the previous tab open. The
fix (`extension.ts`'s `swapEditorKind()`): open the new view **first** (so the shared
`TextDocument` keeps at least one reference), then close the old tab — closing while
another view still holds the document skips VS Code's unsaved-changes prompt. This
mirrors what the built-in "Reopen Editor With…" command does. **"Open Text Editor to the
Side"** (`confy.openTextBeside`) is a separate, unaffected command — it always opens a
genuinely new tab in `ViewColumn.Beside` and is contributed as an `editor/title` icon
button next to "Reopen as Text Editor" (not a button inside the confy panel itself).

## Boot-path localStorage guards

`host-io.ts`'s `initTheme`/`toggleTheme` and `i18n.ts`'s `getLang`/`setLang` all wrap
`localStorage` access in `try/catch` — a sandboxed webview may throw on any access, and
these run on the boot path before `ready` is even posted, so an unguarded throw would
white-screen before the host ever hears from the webview. The guards are
behavior-neutral for the browser/Tauri hosts and are **not** `VSHOST`-gated. Persistence
unreliability in webviews is accepted for M1 — theme comes from the VS Code observer
regardless, and lang re-arrives on every `init`.
