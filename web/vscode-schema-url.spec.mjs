// Plain-Node test for vscode.ts's requestSchemaUrl, covering the
// read-schema-url / schema-url / schema-url-error round trip added to fix
// remote $schema loading inside the VS Code extension host (the webview's
// CSP connect-src blocks arbitrary external fetches, so the extension host
// fetches instead — see fs-vscode-schema.spec.mjs's sibling fix for the
// Local counterpart). Follows the same convention: no test framework,
// esbuild-bundled Node execution, fake acquireVsCodeApi/window channel.
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

// ---- fake VS Code webview host ----

const posted = [];
let messageListeners = [];
globalThis.acquireVsCodeApi = () => ({
  postMessage: (msg) => posted.push(msg),
});
globalThis.window = {
  addEventListener: (type, listener) => {
    if (type === "message") messageListeners.push(listener);
  },
  removeEventListener: (type, listener) => {
    if (type === "message") messageListeners = messageListeners.filter((l) => l !== listener);
  },
};
function replyFromHost(data) {
  for (const listener of messageListeners) listener({ data });
}

const result = await esbuild.build({
  entryPoints: [path.join(here, "vscode.ts")],
  bundle: true,
  write: false,
  format: "esm",
  platform: "node",
  target: "es2022",
});
const modUrl = "data:text/javascript;base64," + Buffer.from(result.outputFiles[0].text).toString("base64");
const { requestSchemaUrl } = await import(modUrl);

// ---- requestSchemaUrl() ----
console.log("-- requestSchemaUrl() --");
{
  posted.length = 0;
  const promise = requestSchemaUrl("https://example.com/schema.json");
  check("posted a read-schema-url request", posted.length === 1 && posted[0].type === "read-schema-url");
  check("request carries the url", posted[0]?.url === "https://example.com/schema.json");
  replyFromHost({ type: "schema-url", text: '{"type":"object"}' });
  const text = await promise;
  check("resolves with the host's text", text === '{"type":"object"}');
}
{
  posted.length = 0;
  const promise = requestSchemaUrl("https://example.com/missing.json");
  replyFromHost({ type: "schema-url-error", message: "HTTP 404 Not Found" });
  let rejected = false;
  let message = "";
  try {
    await promise;
  } catch (e) {
    rejected = true;
    message = e.message;
  }
  check("rejects on schema-url-error", rejected);
  check("rejection carries the host's message", message === "HTTP 404 Not Found");
}

console.log(failures === 0 ? "\nALL VSCODE SCHEMA-URL CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
