import { test } from "node:test";
import assert from "node:assert";
import * as path from "node:path";
import { resolveLocalSchemaPath } from "./schemaPathResolve.ts";

test("resolves a bare filename against the document's directory", () => {
  const result = resolveLocalSchemaPath("/proj/config/app.toml", "schema.json");
  assert.strictEqual(result, path.resolve("/proj/config", "schema.json"));
});

test("resolves a ./relative path against the document's directory", () => {
  const result = resolveLocalSchemaPath("/proj/config/app.toml", "./schemas/app.json");
  assert.strictEqual(result, path.resolve("/proj/config", "./schemas/app.json"));
});

test("resolves a ../relative path against the document's directory", () => {
  const result = resolveLocalSchemaPath("/proj/config/app.toml", "../schemas/app.json");
  assert.strictEqual(result, path.resolve("/proj/config", "../schemas/app.json"));
});

test("passes an absolute path through untouched", () => {
  const abs = path.resolve("/other/place/schema.json");
  assert.strictEqual(resolveLocalSchemaPath("/proj/config/app.toml", abs), abs);
});
