// Built-in demo doc + sample-mode state, shared by the desktop (ui.ts) and
// touch (touch/app.ts) orchestrators so both surfaces boot the same tree.
//
// All three samples carry the *same* tree (identical keys/values). The
// `[schema]` branch is pre-wired to `schema-sample.json` (served alongside
// this bundle) via each format's own hint convention — TOML `#:schema`,
// JSON root `$schema`, YAML's `yaml-language-server` modeline — so opening
// any sample format immediately demos live constrained editing (`editor`'s
// off-enum value opens the enum picker, whose `0` option is a deliberately
// mixed-type member — picking it demos the type-change confirmation) and
// (`editor` and `poll_ms` both start out schema-invalid on purpose). JSON
// stays fully comment-free, unlike TOML/YAML (which keep two short inline
// explainer comments on the `[schema]` rows): `detect_json`'s hint scan
// requires strict JSON, so a single stray `//` comment anywhere in the
// document would silently block detection — this is also why the former
// leading-comment welcome banner and the `lossless` field's trailing note
// are now real tree data (`[welcome]`, `about.round_trip`) instead of
// comments, keeping the tree genuinely identical across all three formats.
// The pill cycles these while the doc is the unsaved sample (`sampleMode`);
// opening or saving a real file leaves sample mode and freezes it.

// Workspace version stamped in at build time (see `build.mjs` `define`); falls
// back to "dev" when the bundle is loaded without that define (e.g. raw serve).
declare const __APP_VERSION__: string;
const APP_VERSION =
  typeof __APP_VERSION__ !== "undefined" ? __APP_VERSION__ : "dev";

// Absolute URL to the sample JSON Schema, computed once at module load so it
// resolves correctly under any origin/subpath (local dev server, Cloudflare
// Pages, Tauri's bundled asset origin). Must be an explicit http(s) URL
// (never a relative "Local" hint) — the sample has no backing file, so a
// Local hint's sibling-directory resolution has nothing to resolve against.
const SCHEMA_SAMPLE_URL = new URL("schema-sample.json", location.href).href;

export type SampleFormat = "toml" | "json" | "yaml";

export const SAMPLES: Record<SampleFormat, string> = {
  toml: `#:schema ${SCHEMA_SAMPLE_URL}

[welcome]
greeting = "👋 Welcome to confy — a lossless editor for TOML · JSON · YAML"
tips = "Click a row to select · drag the ⠿ grip to reparent · ⌘S to save"

[about]
name = "confy"
pitch = "Three config dialects, one tidy tree 🌳"
version = "${APP_VERSION}"
lossless = true
round_trip = "untouched bytes round-trip byte-for-byte"

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

[schema]
editor = "sublime"    # not in the schema's enum — edit this row to see the picker (pick "0" for a type-change confirm)
poll_ms = 253          # multiple of 5, 100-2000 — try ← / → to see it snap
`,
  json: `{
  "$schema": "${SCHEMA_SAMPLE_URL}",
  "welcome": {
    "greeting": "👋 Welcome to confy — a lossless editor for TOML · JSON · YAML",
    "tips": "Click a row to select · drag the ⠿ grip to reparent · ⌘S to save"
  },
  "about": {
    "name": "confy",
    "pitch": "Three config dialects, one tidy tree 🌳",
    "version": "${APP_VERSION}",
    "lossless": true,
    "round_trip": "untouched bytes round-trip byte-for-byte"
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
  },
  "schema": {
    "editor": "sublime",
    "poll_ms": 253
  }
}
`,
  yaml: `# yaml-language-server: $schema=${SCHEMA_SAMPLE_URL}

welcome:
  greeting: 👋 Welcome to confy — a lossless editor for TOML · JSON · YAML
  tips: Click a row to select · drag the ⠿ grip to reparent · ⌘S to save

about:
  name: confy
  pitch: Three config dialects, one tidy tree 🌳
  version: "${APP_VERSION}"
  lossless: true
  round_trip: untouched bytes round-trip byte-for-byte

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

schema:
  editor: sublime        # not in the schema's enum — edit this row to see the picker (pick "0" for a type-change confirm)
  poll_ms: 253            # multiple of 5, 100-2000 — try ← / → to see it snap
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
