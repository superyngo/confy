// `vscode` is imported type-only (erased by --experimental-strip-types) and
// required lazily inside byteOffsetsToRange, so this module stays importable
// under plain `node --experimental-strip-types src/byteToPosition.test.ts` —
// there is no `vscode` package outside the extension host, and the test
// exercises only the pure converter below. The extension always runs this
// file inside build.mjs's esbuild CJS bundle, where require("vscode") is the
// extension host's own module and resolves natively.
import type * as vscode from "vscode";

// rowan's TextRange (and therefore OutlineNode.text_range/key_text_range) is
// UTF-8 byte offsets; vscode.TextDocument.positionAt expects UTF-16 code-unit
// offsets. This walks `text` once, converting a UTF-8 byte offset target into
// a UTF-16 code-unit offset — a single linear pass shared by every symbol in
// one document's outline() call (call once per byte offset needed; callers
// batch-sort offsets ascending for one shared forward pass if profiling ever
// shows this matters — not needed at config-file scale today).
export function utf8ByteOffsetToUtf16Offset(text: string, byteOffset: number): number {
  let bytes = 0;
  for (let i = 0; i < text.length; i++) {
    if (bytes >= byteOffset) return i;
    const code = text.codePointAt(i)!;
    bytes += utf8ByteLength(code);
    if (code > 0xffff) i++; // surrogate pair consumes two UTF-16 units
  }
  return text.length;
}

function utf8ByteLength(codePoint: number): number {
  if (codePoint <= 0x7f) return 1;
  if (codePoint <= 0x7ff) return 2;
  if (codePoint <= 0xffff) return 3;
  return 4;
}

/** Inverse of `utf8ByteOffsetToUtf16Offset`: walk `text` once, converting a
 * UTF-16 code-unit offset (e.g. `document.offsetAt(position)`) into the
 * UTF-8 byte offset comparable against `OutlineNode.text_range`/
 * `ViolationView.text_range` — used by the hover provider's cursor→Path
 * lookup (`outlineHitTest.ts`). */
export function utf16OffsetToUtf8ByteOffset(text: string, utf16Offset: number): number {
  let units = 0;
  let bytes = 0;
  for (let i = 0; i < text.length && units < utf16Offset; ) {
    const code = text.codePointAt(i)!;
    const len = utf8ByteLength(code);
    const width = code > 0xffff ? 2 : 1; // surrogate pair consumes two UTF-16 units
    bytes += len;
    units += width;
    i += width;
  }
  return bytes;
}

export function byteOffsetsToRange(
  document: vscode.TextDocument,
  startByte: number,
  endByte: number,
): vscode.Range {
  const text = document.getText();
  const { Range } = require("vscode") as typeof vscode;
  const start = document.positionAt(utf8ByteOffsetToUtf16Offset(text, startByte));
  const end = document.positionAt(utf8ByteOffsetToUtf16Offset(text, endByte));
  return new Range(start, end);
}
