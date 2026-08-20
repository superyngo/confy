// Plain-Node test for readSiblingFile's VS Code branch (fs.ts), covering the
// resolveSchemaFetchRequest -> readSiblingFile -> requestSchemaFile round
// trip added to fix local $schema loading inside the VS Code extension host
// (spec §1 never covered the VS Code surface — see the fix's design notes).
// Follows host-io.spec.mjs's convention: no test framework, esbuild-bundled
// Node execution (fs.ts pulls in real runtime imports from vscode.ts).
//
// The webview has no real `window`/`acquireVsCodeApi` in Node, so we stub
// exactly the two global seams vscode.ts reads: `acquireVsCodeApi` (to make
// `isVsCode()` true) and `window.addEventListener`/`removeEventListener`
// (the postMessage channel `requestSchemaFile` listens on).
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
  entryPoints: [path.join(here, "fs.ts")],
  bundle: true,
  write: false,
  format: "esm",
  platform: "node",
  target: "es2022",
});
const modUrl = "data:text/javascript;base64," + Buffer.from(result.outputFiles[0].text).toString("base64");
const { readSiblingFile } = await import(modUrl);

// ---- readSiblingFile: VS Code branch ----
console.log("-- readSiblingFile() VS Code branch --");
{
  posted.length = 0;
  const promise = readSiblingFile("./schema.json", null);
  check("posted a read-schema-file request", posted.length === 1 && posted[0].type === "read-schema-file");
  check("request carries the relative path", posted[0]?.relativePath === "./schema.json");
  replyFromHost({ type: "schema-file", text: '{"type":"object"}' });
  const text = await promise;
  check("resolves with the host's text", text === '{"type":"object"}');
}
{
  posted.length = 0;
  const promise = readSiblingFile("./missing.json", null);
  replyFromHost({ type: "schema-file-error", message: "ENOENT" });
  let rejected = false;
  let message = "";
  try {
    await promise;
  } catch (e) {
    rejected = true;
    message = e.message;
  }
  check("rejects on schema-file-error", rejected);
  check("rejection carries the host's message", message === "ENOENT");
}

console.log(failures === 0 ? "\nALL FS/VSCODE-SCHEMA CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
