# VS Code M1.5 — Shared Dirty State (CustomTextEditorProvider) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebase the confy VS Code extension from `CustomEditorProvider` (confy owns the
document) onto `CustomTextEditorProvider` (VS Code's `TextDocument` is the single source of
truth), so the built-in text editor and confy share one document, one dirty state, and one
undo stack — the title-bar toggle swaps views with no disk round-trip and no save prompt,
and an editable side-by-side text editor stays live in both directions.

**Architecture:** The webview Session becomes a *view* over the `TextDocument`. Webview
mutations post `edit {text}` (full `serialize()`); the host applies it as a minimal-span
`WorkspaceEdit` (common prefix/suffix trim), which drives VS Code's native dirty tracking,
undo stack, save, revert, and hot exit. Every document change the webview doesn't already
hold (side-by-side typing, undo/redo, revert, git ops) flows back as `text-changed {text}`,
and the webview reloads its Session from it, restoring expansion + cursor by path. The
entire M1 edit-token/`edited`/`synced`/`edit-cancelled`/`save-request`/`save-ok` machinery
is deleted — VS Code's text-document machinery replaces all of it.

**Tech Stack:** VS Code extension API (`CustomTextEditorProvider`, `WorkspaceEdit`),
TypeScript, esbuild, vsce. No Rust/core changes.

## Global Constraints

- **No `crates/` changes.** `SessionSnapshot.history_len` stays in core (harmless; other
  hosts may use it later) but the webview stops reading it.
- **Browser/Tauri hosts must be behaviorally unchanged** — every new behavior is gated on
  `VSHOST` (`isVsCode()`), exactly like M1.
- **esbuild must run from a scratchpad copy** — it deadlocks bundling from the
  `/Volumes/Home` repo path. Copy sources to scratchpad, build, copy artifacts back.
- **`vsce package` must run from the repo directory** — outside a git repo it silently
  produces an EMPTY file list and fails with "entrypoint missing" (learned 2026-07-16).
- **i18n keys change ⇒ full rebuild pipeline** — `i18n/*.json` is `include_str!`'d into
  confy-core, so Task 3 runs wasm-pack + web bundle + functional smoke per the
  rebuild-wasm-web-after-core-change rule, even though no `.rs` file changes.
- `tsc --noEmit` must stay clean in both `web/` and `editors/vscode/`.
- Work on local branch `vscode-m1_5`; **never push or merge until the user asks**.
- Extension version bumps to `0.2.0`.
- Spec: `docs/superpowers/specs/2026-07-15-vscode-extension-design.md` (M1.5 goal in
  Non-goals + the Post-M1 addendum). This plan supersedes the M1 mechanics it replaces.

## Decisions locked in this plan (user-visible)

1. **Raw preview is retired.** The `confy-raw://` read-only `TextDocumentContentProvider`
   existed because M1's document was webview-owned. With a shared `TextDocument`, the real
   text editor opened beside (`vscode.openWith … "default", ViewColumn.Beside`) is a live,
   *editable* superset of it. `confy.openRawPreview` is replaced by `confy.openTextBeside`
   ("confy: Open Text Editor to the Side"); `rawPreview.ts` is deleted. (The in-webview
   Tree|Raw toggle is untouched.)
2. **add→Esc wart is eliminated by Edit-mode gating** (grilling Q2). `notifyHost` defers
   posting while `Mode::Edit` is active: an `a`-add's immediate Insert never reaches the
   host; Esc rolls the Session back to `lastNotifyText` and nothing is posted (no dirty,
   no undo entries), while a commit posts one single `edit` for the whole add. Side
   effects, both accepted: a side-by-side text editor doesn't see in-flight inline-edit /
   nudge churn until commit, and a save/hot-exit during an in-flight edit stores the text
   *without* the transient placeholder (more correct than storing it).
3. **Invalid text pauses the tree.** While side-by-side text doesn't parse, the webview
   keeps the last good tree, shows a status message, *dims the tree* (`body.stale-tree`
   CSS, grilling Q3 — browsable/copyable but visibly paused), and *stops posting edits*
   (so a stale tree can never clobber newer raw text). Tree edits made during the stale
   window are dropped on the next successful reload — accepted wart, rare.
4. **External-change reload resets transient UI.** Expansion + cursor are restored by
   path; an in-flight inline edit / modal / selection / filter is discarded by the reload.
   Accepted wart (matches revert semantics).

## Message protocol (target state)

```
Host → Webview                          Webview → Host
──────────────                          ──────────────
init  {text,name,format,lang,dirty}     ready
text-changed {text,dirty}               edit {text}
saved                                   request-undo / request-redo / request-save
                                        convert-save {suggestedName,text}
                                        parse-error {message}
```

Echo rule: the host tracks `webviewText` (last text the webview is known to hold — set on
`init`, on every received `edit`, and on every posted `text-changed`). An
`onDidChangeTextDocument` whose result equals `webviewText` is the echo of our own
`applyEdit` and is not posted back.

---

### Task 1: Protocol rewrite + webview adapter (`web/`)

**Files:**
- Rewrite: `web/vscode-protocol.ts`
- Modify: `web/ui.ts` (VS Code host bridge block ~lines 696–794, `render()` dirty class
  ~line 280)
- Modify: `web/style.css` (one rule in the app-only appendix)
- Modify: `i18n/en.json`, `i18n/zh-TW.json` (one new key)
- Test: `cd web && npx tsc --noEmit` (no unit-test harness exists for `web/`; repo
  precedent is typecheck + functional smoke + manual acceptance)

**Interfaces:**
- Consumes: existing `openText(text, format, handle, name)`, `send(intent)`,
  `session.dispatch(intent)`, `rowAt(path)`, `setStatus`, `formatFromName`, `t()`.
- Produces: the new `HostToWebview`/`WebviewToHost` types (Task 2's host compiles against
  them); `ConfigFormat` export unchanged (host-io.ts re-exports it — do not rename).

- [ ] **Step 1: Create branch**

```bash
git checkout -b vscode-m1_5
```

- [ ] **Step 2: Rewrite `web/vscode-protocol.ts`** (full file replacement)

```ts
// Message protocol between the VS Code extension host and the confy webview.
// Imported by web/vscode.ts (webview side) and editors/vscode/src/* (host
// side) so protocol drift is a compile error, not a runtime surprise.
// Design: docs/superpowers/specs/2026-07-15-vscode-extension-design.md
// (M1.5: the TextDocument is the single source of truth — see the plan
// docs/superpowers/plans/2026-07-16-vscode-m1_5-shared-dirty-state.md).

// The single definition of ConfigFormat — web/host-io.ts re-exports this.
// `.yml` folds to "yaml" and `.jsonc` to "json"; the wire never carries "yml".
export type ConfigFormat = "toml" | "json" | "yaml";

export type HostToWebview =
  // `dirty` rides along because the TextDocument may already be dirty when the
  // confy editor opens (toggle from an unsaved text editor).
  | { type: "init"; text: string; name: string; format: ConfigFormat; lang: string; dirty: boolean }
  // The document changed under us (side-by-side typing, undo/redo, revert,
  // git). The webview reloads its Session from this text; echoes of the
  // webview's own `edit` are filtered host-side and never arrive here.
  | { type: "text-changed"; text: string; dirty: boolean }
  // The document was saved (any save path) — webview clears its dirty pill.
  | { type: "saved" };

export type WebviewToHost =
  | { type: "ready" }
  // A Session mutation happened: `text` is session.serialize(). The host
  // applies it to the TextDocument as a minimal WorkspaceEdit — VS Code's
  // dirty/undo/save machinery takes over from there.
  | { type: "edit"; text: string }
  // Webview keyboard/toolbar undo/redo/save forward to the workbench, which
  // owns the text document's stacks.
  | { type: "request-undo" }
  | { type: "request-redo" }
  | { type: "request-save" }
  // Convert (and same-format save-a-copy) output: host shows a save dialog.
  | { type: "convert-save"; suggestedName: string; text: string }
  | { type: "parse-error"; message: string };
```

- [ ] **Step 3: Replace the ui.ts host-bridge block**

Replace the whole block from `// ---- VS Code host bridge (no-op unless VSHOST) ----`
through the end of `handleHostMsg` (currently `let hostInitiated = false; … lastNotifyDepth
… notifyHost() { … } … handleHostMsg { init/undo/redo/revert/save-request/save-ok }`) with:

```ts
// ---- VS Code host bridge (no-op unless VSHOST) ----
// M1.5: VS Code's TextDocument is the single source of truth. The Session is
// a view: user mutations post `edit {serialize()}`; every document change the
// webview doesn't already hold comes back as `text-changed` and reloads the
// Session (expansion + cursor restored by path).
let hostInitiated = false; // reloading from host text — don't echo it back
let lastNotifyText: string | null = null;
let hostDirty = false; // tab dirty mirror; authoritative over snap.is_dirty here
// Set when a text-changed failed to parse: the visible tree is stale, so
// posting edits from it would clobber newer raw text. Cleared by the next
// text-changed that parses.
let staleTree = false;

function hostDispatch(i: Intent) {
  hostInitiated = true;
  try {
    send(i);
  } finally {
    hostInitiated = false;
  }
}

// Called after every render outside a batch (and once per batch): posts `edit`
// whenever the serialized text actually moved. Navigation-only intents change
// nothing and post nothing.
function notifyHost() {
  if (!VSHOST || !session || !snap) return;
  // Edit-mode gating (plan Decisions #2): defer while an inline edit is in
  // flight — BEFORE lastNotifyText moves, so an add→Esc rollback that lands
  // back on lastNotifyText posts nothing at all, and a commit posts one edit.
  if (typeof snap.mode === "object" && "Edit" in snap.mode) return;
  const text = session.serialize();
  if (text === lastNotifyText) return;
  lastNotifyText = text;
  if (hostInitiated || staleTree) return;
  hostDirty = true;
  document.body.classList.toggle("dirty", true);
  post({ type: "edit", text });
}

// Expansion + cursor survive a text-changed reload by path. A branch row is
// expanded iff its successor row is deeper (ViewRow carries no expanded flag).
function isExpandedRow(rows: ViewRow[], i: number): boolean {
  return rows[i].is_branch && i + 1 < rows.length && rows[i + 1].depth > rows[i].depth;
}

function captureTreeState(): { expanded: Path[]; cursor: Path } | null {
  if (!snap) return null;
  const expanded = snap.rows.filter((_, i) => isExpandedRow(snap!.rows, i)).map((r) => r.path);
  return { expanded, cursor: snap.cursor };
}

function restoreTreeState(saved: { expanded: Path[]; cursor: Path } | null) {
  if (!saved || !session || !snap) return;
  // Parents precede children in row order, so expanding in order always finds
  // the child row once its parent is open. dispatch() directly (not send) —
  // no notifyHost churn for view-only changes.
  for (const p of saved.expanded) {
    const rows = snap.rows;
    const key = JSON.stringify(p);
    const i = rows.findIndex((r) => JSON.stringify(r.path) === key);
    if (i >= 0 && !isExpandedRow(rows, i)) {
      snap = session.dispatch({ SetCursor: p });
      snap = session.dispatch("ToggleExpand");
    }
  }
  if (rowAt(saved.cursor)) snap = session.dispatch({ SetCursor: saved.cursor });
  render();
}

// Reload the Session from host-provided text (init dirty carry / text-changed).
function reloadFromHost(text: string, format: ConfigFormat, name: string | null) {
  const saved = captureTreeState();
  const before = session;
  hostInitiated = true;
  try {
    openText(text, format, null, name);
    // openText dispatches directly (not via send), so no notification fires on
    // its own — run notifyHost explicitly to prime lastNotifyText with this text.
    notifyHost();
  } finally {
    hostInitiated = false;
  }
  if (session !== before) {
    staleTree = false;
    restoreTreeState(saved);
  } else {
    // Parse failed: replaceSession left the old session in place. Freeze it —
    // see staleTree above. Status carries the reason (replaceSession already
    // wrote the parse error via its err callback; append the pause notice).
    staleTree = true;
    setStatus("", t("web.vscode.staleTree"));
  }
  // Visual cue while stale (grilling Q3): the tree dims but stays browsable.
  document.body.classList.toggle("stale-tree", staleTree);
}

function handleHostMsg(msg: HostToWebview) {
  switch (msg.type) {
    case "init": {
      // VS Code's display language is authoritative in this host (same
      // principle as theme). Apply before openText so its internal
      // SetLang(getLang()) picks it up.
      setLang(msg.lang === "zh-TW" ? "zh-TW" : "en");
      hostDirty = msg.dirty;
      hostInitiated = true;
      try {
        openText(msg.text, msg.format, null, msg.name);
        notifyHost();
      } finally {
        hostInitiated = false;
      }
      // openText leaves `session` untouched on a parse failure — surface it to
      // the host so it can offer the plain text editor instead.
      if (!session) {
        post({ type: "parse-error", message: errorEl.textContent || "parse failed" });
      }
      break;
    }
    case "text-changed":
      hostDirty = msg.dirty;
      reloadFromHost(msg.text, formatFromName(fileName ?? "config.toml"), fileName);
      break;
    case "saved":
      hostDirty = false;
      // Session may be stale (see staleTree) — the class toggle below still
      // runs via render(); marking the session clean is safe either way.
      hostDispatch("Save");
      break;
  }
}
```

Notes for the implementer:
- `ViewRow` and `Path` are already imported in ui.ts (used by `rowAt`/`navSelect`).
- `reloadFromHost`'s `session !== before` works because `openText` replaces the module
  `session` variable only on parse success.
- Delete nothing else in the file — `uiUndo`/`uiRedo` (`request-undo`/`request-redo`),
  `doSave` (`request-save`), `saveCopy`/`convert_write` (`convert-save`), and the init-time
  `parse-error` post all keep working against the new protocol unchanged.

- [ ] **Step 4: Make the dirty pill host-driven in VSHOST**

In `render()` change:

```ts
document.body.classList.toggle("dirty", snap.is_dirty);
```

to:

```ts
document.body.classList.toggle("dirty", VSHOST ? hostDirty : snap.is_dirty);
```

(`render` is declared after the bridge block's `let hostDirty` in module scope order —
hoisting is fine because `render` only runs after boot. If the bridge block currently sits
*below* `render` in the file, move the `let hostDirty`/`let staleTree` declarations up next
to `const VSHOST` at ~line 112 instead.)

- [ ] **Step 5: Add the stale-tree dim style**

In `web/style.css`, inside the app-only appendix (the fenced section starting ~line 405;
put it next to the existing `body.host-vscode` rules ~line 643):

```css
/* VS Code host: tree is stale while the side-by-side raw text doesn't parse
   (see ui.ts staleTree) — dim it but keep it browsable/copyable. */
body.stale-tree #tree { opacity: 0.45; }
```

- [ ] **Step 6: Add the stale-tree i18n key**

In `i18n/en.json` (flat keys, keep alphabetical-ish grouping with other `web.*` keys):

```json
"web.vscode.staleTree": "Raw text has a syntax error — tree is paused (edits here are ignored) until it parses again",
```

In `i18n/zh-TW.json`:

```json
"web.vscode.staleTree": "原始文字有語法錯誤——樹狀檢視已暫停（此處的編輯會被忽略），待可解析後恢復",
```

- [ ] **Step 7: Typecheck web/**

```bash
cd /Volumes/Home/Users/wen/repos/confy/web && npx tsc --noEmit
```

Expected: clean. (editors/vscode will NOT compile until Task 2 — that's expected;
its gate runs there.)

- [ ] **Step 8: Commit**

```bash
git add web/vscode-protocol.ts web/ui.ts web/style.css i18n/en.json i18n/zh-TW.json
git commit -m "feat(web): M1.5 vscode protocol — TextDocument-owned edits, text-changed reload with tree-state restore"
```

---

### Task 2: Extension host rewrite (`editors/vscode/`)

**Files:**
- Rewrite: `editors/vscode/src/editorProvider.ts`
- Rewrite: `editors/vscode/src/extension.ts`
- Delete: `editors/vscode/src/rawPreview.ts`
- Modify: `editors/vscode/package.json`
- Test: `cd editors/vscode && npm run check`

**Interfaces:**
- Consumes: Task 1's `HostToWebview`/`WebviewToHost`/`ConfigFormat` from
  `../../../web/vscode-protocol.js`.
- Produces: `ConfyEditorProvider` (viewType `"confy.editor"`, `activeUri` getter),
  commands `confy.openTextBeside`, `confy.openWithConfy`, `confy.reopenAsText`.

- [ ] **Step 1: Rewrite `editors/vscode/src/editorProvider.ts`** (full file replacement)

```ts
import * as vscode from "vscode";
import type { ConfigFormat, HostToWebview, WebviewToHost } from "../../../web/vscode-protocol.js";

// Mirrors web/host-io.ts's formatFromName (same folding: .jsonc→json,
// .yml→yaml); duplicated because the extension host must not import web
// internals, but the return type is the one shared ConfigFormat.
function formatFromName(name: string): ConfigFormat {
  if (name.endsWith(".json") || name.endsWith(".jsonc")) return "json";
  if (name.endsWith(".yaml") || name.endsWith(".yml")) return "yaml";
  return "toml";
}

function basename(uri: vscode.Uri): string {
  return uri.path.split("/").pop() ?? "config.toml";
}

// M1.5: VS Code's TextDocument owns the content, dirty state, undo stack,
// save, revert, backup, and hot exit. This provider is a view adapter:
// webview `edit` → minimal WorkspaceEdit; document change → `text-changed`.
export class ConfyEditorProvider implements vscode.CustomTextEditorProvider {
  static readonly viewType = "confy.editor";

  private activeDocument: vscode.TextDocument | undefined;

  get activeUri(): vscode.Uri | undefined {
    return this.activeDocument?.uri;
  }

  constructor(private readonly context: vscode.ExtensionContext) {}

  async resolveCustomTextEditor(
    document: vscode.TextDocument,
    panel: vscode.WebviewPanel,
  ): Promise<void> {
    this.activeDocument = document;
    const mediaRoot = vscode.Uri.joinPath(this.context.extensionUri, "media");
    panel.webview.options = { enableScripts: true, localResourceRoots: [mediaRoot] };
    panel.webview.html = await this.html(panel.webview, mediaRoot);

    // Last text the webview is known to hold — set on init, on every received
    // `edit`, and on every posted `text-changed`. A document-change event whose
    // result equals it is the echo of our own applyEdit: skip it.
    let webviewText: string | null = null;
    let debounce: ReturnType<typeof setTimeout> | undefined;

    const postMsg = (msg: HostToWebview) => void panel.webview.postMessage(msg);

    const postText = () => {
      const text = document.getText();
      if (text === webviewText) return;
      webviewText = text;
      postMsg({ type: "text-changed", text, dirty: document.isDirty });
    };

    const changeSub = vscode.workspace.onDidChangeTextDocument((e) => {
      if (e.document.uri.toString() !== document.uri.toString()) return;
      if (e.document.getText() === webviewText) return; // echo of our applyEdit
      // Coalesce side-by-side keystrokes; each reload reparses the whole doc.
      clearTimeout(debounce);
      debounce = setTimeout(postText, 150);
    });

    const saveSub = vscode.workspace.onDidSaveTextDocument((d) => {
      if (d.uri.toString() !== document.uri.toString()) return;
      postMsg({ type: "saved" });
    });

    panel.onDidChangeViewState(() => {
      if (panel.active) this.activeDocument = document;
    });

    panel.onDidDispose(() => {
      clearTimeout(debounce);
      changeSub.dispose();
      saveSub.dispose();
      if (this.activeDocument === document) this.activeDocument = undefined;
    });

    panel.webview.onDidReceiveMessage((msg: WebviewToHost) => {
      switch (msg.type) {
        case "ready": {
          const name = basename(document.uri);
          const lang = vscode.env.language.toLowerCase() === "zh-tw" ? "zh-TW" : "en";
          webviewText = document.getText();
          postMsg({
            type: "init",
            text: webviewText,
            name,
            format: formatFromName(name),
            lang,
            dirty: document.isDirty,
          });
          break;
        }
        case "edit":
          void this.applyWebviewEdit(document, msg.text, (t) => {
            webviewText = t;
          }, postText);
          break;
        case "request-undo":
          void vscode.commands.executeCommand("undo");
          break;
        case "request-redo":
          void vscode.commands.executeCommand("redo");
          break;
        case "request-save":
          void vscode.commands.executeCommand("workbench.action.files.save");
          break;
        case "convert-save":
          void this.convertSave(document, msg.suggestedName, msg.text);
          break;
        case "parse-error":
          void this.parseError(document, panel, msg.message);
          break;
      }
    });
  }

  // Apply a webview serialize() to the TextDocument, replacing only the
  // changed span (common prefix/suffix trim) so a side-by-side text editor's
  // cursor and scroll survive confy edits.
  private async applyWebviewEdit(
    document: vscode.TextDocument,
    text: string,
    markKnown: (t: string) => void,
    resync: () => void,
  ): Promise<void> {
    const old = document.getText();
    if (old === text) return;
    // Before the await: the change event must already see this as an echo.
    markKnown(text);
    let start = 0;
    const maxStart = Math.min(old.length, text.length);
    while (start < maxStart && old[start] === text[start]) start++;
    let endOld = old.length;
    let endNew = text.length;
    while (endOld > start && endNew > start && old[endOld - 1] === text[endNew - 1]) {
      endOld--;
      endNew--;
    }
    const edit = new vscode.WorkspaceEdit();
    edit.replace(
      document.uri,
      new vscode.Range(document.positionAt(start), document.positionAt(endOld)),
      text.slice(start, endNew),
    );
    const ok = await vscode.workspace.applyEdit(edit);
    if (!ok) {
      // Rejected (readonly file, concurrent conflicting edit, …): the webview
      // now holds text the document doesn't. Force a resync back to reality.
      markKnown(" never-matches ");
      resync();
    }
  }

  // Convert (or same-format save-a-copy) output: the destination pick is the
  // native save dialog — the webview cannot pick paths. The open document is
  // never touched; offer to open the result in a new confy tab.
  private async convertSave(
    document: vscode.TextDocument,
    suggestedName: string,
    text: string,
  ): Promise<void> {
    const target = await vscode.window.showSaveDialog({
      defaultUri: vscode.Uri.joinPath(document.uri, "..", suggestedName),
    });
    if (!target) return;
    try {
      await vscode.workspace.fs.writeFile(target, new TextEncoder().encode(text));
    } catch (e) {
      void vscode.window.showErrorMessage(`confy: write failed: ${String(e)}`);
      return;
    }
    const action = await vscode.window.showInformationMessage(
      `confy: saved ${basename(target)}`,
      "Open with confy",
    );
    if (action) {
      void vscode.commands.executeCommand("vscode.openWith", target, ConfyEditorProvider.viewType);
    }
  }

  // Initial text failed to parse in the webview: never white-screen — offer
  // the default text editor for this uri instead.
  private async parseError(
    document: vscode.TextDocument,
    panel: vscode.WebviewPanel,
    message: string,
  ): Promise<void> {
    const action = await vscode.window.showErrorMessage(
      `confy: cannot parse ${basename(document.uri)}: ${message}`,
      "Open in text editor",
    );
    if (action) {
      panel.dispose();
      void vscode.commands.executeCommand("vscode.openWith", document.uri, "default");
    }
  }

  // The webview page is web/dist's index.html verbatim, with: the browser-only
  // inline scripts stripped (touch-redirect entry router + service-worker
  // registration — both wrong inside a webview and blocked by CSP anyway),
  // the PWA manifest link removed, a strict CSP injected, and every relative
  // asset URL rewritten to a webview URI. ui.js resolves its wasm via
  // `new URL("./pkg/confy_ffi_bg.wasm", import.meta.url)`, which lands under
  // the rewritten media root automatically — `connect-src` allows that fetch.
  private async html(webview: vscode.Webview, mediaRoot: vscode.Uri): Promise<string> {
    const raw = new TextDecoder().decode(
      await vscode.workspace.fs.readFile(vscode.Uri.joinPath(mediaRoot, "index.html")),
    );
    const uri = (rel: string) =>
      webview.asWebviewUri(vscode.Uri.joinPath(mediaRoot, rel)).toString();
    const csp = [
      "default-src 'none'",
      `img-src ${webview.cspSource} data:`,
      `style-src ${webview.cspSource} 'unsafe-inline'`,
      `font-src ${webview.cspSource}`,
      `script-src ${webview.cspSource} 'wasm-unsafe-eval'`,
      `connect-src ${webview.cspSource}`,
    ].join("; ");
    return raw
      .replace(/<script>[\s\S]*?<\/script>/g, "")
      .replace(/<link rel="manifest"[^>]*>\s*/, "")
      .replace(
        '<meta charset="utf-8" />',
        `<meta charset="utf-8" />\n    <meta http-equiv="Content-Security-Policy" content="${csp}" />`,
      )
      .replace('href="./style.css"', `href="${uri("style.css")}"`)
      .replace('src="./ui.js"', `src="${uri("ui.js")}"`)
      .replace(/"\.\/icons\/icon-192\.png"/g, `"${uri("icons/icon-192.png")}"`);
  }
}
```

- [ ] **Step 2: Rewrite `editors/vscode/src/extension.ts`** (full file replacement)

```ts
import * as vscode from "vscode";
import { ConfyEditorProvider } from "./editorProvider.js";

export function activate(context: vscode.ExtensionContext): void {
  const provider = new ConfyEditorProvider(context);
  context.subscriptions.push(
    vscode.window.registerCustomEditorProvider(ConfyEditorProvider.viewType, provider, {
      // Spec: the Session lives in webview memory; keep it alive when the tab
      // is backgrounded instead of serializing/restoring state.
      webviewOptions: { retainContextWhenHidden: true },
      supportsMultipleEditorsPerDocument: false,
    }),
    // M1.5 replacement for the read-only raw preview: the real text editor,
    // editable and live in both directions (shared TextDocument).
    vscode.commands.registerCommand("confy.openTextBeside", () => {
      const target = vscode.window.activeTextEditor?.document.uri ?? provider.activeUri;
      if (target) {
        void vscode.commands.executeCommand(
          "vscode.openWith",
          target,
          "default",
          vscode.ViewColumn.Beside,
        );
      }
    }),
    // Title-bar toggle: vscode.openWith on the same uri in the active group
    // swaps the tab in place (a resource opens at most once per group). The
    // shared TextDocument carries dirty state across the swap — no save needed.
    vscode.commands.registerCommand("confy.openWithConfy", (uri?: vscode.Uri) => {
      const target = uri ?? vscode.window.activeTextEditor?.document.uri;
      if (target) {
        void vscode.commands.executeCommand("vscode.openWith", target, ConfyEditorProvider.viewType);
      }
    }),
    vscode.commands.registerCommand("confy.reopenAsText", (uri?: vscode.Uri) => {
      const target = uri ?? provider.activeUri;
      if (target) void vscode.commands.executeCommand("vscode.openWith", target, "default");
    }),
  );
}

export function deactivate(): void {}
```

- [ ] **Step 3: Delete the raw preview module**

```bash
git rm editors/vscode/src/rawPreview.ts
```

- [ ] **Step 4: Update `editors/vscode/package.json`**

- `"version": "0.1.0"` → `"0.2.0"`.
- In `contributes.commands`, replace the `confy.openRawPreview` entry with:

```json
{ "command": "confy.openTextBeside", "title": "confy: Open Text Editor to the Side" }
```

(keep `confy.openWithConfy` / `confy.reopenAsText` and the `menus` block from the
post-M1 toggle exactly as they are).

- [ ] **Step 5: Typecheck the extension**

```bash
cd /Volumes/Home/Users/wen/repos/confy/editors/vscode && npm run check
```

Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add -A editors/vscode
git commit -m "feat(vscode): M1.5 — CustomTextEditorProvider rebase; TextDocument owns dirty/undo/save; raw preview retired for live text-beside"
```

---

### Task 3: Build, package, and verify

**Files:**
- Build artifacts only (`web/ui.js`, `web/dist/`, `editors/vscode/dist/`,
  `editors/vscode/media/`, `confy-vscode-0.2.0.vsix`) — no source changes.

**Interfaces:**
- Consumes: Tasks 1–2 source. Produces: installable `.vsix` for the acceptance pass.

- [ ] **Step 1: Rebuild wasm (i18n catalogs are embedded in core)**

```bash
cd /Volumes/Home/Users/wen/repos/confy/crates/confy-ffi && wasm-pack build --target web
```

Expected: `pkg/` regenerated, exit 0. (cargo/wasm-pack are fine on the volume; only
esbuild is not.)

- [ ] **Step 2: Build the web bundle from a scratchpad copy**

```bash
SCRATCH="${TMPDIR:-/tmp}/confy-m15web"   # any path OFF /Volumes/Home works
rm -rf "$SCRATCH" && mkdir -p "$SCRATCH/crates/confy-ffi"
cd /Volumes/Home/Users/wen/repos/confy
cp Cargo.toml "$SCRATCH/"
cp -R i18n "$SCRATCH/i18n"
cp -R web "$SCRATCH/web"
cp -R crates/confy-ffi/pkg "$SCRATCH/crates/confy-ffi/pkg"
cd "$SCRATCH/web" && node build.mjs
```

Expected: `built: ui.js + touch/app.js + pkg/`. Then assemble dist (cf-build.sh step 4)
and copy back:

```bash
cd "$SCRATCH/web"
rm -rf dist && mkdir -p dist/touch dist/pkg dist/icons
cp index.html touch.html style.css ui.js ui.js.map manifest.webmanifest sw.js dist/
cp touch/style.css touch/app.js touch/app.js.map dist/touch/
cp icons/icon-192.png icons/icon-512.png dist/icons/
cp -r pkg/. dist/pkg/
rm -rf /Volumes/Home/Users/wen/repos/confy/web/dist /Volumes/Home/Users/wen/repos/confy/web/pkg /Volumes/Home/Users/wen/repos/confy/web/ui.js /Volumes/Home/Users/wen/repos/confy/web/ui.js.map /Volumes/Home/Users/wen/repos/confy/web/touch/app.js /Volumes/Home/Users/wen/repos/confy/web/touch/app.js.map
cp -R dist /Volumes/Home/Users/wen/repos/confy/web/dist
cp -R pkg /Volumes/Home/Users/wen/repos/confy/web/pkg
cp ui.js ui.js.map /Volumes/Home/Users/wen/repos/confy/web/
cp touch/app.js touch/app.js.map /Volumes/Home/Users/wen/repos/confy/web/touch/
```

- [ ] **Step 3: Run the ffi functional smoke**

```bash
cd /Volumes/Home/Users/wen/repos/confy/crates/confy-ffi && node functional_smoke.mjs
```

Expected: all 36 checks pass (no core logic changed; this guards the wasm rebuild).

- [ ] **Step 4: Build the extension bundle from scratchpad, package from the repo**

```bash
SCRATCH="${TMPDIR:-/tmp}/confy-m15ext"   # any path OFF /Volumes/Home works
rm -rf "$SCRATCH" && mkdir -p "$SCRATCH/editors" "$SCRATCH/web"
cd /Volumes/Home/Users/wen/repos/confy
cp -R editors/vscode "$SCRATCH/editors/vscode"
cp -R web/dist "$SCRATCH/web/dist"
cp web/vscode-protocol.ts "$SCRATCH/web/"
cd "$SCRATCH/editors/vscode" && npm run check && npm run build
# vsce MUST run from the repo dir (empty file list outside git):
rm -rf /Volumes/Home/Users/wen/repos/confy/editors/vscode/dist /Volumes/Home/Users/wen/repos/confy/editors/vscode/media
cp -R dist /Volumes/Home/Users/wen/repos/confy/editors/vscode/dist
cp -R media /Volumes/Home/Users/wen/repos/confy/editors/vscode/media
cd /Volumes/Home/Users/wen/repos/confy/editors/vscode && npm run package
```

Expected: `DONE Packaged: …/confy-vscode-0.2.0.vsix`.

- [ ] **Step 5: Sanity-check the vsix contents**

```bash
cd /Volumes/Home/Users/wen/repos/confy/editors/vscode
unzip -p confy-vscode-0.2.0.vsix extension/package.json | grep -c "openTextBeside"
unzip -p confy-vscode-0.2.0.vsix extension/dist/extension.js | grep -c "text-changed"
```

Expected: both ≥ 1.

- [ ] **Step 6: Commit** (artifacts are gitignored; this commit is only needed if any
      source drifted — normally nothing to commit; verify with `git status`)

---

### Task 4: Docs + memory

**Files:**
- Modify: `docs/superpowers/specs/2026-07-15-vscode-extension-design.md`
- Modify: `editors/vscode/README.md`
- Modify: `CHANGELOG.md`
- Modify: `CLAUDE.md` (the `editors/vscode/` module-map paragraph)
- Modify: `WEBUI.md` (§"VS Code (webview host)", ~line 574)

- [ ] **Step 1: Spec — flip the M1.5 goal to shipped**

In the Non-goals bullet that promotes bidirectional sync to M1.5, replace the trailing
sentence `This reworks the save/undo/edit-token protocol; until then the title-bar toggle
(below) closes one editor before opening the other.` with `**SHIPPED as M1.5
(2026-07-16)** — see docs/superpowers/plans/2026-07-16-vscode-m1_5-shared-dirty-state.md;
the TextDocument owns content/dirty/undo, the webview posts whole-serialize edits applied
as minimal-span WorkspaceEdits, and external changes reload the Session with tree-state
restore.` In the Post-M1 addendum, replace the dirty-buffer sentences with a note that the
toggle now carries dirty state seamlessly (shared TextDocument). Also update the M1
Non-goals line "Reconciling external on-disk changes while a confy editor is open" —
on-disk changes to an *open* TextDocument now flow through `onDidChangeTextDocument` and
are handled.

- [ ] **Step 2: README — new behavior**

Update `editors/vscode/README.md`: version 0.2.0 in the install line; toggle bullet loses
the save-first caveat; replace the Raw Preview bullet with **confy: Open Text Editor to
the Side** (editable, live both directions); replace the trailing "Not in M1" paragraph's
M1.5 sentence with "M1.5 (shared `TextDocument`) shipped — switching carries unsaved
changes; side-by-side text editing syncs live. Still out of scope: Marketplace (M2)."

- [ ] **Step 3: CHANGELOG entry** (under `## [Unreleased]` / `### Added`)

```markdown
- 2026-07-16 feat(vscode): M1.5 — rebase onto CustomTextEditorProvider: shared TextDocument owns dirty/undo/save/hot-exit; toggle carries unsaved changes; editable side-by-side text sync (150ms debounce, tree pauses on invalid text); raw preview retired for "Open Text Editor to the Side"; vsix 0.2.0
```

- [ ] **Step 4: CLAUDE.md module-map paragraph**

Rewrite the `editors/vscode/` paragraph: remove the `confy-raw://` provider, `save-ok`
ack, `request-undo`/edit-stack single-owner, `history_len`/`edit-cancelled` mechanics;
describe the M1.5 model in 4–6 lines (CustomTextEditorProvider; TextDocument single source
of truth; webview `edit` → minimal WorkspaceEdit with echo suppression via `webviewText`;
`text-changed` (150ms debounce) → Session reload with expansion/cursor restore +
stale-tree pause on parse failure; `saved` clears the dirty pill; `history_len` remains in
core but is no longer read by the webview).

- [ ] **Step 5: WEBUI.md §"VS Code (webview host)"**

Rewrite the section's protocol description to the M1.5 model: `CustomTextEditorProvider`
(not `CustomEditorProvider`), the message table from this plan's "Message protocol" section,
Edit-mode gating, stale-tree pause (`body.stale-tree` dim + status), expansion/cursor
restore on `text-changed`, and the retirement of the raw preview in favor of
`confy.openTextBeside`. Keep the Chrome-trimming and Theme subsections unchanged.

- [ ] **Step 6: Commit**

```bash
git add docs/superpowers/specs/2026-07-15-vscode-extension-design.md editors/vscode/README.md CHANGELOG.md CLAUDE.md WEBUI.md
git commit -m "docs(vscode): M1.5 shipped — spec/README/CHANGELOG/CLAUDE.md/WEBUI.md reflect CustomTextEditorProvider model"
```

---

## Manual acceptance checklist (user, on the installed 0.2.0 vsix)

Run `code --install-extension editors/vscode/confy-vscode-0.2.0.vsix` first.

1. **Dirty carries text→confy:** type in the text editor (don't save) → title-bar "Open
   with confy" → confy shows the edited content, tab dot stays, no save prompt.
2. **Dirty carries confy→text:** edit in confy (don't save) → "Reopen as Text Editor" →
   text shows the edit, tab dot stays, no prompt; ⌘Z there undoes the confy edit.
3. **Side-by-side live sync:** "confy: Open Text Editor to the Side" → typing in the text
   editor updates the tree (~150ms); editing in the tree updates the text without the text
   cursor jumping to end-of-file.
4. **Invalid text pause:** in side-by-side, break the syntax → tree pauses with the status
   message → fix the syntax → tree resumes; expansion + cursor survive.
5. **Native undo/redo/save:** ⌘Z/⌘⇧Z/⌘S in the confy tab behave (undo steps through confy
   edits; save clears the dot in both views); add→Esc leaves the tab CLEAN with no undo
   entry, and add→commit is exactly one undo step (Edit-mode gating).
6. **Revert:** File > Revert while confy is open reloads the tree to disk content.
7. **Hot exit:** edit in confy → quit VS Code without saving → relaunch → tab restores
   dirty with the edit intact.
8. **Parse-error fallback:** open an invalid `.toml` with confy → error notification →
   "Open in text editor" works.
9. **Convert:** convert flow still saves a copy via the native dialog and offers "Open
   with confy".
