// editors/vscode/src/schemaCoexistence.test.ts
import { test } from "node:test";
import assert from "node:assert";
import { isDiagnosticsDeferred } from "./schemaCoexistence.ts";

test("defers TOML diagnostics when Even Better TOML is installed", () => {
  assert.strictEqual(
    isDiagnosticsDeferred("toml", (id) => id === "tamasfe.even-better-toml"),
    true,
  );
});

test("does not defer TOML diagnostics when nothing relevant is installed", () => {
  assert.strictEqual(isDiagnosticsDeferred("toml", () => false), false);
});

test("defers YAML diagnostics when redhat.vscode-yaml is installed", () => {
  assert.strictEqual(
    isDiagnosticsDeferred("yaml", (id) => id === "redhat.vscode-yaml"),
    true,
  );
});

test("a TOML extension installed does not defer YAML", () => {
  assert.strictEqual(
    isDiagnosticsDeferred("yaml", (id) => id === "tamasfe.even-better-toml"),
    false,
  );
});
