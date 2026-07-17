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
