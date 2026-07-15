import * as vscode from "vscode";
import type { ConfigFormat, HostToWebview, WebviewToHost } from "../../../web/vscode-protocol.js";
import { RawPreviewProvider } from "./rawPreview.js";

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

export class ConfyDocument implements vscode.CustomDocument {
  // Mirror of the session's latest serialize(): feeds the raw preview and is
  // the fallback text if a save races a dead webview.
  latestText: string;
  panel: vscode.WebviewPanel | undefined;
  // One token per pushed VS Code edit entry; `edit-cancelled` flips the newest
  // live one so its undo/redo callbacks no-op — the Session already rolled
  // that edit back via History::cancel_last (see notifyHost's depth rule).
  readonly editTokens: { cancelled: boolean }[] = [];

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
    // A disposed webview throws synchronously on postMessage — clear the
    // reference so postToWebview's optional chain actually protects the
    // shutdown path (hot-exit backups race tab teardown).
    panel.onDidDispose(() => {
      if (document.panel === panel) document.panel = undefined;
      if (this.activeDocument === document) this.activeDocument = undefined;
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
      // Task 4: edited / synced / edit-cancelled / save-response
      //         / request-undo / request-redo / request-save
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
