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
    // Title-bar toggle: vscode.openWith on the same uri in the active group
    // swaps the tab in place (a resource opens at most once per group).
    vscode.commands.registerCommand("confy.openWithConfy", async (uri?: vscode.Uri) => {
      const target = uri ?? vscode.window.activeTextEditor?.document.uri;
      if (!target) return;
      const open = vscode.workspace.textDocuments.find(
        (d) => d.uri.toString() === target.toString(),
      );
      // openCustomDocument reads from disk, so flush an unsaved text buffer
      // first — otherwise confy would show stale content beside a live dirty doc.
      if (open?.isDirty) await open.save();
      await vscode.commands.executeCommand("vscode.openWith", target, ConfyEditorProvider.viewType);
    }),
    vscode.commands.registerCommand("confy.reopenAsText", (uri?: vscode.Uri) => {
      const target = uri ?? provider.activeUri;
      if (target) void vscode.commands.executeCommand("vscode.openWith", target, "default");
    }),
  );
}

export function deactivate(): void {}
