// Built-in demo doc + sample-mode state, shared by the desktop (ui.ts) and
// touch (touch/app.ts) orchestrators so both surfaces boot the same tree.
//
// All three samples carry the *same* tree (identical keys/values/comments);
// only the dialect's notation and comment marker differ, so cycling the header
// pill shows one config wearing three outfits. The pill cycles these while the
// doc is the unsaved sample (`sampleMode`); opening or saving a real file
// leaves sample mode and freezes it.

// Workspace version stamped in at build time (see `build.mjs` `define`); falls
// back to "dev" when the bundle is loaded without that define (e.g. raw serve).
declare const __APP_VERSION__: string;
const APP_VERSION =
  typeof __APP_VERSION__ !== "undefined" ? __APP_VERSION__ : "dev";

export type SampleFormat = "toml" | "json" | "yaml";

export const SAMPLES: Record<SampleFormat, string> = {
  toml: `# 👋 Welcome to confy — a lossless editor for TOML · JSON · YAML
# Click a row to select · drag the ⠿ grip to reparent · ⌘S to save

[about]
name = "confy"
pitch = "Three config dialects, one tidy tree 🌳"
version = "${APP_VERSION}"
lossless = true    # untouched bytes round-trip byte-for-byte

[basics]
select = ["click = one", "shift-click = range", "cmd-click = toggle"]
add_child = "hover a branch, hit the ＋"
undo_redo = "z and y — we all fat-finger 🙃"

[formats]
toml = "tables, dotted keys, datetimes"
json = "// comments quietly upgrade it to JSONC"
yaml = "block + flow, plain-where-safe"

[fun]
emoji_welcome = true
brackets_collected = ["{ }", "[ ]", "< >"]
coffees_per_config = 3
`,
  json: `// 👋 Welcome to confy — a lossless editor for TOML · JSON · YAML
// Click a row to select · drag the ⠿ grip to reparent · ⌘S to save
{
  "about": {
    "name": "confy",
    "pitch": "Three config dialects, one tidy tree 🌳",
    "version": "${APP_VERSION}",
    "lossless": true    // untouched bytes round-trip byte-for-byte
  },
  "basics": {
    "select": ["click = one", "shift-click = range", "cmd-click = toggle"],
    "add_child": "hover a branch, hit the ＋",
    "undo_redo": "z and y — we all fat-finger 🙃"
  },
  "formats": {
    "toml": "tables, dotted keys, datetimes",
    "json": "// comments quietly upgrade it to JSONC",
    "yaml": "block + flow, plain-where-safe"
  },
  "fun": {
    "emoji_welcome": true,
    "brackets_collected": ["{ }", "[ ]", "< >"],
    "coffees_per_config": 3
  }
}
`,
  yaml: `# 👋 Welcome to confy — a lossless editor for TOML · JSON · YAML
# Click a row to select · drag the ⠿ grip to reparent · ⌘S to save

about:
  name: confy
  pitch: Three config dialects, one tidy tree 🌳
  version: "${APP_VERSION}"
  lossless: true    # untouched bytes round-trip byte-for-byte

basics:
  select: ["click = one", "shift-click = range", "cmd-click = toggle"]
  add_child: hover a branch, hit the ＋
  undo_redo: z and y — we all fat-finger 🙃

formats:
  toml: tables, dotted keys, datetimes
  json: "// comments quietly upgrade it to JSONC"
  yaml: block + flow, plain-where-safe

fun:
  emoji_welcome: true
  brackets_collected: ["{ }", "[ ]", "< >"]
  coffees_per_config: 3
`,
};

// Pill-cycle order.
const SAMPLE_ORDER: SampleFormat[] = ["toml", "json", "yaml"];

// True while the open doc is the built-in sample (no backing file) — enables
// the format-pill toggle. Set false by the host's openText for real files.
let sampleMode = false;
let sampleFormat: SampleFormat = "toml";

export function inSampleMode(): boolean {
  return sampleMode;
}
export function setSampleMode(on: boolean): void {
  sampleMode = on;
}

// Load the built-in sample in `format` via the host's opener (which enters
// sample mode by calling `setSampleMode(true)` for a sample open).
export function loadSample(
  format: SampleFormat,
  open: (text: string, format: SampleFormat) => void,
): void {
  sampleFormat = format;
  open(SAMPLES[format], format);
}

// Cycle the sample doc to the next backend (pill click while in sample mode).
export function cycleSampleFormat(
  open: (text: string, format: SampleFormat) => void,
): void {
  if (!sampleMode) return;
  const next =
    SAMPLE_ORDER[(SAMPLE_ORDER.indexOf(sampleFormat) + 1) % SAMPLE_ORDER.length];
  loadSample(next, open);
}
