import * as vscode from "vscode";
import { ConfyEditorProvider } from "./editorProvider.js";

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
// least one reference) then closing the old tab mirrors what the built-in
// "Reopen Editor With…" does — and means the close never triggers an
// unsaved-changes prompt, since the document is still open in the new tab.
async function swapEditorKind(uri: vscode.Uri, viewType: string): Promise<void> {
  const group = vscode.window.tabGroups.activeTabGroup;
  const oldTab = group?.tabs.find((t) => {
    const info = tabInfo(t);
    return info !== undefined && info.uri.toString() === uri.toString() && info.viewType !== viewType;
  });
  await vscode.commands.executeCommand("vscode.openWith", uri, viewType, group?.viewColumn);
  if (oldTab) await vscode.window.tabGroups.close(oldTab, true);
}

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
  );
}

export function deactivate(): void {}
