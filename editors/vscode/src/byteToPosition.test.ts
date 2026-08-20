// Run with plain Node (no test-framework dependency), matching the repo's
// plain node:assert convention (crates/confy-ffi/functional_smoke.mjs,
// web/*.spec.mjs):
//
//   cd editors/vscode && node --experimental-strip-types src/byteToPosition.test.ts
//
// The import uses the explicit .ts extension — Node does not rewrite a .js
// specifier to .ts the way tsc/esbuild do (allowImportingTsExtensions).
import { test } from "node:test";
import assert from "node:assert";
import { utf8ByteOffsetToUtf16Offset } from "./byteToPosition.ts";

test("ASCII: byte offset equals UTF-16 offset", () => {
  assert.strictEqual(utf8ByteOffsetToUtf16Offset("port = 8080", 7), 7);
});

test("CJK: multi-byte UTF-8 char counted as 1 UTF-16 unit", () => {
  // "鍵" is 3 UTF-8 bytes, 1 UTF-16 code unit.
  const text = "鍵 = 1";
  assert.strictEqual(utf8ByteOffsetToUtf16Offset(text, 3), 1); // right after 鍵
  assert.strictEqual(utf8ByteOffsetToUtf16Offset(text, 4), 2); // right after the space
});

test("emoji: astral char counted as 2 UTF-16 units (surrogate pair)", () => {
  // "😀" is 4 UTF-8 bytes, 2 UTF-16 code units (surrogate pair).
  const text = "😀x";
  assert.strictEqual(utf8ByteOffsetToUtf16Offset(text, 4), 2); // right after 😀
});
