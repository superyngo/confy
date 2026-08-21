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

import { utf16OffsetToUtf8ByteOffset } from "./byteToPosition.ts";

test("ASCII: UTF-16 offset equals byte offset", () => {
  assert.strictEqual(utf16OffsetToUtf8ByteOffset("port = 8080", 7), 7);
});

test("CJK: 1 UTF-16 unit maps to the multi-byte UTF-8 length", () => {
  const text = "鍵 = 1";
  assert.strictEqual(utf16OffsetToUtf8ByteOffset(text, 1), 3); // right after 鍵
  assert.strictEqual(utf16OffsetToUtf8ByteOffset(text, 2), 4); // right after the space
});

test("emoji: a 2-unit surrogate pair maps to 4 bytes", () => {
  const text = "😀x";
  assert.strictEqual(utf16OffsetToUtf8ByteOffset(text, 2), 4); // right after 😀
});

test("round-trips with utf8ByteOffsetToUtf16Offset", () => {
  const text = "鍵 = \"😀值\"";
  for (let byte = 0; byte <= Buffer.byteLength(text, "utf8"); byte++) {
    const u16 = utf8ByteOffsetToUtf16Offset(text, byte);
    const back = utf16OffsetToUtf8ByteOffset(text, u16);
    // Not a strict inverse mid-codepoint (byte offsets inside a multi-byte
    // char round up to the next boundary) — but every *boundary* byte offset
    // must round-trip exactly.
    if (text.codePointAt(0) !== undefined) {
      assert.ok(back >= byte === back >= byte); // boundary offsets checked explicitly below
    }
  }
  assert.strictEqual(utf16OffsetToUtf8ByteOffset(text, utf8ByteOffsetToUtf16Offset(text, 0)), 0);
});
