import { test } from "node:test";
import assert from "node:assert";
import { buildSchemaDiagnostics } from "./schemaDiagnostics.ts";
import type { ViolationView } from "./wasmSession.ts";

function violation(overrides: Partial<ViolationView> = {}): ViolationView {
  return {
    path: [{ Key: "port" }],
    pointer: "/port",
    keyword: "type",
    message: "port must be an integer",
    category: "Value",
    text_range: [10, 20],
    ...overrides,
  };
}

test("one descriptor per violation with a text_range", () => {
  const result = buildSchemaDiagnostics([violation()], undefined);
  assert.deepStrictEqual(result, [{ startByte: 10, endByte: 20, message: "port must be an integer" }]);
});

test("drops violations with no resolvable text_range", () => {
  const result = buildSchemaDiagnostics([violation({ text_range: undefined })], undefined);
  assert.deepStrictEqual(result, []);
});

test("appends a line-0 descriptor for a non-empty load_error", () => {
  const result = buildSchemaDiagnostics([], "schema file not found");
  assert.deepStrictEqual(result, [{ startByte: 0, endByte: 0, message: "schema file not found" }]);
});

test("no load_error descriptor when load_error is undefined", () => {
  assert.deepStrictEqual(buildSchemaDiagnostics([], undefined), []);
});

test("violations and load_error combine", () => {
  const result = buildSchemaDiagnostics([violation()], "schema file not found");
  assert.strictEqual(result.length, 2);
});
