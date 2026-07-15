# confy VS Code Extension M1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A sideload-able `.vsix` VS Code extension that opens `.toml`/`.json`/`.jsonc`/`.yaml`/`.yml` files in the full confy tree editor (webview reusing `web/dist` + the wasm Session), with native dirty/save/undo and a live read-only `confy-raw://` preview.

**Architecture:** Third host shell (after browser and Tauri). A `CustomEditorProvider` in a new `editors/vscode/` TS package owns file I/O and the VS Code document lifecycle; the webview runs the unmodified `ui.js` bundle plus a small `web/vscode.ts` adapter. The only channel is `postMessage`, typed by a shared `web/vscode-protocol.ts`.

**Tech Stack:** TypeScript, esbuild, `@types/vscode` (engine `^1.85.0`), `@vscode/vsce`. **No Rust/core changes** — the existing `crates/confy-ffi/pkg` wasm build is reused as-is.

**Spec:** `docs/superpowers/specs/2026-07-15-vscode-extension-design.md` (APPROVED). One refinement over the spec: the host→webview `theme` message is replaced by a webview-side `MutationObserver` on VS Code's `body` class stamps (`vscode-dark`/`vscode-light`/…) — same behavior, no extra protocol.

## Global Constraints

- **esbuild must not run against the `/Volumes/Home/...` repo path** — it deadlocks there. Every esbuild invocation (web bundle AND extension bundle) runs from a scratchpad copy; results are copied back. Exact commands are given in the steps. `npm install`, `tsc`, `vsce`, and `git` are safe on the volume.
- **Never push or merge the feature branch** without the user's explicit ask. All work happens on local branch `vscode-m1`.
- **No changes to `crates/`** (Rust core, ffi, tui, tauri) in this plan. If a step seems to need one, stop and ask.
- Existing web hosts must be unaffected: every `web/` change is gated on the `VSHOST` flag; the pure-browser and Tauri behavior must be byte-identical when `acquireVsCodeApi` is absent.
- `tsc --noEmit` must stay clean in `web/` and in `editors/vscode/` after every task.
- After each task, append an `Unreleased Update` entry to `CHANGELOG.md` (timestamp + description matching the commit message) before committing.
- Verification is build-level (tsc + esbuild + artifact presence). Runtime acceptance is the user's manual checklist (spec §Goal, 7 criteria) on a real `.vsix` install — the plan's final task prepares that checklist; do not claim runtime behavior works.

## File Structure

```
web/vscode-protocol.ts    NEW  shared host⇄webview message types (single source of truth)
web/vscode.ts             NEW  webview-side adapter: acquireVsCodeApi, post/onHostMessage, theme observer
web/ui.ts                 MOD  VSHOST boot branch, host-message handler, notifyHost, save/undo/convert reroutes
web/style.css             MOD  appendix rule hiding host-owned chrome under body.host-vscode
editors/vscode/
  package.json            NEW  manifest (customEditors + command) + scripts + devDeps
  tsconfig.json           NEW  noEmit typecheck config (includes ../../web/vscode-protocol.ts)
  .gitignore              NEW  dist/ media/ node_modules/ *.vsix
  .vscodeignore           NEW  vsce packaging excludes
  .vscode/launch.json     NEW  F5 Extension Development Host config
  build.mjs               NEW  esbuild extension bundle + copy web/dist → media/
  README.md               NEW  install/use/build notes (Task 6)
  src/extension.ts        NEW  activate(): register provider + preview + command
  src/editorProvider.ts   NEW  ConfyDocument + ConfyEditorProvider (lifecycle ⇄ messages, webview HTML)
  src/rawPreview.ts       NEW  RawPreviewProvider (confy-raw:// TextDocumentContentProvider)
```

---

### Task 1: Feature branch + shared protocol + webview adapter

**Files:**
- Create: `web/vscode-protocol.ts`
- Create: `web/vscode.ts`

**Interfaces:**
- Produces (used by Tasks 2–5):
  - `web/vscode-protocol.ts`: `type ConfigFormat = "toml" | "json" | "yaml" | "yml"`, `type HostToWebview`, `type WebviewToHost` (exact definitions below).
  - `web/vscode.ts`: `isVsCode(): boolean`, `post(msg: WebviewToHost): void`, `onHostMessage(handler: (msg: HostToWebview) => void): void`, `trackVsCodeTheme(): void`.

- [ ] **Step 1: Create the branch**

```bash
cd /Volumes/Home/Users/wen/repos/confy
git checkout -b vscode-m1
```

- [ ] **Step 2: Write `web/vscode-protocol.ts`**

```ts
// Message protocol between the VS Code extension host and the confy webview.
// Imported by web/vscode.ts (webview side) and editors/vscode/src/* (host
// side) so protocol drift is a compile error, not a runtime surprise.
// Design: docs/superpowers/specs/2026-07-15-vscode-extension-design.md.

export type ConfigFormat = "toml" | "json" | "yaml" | "yml";

export type HostToWebview =
  | { type: "init"; text: string; name: string; format: ConfigFormat; lang: string }
  // VS Code edit-stack callbacks — the *only* way an undo/redo reaches the
  // Session in this host (single-owner rule, spec §Undo).
  | { type: "undo" }
  | { type: "redo" }
  // Save/Save As/backup all fetch the serialized text through this.
  | { type: "save-request"; id: number }
  // Sent only after workspace.fs.writeFile succeeded; the webview marks the
  // session clean (Intent::Save) only on this ack.
  | { type: "save-ok"; id: number }
  | { type: "revert"; text: string };

export type WebviewToHost =
  | { type: "ready" }
  // A user-initiated mutation: host pushes one VS Code edit entry + refreshes
  // the raw preview. `text` is session.serialize() (cheap token concat).
  | { type: "edited"; dirty: boolean; text: string }
  // A host-initiated change landed (undo/redo/revert/save-ok): refresh the
  // preview/dirty mirror but do NOT push an edit entry.
  | { type: "synced"; dirty: boolean; text: string }
  | { type: "save-response"; id: number; text: string }
  // Webview keyboard/toolbar undo forwards to the host so VS Code's stack
  // stays the single entry point.
  | { type: "request-undo" }
  | { type: "request-redo" }
  // Webview Save button / ⌘S inside the webview → workbench save.
  | { type: "request-save" }
  // Convert (and same-format save-a-copy) output: host shows a save dialog.
  | { type: "convert-save"; suggestedName: string; text: string }
  | { type: "parse-error"; message: string };
```

- [ ] **Step 3: Write `web/vscode.ts`**

```ts
// VS Code webview host adapter — the third host shell (see fs.ts's Tauri
// branch for the pattern). Everything VS Code-specific on the webview side
// lives here: acquireVsCodeApi detection, the typed postMessage channel, and
// the body-class → data-theme mapping. Pure module: no imports from ui.ts.
import type { HostToWebview, WebviewToHost } from "./vscode-protocol.js";

interface VsCodeApi {
  postMessage(msg: unknown): void;
}
declare const acquireVsCodeApi: (() => VsCodeApi) | undefined;

// acquireVsCodeApi throws if called twice — cache the one acquisition.
let acquired = false;
let api: VsCodeApi | null = null;
function vsapi(): VsCodeApi | null {
  if (!acquired) {
    acquired = true;
    api = typeof acquireVsCodeApi === "function" ? acquireVsCodeApi() : null;
  }
  return api;
}

/** True when running inside a VS Code webview. */
export function isVsCode(): boolean {
  return vsapi() !== null;
}

export function post(msg: WebviewToHost): void {
  vsapi()?.postMessage(msg);
}

export function onHostMessage(handler: (msg: HostToWebview) => void): void {
  window.addEventListener("message", (e: MessageEvent) => {
    handler(e.data as HostToWebview);
  });
}

// VS Code stamps the active theme kind on <body> (`vscode-light` /
// `vscode-dark` / `vscode-high-contrast`, plus `vscode-high-contrast-light`
// for light HC) and re-stamps it live on theme switch. Map it onto confy's
// existing `:root[data-theme]` palettes and keep tracking — this replaces the
// spec's host→webview `theme` message (same behavior, no protocol needed).
export function trackVsCodeTheme(): void {
  const apply = () => {
    const cl = document.body.classList;
    const dark =
      cl.contains("vscode-dark") ||
      (cl.contains("vscode-high-contrast") && !cl.contains("vscode-high-contrast-light"));
    document.documentElement.dataset.theme = dark ? "dark" : "light";
  };
  apply();
  new MutationObserver(apply).observe(document.body, {
    attributes: true,
    attributeFilter: ["class"],
  });
}
```

- [ ] **Step 4: Typecheck**

Run: `cd /Volumes/Home/Users/wen/repos/confy/web && npx tsc --noEmit`
Expected: exit 0, no output. (If `web/` has no `tsconfig.json`, run `npx tsc --noEmit vscode-protocol.ts vscode.ts --target es2022 --module esnext --moduleResolution bundler --strict --lib es2022,dom` instead — but check for the tsconfig first; the repo's standard completion step runs tsc, so one should exist.)

- [ ] **Step 5: CHANGELOG + commit**

Append to `CHANGELOG.md` under `Unreleased Update`:
`- 2026-07-15 feat(web): add VS Code webview host protocol + adapter modules`

```bash
git add web/vscode-protocol.ts web/vscode.ts CHANGELOG.md
git commit -m "feat(web): add VS Code webview host protocol + adapter modules"
```

---

### Task 2: Wire the VS Code host into `ui.ts` + hide host-owned chrome

**Files:**
- Modify: `web/ui.ts` (imports; `VSHOST` const; `io` flags; `main()` boot branch; host-message handler + `notifyHost` + `hostDispatch` block; `send`/`batch`; `doSave`; undo/redo reroutes at the keydown switch, `toolbarEntries`, and `bindGlobal` listeners; `convert_write` interception in `render()`; `saveCopy` at both `runSaveConvertShared` callsites)
- Modify: `web/style.css` (app-only appendix)

**Interfaces:**
- Consumes: Task 1's `isVsCode`, `post`, `onHostMessage`, `trackVsCodeTheme`, `HostToWebview`.
- Produces: the webview-side behavior contract Tasks 3–5's host code relies on — posts `ready` on boot; answers `init`/`undo`/`redo`/`revert`/`save-request`/`save-ok`; emits `edited`/`synced`/`save-response`/`request-*`/`convert-save`/`parse-error` exactly as defined in Task 1.

- [ ] **Step 1: Add imports and the `VSHOST` flag**

In `web/ui.ts`, after the existing `./fs.js` import block add:

```ts
import { isVsCode, onHostMessage, post, trackVsCodeTheme } from "./vscode.js";
import type { HostToWebview } from "./vscode-protocol.js";
```

After `const FS_AVAILABLE = fsAccessAvailable();` (line ~104) add:

```ts
// VS Code webview host (third shell): the extension host owns file I/O and
// the undo entry point — see web/vscode.ts + editors/vscode/. All VS Code
// behavior differences below are gated on this flag so the browser and Tauri
// hosts are untouched when acquireVsCodeApi is absent.
const VSHOST = isVsCode();
```

In the `io: HostIo` literal change the two capability flags (a webview has no
FS Access API and must not pick its own destinations — `canSaveAs: false`
routes any residual save-as attempt to the existing unavailable hint):

```ts
  fsAvailable: FS_AVAILABLE && !VSHOST,
  canSaveAs: canSaveAs() && !VSHOST,
```

- [ ] **Step 2: Add the host bridge block**

Add after the `batch()` function (line ~661), one self-contained block:

```ts
// ---- VS Code host bridge (no-op unless VSHOST) ----
// `hostInitiated` marks dispatches triggered by a host message (undo/redo/
// revert/save-ok) so the resulting notification is a `synced` — the host must
// not push a new VS Code edit entry for a change it initiated itself.
let hostInitiated = false;
let lastNotifyText: string | null = null;
let lastNotifyDirty: boolean | null = null;

function hostDispatch(i: Intent) {
  hostInitiated = true;
  try {
    send(i);
  } finally {
    hostInitiated = false;
  }
}

// Called after every render outside a batch (and once per batch): posts
// edited/synced whenever the serialized text or dirty bit actually moved.
// Navigation-only intents change neither and post nothing.
function notifyHost() {
  if (!VSHOST || !session || !snap) return;
  const text = session.serialize();
  if (text === lastNotifyText && snap.is_dirty === lastNotifyDirty) return;
  lastNotifyText = text;
  lastNotifyDirty = snap.is_dirty;
  post({ type: hostInitiated ? "synced" : "edited", dirty: snap.is_dirty, text });
}

function handleHostMsg(msg: HostToWebview) {
  switch (msg.type) {
    case "init": {
      hostInitiated = true;
      try {
        openText(msg.text, msg.format, null, msg.name);
      } finally {
        hostInitiated = false;
      }
      // openText leaves `session` untouched on a parse failure (the error
      // lands in #error via replaceSession's err callback) — surface it to
      // the host so it can offer the plain text editor instead.
      if (!session) {
        post({ type: "parse-error", message: errorEl.textContent || "parse failed" });
      }
      break;
    }
    case "undo":
      hostDispatch("Undo");
      break;
    case "redo":
      hostDispatch("Redo");
      break;
    case "revert": {
      hostInitiated = true;
      try {
        openText(msg.text, formatFromName(fileName ?? "config.toml"), null, fileName);
      } finally {
        hostInitiated = false;
      }
      break;
    }
    case "save-request":
      if (session) post({ type: "save-response", id: msg.id, text: session.serialize() });
      break;
    case "save-ok":
      // Only now may the session mark itself clean (spec: save-ok ack).
      hostDispatch("Save");
      break;
  }
}
```

`formatFromName` is already imported from `./host-io.js`. Note `openText`
inside `init` dispatches `SetLang` internally → `send` → `notifyHost` posts an
initial `synced`, which conveniently seeds the host's preview/text mirror.

- [ ] **Step 3: Hook `notifyHost` into `send` and `batch`**

Change `send` (line ~642) and `batch` (line ~652) to:

```ts
function send(i: Intent) {
  if (!session) return;
  snap = session.dispatch(i);
  if (!batching) {
    render();
    notifyHost();
  }
}
```

```ts
function batch(fn: () => void) {
  if (batching) return fn(); // nested batches render at the outermost level
  batching = true;
  try {
    fn();
  } finally {
    batching = false;
    render();
    notifyHost();
  }
}
```

- [ ] **Step 4: Boot branch in `main()`**

After `initTheme();` (first line of `main()`) add:

```ts
  if (VSHOST) {
    document.body.classList.add("host-vscode");
    trackVsCodeTheme();
  }
```

After `updateSaveLabel();` (right after `await load(wasmUrl);`) add — replacing
entry into the startup-file/url/sample chain for this host:

```ts
  if (VSHOST) {
    onHostMessage(handleHostMsg);
    post({ type: "ready" });
    bindGlobal();
    return;
  }
```

(The existing `const startup = await tauriStartupFile();` … `bindGlobal();`
tail stays as-is for the other hosts.)

- [ ] **Step 5: Reroute Save, Undo/Redo**

Replace `doSave` (line ~744):

```ts
function doSave(): Promise<void> {
  // VS Code host: saving is the workbench's job (dirty tracking + save-ok
  // ack); the webview only requests it.
  if (VSHOST) {
    post({ type: "request-save" });
    return Promise.resolve();
  }
  return doQuickSave(io);
}
```

Add next to it:

```ts
// Undo/redo single-owner rule (spec §Undo): in the VS Code host these forward
// to the workbench so its edit stack stays the sole entry point; the Session
// executes them only via the host's undo/redo callback messages.
function uiUndo() {
  if (VSHOST) post({ type: "request-undo" });
  else send("Undo");
}
function uiRedo() {
  if (VSHOST) post({ type: "request-redo" });
  else send("Redo");
}
```

Then replace all four direct undo/redo dispatch sites with the helpers:
- keydown switch (lines ~622–623): `case "z": return uiUndo();` / `case "y": return uiRedo();`
- `toolbarEntries` (lines ~1213–1214): `run: () => uiUndo()` / `run: () => uiRedo()`
- `bindGlobal` listeners (lines ~1396–1397): `$("btnUndo").addEventListener("click", () => uiUndo());` / `$("btnRedo").addEventListener("click", () => uiRedo());`

- [ ] **Step 6: Reroute Convert output and save-a-copy**

In `render()` (line ~280) replace the `convert_write` line:

```ts
  if (snap.convert_write) {
    if (VSHOST) {
      const [outPath, outText] = snap.convert_write;
      post({
        type: "convert-save",
        suggestedName: outPath.split("/").pop() || outPath,
        text: outText,
      });
    } else {
      void doConvertWrite(io, snap.convert_write[0], snap.convert_write[1]);
    }
  }
```

Add a shared save-copy helper next to `doSave`:

```ts
// Same-format "save a copy" out of the Save/Convert panel. VS Code host: the
// destination pick is the host's save dialog, same as a convert output.
function saveCopy(path: string) {
  if (VSHOST) {
    const text = session?.serialize();
    if (text == null) return;
    send("ExitConvert");
    post({ type: "convert-save", suggestedName: path.split("/").pop() || path, text });
    return;
  }
  void doSaveAsCopy(io, path);
}
```

Replace both `runSaveConvertShared`/`wireConvertDialog` callsites' callback
`doSaveAsCopy: (path: string) => doSaveAsCopy(io, path)` (keydown handler line
~532, and the `wireConvertDialog(...)` call further down) with
`doSaveAsCopy: saveCopy`.

- [ ] **Step 7: Hide host-owned chrome**

In `web/style.css`, inside the fenced app-only appendix (bottom of the file),
add:

```css
/* VS Code webview host: chrome the extension host owns is hidden — Open and
   Save-As/Convert destination picks are VS Code dialogs, the theme follows
   the VS Code theme (web/vscode.ts), and the doc is tab-bound (no Open). The
   Save button stays: it forwards to the workbench save (request-save). */
body.host-vscode #btnOpen,
body.host-vscode #btnSaveAs,
body.host-vscode #btnTheme {
  display: none;
}
```

- [ ] **Step 8: Typecheck + build the web bundle (scratchpad) + smoke**

```bash
cd /Volumes/Home/Users/wen/repos/confy/web && npx tsc --noEmit
```
Expected: exit 0.

Build from scratchpad (esbuild deadlocks on the volume path):

```bash
R=/Volumes/Home/Users/wen/repos/confy
S="$CLAUDE_SCRATCHPAD/webbuild"   # any scratchpad subdir; recreate fresh
rm -rf "$S" && mkdir -p "$S/crates/confy-ffi"
cp "$R/Cargo.toml" "$S/"
cp -R "$R/crates/confy-ffi/pkg" "$S/crates/confy-ffi/pkg"
cp -R "$R/web" "$S/web" && rm -rf "$S/web/node_modules" "$S/web/dist"
cd "$S/web" && npm install && node build.mjs
# assemble dist exactly as cf-build.sh does:
rm -rf dist && mkdir -p dist/touch dist/pkg dist/icons
cp index.html touch.html style.css ui.js ui.js.map manifest.webmanifest sw.js dist/
cp touch/style.css touch/app.js touch/app.js.map dist/touch/
cp icons/icon-192.png icons/icon-512.png dist/icons/
cp -r pkg/. dist/pkg/
# copy the rebuilt artifacts back:
cp ui.js ui.js.map "$R/web/" && cp touch/app.js touch/app.js.map "$R/web/touch/"
rm -rf "$R/web/dist" && cp -R dist "$R/web/dist"
```
Expected: `built: ui.js + touch/app.js + pkg/` from build.mjs; `web/dist/` exists in the repo.

```bash
cd /Volumes/Home/Users/wen/repos/confy/crates/confy-ffi && node functional_smoke.mjs
```
Expected: all 36 checks pass (wasm contract untouched).

- [ ] **Step 9: CHANGELOG + commit**

Append to `CHANGELOG.md`:
`- 2026-07-15 feat(web): VS Code webview host wiring in ui.ts (boot, save/undo/convert reroutes, chrome trim)`

```bash
git add web/ui.ts web/style.css CHANGELOG.md
git commit -m "feat(web): VS Code webview host wiring in ui.ts"
```
(`ui.js`/`dist` are build artifacts — do not commit them if gitignored; check `git status` and leave ignored files alone.)

---

### Task 3: Extension scaffold + webview boot

**Files:**
- Create: `editors/vscode/package.json`, `editors/vscode/tsconfig.json`, `editors/vscode/.gitignore`, `editors/vscode/.vscodeignore`, `editors/vscode/.vscode/launch.json`, `editors/vscode/build.mjs`, `editors/vscode/src/extension.ts`, `editors/vscode/src/rawPreview.ts`, `editors/vscode/src/editorProvider.ts`

**Interfaces:**
- Consumes: `web/vscode-protocol.ts` types (imported as `"../../../web/vscode-protocol.js"`); the built `web/dist` from Task 2.
- Produces: `ConfyEditorProvider` (`static viewType = "confy.editor"`, `openRawPreview(): void`), `ConfyDocument` (`uri: vscode.Uri`, `latestText: string`, `panel: vscode.WebviewPanel | undefined`), `RawPreviewProvider` (`static scheme = "confy-raw"`, `static previewUri(source: vscode.Uri): vscode.Uri`, `update(source: vscode.Uri, text: string): void`) — Task 4/5 fill in behavior but keep these exact names.

- [ ] **Step 1: `package.json`**

```json
{
  "name": "confy-vscode",
  "displayName": "confy",
  "description": "Structural tree editor for TOML / JSON / JSONC / YAML config files",
  "version": "0.1.0",
  "publisher": "superyngo",
  "private": true,
  "license": "MIT",
  "engines": { "vscode": "^1.85.0" },
  "categories": ["Other"],
  "main": "./dist/extension.js",
  "contributes": {
    "customEditors": [
      {
        "viewType": "confy.editor",
        "displayName": "confy",
        "priority": "option",
        "selector": [
          { "filenamePattern": "*.toml" },
          { "filenamePattern": "*.json" },
          { "filenamePattern": "*.jsonc" },
          { "filenamePattern": "*.yaml" },
          { "filenamePattern": "*.yml" }
        ]
      }
    ],
    "commands": [
      { "command": "confy.openRawPreview", "title": "confy: Open Raw Preview" }
    ]
  },
  "scripts": {
    "build": "node build.mjs",
    "check": "tsc --noEmit",
    "package": "vsce package --allow-missing-repository"
  },
  "devDependencies": {
    "@types/node": "^20",
    "@types/vscode": "^1.85.0",
    "@vscode/vsce": "^3.0.0",
    "esbuild": "^0.24.0",
    "typescript": "^5.5.0"
  }
}
```

(Modern VS Code infers `onCustomEditor:`/`onCommand:` activation from `contributes` — no explicit `activationEvents` needed.)

- [ ] **Step 2: `tsconfig.json`, `.gitignore`, `.vscodeignore`, `launch.json`**

`tsconfig.json`:
```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "lib": ["ES2022"],
    "types": ["node"],
    "strict": true,
    "noEmit": true,
    "skipLibCheck": true
  },
  "include": ["src/**/*.ts", "../../web/vscode-protocol.ts"]
}
```

`.gitignore`:
```
node_modules/
dist/
media/
*.vsix
```

`.vscodeignore` (what vsce EXCLUDES from the package; `dist/` + `media/` + manifest stay in):
```
src/**
node_modules/**
build.mjs
tsconfig.json
.vscode/**
.gitignore
*.map
```

`.vscode/launch.json` (F5 when `editors/vscode` is opened as the workspace folder):
```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "name": "Run confy extension",
      "type": "extensionHost",
      "request": "launch",
      "args": ["--extensionDevelopmentPath=${workspaceFolder}"]
    }
  ]
}
```

- [ ] **Step 3: `build.mjs`**

```js
// Bundle the extension host and stage the webview assets. Run this from a
// scratchpad copy of the repo — esbuild deadlocks bundling from the
// /Volumes/Home volume path (see the plan's Global Constraints).
import { cp, rm } from "node:fs/promises";
import esbuild from "esbuild";

await esbuild.build({
  entryPoints: ["src/extension.ts"],
  outfile: "dist/extension.js",
  bundle: true,
  platform: "node",
  format: "cjs",
  target: "node18",
  external: ["vscode"],
  sourcemap: true,
});

// The webview loads the same web/dist bundle the browser and Tauri hosts use.
const MEDIA = new URL("./media/", import.meta.url);
await rm(MEDIA, { recursive: true, force: true });
await cp(new URL("../../web/dist/", import.meta.url), MEDIA, { recursive: true });

console.log("built: dist/extension.js + media/");
```

- [ ] **Step 4: `src/rawPreview.ts`** (full implementation now — it is tiny)

```ts
import * as vscode from "vscode";

// Read-only live mirror of the confy session's serialize() output — the
// spec's one-way sync surface. Content is read-only by scheme; the preview
// uri keeps the source file's path (and thus extension) so VS Code picks the
// right syntax highlighting, and carries the full source uri in the query so
// two same-named files don't collide.
export class RawPreviewProvider implements vscode.TextDocumentContentProvider {
  static readonly scheme = "confy-raw";

  private readonly texts = new Map<string, string>();
  private readonly changeEmitter = new vscode.EventEmitter<vscode.Uri>();
  readonly onDidChange = this.changeEmitter.event;

  static previewUri(source: vscode.Uri): vscode.Uri {
    return vscode.Uri.from({
      scheme: RawPreviewProvider.scheme,
      path: source.path,
      query: source.toString(),
    });
  }

  update(source: vscode.Uri, text: string): void {
    const uri = RawPreviewProvider.previewUri(source);
    this.texts.set(uri.toString(), text);
    this.changeEmitter.fire(uri);
  }

  provideTextDocumentContent(uri: vscode.Uri): string {
    return this.texts.get(uri.toString()) ?? "";
  }
}
```

- [ ] **Step 5: `src/editorProvider.ts` — document, provider skeleton, webview HTML, init handshake**

Lifecycle methods beyond open/resolve are stubs in this task; Task 4 fills
them. The class shape, names, and the HTML rewrite are final here.

```ts
import * as vscode from "vscode";
import type { ConfigFormat, HostToWebview, WebviewToHost } from "../../../web/vscode-protocol.js";
import { RawPreviewProvider } from "./rawPreview.js";

function formatFromName(name: string): ConfigFormat {
  if (name.endsWith(".json") || name.endsWith(".jsonc")) return "json";
  if (name.endsWith(".yaml")) return "yaml";
  if (name.endsWith(".yml")) return "yml";
  return "toml";
}

function basename(uri: vscode.Uri): string {
  return uri.path.split("/").pop() ?? "config.toml";
}

export class ConfyDocument implements vscode.CustomDocument {
  // Mirror of the session's latest serialize(): feeds the raw preview and is
  // the fallback text if a save races a dead webview.
  latestText: string;
  panel: vscode.WebviewPanel | undefined;

  constructor(readonly uri: vscode.Uri, text: string) {
    this.latestText = text;
  }

  dispose(): void {}
}

export class ConfyEditorProvider implements vscode.CustomEditorProvider<ConfyDocument> {
  static readonly viewType = "confy.editor";

  private readonly changeEmitter =
    new vscode.EventEmitter<vscode.CustomDocumentEditEvent<ConfyDocument>>();
  readonly onDidChangeCustomDocument = this.changeEmitter.event;

  private activeDocument: ConfyDocument | undefined;

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly preview: RawPreviewProvider,
  ) {}

  async openCustomDocument(
    uri: vscode.Uri,
    openContext: vscode.CustomDocumentOpenContext,
  ): Promise<ConfyDocument> {
    // A backupId means we're restoring a hot-exit backup instead of the file.
    const src = openContext.backupId ? vscode.Uri.parse(openContext.backupId) : uri;
    const bytes = await vscode.workspace.fs.readFile(src);
    return new ConfyDocument(uri, new TextDecoder().decode(bytes));
  }

  async resolveCustomEditor(
    document: ConfyDocument,
    panel: vscode.WebviewPanel,
  ): Promise<void> {
    document.panel = panel;
    this.activeDocument = document;
    const mediaRoot = vscode.Uri.joinPath(this.context.extensionUri, "media");
    panel.webview.options = { enableScripts: true, localResourceRoots: [mediaRoot] };
    panel.webview.html = await this.html(panel.webview, mediaRoot);
    panel.onDidChangeViewState(() => {
      if (panel.active) this.activeDocument = document;
    });
    panel.webview.onDidReceiveMessage((msg: WebviewToHost) => this.onMessage(document, msg));
  }

  private postToWebview(document: ConfyDocument, msg: HostToWebview): void {
    void document.panel?.webview.postMessage(msg);
  }

  private onMessage(document: ConfyDocument, msg: WebviewToHost): void {
    switch (msg.type) {
      case "ready": {
        const name = basename(document.uri);
        const lang = vscode.env.language.toLowerCase() === "zh-tw" ? "zh-TW" : "en";
        this.postToWebview(document, {
          type: "init",
          text: document.latestText,
          name,
          format: formatFromName(name),
          lang,
        });
        break;
      }
      // Task 4: edited / synced / save-response / request-undo / request-redo
      //         / request-save
      // Task 5: convert-save / parse-error
    }
  }

  // ---- lifecycle stubs (Task 4) ----
  async saveCustomDocument(document: ConfyDocument): Promise<void> {
    throw new Error("not implemented until Task 4");
  }
  async saveCustomDocumentAs(document: ConfyDocument, destination: vscode.Uri): Promise<void> {
    throw new Error("not implemented until Task 4");
  }
  async revertCustomDocument(document: ConfyDocument): Promise<void> {
    throw new Error("not implemented until Task 4");
  }
  async backupCustomDocument(
    document: ConfyDocument,
    context: vscode.CustomDocumentBackupContext,
  ): Promise<vscode.CustomDocumentBackup> {
    throw new Error("not implemented until Task 4");
  }

  // Task 5 fills this in.
  openRawPreview(): void {}

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

- [ ] **Step 6: `src/extension.ts`**

```ts
import * as vscode from "vscode";
import { ConfyEditorProvider } from "./editorProvider.js";
import { RawPreviewProvider } from "./rawPreview.js";

export function activate(context: vscode.ExtensionContext): void {
  const preview = new RawPreviewProvider();
  const provider = new ConfyEditorProvider(context, preview);
  context.subscriptions.push(
    vscode.workspace.registerTextDocumentContentProvider(RawPreviewProvider.scheme, preview),
    vscode.window.registerCustomEditorProvider(ConfyEditorProvider.viewType, provider, {
      // Spec: the Session lives in webview memory; keep it alive when the tab
      // is backgrounded instead of serializing/restoring state.
      webviewOptions: { retainContextWhenHidden: true },
      supportsMultipleEditorsPerDocument: false,
    }),
    vscode.commands.registerCommand("confy.openRawPreview", () => provider.openRawPreview()),
  );
}

export function deactivate(): void {}
```

- [ ] **Step 7: Install, typecheck, build (scratchpad)**

```bash
cd /Volumes/Home/Users/wen/repos/confy/editors/vscode && npm install && npx tsc --noEmit
```
Expected: exit 0.

```bash
R=/Volumes/Home/Users/wen/repos/confy
S="$CLAUDE_SCRATCHPAD/vsxbuild"
rm -rf "$S" && mkdir -p "$S/web" "$S/editors"
cp "$R/web/vscode-protocol.ts" "$S/web/"
cp -R "$R/web/dist" "$S/web/dist"
cp -R "$R/editors/vscode" "$S/editors/vscode"
cd "$S/editors/vscode" && node build.mjs
cp -R dist media "$R/editors/vscode/"
```
Expected: `built: dist/extension.js + media/`; back in the repo, `editors/vscode/dist/extension.js` and `editors/vscode/media/index.html` + `media/pkg/confy_ffi_bg.wasm` exist.

(The scratchpad copy of `editors/vscode` includes `node_modules` via `cp -R` — that is what makes esbuild resolvable there. If `web/dist` is missing, rerun Task 2 Step 8 first.)

- [ ] **Step 8: CHANGELOG + commit**

Append to `CHANGELOG.md`:
`- 2026-07-15 feat(vscode): extension scaffold — custom editor boots the confy webview`

```bash
git add editors/vscode CHANGELOG.md
git commit -m "feat(vscode): extension scaffold — custom editor boots the confy webview"
```

---

### Task 4: Document lifecycle — dirty, save, undo/redo, revert, backup

**Files:**
- Modify: `editors/vscode/src/editorProvider.ts` (fill `onMessage` cases + replace the four lifecycle stubs; add `saveSeq`/`pendingSaves`/`requestText`)

**Interfaces:**
- Consumes: Task 2's webview behavior (`edited`/`synced`/`save-response`/`request-*` emissions; `undo`/`redo`/`save-request`/`save-ok`/`revert` handling) and Task 3's class skeleton.
- Produces: fully working save/dirty/undo for Task 5/6; `requestText(document): Promise<{ id: number; text: string }>` (private).

- [ ] **Step 1: Add the save-request plumbing to `ConfyEditorProvider`**

Add fields after `activeDocument`:

```ts
  private saveSeq = 0;
  private readonly pendingSaves = new Map<number, (text: string) => void>();
```

Add the private method:

```ts
  // Ask the webview for the current serialize(). Falls back to the last
  // mirrored text after 2s so a wedged webview can't hang save/backup forever
  // (latestText tracks every edited/synced, so it is at most one frame stale).
  private requestText(document: ConfyDocument): Promise<{ id: number; text: string }> {
    const id = ++this.saveSeq;
    return new Promise((resolve) => {
      this.pendingSaves.set(id, (text) => resolve({ id, text }));
      this.postToWebview(document, { type: "save-request", id });
      setTimeout(() => {
        const pending = this.pendingSaves.get(id);
        if (pending) {
          this.pendingSaves.delete(id);
          resolve({ id, text: document.latestText });
        }
      }, 2000);
    });
  }
```

- [ ] **Step 2: Fill the `onMessage` cases**

Add to the `switch` in `onMessage` (replacing the Task 4 comment):

```ts
      case "edited":
        document.latestText = msg.text;
        this.preview.update(document.uri, msg.text);
        this.changeEmitter.fire({
          document,
          label: "confy edit",
          undo: () => this.postToWebview(document, { type: "undo" }),
          redo: () => this.postToWebview(document, { type: "redo" }),
        });
        break;
      case "synced":
        // Host-initiated change (undo/redo/revert/save-ok): mirror + preview
        // only — pushing an edit entry here would double-count our own undo.
        document.latestText = msg.text;
        this.preview.update(document.uri, msg.text);
        break;
      case "save-response": {
        const pending = this.pendingSaves.get(msg.id);
        if (pending) {
          this.pendingSaves.delete(msg.id);
          pending(msg.text);
        }
        break;
      }
      case "request-undo":
        void vscode.commands.executeCommand("undo");
        break;
      case "request-redo":
        void vscode.commands.executeCommand("redo");
        break;
      case "request-save":
        void vscode.commands.executeCommand("workbench.action.files.save");
        break;
```

- [ ] **Step 3: Replace the four lifecycle stubs**

```ts
  async saveCustomDocument(document: ConfyDocument): Promise<void> {
    const { id, text } = await this.requestText(document);
    await vscode.workspace.fs.writeFile(document.uri, new TextEncoder().encode(text));
    // Spec's save-ok ack: the session marks itself clean only after the write
    // actually succeeded; a writeFile throw skips this and the doc stays dirty.
    this.postToWebview(document, { type: "save-ok", id });
  }

  async saveCustomDocumentAs(document: ConfyDocument, destination: vscode.Uri): Promise<void> {
    const { text } = await this.requestText(document);
    await vscode.workspace.fs.writeFile(destination, new TextEncoder().encode(text));
  }

  async revertCustomDocument(document: ConfyDocument): Promise<void> {
    const bytes = await vscode.workspace.fs.readFile(document.uri);
    const text = new TextDecoder().decode(bytes);
    document.latestText = text;
    this.preview.update(document.uri, text);
    this.postToWebview(document, { type: "revert", text });
  }

  async backupCustomDocument(
    document: ConfyDocument,
    context: vscode.CustomDocumentBackupContext,
  ): Promise<vscode.CustomDocumentBackup> {
    // Hot exit: same text fetch as save, but no save-ok — the session must
    // not mark itself clean for a backup write.
    const { text } = await this.requestText(document);
    await vscode.workspace.fs.writeFile(context.destination, new TextEncoder().encode(text));
    return {
      id: context.destination.toString(),
      delete: async () => {
        try {
          await vscode.workspace.fs.delete(context.destination);
        } catch {
          // already gone
        }
      },
    };
  }
```

- [ ] **Step 4: Typecheck + rebuild (scratchpad, same commands as Task 3 Step 7)**

Run: `cd /Volumes/Home/Users/wen/repos/confy/editors/vscode && npx tsc --noEmit` → exit 0, then the Task 3 Step 7 scratchpad build block → `built: dist/extension.js + media/`.

- [ ] **Step 5: CHANGELOG + commit**

Append to `CHANGELOG.md`:
`- 2026-07-15 feat(vscode): document lifecycle — dirty tracking, save with save-ok ack, undo/redo single owner, revert, hot-exit backup`

```bash
git add editors/vscode/src/editorProvider.ts CHANGELOG.md
git commit -m "feat(vscode): document lifecycle (dirty/save/undo/revert/backup)"
```

---

### Task 5: Raw preview command, convert-save, parse-error

**Files:**
- Modify: `editors/vscode/src/editorProvider.ts` (`openRawPreview` body; `convert-save` + `parse-error` cases; two private helpers)

**Interfaces:**
- Consumes: `RawPreviewProvider.previewUri`/`update` (Task 3); webview `convert-save`/`parse-error` emissions (Task 2).
- Produces: the complete M1 feature surface; nothing further depends on new names.

- [ ] **Step 1: Implement `openRawPreview`**

Replace the empty body:

```ts
  // "confy: Open Raw Preview" — open the read-only serialize() mirror beside
  // the most recently active confy editor. Content updates arrive via
  // preview.update() on every edited/synced message.
  openRawPreview(): void {
    const doc = this.activeDocument;
    if (!doc) {
      void vscode.window.showInformationMessage("confy: no active confy editor");
      return;
    }
    this.preview.update(doc.uri, doc.latestText);
    void vscode.window.showTextDocument(RawPreviewProvider.previewUri(doc.uri), {
      viewColumn: vscode.ViewColumn.Beside,
      preserveFocus: true,
      preview: true,
    });
  }
```

- [ ] **Step 2: Add the `convert-save` and `parse-error` message cases + helpers**

Cases (replacing the Task 5 comment in `onMessage`):

```ts
      case "convert-save":
        void this.convertSave(document, msg.suggestedName, msg.text);
        break;
      case "parse-error":
        void this.parseError(document, msg.message);
        break;
```

Helpers:

```ts
  // Convert (or same-format save-a-copy) output: the destination pick is the
  // native save dialog — the webview cannot pick paths (spec §UI trimming).
  // The open document is never touched; offer to open the result in a new
  // confy tab.
  private async convertSave(
    document: ConfyDocument,
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
  // the default text editor for this uri instead (spec §Error handling).
  private async parseError(document: ConfyDocument, message: string): Promise<void> {
    const action = await vscode.window.showErrorMessage(
      `confy: cannot parse ${basename(document.uri)}: ${message}`,
      "Open in text editor",
    );
    if (action) {
      document.panel?.dispose();
      void vscode.commands.executeCommand("vscode.openWith", document.uri, "default");
    }
  }
```

- [ ] **Step 3: Typecheck + rebuild (scratchpad, same commands as Task 3 Step 7)**

Run: `cd /Volumes/Home/Users/wen/repos/confy/editors/vscode && npx tsc --noEmit` → exit 0, then the Task 3 Step 7 scratchpad build block → `built: dist/extension.js + media/`.

- [ ] **Step 4: CHANGELOG + commit**

Append to `CHANGELOG.md`:
`- 2026-07-15 feat(vscode): raw preview command, convert-save dialog, parse-error fallback`

```bash
git add editors/vscode/src/editorProvider.ts CHANGELOG.md
git commit -m "feat(vscode): raw preview, convert-save, parse-error fallback"
```

---

### Task 6: Package the `.vsix` + docs + acceptance handoff

**Files:**
- Create: `editors/vscode/README.md`
- Modify: `CLAUDE.md` (module map), `WEBUI.md` (new "VS Code (webview host)" section), `CHANGELOG.md`

**Interfaces:**
- Consumes: everything above; produces the installable artifact + user checklist.

- [ ] **Step 1: `editors/vscode/README.md`**

```markdown
# confy for VS Code (M1 — sideload)

Structural tree editor for TOML / JSON / JSONC / YAML, embedding the confy
web UI + wasm Session in a custom editor. Design:
`docs/superpowers/specs/2026-07-15-vscode-extension-design.md`.

## Build

1. Build the web bundle first (repo root; esbuild must run from a scratchpad
   copy on this machine — see the plan/CLAUDE.md):
   `crates/confy-ffi: wasm-pack build --target web`, then `web: node build.mjs`
   + assemble `web/dist` (cf-build.sh's copy steps).
2. `cd editors/vscode && npm install && npm run build` (same scratchpad rule).
3. `npm run package` → `confy-vscode-0.1.0.vsix`.

## Install / use

- `code --install-extension confy-vscode-0.1.0.vsix`
- Right-click a `.toml`/`.json`/`.jsonc`/`.yaml`/`.yml` tab → "Reopen Editor
  With…" → **confy**. To make confy the default for a glob, use VS Code's
  `workbench.editorAssociations` setting (e.g. `"*.toml": "confy.editor"`).
- Command palette: **confy: Open Raw Preview** — live read-only text mirror
  beside the tree.
- Save/undo/redo/revert are native VS Code (⌘S / ⌘Z / ⌘⇧Z / File > Revert).

Not in M1: Marketplace listing, watching external on-disk edits while open,
editable side-by-side text sync.
```

- [ ] **Step 2: Package**

```bash
cd /Volumes/Home/Users/wen/repos/confy/editors/vscode
cp ../../LICENSE LICENSE 2>/dev/null || echo "no repo LICENSE — vsce will warn, acceptable for a private sideload package"
npm run package
```
Expected: `confy-vscode-0.1.0.vsix` created (vsce may print warnings about repository/license for a private package — acceptable). Verify contents:

```bash
unzip -l confy-vscode-0.1.0.vsix | grep -E "extension.js|media/index.html|media/pkg/confy_ffi_bg.wasm"
```
Expected: all three present.

- [ ] **Step 3: Docs**

- `CLAUDE.md` module map: add an `editors/vscode/` block after `crates/tauri-plugin-confy-picker/`, in the same style — one line per file, noting: third host shell; `CustomEditorProvider` + `confy-raw://` preview; `web/vscode-protocol.ts`/`web/vscode.ts` adapter; the save-ok ack; the request-undo single-owner rule; media/ = build-time copy of web/dist; esbuild-from-scratchpad build rule.
- `WEBUI.md`: add a "VS Code (webview host)" section documenting the `VSHOST` gating in `ui.ts`, hidden chrome (`body.host-vscode`), theme mapping via body-class observer, and the message protocol table (copy from the spec, including the `theme`→observer refinement and the `synced` message).
- `CHANGELOG.md`: `- 2026-07-15 feat(vscode): package sideload .vsix + docs (M1)`

- [ ] **Step 4: Commit**

```bash
git add editors/vscode/README.md CLAUDE.md WEBUI.md CHANGELOG.md
git commit -m "feat(vscode): package sideload .vsix + docs (M1)"
```

- [ ] **Step 5: Hand the user the acceptance checklist**

Report done and ask the user to run the spec's 7 acceptance criteria against the installed `.vsix` (not F5): reopen-with; dirty dot; ⌘S on-disk write; ⌘Z/⌘⇧Z; live raw preview; close-with-unsaved prompt; one file per backend (TOML/JSON/YAML). Also flag the two known verify-first risks: `localStorage` access inside the webview (theme/lang persistence — if it throws, `initTheme`/`getLang` need a try/catch guard) and `executeCommand("undo")` routing to the active custom editor. Do NOT merge to `main` or flip the spec status to SHIPPED until the user confirms.

---

## Self-review notes (done at plan time)

- Spec coverage: activation/priority (T3 manifest), document ownership + init (T3), save-ok ack (T4), undo single-owner (T2+T4), raw preview (T3+T5), convert via host dialog (T2+T5), parse-error fallback (T2+T5), revert/backup (T4), CSP/wasm (T3 html), retainContextWhenHidden (T3), theme (T1 observer — documented spec refinement), i18n via `vscode.env.language` (T3), chrome trimming (T2), `.vsix` + acceptance (T6). External-change watching, Marketplace, bidirectional sync: out of scope per spec.
- `synced` message is an addition over the spec's table (the suppression half of its own undo rule) — documented in WEBUI.md in T6.
- Type consistency: `ConfigFormat` (4-value, includes `"yml"`) matches `Session.fromText`'s accepted strings and `formatFromName`'s outputs on both sides; `requestText` returns `{id, text}` and `save-ok` carries that same `id`.
