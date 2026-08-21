import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";
import path from "node:path";
import * as vscode from "vscode";

const EXTENSION_ID = "wenanlin.confy-vscode";

async function waitFor(predicate, timeoutMs, failureMessage) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    const value = await predicate();
    if (value) return value;
    await delay(100);
  }
  throw new Error(failureMessage);
}

async function main() {
  const extension = vscode.extensions.getExtension(EXTENSION_ID);
  assert.ok(extension, `Missing extension: ${EXTENSION_ID}`);
  await extension.activate();

  const folder = vscode.workspace.workspaceFolders?.[0];
  assert.ok(folder, "Expected a workspace folder for integration fixtures");

  const docUri = vscode.Uri.file(path.join(folder.uri.fsPath, "tasks.toml"));
  const document = await vscode.workspace.openTextDocument(docUri);
  await vscode.window.showTextDocument(document);

  const symbols = await waitFor(
    async () => {
      const result = await vscode.commands.executeCommand(
        "vscode.executeDocumentSymbolProvider",
        docUri,
      );
      return Array.isArray(result) && result.length > 0 ? result : undefined;
    },
    5000,
    "DocumentSymbolProvider returned no symbols",
  );
  assert.ok(symbols.length > 0, "Expected at least one symbol from outline provider");

  const diagnostics = await waitFor(
    async () => {
      const result = vscode.languages.getDiagnostics(docUri);
      return result.length > 0 ? result : undefined;
    },
    8000,
    "Expected schema diagnostics but found none",
  );
  assert.ok(diagnostics.length > 0, "Expected non-empty schema diagnostics");

  const hover = await waitFor(
    async () => {
      const result = await vscode.commands.executeCommand(
        "vscode.executeHoverProvider",
        docUri,
        new vscode.Position(1, 1),
      );
      return Array.isArray(result) && result.length > 0 ? result : undefined;
    },
    5000,
    "Expected non-empty hover result",
  );

  const hoverText = hover
    .flatMap((h) => h.contents)
    .map((c) => (typeof c === "string" ? c : "value" in c ? c.value : ""))
    .join("\n");

  assert.match(
    hoverText,
    /Allowed values:|minimum:|maximum:|multiple of:/,
    `Unexpected hover payload: ${hoverText}`,
  );
}

export async function run() {
  await main();
}
