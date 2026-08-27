// editors/vscode/src/schemaSessionManager.test.ts
import { test } from "node:test";
import assert from "node:assert";
import { SchemaSessionManager } from "./schemaSessionManager.ts";
import type { ConfySessionCtor, ConfySessionHandle } from "./wasmSession.ts";

// A minimal in-memory stand-in for the wasm ConfySession, driving exactly the
// subset of dispatch/snapshot/schema_violations behavior this manager relies
// on (design §"Testing": "mocked minimal vscode document/fs/fetch surface").
class FakeSession implements ConfySessionHandle {
  text: string;
  schemaSource: { Local: string } | { Url: string } | undefined;
  loadError: string | undefined = undefined;
  hint: { Local: string } | undefined;
  failNextReplace = false;

  constructor(text: string, _format: string) {
    this.text = text;
    this.hint = text.includes("#:schema") ? { Local: text.split("#:schema ")[1].split("\n")[0] } : undefined;
  }
  outline() { return []; }
  schema_hint() { return "None" as const; }
  schema_violations() { return []; }
  // Mirrors confy-core's `Session::sync_schema_hint`: only surface a fetch
  // request when the hint is new/changed, or the same hint previously
  // failed to load — the manager itself no longer computes this.
  private fetchRequest(): { Local: string } | undefined {
    if (!this.hint) return undefined;
    const same = this.schemaSource && "Local" in this.schemaSource && this.schemaSource.Local === this.hint.Local;
    if (same && this.loadError === undefined) return undefined;
    return this.hint;
  }
  snapshot() {
    return {
      schema_fetch_request: this.fetchRequest(),
      schema_status: this.schemaSource
        ? { source_label: "s", violation_count: 0, load_error: this.loadError }
        : undefined,
    } as any;
  }
  dispatch(intent: any) {
    if (intent.ApplyReplace !== undefined) {
      if (this.failNextReplace) return { error: "parse error" } as any;
      this.text = intent.ApplyReplace.text;
      this.hint = this.text.includes("#:schema")
        ? { Local: this.text.split("#:schema ")[1].split("\n")[0] }
        : undefined;
      return { error: undefined, ...this.snapshot() } as any;
    }
    if (intent.SchemaLoaded !== undefined) {
      this.schemaSource = intent.SchemaLoaded.source;
      this.loadError = intent.SchemaLoaded.text.Err;
      return this.snapshot();
    }
    throw new Error(`unexpected intent in FakeSession: ${JSON.stringify(intent)}`);
  }
}
const FakeCtor = FakeSession as unknown as ConfySessionCtor;

function deps(overrides: Partial<{ readFile: (p: string) => Promise<string>; fetchUrl: (u: string) => Promise<string> }> = {}) {
  return {
    readFile: overrides.readFile ?? (async () => "{}"),
    fetchUrl: overrides.fetchUrl ?? (async () => "{}"),
  };
}

test("open() with a hint fetches and loads the schema", async () => {
  let readCalls = 0;
  const manager = new SchemaSessionManager(FakeCtor, deps({ readFile: async () => { readCalls++; return "{}"; } }));
  const result = await manager.open("doc1", "/proj/app.toml", "#:schema ./s.json\nport=1\n", "toml");
  assert.strictEqual(readCalls, 1);
  assert.strictEqual(result.invalidSyntax, false);
});

test("reparse() with an unchanged hint does not re-fetch", async () => {
  let readCalls = 0;
  const manager = new SchemaSessionManager(FakeCtor, deps({ readFile: async () => { readCalls++; return "{}"; } }));
  await manager.open("doc1", "/proj/app.toml", "#:schema ./s.json\nport=1\n", "toml");
  assert.strictEqual(readCalls, 1);
  await manager.reparse("doc1", "#:schema ./s.json\nport=2\n");
  assert.strictEqual(readCalls, 1, "same hint must not trigger a second fetch");
});

test("reparse() with invalid syntax reports invalidSyntax and skips schema sync", async () => {
  let readCalls = 0;
  const manager = new SchemaSessionManager(FakeCtor, deps({ readFile: async () => { readCalls++; return "{}"; } }));
  await manager.open("doc1", "/proj/app.toml", "port=1\n", "toml");
  // Reach into the fake to force the next ApplyReplace to fail — a
  // test-only hook exercising the manager's `snap.error` branch.
  (manager as any).docs.get("doc1").session.failNextReplace = true;
  const result = await manager.reparse("doc1", "port=");
  assert.strictEqual(result?.invalidSyntax, true);
  assert.strictEqual(readCalls, 0);
});

test("reparse() on an unknown key returns undefined", async () => {
  const manager = new SchemaSessionManager(FakeCtor, deps());
  const result = await manager.reparse("never-opened", "port=1\n");
  assert.strictEqual(result, undefined);
});

test("close() then a slow fetch resolving later is discarded, not dispatched", async () => {
  let resolveRead: (v: string) => void;
  const pending = new Promise<string>((resolve) => { resolveRead = resolve; });
  // Capture the session the manager constructs so the "not dispatched"
  // claim is observable: if SchemaLoaded ran anyway, schemaSource would be set.
  let created: FakeSession | undefined;
  const CapturingCtor = class extends FakeSession {
    constructor(text: string, format: string) {
      super(text, format);
      created = this;
    }
  } as unknown as ConfySessionCtor;
  const manager = new SchemaSessionManager(CapturingCtor, deps({ readFile: () => pending }));
  const openPromise = manager.open("doc1", "/proj/app.toml", "#:schema ./s.json\nport=1\n", "toml");
  manager.close("doc1");
  resolveRead!("{}");
  const result = await openPromise;
  // open() itself still resolves (it awaited its own fetch), but the
  // document is gone from the manager — no crash, no dangling dispatch.
  assert.strictEqual(manager.outline("doc1"), undefined);
  assert.ok(result); // does not throw
  assert.strictEqual(created!.schemaSource, undefined, "stale fetch must not dispatch SchemaLoaded");
});
