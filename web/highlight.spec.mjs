// Plain-Node test for `highlight.ts` — the fuzzy-match mark renderer shared by
// the desktop and touch trees. Same convention as render.spec.mjs: no test
// framework, esbuild-bundle the TS module, `check()` tally.
//
// The real matcher is the wasm core's `fuzzy_indices` (registered by confy.ts's
// `load()`); this file injects a stub via `setFuzzyMatcher` so the escaping,
// run-coalescing and char-vs-code-unit indexing can be pinned without booting
// wasm. The Rust side's own matching is covered by
// crates/confy-core/src/session/search.rs and functional_smoke.mjs.
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

async function bundle(entry) {
  const result = await esbuild.build({
    entryPoints: [path.join(here, entry)],
    bundle: true,
    write: false,
    format: "esm",
    platform: "node",
    target: "es2022",
  });
  const code = result.outputFiles[0].text;
  const modUrl = "data:text/javascript;base64," + Buffer.from(code).toString("base64");
  return import(modUrl);
}

const { highlightHtml, setFuzzyMatcher } = await bundle("highlight.ts");

// ---- unregistered matcher degrades to plain escaped text ----
console.log("-- highlightHtml(): no matcher registered --");
{
  const html = highlightHtml(`<b>&"x</b>`, "x");
  check("falls back to escaped text, no marks", html === `&lt;b&gt;&amp;&quot;x&lt;/b&gt;`, html);
}

// A stub standing in for the wasm matcher: greedy left-to-right subsequence over
// CHARS (what skim returns), or undefined when the needle doesn't fit.
setFuzzyMatcher((haystack, needle) => {
  const chars = Array.from(haystack);
  const want = Array.from(needle);
  const out = [];
  let w = 0;
  for (let i = 0; i < chars.length && w < want.length; i++) {
    if (chars[i].toLowerCase() === want[w].toLowerCase()) {
      out.push(i);
      w++;
    }
  }
  return w === want.length ? out : undefined;
});

console.log("\n-- highlightHtml(): marks and coalescing --");
{
  check(
    "empty needle → plain text, no marks",
    highlightHtml("server", "") === "server",
    highlightHtml("server", ""),
  );
  check(
    "no match → plain text, no marks",
    highlightHtml("server", "zzz") === "server",
    highlightHtml("server", "zzz"),
  );
  const scattered = highlightHtml("axbycz", "abc");
  check(
    "scattered matches get one mark each",
    scattered === `<mark class="fz">a</mark>x<mark class="fz">b</mark>y<mark class="fz">c</mark>z`,
    scattered,
  );
  const run = highlightHtml("server", "ser");
  check(
    "consecutive matched chars coalesce into ONE mark",
    run === `<mark class="fz">ser</mark>ver`,
    run,
  );
  check(
    "fully-matched text is a single mark",
    highlightHtml("abc", "abc") === `<mark class="fz">abc</mark>`,
    highlightHtml("abc", "abc"),
  );
}

// ---- escaping survives highlighting (the marks are the ONLY raw markup) ----
console.log("\n-- highlightHtml(): escaping inside and outside marks --");
{
  // The needle matches the `<` and `b`, so a hostile char lands INSIDE a mark.
  const html = highlightHtml(`<b>alert</b>`, "<b");
  check("no raw < survives outside marks", !/<(?!\/?mark)/.test(html), html);
  check("the matched < is entity-encoded inside the mark", html.includes("&lt;"), html);
  check("no <b> tag is emitted", !html.includes("<b>"), html);
  const amp = highlightHtml(`a&b`, "ab");
  check("& is encoded between marks", amp.includes("&amp;"), amp);
}

// ---- char indices, not UTF-16 code units ----
console.log("\n-- highlightHtml(): astral chars don't shift the marks --");
{
  // "🌍" is one CHAR but two UTF-16 code units. Indexing by code unit would
  // mark the wrong letter (or split the surrogate pair into mojibake).
  const html = highlightHtml("🌍ab", "ab");
  check(
    "marks land on 'ab', not on the emoji's second half",
    html === `🌍<mark class="fz">ab</mark>`,
    html,
  );
  check("the emoji is intact (no lone surrogate)", html.includes("🌍"), html);
  // CJK is BMP but still worth pinning as a non-ASCII path.
  const cjk = highlightHtml("伺服器port", "port");
  check("CJK prefix doesn't shift the marks", cjk === `伺服器<mark class="fz">port</mark>`, cjk);
}

console.log(failures === 0 ? "\nhighlight.spec: OK" : `\nhighlight.spec: ${failures} FAILED`);
process.exit(failures === 0 ? 0 : 1);
