// Tiny static dev server for the Web UI. Serves web/ + the wasm pkg so ES
// module imports resolve. Run: `node serve.mjs` (after `npm install` + `npm run build`).
import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL(".", import.meta.url));
const PORT = Number(process.env.PORT ?? 8080);
const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".ts": "text/plain; charset=utf-8",
  ".wasm": "application/wasm",
  ".json": "application/json; charset=utf-8",
};

createServer(async (req, res) => {
  try {
    let p = decodeURIComponent(new URL(req.url, "http://x").pathname);
    if (p === "/") p = "/index.html";
    // Allow resolving the wasm package under /pkg.
    const full = normalize(join(ROOT, p));
    if (!full.startsWith(ROOT)) {
      res.writeHead(403).end("forbidden");
      return;
    }
    const st = await stat(full);
    if (st.isDirectory()) {
      res.writeHead(404).end("not found");
      return;
    }
    const buf = await readFile(full);
    res.writeHead(200, { "content-type": MIME[extname(full)] ?? "application/octet-stream" });
    res.end(buf);
  } catch {
    res.writeHead(404).end("not found");
  }
}).listen(PORT, () => {
  console.log(`confy web on http://localhost:${PORT}`);
});
