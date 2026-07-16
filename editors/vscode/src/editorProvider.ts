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

  get activeUri(): vscode.Uri | undefined {
    return this.activeDocument?.uri;
  }

  private saveSeq = 0;
  private readonly pendingSaves = new Map<number, (text: string) => void>();

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

  // Ask the webview for the current serialize(). Falls back to the last
  // mirrored text after 2s so a wedged webview can't hang save/backup forever
  // (latestText tracks every edited/synced, so it is at most one frame stale).
  private requestText(document: ConfyDocument): Promise<{ id: number; text: string }> {
    const id = ++this.saveSeq;
    // No live panel (tab already closed, e.g. a shutdown backup): answer from
    // the mirror immediately instead of eating the 2s timeout.
    if (!document.panel) return Promise.resolve({ id, text: document.latestText });
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
      case "edited": {
        document.latestText = msg.text;
        this.preview.update(document.uri, msg.text);
        const token = { cancelled: false };
        document.editTokens.push(token);
        this.changeEmitter.fire({
          document,
          label: "confy edit",
          undo: () => {
            if (!token.cancelled) this.postToWebview(document, { type: "undo" });
          },
          redo: () => {
            if (!token.cancelled) this.postToWebview(document, { type: "redo" });
          },
        });
        break;
      }
      case "synced":
        // Host-initiated change (undo/redo/revert/save-ok): mirror + preview
        // only — pushing an edit entry here would double-count our own undo.
        document.latestText = msg.text;
        this.preview.update(document.uri, msg.text);
        break;
      case "edit-cancelled": {
        // The Session rolled back its newest history entry (add→Esc via
        // cancel_last). VS Code's stack API can't remove the matching entry,
        // so neuter it: popping it later must not undo an older, wrong
        // Session edit. Cancellation always immediately follows its own push
        // (the add flow is modal), so the newest live token is the target.
        // Residual wart (documented): the neutered entry still counts toward
        // the dirty dot until it's popped by one no-op ⌘Z.
        document.latestText = msg.text;
        this.preview.update(document.uri, msg.text);
        for (let i = document.editTokens.length - 1; i >= 0; i--) {
          if (!document.editTokens[i].cancelled) {
            document.editTokens[i].cancelled = true;
            break;
          }
        }
        break;
      }
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
      case "convert-save":
        void this.convertSave(document, msg.suggestedName, msg.text);
        break;
      case "parse-error":
        void this.parseError(document, msg.message);
        break;
    }
  }

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
