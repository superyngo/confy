// Built-in demo doc + sample-mode state, shared by the desktop (ui.ts) and
// touch (touch/app.ts) orchestrators so both surfaces boot the same tree.
//
// One backbone tree across all three formats (about/basics/servers/types/
// schema/links — identical keys/values) plus a per-format `showcase` branch
// of that format's exclusive notations (TOML dotted/AoT/radix/datetime; JSON
// comments/null/multiline array; YAML flow/block-scalar/anchor). Comments,
// not narrated values, are the teaching voice. The JSON sample's `//`
// comments are deliberate: JSONC is legal, fires the host's comment
// advisory, and still resolves the `$schema` hint (schema/hints.rs detects
// through the JSONC-aware parser). `[schema]` is pre-wired to
// schema-sample.json; `schema.advanced` ships collapsed on purpose so the
// collapsed parent demos has_descendant_violation. The pill cycles these
// while the doc is the unsaved sample (`sampleMode`); opening or saving a
// real file leaves sample mode and freezes it.

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

# the demo tour — every section teaches one trick
[about]
# lossless: untouched bytes round-trip byte-for-byte, comments included
name = "confy"
pitch = "Three config dialects, one tidy tree 🌳"
version = "${APP_VERSION}"
homepage = "https://github.com/superyngo/confy"
lossless = true
round_trip = true

[basics]
# click = select · shift-click = range · ⌘-click = add to selection
select = "click"
# hover a branch and hit ＋ to add a child
add_child = true
# z undoes, y redoes
undo_redo = true
# values are searchable too — try / banana
filter_me = "banana"

# two identical siblings: drag the ⠿ grip to reparent · ⌘-click both hosts to multi-select
# try / then type banana — matches light up everywhere, not just here
[servers]

[servers.primary]
host = "banana.example.com"
port = 8080
tags = ["prod", "banana"]

[servers.replica]
host = "banana-replica.example.com"
port = 8081
tags = ["standby", "banana"]

[types]
# press f for the type filter — tick Bool to isolate flag
flag = true
label = "plain"
ratio = 0.75
count = 42
# TOML has no null — see JSON or YAML for this one

[schema]
# these values break the schema on purpose — watch the violation markers
editor = "sublime"    # not in the enum — edit this row to open the picker (pick 0 for a type-change confirm)
poll_ms = 253          # multiple of 5, 100-2000 — try ← / → to see it snap

# advanced ships collapsed — schema shows its violation marker anyway
[schema.advanced]
retry_pattern = "abc"    # the schema wants digits and commas — this fails on purpose

[links]
# can't click these in the tree — paste into a browser, or open ? → About for clickable links
repo = "https://github.com/superyngo/confy"
live_demo = "https://confy.turkeyang.net/"
vscode_marketplace = "https://marketplace.visualstudio.com/items?itemName=wenanlin.confy-vscode"
open_vsx = "https://open-vsx.org/extension/wenanlin/confy-vscode"
ms_store = "https://apps.microsoft.com/detail/9PLCJGQ3C654"

[showcase]
# TOML-only notations — try K on each row
dotted.nested.value = "dotted keys nest into synthetic tables"
hex = 0xFF
octal = 0o17
binary = 0b1010
sci = 6.02e23
inf = inf
created = 1979-05-27T07:32:00Z    # a datetime — converting (C) to JSON/YAML warns about it

[[showcase.log]]
event = "started"

[[showcase.log]]
event = "finished"
`,
  json: `{
  "$schema": "${SCHEMA_SAMPLE_URL}",
  // yes, this is valid JSON — confy reads the comments and flags them for you
  "about": {
    // lossless: untouched bytes round-trip byte-for-byte, comments included
    "name": "confy",
    "pitch": "Three config dialects, one tidy tree 🌳",
    "version": "${APP_VERSION}",
    "homepage": "https://github.com/superyngo/confy",
    "lossless": true,
    "round_trip": true
  },
  "basics": {
    // click = select · shift-click = range · ⌘-click = add to selection
    "select": "click",
    // hover a branch and hit ＋ to add a child
    "add_child": true,
    // z undoes, y redoes
    "undo_redo": true,
    // values are searchable too — try / banana
    "filter_me": "banana"
  },
  "servers": {
    // two identical siblings: drag the ⠿ grip to reparent · ⌘-click both hosts to multi-select
    // try / then type banana — matches light up everywhere, not just here
    "primary": {
      "host": "banana.example.com",
      "port": 8080,
      "tags": ["prod", "banana"]
    },
    "replica": {
      "host": "banana-replica.example.com",
      "port": 8081,
      "tags": ["standby", "banana"]
    }
  },
  "types": {
    // press f for the type filter — tick Bool + Null to isolate these two
    "flag": true,
    "nothing": null,
    "label": "plain",
    "ratio": 0.75,
    "count": 42
  },
  "schema": {
    // these values break the schema on purpose — watch the violation markers
    "editor": "sublime",
    "poll_ms": 253,
    // advanced ships collapsed — schema shows its violation marker anyway
    "advanced": {
      // the schema wants digits and commas — this fails on purpose
      "retry_pattern": "abc"
    }
  },
  "links": {
    // can't click these in the tree — paste into a browser, or open ? → About for clickable links
    "repo": "https://github.com/superyngo/confy",
    "live_demo": "https://confy.turkeyang.net/",
    "vscode_marketplace": "https://marketplace.visualstudio.com/items?itemName=wenanlin.confy-vscode",
    "open_vsx": "https://open-vsx.org/extension/wenanlin/confy-vscode",
    "ms_store": "https://apps.microsoft.com/detail/9PLCJGQ3C654"
  },
  "showcase": {
    // JSON's own party trick: comments. Yes, in a .json file. We know.
    "empty": null,
    "sci": 6.02e23,
    "log": [
      { "event": "started" },
      { "event": "finished" }
    ]
  }
}
`,
  yaml: `# yaml-language-server: $schema=${SCHEMA_SAMPLE_URL}

# the demo tour — every section teaches one trick
about:
  # lossless: untouched bytes round-trip byte-for-byte, comments included
  name: confy
  pitch: Three config dialects, one tidy tree 🌳
  version: "${APP_VERSION}"
  homepage: https://github.com/superyngo/confy
  lossless: true
  round_trip: true

basics:
  # click = select · shift-click = range · ⌘-click = add to selection
  select: click
  # hover a branch and hit ＋ to add a child
  add_child: true
  # z undoes, y redoes
  undo_redo: true
  # values are searchable too — try / banana
  filter_me: banana

# two identical siblings: drag the ⠿ grip to reparent · ⌘-click both hosts to multi-select
# try / then type banana — matches light up everywhere, not just here
servers:
  primary:
    host: banana.example.com
    port: 8080
    tags: [prod, banana]
  replica:
    host: banana-replica.example.com
    port: 8081
    tags: [standby, banana]

types:
  # press f for the type filter — tick Bool + Null to isolate these two
  flag: true
  nothing: null
  label: plain
  ratio: 0.75
  count: 42

schema:
  # these values break the schema on purpose — watch the violation markers
  editor: sublime
  poll_ms: 253
  # advanced ships collapsed — schema shows its violation marker anyway
  advanced:
    retry_pattern: "abc"    # the schema wants digits and commas — this fails on purpose

links:
  # can't click these in the tree — paste into a browser, or open ? → About for clickable links
  repo: https://github.com/superyngo/confy
  live_demo: https://confy.turkeyang.net/
  vscode_marketplace: https://marketplace.visualstudio.com/items?itemName=wenanlin.confy-vscode
  open_vsx: https://open-vsx.org/extension/wenanlin/confy-vscode
  ms_store: https://apps.microsoft.com/detail/9PLCJGQ3C654

showcase:
  # YAML-only notations — try K on flow_seq; the last two rows are read-only
  flow_seq: [a, b, c]          # a flow sequence — try K to convert it to block
  block_map:
    nested: true
  literal: |
    line one
    line two
  folded: >
    this reflows
    into one line
  # anchors and aliases render read-only — these rows can't be edited, only viewed
  pinned: &pin "confy"
  alias_of_pinned: *pin
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
