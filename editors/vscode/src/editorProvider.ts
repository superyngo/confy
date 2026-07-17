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
  private activePanel: vscode.WebviewPanel | undefined;

  get activeUri(): vscode.Uri | undefined {
    return this.activeDocument?.uri;
  }

  // "…" More Actions title-bar commands (Save As/Convert, Help, About, Choose
  // Language) have no keyboard/toolbar entry point inside the webview's own
  // chrome (hidden under host-vscode — see VSCODE.md § Chrome trimming), so
  // they post straight to whichever confy webview panel is currently active.
  postToActive(msg: HostToWebview): void {
    void this.activePanel?.webview.postMessage(msg);
  }

  constructor(private readonly context: vscode.ExtensionContext) {}

  async resolveCustomTextEditor(
    document: vscode.TextDocument,
    panel: vscode.WebviewPanel,
  ): Promise<void> {
    this.activeDocument = document;
    this.activePanel = panel;
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
      if (panel.active) {
        this.activeDocument = document;
        this.activePanel = panel;
      }
    });

    panel.onDidDispose(() => {
      clearTimeout(debounce);
      changeSub.dispose();
      saveSub.dispose();
      if (this.activeDocument === document) this.activeDocument = undefined;
      if (this.activePanel === panel) this.activePanel = undefined;
    });

    panel.webview.onDidReceiveMessage((msg: WebviewToHost) => {
      switch (msg.type) {
        case "ready": {
          const name = basename(document.uri);
          // A user-picked language (confy.chooseLanguage → globalState) wins
          // over VS Code's display language once set; otherwise fall back to
          // the same env.language mapping as before.
          const savedLang = this.context.globalState.get<string>("confy.lang");
          const lang: "en" | "zh-TW" =
            savedLang === "en" || savedLang === "zh-TW"
              ? savedLang
              : vscode.env.language.toLowerCase() === "zh-tw"
                ? "zh-TW"
                : "en";
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
      markKnown(" never-matches ");
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
