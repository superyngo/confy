// Plain-Node test for the shared save/open/convert flows in `host-io.ts`
// (see docs/superpowers/plans/2026-08-11-web-code-audit-remediation-plan.md,
// Task 7). Follows `toolbar-fold.spec.mjs`'s convention: no test framework,
// just `node:assert`-free `check()` tallying; bundles via esbuild
// (`build({ bundle: true, write: false })`, same technique `render.spec.mjs`
// uses) since `host-io.ts` pulls in real runtime imports from `fs.js`.
//
// `doQuickSave`/`doSaveAsCopy`/`doConvertWrite`'s "no handle" branch calls
// `fs.ts`'s real `pickSaveFile()`, which reads the ambient `window` global
// directly (no `typeof window !== "undefined"` guard) — so exercising that
// branch needs *some* `window` defined, exactly like `openFromUrl` needs a
// `fetch`. We stub only the single global entry point each path reads
// (`window.showSaveFilePicker`, `globalThis.fetch`) — no jsdom, matching this
// file's zero-framework convention.
//
// EXCLUDED: the `io.fsAvailable === false` download-fallback branch (all
// three write flows) calls `fs.ts`'s real `downloadText()`, which hits
// `document.createElement("a")` — actual DOM, not stubbable with a single
// global like the picker/fetch paths above. Covering it would need jsdom,
// which this suite deliberately avoids (see file header above and
// `toolbar-fold.spec.mjs`'s own rationale). Also excluded: `replaceSession`
// (calls the real wasm `Session.fromText()` — see the plan's Architecture
// Decisions for why that needs real wasm-loading infrastructure, not a fake).
import path from "node:path";
import { fileURLToPath } from "node:url";
import * as esbuild from "esbuild";

const here = path.dirname(fileURLToPath(import.meta.url));

let failures = 0;
function check(name, cond, extra = "") {
  if (cond) {
    console.log(`  ✓ ${name}`);
  } else {
    console.log(`  ✗ ${name} ${extra}`);
    failures++;
  }
}

const result = await esbuild.build({
  entryPoints: [path.join(here, "host-io.ts")],
  bundle: true,
  write: false,
  format: "esm",
  platform: "node",
  target: "es2022",
});
const modUrl = "data:text/javascript;base64," + Buffer.from(result.outputFiles[0].text).toString("base64");
const {
  doQuickSave,
  doSaveAsCopy,
  doConvertWrite,
  openFromUrl,
  formatFromNameOrType,
  resolveSchemaFetchRequest,
} =
  await import(modUrl);

// ---- fakes ----

function fakeHandle(name) {
  const writable = { writes: [], closed: false };
  return {
    name,
    writable,
    async getFile() {
      return { name };
    },
    async createWritable() {
      return {
        async write(text) {
          writable.writes.push(text);
        },
        async close() {
          writable.closed = true;
        },
      };
    },
  };
}

function fakeIo(overrides = {}) {
  const calls = { send: [], ok: [], err: [], adoptFile: [], afterSaveAs: [], beforeConvertWrite: 0 };
  return {
    calls,
    fsAvailable: true,
    canSaveAs: true,
    getSnap: () => ({ doc_format: "Toml" }),
    send: (i) => calls.send.push(i),
    batch: (fn) => fn(),
    serialize: () => "key = 1\n",
    getFileName: () => "confy-export.toml",
    getHandle: () => null,
    setHandle: () => {},
    ok: (msg) => calls.ok.push(msg),
    err: (msg) => calls.err.push(msg),
    adoptFile: (text, format, handle, name) => calls.adoptFile.push({ text, format, handle, name }),
    afterSaveAs: (handle, name) => calls.afterSaveAs.push({ handle, name }),
    ...overrides,
  };
}

// ---- doQuickSave ----
console.log("-- doQuickSave() --");
{
  // Existing handle: in-place write, `ok`, never `adoptFile`.
  const handle = fakeHandle("open.toml");
  const io = fakeIo({ getHandle: () => handle });
  await doQuickSave(io);
  check("existing handle: writes serialized text in place", handle.writable.writes.includes("key = 1\n"));
  check("existing handle: closes the writable", handle.writable.closed === true);
  check("existing handle: sends Save intent", io.calls.send.includes("Save"));
  check("existing handle: reports ok", io.calls.ok.length === 1, JSON.stringify(io.calls.ok));
  check("existing handle: never adopts a file", io.calls.adoptFile.length === 0);
}
{
  // No handle, fsAvailable: falls back to the Save-As-equivalent path
  // (`pickSaveFile` via the stubbed `window.showSaveFilePicker`).
  const picked = fakeHandle("confy-export.toml");
  globalThis.window = { showSaveFilePicker: async () => picked };
  const io = fakeIo({ getHandle: () => null });
  await doQuickSave(io);
  check("no handle: picks a destination and writes", picked.writable.writes.includes("key = 1\n"));
  check(
    "no handle: adopts the newly picked file",
    io.calls.adoptFile.length === 1 && io.calls.adoptFile[0].handle === picked,
    JSON.stringify(io.calls.adoptFile),
  );
  check("no handle: fires afterSaveAs", io.calls.afterSaveAs.length === 1);
  check("no handle: sends Save intent", io.calls.send.includes("Save"));
  delete globalThis.window;
}

// ---- doSaveAsCopy ----
console.log("\n-- doSaveAsCopy() --");
{
  const picked = fakeHandle("copy.toml");
  globalThis.window = { showSaveFilePicker: async () => picked };
  const io = fakeIo();
  await doSaveAsCopy(io, "/tmp/copy.toml");
  check(
    "writes serialized text verbatim (byte-for-byte)",
    picked.writable.writes.length === 1 && picked.writable.writes[0] === io.serialize(),
  );
  check("exits convert mode first", io.calls.send.includes("ExitConvert"));
  check(
    "adopts the new handle/name",
    io.calls.adoptFile.length === 1 && io.calls.adoptFile[0].handle === picked,
    JSON.stringify(io.calls.adoptFile),
  );
  check("reports ok with the saved name", io.calls.ok.some((m) => m.includes("copy.toml")), JSON.stringify(io.calls.ok));
  delete globalThis.window;
}

// ---- doConvertWrite ----
console.log("\n-- doConvertWrite() --");
{
  const picked = fakeHandle("out.json");
  globalThis.window = { showSaveFilePicker: async () => picked };
  const io = fakeIo({ beforeConvertWrite: () => { io.calls.beforeConvertWrite++; } });
  await doConvertWrite(io, "/tmp/out.json", '{"key":1}');
  check("calls beforeConvertWrite hook", io.calls.beforeConvertWrite === 1);
  check("writes the converted text", picked.writable.writes.includes('{"key":1}'));
  check(
    "adopts the new handle/name on success",
    io.calls.adoptFile.length === 1 && io.calls.adoptFile[0].handle === picked,
    JSON.stringify(io.calls.adoptFile),
  );
  check("reports ok mentioning the converted name", io.calls.ok.some((m) => m.includes("out.json")), JSON.stringify(io.calls.ok));
  delete globalThis.window;
}

// ---- openFromUrl ----
console.log("\n-- openFromUrl() --");
{
  // Success: `formatFromNameOrType` picks the right ConfigFormat from the URL name.
  globalThis.fetch = async () => ({
    ok: true,
    status: 200,
    statusText: "OK",
    text: async () => "key: 1\n",
    headers: { get: () => "text/yaml" },
  });
  const io = fakeIo();
  const opened = [];
  const openText = (text, format, handle, name) => opened.push({ text, format, handle, name });
  const ok = await openFromUrl(io, openText, "https://example.com/config.yaml");
  check("returns true on success", ok === true);
  check("calls openText with the fetched text", opened.length === 1 && opened[0].text === "key: 1\n", JSON.stringify(opened));
  check(
    "picks ConfigFormat from the URL name (.yaml)",
    opened[0].format === formatFromNameOrType("config.yaml", "text/yaml"),
    opened[0]?.format,
  );
  check("no on-disk handle for a URL-opened file", opened[0].handle === null);
}
{
  // No extension: falls back to the HTTP Content-Type.
  globalThis.fetch = async () => ({
    ok: true,
    status: 200,
    statusText: "OK",
    text: async () => "{}",
    headers: { get: () => "application/json; charset=utf-8" },
  });
  const io = fakeIo();
  const opened = [];
  const ok = await openFromUrl(io, (text, format, handle, name) => opened.push({ text, format, handle, name }), "https://example.com/raw/config");
  check("returns true on success (no extension)", ok === true);
  check("falls back to Content-Type for format (.json)", opened[0]?.format === "json", opened[0]?.format);
}
{
  // Failure: HTTP error surfaces through `io.err`, `openText` never runs, returns false.
  globalThis.fetch = async () => ({
    ok: false,
    status: 404,
    statusText: "Not Found",
    text: async () => "",
    headers: { get: () => null },
  });
  const io = fakeIo();
  let openTextCalled = false;
  const ok = await openFromUrl(io, () => { openTextCalled = true; }, "https://example.com/missing.toml");
  check("returns false on HTTP failure", ok === false);
  check("reports the failure via io.err", io.calls.err.length === 1, JSON.stringify(io.calls.err));
  check("never calls openText on failure", openTextCalled === false);
}
// ---- resolveSchemaFetchRequest ----
console.log("\n-- resolveSchemaFetchRequest() --");
{
  // `http://` schema URL hints are upgraded to `https://` before the browser
  // fetch — an https page blocks the http fetch as mixed content before the
  // server's 301 redirect can run ("Failed to fetch").
  const fetched = [];
  globalThis.fetch = async (url) => {
    fetched.push(String(url));
    return {
      ok: true,
      status: 200,
      statusText: "OK",
      text: async () => "{}",
      headers: { get: () => "application/schema+json" },
    };
  };
  const io = fakeIo();
  const session = { dispatch: (intent) => ({ dispatched: intent }) };
  await resolveSchemaFetchRequest(io, session, { Url: "http://json-schema.org/draft-07/schema#" }, null);
  check(
    "upgrades http:// schema URL hints to https://",
    fetched.length === 1 && fetched[0] === "https://json-schema.org/draft-07/schema#",
    JSON.stringify(fetched),
  );
  check("leaves https:// hints untouched", (await (async () => {
    await resolveSchemaFetchRequest(io, session, { Url: "https://example.com/s.json" }, null);
    return fetched[1] === "https://example.com/s.json";
  })()));
}
delete globalThis.fetch;

delete globalThis.fetch;

console.log(failures === 0 ? "\nALL HOST-IO CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
