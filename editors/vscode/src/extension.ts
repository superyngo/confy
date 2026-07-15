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
