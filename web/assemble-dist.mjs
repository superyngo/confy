import { cp, mkdir, rm } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const distDir = path.join(__dirname, "dist");
const touchDir = path.join(distDir, "touch");
const iconsDir = path.join(distDir, "icons");

await rm(distDir, { recursive: true, force: true });
await mkdir(touchDir, { recursive: true });
await mkdir(iconsDir, { recursive: true });

for (const src of [
  "index.html",
  "touch.html",
  "privacy.html",
  "style.css",
  "entry-desktop.js",
  "entry-touch.js",
  "register-sw.js",
  "ui.js",
  "ui.js.map",
  "manifest.webmanifest",
  "sw.js",
  "schema-sample.json",
]) {
  await cp(path.join(__dirname, src), path.join(distDir, src), { recursive: true });
}

await cp(path.join(__dirname, "touch", "style.css"), path.join(touchDir, "style.css"));
await cp(path.join(__dirname, "touch", "app.js"), path.join(touchDir, "app.js"));
await cp(path.join(__dirname, "touch", "app.js.map"), path.join(touchDir, "app.js.map"));
await cp(path.join(__dirname, "icons", "icon-192.png"), path.join(iconsDir, "icon-192.png"));
await cp(path.join(__dirname, "icons", "icon-512.png"), path.join(iconsDir, "icon-512.png"));
await cp(path.join(__dirname, "pkg"), path.join(distDir, "pkg"), { recursive: true });

console.log("assembled: web/dist");
