// Regression test for issue #1 (comment-advisory follow-up): a `.json`
// *sample* document (opened via loadSample/openSample, no real filename)
// must still set `strict_json`, exactly like a real `.json` file does.
// `openSample` passes the literal name "sample" (no `.json` suffix), so the
// `isPlainJson` check must key off `format`/`asSample`, not just a filename
// regex — plain-Node source assertions, no test framework (matches this
// directory's other `*.spec.mjs` files).
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const here = path.dirname(fileURLToPath(import.meta.url));
let failures = 0;
function check(name, cond, extra = "") {
  if (cond) {
    console.log(`  ✓ ${name}`);
  } else {
    failures++;
    console.error(`  ✗ ${name}${extra ? ` — ${extra}` : ""}`);
  }
}

for (const file of ["ui.ts", "touch/app.ts"]) {
  const src = readFileSync(path.join(here, file), "utf8");
  console.log(`-- ${file} --`);
  const openTextBody = src.slice(src.indexOf("function openText("), src.indexOf("function openText(") + 2200);

  check(
    `${file}: openText computes isPlainJson from format+asSample, not just the name regex`,
    /isPlainJson\s*=\s*\n?\s*format === "json" && \(asSample \|\| \(!!name && \/\\\.json\$\/i\.test\(name\)\)\)/.test(
      openTextBody,
    ),
    "isPlainJson must be true for a .json-format sample, not only a real .json filename",
  );

  check(
    `${file}: openText calls session.setStrictJson(true) when isPlainJson`,
    /if \(isPlainJson\) session\.setStrictJson\(true\);/.test(openTextBody),
  );

  check(
    `${file}: openText fires the one-shot json-comments-detected notice when isPlainJson && hadCommentsAtOpen`,
    /if \(isPlainJson && session\.hadCommentsAtOpen\(\)\)/.test(openTextBody),
  );
}

console.log(failures === 0 ? "\nAll checks passed." : `\n${failures} check(s) failed.`);
process.exit(failures === 0 ? 0 : 1);
