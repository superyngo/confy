import { test } from "node:test";
import assert from "node:assert";
import { findPathAtByteOffset } from "./outlineHitTest.ts";
import type { OutlineNode } from "./wasmSession.ts";

function node(key: string, range: [number, number], children: OutlineNode[] = []): OutlineNode {
  return { key, path: [{ Key: key }], type_label: "string", value: null, text_range: range, key_text_range: undefined, children };
}

test("finds the deepest node whose range contains the offset", () => {
  const tree = [node("server", [0, 30], [node("port", [10, 20])])];
  assert.deepStrictEqual(findPathAtByteOffset(tree, 15), [{ Key: "port" }]);
});

test("falls back to the shallower ancestor when the offset misses every child", () => {
  const tree = [node("server", [0, 30], [node("port", [10, 20])])];
  assert.deepStrictEqual(findPathAtByteOffset(tree, 25), [{ Key: "server" }]);
});

test("returns undefined when the offset is outside every node", () => {
  const tree = [node("server", [0, 30])];
  assert.strictEqual(findPathAtByteOffset(tree, 99), undefined);
});
