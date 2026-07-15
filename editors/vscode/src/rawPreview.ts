import * as vscode from "vscode";

// Read-only live mirror of the confy session's serialize() output — the
// spec's one-way sync surface. Content is read-only by scheme; the preview
// uri keeps the source file's path (and thus extension) so VS Code picks the
// right syntax highlighting, and carries the full source uri in the query so
// two same-named files don't collide.
export class RawPreviewProvider implements vscode.TextDocumentContentProvider {
  static readonly scheme = "confy-raw";

  private readonly texts = new Map<string, string>();
  private readonly changeEmitter = new vscode.EventEmitter<vscode.Uri>();
  readonly onDidChange = this.changeEmitter.event;

  static previewUri(source: vscode.Uri): vscode.Uri {
    return vscode.Uri.from({
      scheme: RawPreviewProvider.scheme,
      path: source.path,
      query: source.toString(),
    });
  }

  update(source: vscode.Uri, text: string): void {
    const uri = RawPreviewProvider.previewUri(source);
    this.texts.set(uri.toString(), text);
    this.changeEmitter.fire(uri);
  }

  provideTextDocumentContent(uri: vscode.Uri): string {
    return this.texts.get(uri.toString()) ?? "";
  }
}
