// Tests for Task 14: Prompt question source of truth, schemaHintText i18n, and has_descendant_violation rename
import path from "node:path";
import { fileURLToPath } from "node:url";
import { readFileSync } from "node:fs";
import { execSync } from "node:child_process";
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

async function bundleSource(contents) {
  const result = await esbuild.build({
    stdin: {
      contents,
      resolveDir: here,
      sourcefile: "test-entry.ts",
      loader: "ts",
    },
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

console.log("\n-- Task 14 Structural Invariants --");

// 1. web/prompt.ts should not have PROMPT_QUESTIONS or promptQuestion
const promptSrc = readFileSync(path.join(here, "prompt.ts"), "utf8");
check("prompt.ts does not include PROMPT_QUESTIONS", !promptSrc.includes("PROMPT_QUESTIONS"));
check("prompt.ts does not include promptQuestion", !promptSrc.includes("promptQuestion"));

// 2. web/panel.ts should use core.hint.enum / bounded and not hardcode "Valid values:"
const panelSrc = readFileSync(path.join(here, "panel.ts"), "utf8");
check("panel.ts does not hardcode 'Valid values:'", !panelSrc.includes('"Valid values:'));
check("panel.ts references core.hint.enum", panelSrc.includes("core.hint.enum"));
check("panel.ts references core.hint.bounded", panelSrc.includes("core.hint.bounded"));

// Catalog keys check
const enCatalog = JSON.parse(readFileSync(path.join(here, "..", "i18n", "en.json"), "utf8"));
const zhCatalog = JSON.parse(readFileSync(path.join(here, "..", "i18n", "zh-TW.json"), "utf8"));
check("en.json does not have web.prompt.confirmFallback", !("web.prompt.confirmFallback" in enCatalog));
check("zh-TW.json does not have web.prompt.confirmFallback", !("web.prompt.confirmFallback" in zhCatalog));
check("en.json has core.hint.enum", "core.hint.enum" in enCatalog);
check("zh-TW.json has core.hint.enum", "core.hint.enum" in zhCatalog);
check("en.json has core.hint.bounded", "core.hint.bounded" in enCatalog);
check("zh-TW.json has core.hint.bounded", "core.hint.bounded" in zhCatalog);
const oldIdentifier = ["has", "descendant", "warning"].join("_");
let warningHits = "";
try {
  warningHits = execSync(
    `grep -rn '${oldIdentifier}' ` +
      path.join(here, "..", "web") +
      " " +
      path.join(here, "..", "crates") +
      " | grep -v '\\.wasm' | grep -v 'prompt-schema-hint\\.spec\\.mjs' || true",
    { encoding: "utf8" }
  ).trim();
} catch (e) {
  warningHits = "";
}
check("has_descendant_warning has zero occurrences in web/ and crates/", warningHits === "", warningHits);

console.log("\n-- Task 14 Behavioral: schemaHintText i18n --");
const { schemaHintText, setLang } = await bundleSource(`
  export { schemaHintText } from "./panel.ts";
  export { setLang } from "./i18n.ts";
`);

// English tests
setLang("en");
const enumHint = { Enum: [["first", 1], ["second", 2]] };
const enumText = schemaHintText(enumHint);
check("schemaHintText(Enum) renders via catalog in en", enumText === "Valid values: first, second", `got: ${enumText}`);

const boundedHint = { Bounded: { minimum: 5, maximum: 25, multiple_of: undefined } };
const boundedText = schemaHintText(boundedHint);
check("schemaHintText(Bounded) renders via catalog in en", boundedText === "Must be between 5 and 25", `got: ${boundedText}`);

check("schemaHintText('None') is empty string", schemaHintText("None") === "");
const minHint = { Bounded: { minimum: 10, maximum: undefined, multiple_of: undefined } };
check("schemaHintText(min only) en", schemaHintText(minHint) === "Must be at least 10", `got: ${schemaHintText(minHint)}`);
const maxHint = { Bounded: { minimum: undefined, maximum: 50, multiple_of: undefined } };
check("schemaHintText(max only) en", schemaHintText(maxHint) === "Must be at most 50", `got: ${schemaHintText(maxHint)}`);
const multHint = { Bounded: { minimum: undefined, maximum: undefined, multiple_of: 3 } };
check("schemaHintText(multiple_of only) en", schemaHintText(multHint) === "Must be a multiple of 3", `got: ${schemaHintText(multHint)}`);

// Chinese tests
setLang("zh-TW");
const enumTextZh = schemaHintText(enumHint);
check("schemaHintText(Enum) renders via catalog in zh-TW", enumTextZh === "有效值：first, second", `got: ${enumTextZh}`);

const boundedTextZh = schemaHintText(boundedHint);
check("schemaHintText(Bounded) renders via catalog in zh-TW", boundedTextZh === "必須介於 5 與 25 之間", `got: ${boundedTextZh}`);
check("schemaHintText(min only) zh-TW", schemaHintText(minHint) === "必須至少為 10", `got: ${schemaHintText(minHint)}`);
check("schemaHintText(max only) zh-TW", schemaHintText(maxHint) === "必須至多為 50", `got: ${schemaHintText(maxHint)}`);
check("schemaHintText(multiple_of only) zh-TW", schemaHintText(multHint) === "必須為 3 的倍數", `got: ${schemaHintText(multHint)}`);

setLang("en");

console.log("\n-- Task 14 Behavioral: prompt question source of truth in ui.ts / touch/app.ts --");
const uiSrc = readFileSync(path.join(here, "ui.ts"), "utf8");
check("ui.ts does not call promptQuestion", !uiSrc.includes("promptQuestion("));
const uiPromptIdx = uiSrc.indexOf('} else if (tag === "Prompt") {');
const uiPromptSection = uiSrc.slice(uiPromptIdx, uiPromptIdx + 300);
check("ui.ts reads question from Prompt mode and escapes it into h3", uiPromptSection.includes("escapeHtml(question)"));
const touchAppSrc = readFileSync(path.join(here, "touch/app.ts"), "utf8");
check("touch/app.ts does not call promptQuestion", !touchAppSrc.includes("promptQuestion("));
const touchPromptSection = touchAppSrc.slice(touchAppSrc.indexOf('function renderPromptSheet('), touchAppSrc.indexOf('function openKindSheet('));
check("touch/app.ts renderPromptSheet uses question param directly", touchPromptSection.includes("renderPromptSheet(kind: PromptView, question: string)") && touchPromptSection.includes("esc(question)"));

// Verify prompt overlay rendering directly
const { promptButtonsHTML, promptTitle } = await bundleSource(`
  export { promptButtonsHTML, promptTitle } from "./prompt.ts";
`);
const quitTitle = promptTitle("ConfirmQuit");
check("promptTitle('ConfirmQuit') works", quitTitle.length > 0);
const quitBtns = promptButtonsHTML("ConfirmQuit");
check("promptButtonsHTML('ConfirmQuit') renders cancel and quit buttons", quitBtns.includes('data-pk="n"') && quitBtns.includes('data-pk="y"'));

const collisionBtns = promptButtonsHTML("Collision");
check("promptButtonsHTML('Collision') renders cancel, rename, overwrite", collisionBtns.includes('data-pk="r"') && collisionBtns.includes('data-pk="o"'));
process.exit(failures === 0 ? 0 : 1);
