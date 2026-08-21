// editors/vscode/src/schemaDedup.test.ts
import { test } from "node:test";
import assert from "node:assert";
import { needsSchemaReload } from "./schemaDedup.ts";

test("no hint detected: never reloads", () => {
  assert.strictEqual(needsSchemaReload(undefined, { Local: "s.json" }, undefined), false);
});

test("first detection with nothing loaded yet: reloads", () => {
  assert.strictEqual(needsSchemaReload({ Local: "s.json" }, undefined, undefined), true);
});

test("same Local source already loaded successfully: does not reload", () => {
  const source = { Local: "s.json" };
  const status = { source_label: "s.json", violation_count: 0, load_error: undefined };
  assert.strictEqual(needsSchemaReload(source, source, status), false);
});

test("same source but the previous load failed: retries", () => {
  const source = { Local: "s.json" };
  const status = { source_label: "s.json", violation_count: 0, load_error: "not found" };
  assert.strictEqual(needsSchemaReload(source, source, status), true);
});

test("detected source differs from what's loaded: reloads", () => {
  assert.strictEqual(
    needsSchemaReload({ Local: "b.json" }, { Local: "a.json" }, undefined),
    true,
  );
});

test("Local and Url with the same string are not the same source: reloads", () => {
  assert.strictEqual(
    needsSchemaReload({ Url: "s.json" }, { Local: "s.json" }, undefined),
    true,
  );
});
