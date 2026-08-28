// Plain-Node test for `key-intent.ts`'s pure mode/key resolution, extracted
// from `ui.ts`'s `onKey` (see
// docs/superpowers/plans/2026-08-11-web-code-audit-remediation-plan.md,
// Task 8 — the highest-risk task in that plan, since it touches the primary
// keyboard-input dispatch path). Follows `toolbar-fold.spec.mjs`'s
// convention: no test framework, just `node:assert`-free `check()` tallying.
// `key-intent.ts` pulls in `mode.ts` (a real runtime import), so this file
// bundles via esbuild (same technique as `render.spec.mjs`/`host-io.spec.mjs`)
// instead of transforming a single file.
//
// Table-driven: at least one case per guard clause in `ui.ts`'s original
// `onKey` (now `resolveKeyIntent`'s branches), covering every mode in the
// documented precedence order — Edit > Prompt > Convert > TypeFilter >
// KindSwitch > SchemaEnum > Help > tree shortcuts — plus the ctrl/rawView
// escape hatches and the `nav`/`native`/`typefilter-page` non-plain-Intent
// results.
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
  entryPoints: [path.join(here, "key-intent.ts")],
  bundle: true,
  write: false,
  format: "esm",
  platform: "node",
  target: "es2022",
});
const modUrl = "data:text/javascript;base64," + Buffer.from(result.outputFiles[0].text).toString("base64");
const { resolveKeyIntent, navRowCount } = await import(modUrl);

// ---- fixtures (types.ts:34-132) ----
const editMode = { Edit: { field: "Value", buffer: "x", cursor: 1, key: "", is_element: false, is_comment: false, rename_only: false } };
const promptMode = { Prompt: { kind: "ConfirmQuit", question: "" } };
const convertFormat = { Convert: { step: "Format", cursor: 0, options: ["Toml"], target: "Toml", path: "", path_cursor: 0, warnings: [] } };
const convertPath = { Convert: { step: "Path", cursor: 0, options: ["Toml"], target: "Toml", path: "out", path_cursor: 3, warnings: [] } };
const convertConfirm = { Convert: { step: "Confirm", cursor: 0, options: ["Toml"], target: "Toml", path: "out.toml", path_cursor: 8, warnings: [] } };
const typeFilterGrid = {
  rows: [
    { Header: "Scalars" },
    { Cells: [{ label: "String", state: "Off", is_cursor: false }] },
    { Cells: [{ label: "Integer", state: "Off", is_cursor: true }] },
  ],
  cursor_row: 2,
  cursor_col: 0,
  active: true,
};
const typeFilterMode = { TypeFilter: typeFilterGrid };
const kindSwitchMode = { KindSwitch: { cursor: 0, options: [] } };
const schemaEnumMode = { SchemaEnum: { options: ["a", "b", "c"], cursor: 0 } };
const helpMode = { Help: { tab: "Help" } };
const normalMode = "Normal";

const noMods = { ctrl: false, shift: false };
const resolve = (mode, key, mods = noMods, rawView = false, vshost = false) =>
  resolveKeyIntent(mode, key, mods, rawView, vshost);

// ---- Edit mode (highest precedence) ----
console.log("-- Edit mode --");
{
  const r = resolve(editMode, "Enter");
  check("Enter -> EditCommit", r?.kind === "intent" && r.intent === "EditCommit", JSON.stringify(r));
}
{
  const r = resolve(editMode, "Escape");
  check("Escape -> EditCancel", r?.kind === "intent" && r.intent === "EditCancel", JSON.stringify(r));
}
{
  const r = resolve(editMode, "Tab");
  check("Tab -> EditToggleField, preventDefault", r?.kind === "intent" && r.intent === "EditToggleField" && r.preventDefault === true, JSON.stringify(r));
}
{
  const r = resolve(editMode, "Backspace");
  check("Backspace -> EditBackspace", r?.kind === "intent" && r.intent === "EditBackspace", JSON.stringify(r));
}
{
  const r = resolve(editMode, "x");
  check("single char -> EditChar", r?.kind === "intent" && r.intent?.EditChar === "x", JSON.stringify(r));
}
{
  const r = resolve(editMode, "ArrowUp");
  check("unmatched multi-char key -> null", r === null, JSON.stringify(r));
}

// ---- Prompt mode ----
console.log("\n-- Prompt mode --");
{
  const r = resolve(promptMode, "y");
  check('"y" -> PromptKey:"y"', r?.kind === "intent" && r.intent?.PromptKey === "y", JSON.stringify(r));
}
{
  const r = resolve(promptMode, "Enter");
  check('Enter -> PromptKey:"y" (same as y)', r?.kind === "intent" && r.intent?.PromptKey === "y", JSON.stringify(r));
}
{
  const r = resolve(promptMode, "N");
  check('"N" -> PromptKey:"n"', r?.kind === "intent" && r.intent?.PromptKey === "n", JSON.stringify(r));
}
{
  const r = resolve(promptMode, "o");
  check('"o" (Collision Overwrite) -> PromptKey:"o"', r?.kind === "intent" && r.intent?.PromptKey === "o", JSON.stringify(r));
}
{
  const r = resolve(promptMode, "z");
  check("unhandled key -> null", r === null, JSON.stringify(r));
}

// ---- Convert mode ----
console.log("\n-- Convert mode --");
{
  const r = resolve(convertFormat, "ArrowDown");
  check("Format step + ArrowDown -> ConvertMove:1", r?.kind === "intent" && r.intent?.ConvertMove === 1, JSON.stringify(r));
}
{
  const r = resolve(convertFormat, "ArrowUp");
  check("Format step + ArrowUp -> ConvertMove:-1", r?.kind === "intent" && r.intent?.ConvertMove === -1, JSON.stringify(r));
}
{
  const r = resolve(convertFormat, "Escape");
  check("any step + Escape -> Escape intent", r?.kind === "intent" && r.intent === "Escape", JSON.stringify(r));
}
{
  const r = resolve(convertPath, "Enter");
  check("Path step + Enter -> native save-convert", r?.kind === "native" && r.action === "save-convert", JSON.stringify(r));
}
{
  const r = resolve(convertPath, "x");
  check("Path step + char -> ConvertPathChar", r?.kind === "intent" && r.intent?.ConvertPathChar === "x", JSON.stringify(r));
}
{
  const r = resolve(convertConfirm, "y");
  check("Confirm step + y -> ConvertConfirm", r?.kind === "intent" && r.intent === "ConvertConfirm", JSON.stringify(r));
}
{
  const r = resolve(convertConfirm, "q");
  check("Confirm step + any other key -> unconditional Escape fallback", r?.kind === "intent" && r.intent === "Escape", JSON.stringify(r));
}

// ---- TypeFilter mode ----
console.log("\n-- TypeFilter mode --");
{
  const r = resolve(typeFilterMode, "ArrowUp");
  check("ArrowUp -> TypeFilterMove [-1,0]", r?.kind === "intent" && JSON.stringify(r.intent?.TypeFilterMove) === "[-1,0]", JSON.stringify(r));
}
{
  const r = resolve(typeFilterMode, "Home");
  const expected = -navRowCount(typeFilterGrid);
  check("Home -> TypeFilterMove [-navRowCount,0]", r?.kind === "intent" && r.intent?.TypeFilterMove?.[0] === expected, JSON.stringify(r));
}
{
  const r = resolve(typeFilterMode, "PageUp");
  check("PageUp -> typefilter-page dir:-1 (DOM page size left to onKey)", r?.kind === "typefilter-page" && r.dir === -1, JSON.stringify(r));
}
{
  const r = resolve(typeFilterMode, " ");
  check('Space -> TypeFilterToggle, preventDefault', r?.kind === "intent" && r.intent === "TypeFilterToggle" && r.preventDefault === true, JSON.stringify(r));
}

// ---- KindSwitch mode ----
console.log("\n-- KindSwitch mode --");
{
  const r = resolve(kindSwitchMode, "ArrowDown");
  check("ArrowDown -> KindSwitchMove:1", r?.kind === "intent" && r.intent?.KindSwitchMove === 1, JSON.stringify(r));
}
{
  const r = resolve(kindSwitchMode, "Escape");
  check("Escape -> ExitKindSwitch", r?.kind === "intent" && r.intent === "ExitKindSwitch", JSON.stringify(r));
}

// ---- SchemaEnum mode ----
console.log("\n-- SchemaEnum mode --");
{
  const r = resolve(schemaEnumMode, "ArrowUp");
  check("ArrowUp -> SchemaEnumMove:-1, preventDefault", r?.kind === "intent" && r.intent?.SchemaEnumMove === -1 && r.preventDefault === true, JSON.stringify(r));
}
{
  const r = resolve(schemaEnumMode, "Home");
  check("Home -> SchemaEnumJump:-options.length", r?.kind === "intent" && r.intent?.SchemaEnumJump === -3, JSON.stringify(r));
}
{
  const r = resolve(schemaEnumMode, "Enter");
  check("Enter -> SchemaEnumCommit, no preventDefault", r?.kind === "intent" && r.intent === "SchemaEnumCommit" && r.preventDefault === false, JSON.stringify(r));
}

// ---- Help mode ----
console.log("\n-- Help mode --");
{
  const r = resolve(helpMode, "j");
  check("Help mode + j -> null (tree shortcuts suppressed)", r === null, JSON.stringify(r));
}
{
  const r = resolve(helpMode, "Escape");
  check("Escape -> Escape intent", r?.kind === "intent" && r.intent === "Escape", JSON.stringify(r));
}
{
  const r = resolve(helpMode, "Tab");
  check("Tab -> ToggleHelpTab, preventDefault", r?.kind === "intent" && r.intent === "ToggleHelpTab" && r.preventDefault === true, JSON.stringify(r));
}

// ---- ctrl / rawView escape hatches (Normal mode) ----
console.log("\n-- ctrl / rawView --");
{
  const r = resolve(normalMode, "s", { ctrl: true, shift: false });
  check("ctrl+s -> native save, preventDefault", r?.kind === "native" && r.action === "save" && r.preventDefault === true, JSON.stringify(r));
}
{
  const r = resolve(normalMode, "o", { ctrl: true, shift: false });
  check("ctrl+o -> native open, preventDefault", r?.kind === "native" && r.action === "open" && r.preventDefault === true, JSON.stringify(r));
}
{
  const r = resolve(normalMode, "a", { ctrl: true, shift: false });
  check("ctrl + non-s/o key -> null (left to browser/OS)", r === null, JSON.stringify(r));
}
{
  const r = resolve(normalMode, "j", noMods, /* rawView */ true);
  check("rawView + j -> null (native <pre> behavior)", r === null, JSON.stringify(r));
}

// ---- shift multi-select extend ----
console.log("\n-- shift extend-select --");
{
  const r = resolve(normalMode, "ArrowDown", { ctrl: false, shift: true });
  check("shift+ArrowDown -> ExtendSelectDown, preventDefault", r?.kind === "intent" && r.intent === "ExtendSelectDown" && r.preventDefault === true, JSON.stringify(r));
}
{
  const r = resolve(normalMode, "ArrowUp", { ctrl: false, shift: true });
  check("shift+ArrowUp -> ExtendSelectUp", r?.kind === "intent" && r.intent === "ExtendSelectUp", JSON.stringify(r));
}

// ---- tree shortcuts (Normal mode, no mods) ----
console.log("\n-- tree shortcuts --");
{
  const r = resolve(normalMode, "j");
  check('"j" -> nav CursorDown, no preventDefault', r?.kind === "nav" && r.intent === "CursorDown" && r.preventDefault === false, JSON.stringify(r));
}
{
  const r = resolve(normalMode, "ArrowDown");
  check('"ArrowDown" -> nav CursorDown, preventDefault (same intent, different key)', r?.kind === "nav" && r.intent === "CursorDown" && r.preventDefault === true, JSON.stringify(r));
}
{
  const r = resolve(normalMode, "Enter");
  check("Enter -> intent ToggleDetail, no preventDefault", r?.kind === "intent" && r.intent === "ToggleDetail" && r.preventDefault === false, JSON.stringify(r));
}
{
  const r = resolve(normalMode, " ");
  check('"Space" -> native toggle-branches, preventDefault', r?.kind === "native" && r.action === "toggle-branches" && r.preventDefault === true, JSON.stringify(r));
}
{
  const r = resolve(normalMode, "e");
  check('"e" -> BeginEdit, preventDefault', r?.kind === "intent" && r.intent === "BeginEdit" && r.preventDefault === true, JSON.stringify(r));
}
{
  const r = resolve(normalMode, "F2");
  check('"F2" -> BeginRename, preventDefault (mirrors TUI F2)', r?.kind === "intent" && r.intent === "BeginRename" && r.preventDefault === true, JSON.stringify(r));
}
{
  const r = resolve(normalMode, "d");
  check('"d" -> DeleteSelected, no preventDefault', r?.kind === "intent" && r.intent === "DeleteSelected" && r.preventDefault === false, JSON.stringify(r));
}
{
  const r = resolve(normalMode, "z");
  check('"z" -> native undo', r?.kind === "native" && r.action === "undo", JSON.stringify(r));
}
{
  // Mode-precedence disambiguation: "y" means PromptKey in Prompt mode (above)
  // but redo in Normal mode — same key, different mode, different result.
  const r = resolve(normalMode, "y");
  check('"y" in Normal mode -> native redo (vs PromptKey in Prompt mode)', r?.kind === "native" && r.action === "redo", JSON.stringify(r));
}
{
  const r = resolve(normalMode, "+");
  check('"+" -> Nudge:1, no preventDefault', r?.kind === "intent" && r.intent?.Nudge === 1 && r.preventDefault === false, JSON.stringify(r));
}
{
  const r = resolve(normalMode, "ArrowRight");
  check('"ArrowRight" -> Nudge:1, preventDefault (same intent as +, different key)', r?.kind === "intent" && r.intent?.Nudge === 1 && r.preventDefault === true, JSON.stringify(r));
}
{
  const r = resolve(normalMode, "/");
  check('"/" -> native focus-search, preventDefault', r?.kind === "native" && r.action === "focus-search" && r.preventDefault === true, JSON.stringify(r));
}
{
  const r = resolve(normalMode, "q", noMods, false, /* vshost */ false);
  check('"q" -> QuitRequested when not VS Code host', r?.kind === "intent" && r.intent === "QuitRequested", JSON.stringify(r));
}
{
  const r = resolve(normalMode, "q", noMods, false, /* vshost */ true);
  check('"q" -> null in the VS Code host (no quit action)', r === null, JSON.stringify(r));
}
{
  const r = resolve(normalMode, "F12");
  check("unrecognized key -> null", r === null, JSON.stringify(r));
}

console.log(failures === 0 ? "\nALL KEY-INTENT CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
