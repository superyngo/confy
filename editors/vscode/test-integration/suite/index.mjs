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

function findChildByName(symbol, name) {
  return symbol.children.find((child) => child.name === name);
}

async function main() {
  const extension = vscode.extensions.getExtension(EXTENSION_ID);
  assert.ok(extension, `Missing extension: ${EXTENSION_ID}`);
  await extension.activate();

  const folder = vscode.workspace.workspaceFolders?.[0];
  assert.ok(folder, "Expected a workspace folder for integration fixtures");

  const tomlUri = vscode.Uri.file(path.join(folder.uri.fsPath, "tasks.toml"));
  const tomlDocument = await vscode.workspace.openTextDocument(tomlUri);
  await vscode.window.showTextDocument(tomlDocument);

  const tomlSymbols = await waitFor(
    async () => {
      const result = await vscode.commands.executeCommand(
        "vscode.executeDocumentSymbolProvider",
        tomlUri,
      );
      return Array.isArray(result) && result.length > 0 ? result : undefined;
    },
    5000,
    "DocumentSymbolProvider returned no symbols for tasks.toml",
  );
  assert.ok(tomlSymbols.length > 0, "Expected at least one symbol from outline provider for tasks.toml");

  const workspaceTomlUri = vscode.Uri.file(path.join(folder.uri.fsPath, "workspace-package.toml"));
  const workspaceTomlDocument = await vscode.workspace.openTextDocument(workspaceTomlUri);
  await vscode.window.showTextDocument(workspaceTomlDocument);

  const workspaceSymbols = await waitFor(
    async () => {
      const result = await vscode.commands.executeCommand(
        "vscode.executeDocumentSymbolProvider",
        workspaceTomlUri,
      );
      return Array.isArray(result) && result.length > 0 ? result : undefined;
    },
    5000,
    "DocumentSymbolProvider returned no symbols for workspace-package.toml",
  );

  const workspace = workspaceSymbols.find((symbol) => symbol.name === "workspace");
  assert.ok(workspace, "Expected workspace root table symbol");

  const pkg = findChildByName(workspace, "package");
  assert.ok(pkg, "Expected workspace.package symbol");
  assert.ok(
    workspace.range.contains(pkg.range),
    "Expected parent workspace range to contain workspace.package range for breadcrumb nesting",
  );

  const yamlUri = vscode.Uri.file(path.join(folder.uri.fsPath, "sample.yaml"));
  const yamlDocument = await vscode.workspace.openTextDocument(yamlUri);
  await vscode.window.showTextDocument(yamlDocument);

  const yamlSymbols = await waitFor(
    async () => {
      const result = await vscode.commands.executeCommand(
        "vscode.executeDocumentSymbolProvider",
        yamlUri,
      );
      return Array.isArray(result) && result.length > 0 ? result : undefined;
    },
    5000,
    "DocumentSymbolProvider returned no symbols for sample.yaml",
  );
  assert.ok(yamlSymbols.length > 0, "Expected at least one symbol from outline provider for sample.yaml");

  const diagnostics = await waitFor(
    async () => {
      const result = vscode.languages.getDiagnostics(tomlUri);
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
        tomlUri,
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
