// Real-browser check of the Raw pane's scroll behavior on a given engine.
// Usage: node check.mjs firefox|chromium
import { firefox, chromium } from "playwright";

const which = process.argv[2] ?? "firefox";
const browserType = which === "firefox" ? firefox : chromium;
const channel = process.argv[3]; // e.g. "msedge"
const browser = await browserType.launch(channel ? { channel, headless: false } : { headless: false });
const page = await browser.newPage({ viewport: { width: 900, height: 320 } });
await page.goto("http://127.0.0.1:8734/index.html");
await page.waitForSelector("#tree .row", { timeout: 20000 });

const out = [];
const top = () => page.evaluate(() => document.getElementById("rawEdit").scrollTop);
const cls = () => page.evaluate(() => document.body.className);

// Raw view, scrolled with a real wheel over the pane.
await page.click("#btnViewToggle");
await page.waitForTimeout(400);
await page.hover("#rawEdit");
for (let i = 0; i < 8; i++) await page.mouse.wheel(0, 120);
await page.waitForTimeout(500);
const scrolled = await top();
out.push(["raw view scrolled by wheel", scrolled > 200, scrolled]);

// 1) view -> Edit (real click on the band's primary control)
await page.click("#btnRawEdit");
await page.waitForTimeout(600);
const afterEdit = await top();
out.push(["1) Edit keeps the scroll", Math.abs(afterEdit - scrolled) < 5, `${scrolled} -> ${afterEdit} (${await cls()})`]);

// 2a) dirty Cancel — type for real, then click Cancel
await page.keyboard.type("# typed by the test\n");
await page.waitForTimeout(300);
await page.evaluate((v) => { document.getElementById("rawEdit").scrollTop = v; }, scrolled);
await page.waitForTimeout(300);
const beforeCancel = await top();
await page.click("#btnRawCancel");
await page.waitForTimeout(900);
const afterCancel = await top();
out.push(["2a) dirty Cancel keeps the scroll", Math.abs(afterCancel - beforeCancel) < 5, `${beforeCancel} -> ${afterCancel} (${await cls()})`]);

// 2b) clean Cancel (regression guard)
await page.click("#btnRawEdit");
await page.waitForTimeout(500);
await page.evaluate((v) => { document.getElementById("rawEdit").scrollTop = v; }, scrolled);
await page.waitForTimeout(300);
const beforeClean = await top();
await page.click("#btnRawCancel");
await page.waitForTimeout(900);
const afterClean = await top();
out.push(["2b) clean Cancel keeps the scroll", Math.abs(afterClean - beforeClean) < 5, `${beforeClean} -> ${afterClean}`]);

// 3) Apply with a dirty buffer
await page.click("#btnRawEdit");
await page.waitForTimeout(500);
await page.keyboard.type("# applied\n");
await page.evaluate((v) => { document.getElementById("rawEdit").scrollTop = v; }, scrolled);
await page.waitForTimeout(300);
const beforeApply = await top();
await page.click("#btnRawEdit"); // primary control is Apply in write mode
await page.waitForTimeout(900);
const afterApply = await top();
out.push(["3) Apply keeps the scroll", Math.abs(afterApply - beforeApply) < 5, `${beforeApply} -> ${afterApply} (${await cls()})`]);

// 4) tree scroll across the Raw round trip
await page.evaluate(() => { if (document.body.classList.contains("raw-view")) document.getElementById("btnViewToggle").click(); });
await page.waitForTimeout(500);
await page.evaluate(() => { document.getElementById("treeWrap").scrollTop = 200; });
await page.waitForTimeout(300);
await page.click("#btnViewToggle");
await page.waitForTimeout(500);
await page.click("#btnViewToggle");
await page.waitForTimeout(600);
const treeBack = await page.evaluate(() => document.getElementById("treeWrap").scrollTop);
out.push(["4) tree keeps its scroll across Raw", treeBack === 200, String(treeBack)]);

let bad = 0;
for (const [name, ok, extra] of out) {
  if (!ok) bad++;
  console.log(`${ok ? "  PASS" : "  FAIL"}  ${name}  [${extra}]`);
}
console.log(`${which}${channel ? "/" + channel : ""}: ${bad === 0 ? "ALL PASS" : bad + " FAILURES"}`);
await browser.close();
process.exit(bad === 0 ? 0 : 1);
