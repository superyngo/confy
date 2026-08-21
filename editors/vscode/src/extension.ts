import * as vscode from "vscode";
import { ConfyEditorProvider } from "./editorProvider.js";
import { ConfyOutlineProvider } from "./outlineProvider.js";
import { readFile } from "node:fs/promises";
import { formatFromName } from "./formatFromName.js";
import { loadConfySession } from "./wasmSession.js";
import { SchemaSessionManager, type DocSyncResult } from "./schemaSessionManager.js";
import { ConfySchemaHoverProvider } from "./schemaHoverProvider.js";
import { isDiagnosticsDeferred } from "./schemaCoexistence.js";
import { buildSchemaDiagnostics } from "./schemaDiagnostics.js";
import { byteOffsetsToRange } from "./byteToPosition.js";

// A tab's (uri, viewType) identity — "default" for the built-in text editor,
// since TabInputText carries no viewType of its own.
function tabInfo(tab: vscode.Tab): { uri: vscode.Uri; viewType: string } | undefined {
  const input = tab.input;
  if (input instanceof vscode.TabInputCustom) return { uri: input.uri, viewType: input.viewType };
  if (input instanceof vscode.TabInputText) return { uri: input.uri, viewType: "default" };
  return undefined;
}

// Swap the tab showing `uri` to `viewType`, replacing rather than stacking:
// VS Code tracks tabs by (uri, viewType), so a plain `vscode.openWith` for a
// different viewType leaves the old tab open alongside the new one instead of
// reusing it. Opening the new view FIRST (so the shared TextDocument keeps at
// least one reference) then closing the old tab is the closest an extension can
// get to the built-in "Reopen Editor With…" swap — but `tabGroups.close()` still
// shows the unsaved-changes confirmation on a dirty document regardless of
// another tab sharing it (see its API doc); there is no public API for the
// in-place editor-input replace VS Code's own UI uses, so that prompt is a known,
// unavoidable limitation here (VSCODE.md § Title-bar tab swap).
async function swapEditorKind(uri: vscode.Uri, viewType: string): Promise<void> {
  const group = vscode.window.tabGroups.activeTabGroup;
  const oldTab = group?.tabs.find((t) => {
    const info = tabInfo(t);
    return info !== undefined && info.uri.toString() === uri.toString() && info.viewType !== viewType;
  });
  await vscode.commands.executeCommand("vscode.openWith", uri, viewType, group?.viewColumn);
  if (oldTab) await vscode.window.tabGroups.close(oldTab, true);
}

export async function activate(context: vscode.ExtensionContext): Promise<void> {
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
    // Title-bar toggle: swapEditorKind replaces the active tab in place. The
    // shared TextDocument carries dirty state across the swap — no save needed.
    vscode.commands.registerCommand("confy.openWithConfy", (uri?: vscode.Uri) => {
      const target = uri ?? vscode.window.activeTextEditor?.document.uri;
      if (target) void swapEditorKind(target, ConfyEditorProvider.viewType);
    }),
    vscode.commands.registerCommand("confy.reopenAsText", (uri?: vscode.Uri) => {
      const target = uri ?? provider.activeUri;
      if (target) void swapEditorKind(target, "default");
    }),
    // M1.6: with the confy toolbar header hidden in this host (VSCODE.md §
    // Chrome trimming), these live in the editor title's "…" More Actions menu.
    vscode.commands.registerCommand("confy.saveAsConvert", () => {
      provider.postToActive({ type: "exec", action: "save-as" });
    }),
    vscode.commands.registerCommand("confy.help", () => {
      provider.postToActive({ type: "exec", action: "help" });
    }),
    vscode.commands.registerCommand("confy.about", () => {
      provider.postToActive({ type: "exec", action: "about" });
    }),
    // Language is a native submenu of the "…" menu (contributes.submenus) —
    // one command per language, picked directly, no intermediate QuickPick.
    vscode.commands.registerCommand("confy.langEnglish", () => setLang("en")),
    vscode.commands.registerCommand("confy.langZhTw", () => setLang("zh-TW")),
    // Theme is the same pattern as language: a native submenu, one command
    // per mode, no intermediate QuickPick.
    vscode.commands.registerCommand("confy.themeAuto", () => setTheme("auto")),
    vscode.commands.registerCommand("confy.themeLight", () => setTheme("light")),
    vscode.commands.registerCommand("confy.themeDark", () => setTheme("dark")),
  );

  // Native-editor Outline / breadcrumbs for TOML/YAML opened in VS Code's own
  // text editor (Explorer's Outline view, ⇧⌘O go-to-symbol, the breadcrumb
  // bar): the extension host loads the wasm core itself (not the webview) and
  // maps its read-only outline() tree onto DocumentSymbols. confy's own custom
  // editor tab is a webview and stays out of this by design (spec's Platform
  // constraint). runtime-only registration has no declarative `contributes`
  // equivalent, hence package.json's explicit activationEvents.
  context.subscriptions.push(
    vscode.languages.registerDocumentSymbolProvider(
      [{ pattern: "**/*.toml" }, { pattern: "**/*.yaml" }, { pattern: "**/*.yml" }],
      new ConfyOutlineProvider(context),
    ),
  );

  const SCHEMA_SELECTOR = [
    { pattern: "**/*.toml" },
    { pattern: "**/*.yaml" },
    { pattern: "**/*.yml" },
  ];

  const diagnostics = vscode.languages.createDiagnosticCollection("confy-schema");
  const deferredDocs = new Set<string>(); // keys currently deferring diagnostics
  let managerPromise: Promise<SchemaSessionManager> | undefined;
  async function getManager(): Promise<SchemaSessionManager> {
    if (!managerPromise) {
      managerPromise = loadConfySession(context).then(
        (ctor) =>
          new SchemaSessionManager(ctor, {
            readFile: (p) => readFile(p, "utf8"),
            fetchUrl: async (url) => {
              const res = await fetch(url);
              if (!res.ok) throw new Error(`HTTP ${res.status}`);
              return res.text();
            },
          }),
      );
    }
    return managerPromise;
  }

  function updateDiagnostics(document: vscode.TextDocument, result: DocSyncResult): void {
    const key = document.uri.toString();
    if (deferredDocs.has(key)) return;
    const descriptors = buildSchemaDiagnostics(result.violations, result.loadError);
    diagnostics.set(
      document.uri,
      descriptors.map(
        (d) =>
          new vscode.Diagnostic(
            byteOffsetsToRange(document, d.startByte, d.endByte),
            d.message,
            vscode.DiagnosticSeverity.Warning,
          ),
      ),
    );
  }

  async function openDoc(document: vscode.TextDocument): Promise<void> {
    if (!SCHEMA_SELECTOR.some((s) => vscode.languages.match(s, document) > 0)) return;
    const key = document.uri.toString();
    if (isDiagnosticsDeferred(document.fileName.endsWith(".yaml") || document.fileName.endsWith(".yml") ? "yaml" : "toml", (id) => vscode.extensions.getExtension(id) !== undefined)) {
      deferredDocs.add(key);
    } else {
      deferredDocs.delete(key);
    }
    const manager = await getManager();
    const result = await manager.open(key, document.uri.fsPath, document.getText(), formatFromName(document.fileName));
    updateDiagnostics(document, result);
  }

  const reparseTimers = new Map<string, NodeJS.Timeout>();
  function scheduleReparse(document: vscode.TextDocument): void {
    const key = document.uri.toString();
    const existing = reparseTimers.get(key);
    clearTimeout(existing);
    reparseTimers.set(
      key,
      setTimeout(async () => {
        reparseTimers.delete(key);
        const manager = await getManager();
        const result = await manager.reparse(key, document.getText());
        if (!result) return;
        if (result.invalidSyntax) {
          if (!deferredDocs.has(key)) diagnostics.set(document.uri, []);
          return;
        }
        updateDiagnostics(document, result);
      }, 300),
    );
  }

  context.subscriptions.push(
    diagnostics,
    vscode.languages.registerHoverProvider(SCHEMA_SELECTOR, new ConfySchemaHoverProvider(await getManager())),
    vscode.workspace.onDidOpenTextDocument(openDoc),
    vscode.workspace.onDidChangeTextDocument((e) => {
      if (SCHEMA_SELECTOR.some((s) => vscode.languages.match(s, e.document) > 0)) scheduleReparse(e.document);
    }),
    vscode.workspace.onDidCloseTextDocument(async (document) => {
      const key = document.uri.toString();
      const timer = reparseTimers.get(key);
      clearTimeout(timer);
      reparseTimers.delete(key);
      deferredDocs.delete(key);
      (await getManager()).close(key);
      diagnostics.delete(document.uri);
    }),
  );
  // Documents already open when the extension activates (e.g. a restored
  // window) still need their initial schema sync — mirrors why the
  // outline provider needs no equivalent (it's request-driven, not
  // event-driven).
  for (const document of vscode.workspace.textDocuments) {
    if (SCHEMA_SELECTOR.some((s) => vscode.languages.match(s, document) > 0)) void openDoc(document);
  }

  async function setLang(lang: "en" | "zh-TW"): Promise<void> {
    await context.globalState.update("confy.lang", lang);
    provider.postToActive({ type: "set-lang", lang });
  }

  async function setTheme(theme: "auto" | "light" | "dark"): Promise<void> {
    await context.globalState.update("confy.theme", theme);
    provider.postToActive({ type: "set-theme", theme });
  }
}

export function deactivate(): void {}
