# confy web UI built-in sample overhaul

## Context

The built-in demo document (`web/samples.ts`, `web/schema-sample.json`) is the first thing every
web/touch/VS Code-webview user sees. Today it's a flat 6-table/~20-leaf tree, JSON has zero
comments (so `comment_advisory`/JSONC-upgrade never demos), the schema only exercises 2 of the
constraint kinds the engine supports, there's no repeated filter term, no multi-level nesting, no
format-exclusive notations (dotted keys, AoT, radix ints, exponent floats, YAML flow/block-scalar/
opaque nodes), and no resource links. Task: rewrite the sample content end-to-end so it teaches
every current feature (filter highlight, type filter, schema violations/hints/descendant-warning,
kind-switch, drag/paste, remark, convert warnings, comment advisory) through a self-explanatory
tree, in a lightly comedic register, using **comments** (not narrated values) as the teaching
voice — decided at the prior turn as:

- **E-1 = B**: one shared backbone tree across TOML/JSON/YAML (so the pill-cycle stays a
  continuous "same doc, different clothes" demo) plus one per-format `showcase` branch holding
  that format's exclusive notations.
- **E-2 = fix**: investigate the comment-advisory/schema-notice collision flagged in
  `docs/superpowers/plans/2026-08-28-comment-advisory-followup-issues.md` §2. **Finding, already
  verified this session:** already fixed — `dispatch.rs` peeks `pending_schema_fetch` (`self
  .pending_schema_fetch.clone()`, `dispatch.rs:329,396`) instead of draining it, so a `SetLang`
  + `SetHostNotice` double-dispatch at open no longer loses the fetch request. No source fix is
  required; this plan's only "fix" obligation is Step 8's verification that the new JSON sample
  (comment + `$schema` hint together, the exact repro shape from that memo) shows both the
  comment-advisory underline and successful schema validation with no dropped notice.
- **E-3 = default-collapsed**: at least one branch (the schema-violation one used to demo
  `has_descendant_violation`) ships collapsed by default.
- **D = c**: resource links appear both as real tree data (a `[links]`/`links:` branch, plain
  text, not clickable — trees never linkify) **and** in the About panel (clickable), which
  requires widening the About-panel linkifier from first-match-only to every URL and adding the
  VS Marketplace / Open VSX / MS Store / live-demo links to `ABOUT_TEXT`/`ABOUT_TEXT_ZH_TW`.

## Approach

### 1. Design the shared backbone tree (content, not code yet)

One logical tree, expressed identically (same keys/values, same nesting) in all three formats.
Sections, each a **table/object/mapping**, teaching one feature via comments:

- `about` — `name`, `pitch`, `version` (`${APP_VERSION}` interpolation, unchanged mechanism),
  `homepage` (repo URL), `lossless`, `round_trip`. One TOML/YAML comment line above the section
  explaining lossless round-tripping; JSON gets the equivalent as a `//` comment (this is where
  JSON's advisory first fires — see Step 3).
- `basics` — unchanged in spirit to today's (`select`, `add_child`, `undo_redo`), but trim the
  joke-value strings that read as prose; move any remaining teaching text into a comment above
  each key, keep values short and real-looking (e.g. `select = "click"` becomes a real enum-ish
  string, not a run-on sentence). Add one new key `filter_me = "banana"` here.
- `servers` — **new**, 3 levels deep, the nesting/drag/paste/multi-select demo:
  ```
  servers.primary.host = "banana.example.com"
  servers.primary.port = 8080
  servers.primary.tags = ["prod", "banana"]
  servers.replica.host = "banana-replica.example.com"
  servers.replica.port = 8081
  servers.replica.tags = ["standby", "banana"]
  ```
  `primary`/`replica` are structurally identical siblings (drag one onto the other's parent to
  reparent; multi-select both `host` leaves with ⌘-click). The word **"banana"** is deliberately
  repeated across `about.pitch` is NOT reused — keep "banana" *only* inside `servers.*` and
  `basics.filter_me` (4 hits: `filter_me`'s value, `primary.host`, `primary.tags[1]`,
  `replica.host`, `replica.tags[1]` — 5 hits, spanning two branches) so hitting `/banana` visibly
  highlights matches across multiple rows/branches at once — the concrete filter-highlight demo.
  A comment above `servers` explains this: "// try / then type banana — matches light up
  everywhere, not just here".
- `types` — **new**, the type-filter (`f`) and kind-switch (`K`) demo, one leaf per interesting
  scalar shape valid in *that* format (values differ slightly per format since not every scalar
  kind exists in every format — this is fine, it's still the "same section" structurally):
  - all formats: `flag: true`, `nothing` (`null` in JSON/YAML only — TOML has no null; TOML's
    `types` section omits this key entirely, noted in a comment: `# TOML has no null — see JSON
    or YAML for this one`),
  - a plain string, a float, an integer — identical across formats.
  Comment: "// press f to open the type filter, tick Bool + Null to isolate these".
- `schema` — kept, values kept schema-invalid on purpose (unchanged spirit), but now also holds
  a **nested, collapsed-by-default** sub-table `schema.advanced` (see Step 3/E-3) whose one leaf
  is *also* schema-invalid, so the `schema` row itself shows `has_descendant_violation` while
  collapsed.
- `links` — **new**, plain-text resource list (not clickable — see D). One key per resource,
  values are bare URLs:
  ```
  repo = "https://github.com/superyngo/confy"
  live_demo = "https://confy.turkeyang.net/"
  vscode_marketplace = "https://marketplace.visualstudio.com/items?itemName=wenanlin.confy-vscode"
  open_vsx = "https://open-vsx.org/extension/wenanlin/confy-vscode"
  ms_store = "https://apps.microsoft.com/detail/9PLCJGQ3C654"
  ```
  Comment above the section: "// can't click these in the tree — paste in a browser, or see the
  About panel (? then About) for clickable links".
- `showcase` — **new**, per-format only (Step 2), the exclusive-notation demo.

Every section keeps a one-line comment above its `[header]`/`"key": {`/`key:` explaining what to
try, written in the "light and funny" register the user asked for (short, dry, not cutesy walls
of text) — this is the "comments as teaching voice" mechanism replacing the old prose-in-values
style. Do not literally guess exact final wording beyond the examples above; the implementer picks
the exact phrasing but MUST keep it: (a) one line, (b) imperative or "// try X", (c) references a
real key binding from `crates/confy-tui/src/tui/keys.rs`/`web/help-content.ts` where applicable.

### 2. Per-format `showcase` branch (format-exclusive notations)

Structurally the same key name (`showcase`) in every format, but content is format-specific —
this is the one section allowed to diverge in shape between formats, per E-1's decision.

**TOML** (`showcase` table):
```toml
[showcase]
dotted.nested.value = "dotted keys nest into synthetic tables — try K on this row"
hex = 0xFF
octal = 0o17
binary = 0b1010
sci = 6.02e23
inf = inf
created = 1979-05-27T07:32:00Z

[[showcase.log]]
event = "started"

[[showcase.log]]
event = "finished"
```
(`[[showcase.log]]` demos array-of-tables; `dotted.nested.value` demos `[T/D]`; `hex`/`octal`/
`binary` demo integer radix `K` targets; `sci`/`inf` demo float exponent/`inf` `K` targets;
`created` demos TOML datetime, which Step 5's `C` convert to JSON/YAML must warn about — this is
intentional, see Step 5.)

**JSON** (`showcase` object) — JSON has no radix/AoT/dotted-key notations, so this branch
instead demos JSONC comments + `null` + nested `$defs`-backed object (ties to Step 3's schema
work) + a multiline-formatted array to show `K`'s Inline↔Multiline toggle:
```json
"showcase": {
  // JSON's own party trick: comments. Yes, in a .json file. We know.
  "empty": null,
  "sci": 6.02e23,
  "log": [
    { "event": "started" },
    { "event": "finished" }
  ]
}
```
**YAML** (`showcase` mapping) — the flow/block + literal/folded + opaque demo:
```yaml
showcase:
  flow_seq: [a, b, c]          # [A/F] — try K to convert to block
  block_map:
    nested: true
  literal: |
    line one
    line two
  folded: >
    this reflows
    into one line
  # anchors/aliases/tags render read-only — this row can't be edited, only viewed
  pinned: &pin "confy"
  alias_of_pinned: *pin
```
(`pinned`/`alias_of_pinned` demo YAML's read-only opaque-node handling — confirm via
`crates/confy-core/src/model/yaml/project.rs` that `&anchor`/`*alias` still project as `[opaq ]`
read-only nodes with no further change needed, since this is existing, unmodified behavior being
*exercised* by new sample content, not new engine behavior.)

### 3. Update `web/schema-sample.json` — richer constraint coverage, `$defs`/`$ref`, collapsed violation

Replace the file's contents (currently only `schema.editor` enum + `schema.poll_ms` bounds) with:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$defs": {
    "editorName": {
      "description": "Your text editor of choice",
      "enum": ["vim", "nano", "helix", "confy", 0]
    }
  },
  "type": "object",
  "properties": {
    "schema": {
      "type": "object",
      "properties": {
        "editor": { "$ref": "#/$defs/editorName" },
        "poll_ms": {
          "type": "integer",
          "minimum": 100,
          "maximum": 2000,
          "multipleOf": 5,
          "description": "How often (ms) confy checks for external file changes"
        },
        "advanced": {
          "type": "object",
          "properties": {
            "retry_pattern": {
              "type": "string",
              "pattern": "^[0-9]+(,[0-9]+)*$",
              "description": "Comma-separated retry delays in seconds"
            }
          }
        }
      }
    }
  }
}
```
- `schema.editor` now resolves through `$ref` (`hints_edit.rs`'s `deref`/same-document `$ref`
  path, already implemented) — demos `$defs`+`$ref` resolution and a schema-level `description`
  surfaced by `resolve_schema_info`.
- `schema.poll_ms` keeps its existing bounded-numeric demo, gains a `description` line.
- `schema.advanced.retry_pattern` is new: a `pattern`-constrained string, demoing
  `schema_type_line`'s `Pattern:` output. Give it a schema-invalid seed value in the sample tree
  (Step 1/4) so `schema.advanced` shows a violation, and — because `schema.advanced` ships
  **collapsed by default** (Step 4) — the collapsed `schema` and `schema.advanced` rows both show
  `has_descendant_violation`, satisfying E-3.

`schema-sample.json` stays a single file (no new asset paths), so `web/assemble-dist.mjs:24`,
`web/cf-build.sh:39`, `web/sw.js:20` need **no changes** — confirm this by re-checking those three
lines reference only the filename, not its contents, after the edit.

### 4. Rewrite `web/samples.ts`

- Delete the entire existing file-header comment block (lines 1–20) explaining the now-obsolete
  "JSON must stay comment-free" constraint; replace with a comment describing the new shared-
  backbone + per-format-showcase structure (mirrors this plan's Step 1/2 summary in ~6–8 lines,
  written to match the file's existing terse technical-comment style, not prose).
- Rewrite the three `SAMPLES.toml`/`.json`/`.yaml` template literals per Steps 1–2. Escape any
  literal `` ` `` or `${` that appears in comment text (none currently planned, but the
  implementer must check `sci = 6.02e23`-style content introduces no accidental `${`).
  - **JSON gets comments this time** (`//` on the `showcase` section per Step 2, plus a short
    `//` comment above `about` mirroring TOML/YAML's, e.g. `// yes, this is valid JSON — see
    "showcase" below`). This is the deliberate `comment_advisory` trigger E-2 relies on for
    verification — every JSON sample open now has `had_comments_at_open() == true` and every
    comment row gets `comment_advisory: Some(...)`.
  - Give `schema.advanced.retry_pattern` a schema-invalid seed, e.g. `"abc"` (fails the `pattern`
    regex), across all three formats.
  - Wrap `schema.advanced` (and its YAML/TOML/JSON equivalents) so it renders **collapsed by
    default** — confirm the mechanism: check `expanded: HashSet<Path>` default state in
    `crates/confy-core/src/session/session.rs` (the "empty root = default state" convention noted
    in this repo's Navigation docs) to confirm nothing needs to happen at sample-authoring time
    for a *specific* branch to start collapsed (i.e. confirm collapse-by-default is already every
    non-root branch's initial state, so `schema.advanced` is already collapsed with zero extra
    code — only `expand_collapse_level`/explicit expansion changes that). **If instead some other
    branches are pre-expanded by a startup `Intent`/host call** (grep `web/ui.ts`/
    `web/touch/app.ts` for any `Expand`/`SetExpanded` dispatch fired right after
    `loadSample`/`openSample`), confirm none targets `schema` or `schema.advanced`, and if one
    does, exclude those paths so `schema.advanced` stays collapsed. This is a verification
    sub-step, not new code, unless the grep finds an existing blanket auto-expand — in that case
    scope it to exclude `["schema", "advanced"]`.
- Update the `SAMPLE_ORDER`/pill-cycle logic: unchanged, still `["toml", "json", "yaml"]`.
- Keep `SCHEMA_SAMPLE_URL` computation unchanged (`web/samples.ts` lines ~28–33 today).

### 5. Verify Step 2's TOML `created` datetime produces a real, visible convert warning

Per `crates/confy-core/src/model/convert.rs`'s documented loss policy ("TOML datetime→JSON/YAML
... warn"), converting the TOML sample (`C` in the UI) to JSON or YAML must surface a warning
mentioning the datetime path. No code change — this step is pure verification (Step 8) that the
new sample content actually exercises this existing, unmodified warning path, since today's
sample has no datetime and never has.

### 6. Widen the About-panel linkifier + add resource links to `ABOUT_TEXT`

- `web/help-content.ts:158-161`: `escapeHtml(body).replace(/(https:\/\/\S+)/, ...)` only
  linkifies the **first** URL in the About body (`String.replace` without a global flag). Change
  the regex to `/(https:\/\/\S+)/g` so every URL in the (about-to-be-longer) About body becomes a
  clickable `<a target="_blank" rel="noopener noreferrer">`. Trailing punctuation is not currently
  a concern (`ABOUT_TEXT`'s existing GitHub line has no trailing punctuation after the URL); keep
  it that way in the new lines too (each URL line ends with `\n`, not `.` or `)`).
- `crates/confy-core/src/session/state.rs`: extend `ABOUT_TEXT` (English) and `ABOUT_TEXT_ZH_TW`
  with new lines after the existing `GitHub:` line and before the blank line + `Privacy:`
  paragraph:
  ```
  "Live demo: https://confy.turkeyang.net/\n",
  "VS Code:   https://marketplace.visualstudio.com/items?itemName=wenanlin.confy-vscode\n",
  "Open VSX:  https://open-vsx.org/extension/wenanlin/confy-vscode\n",
  "MS Store:  https://apps.microsoft.com/detail/9PLCJGQ3C654\n",
  ```
  zh-TW mirrors with translated labels matching that file's existing label style (`即時展示：`,
  `VS Code：`, `Open VSX：`, `MS Store：`), same URLs. Keep the `\n` (co)location and label
  column-alignment style (`concat!` literal strings, colon-padded) consistent with the existing
  `Author:`/`License:`/`Copyright:`/`GitHub:` block — pad new labels to the same column width
  (`Author:    ` is 4-space-padded to 11 chars incl. colon+spaces; match that for `Live demo:`,
  `VS Code:`, `Open VSX:`, `MS Store:`).
- No signature change to `about_text(lang)` — same two `&'static str` consts, same call sites
  (`crates/confy-ffi/src/lib.rs`, `crates/confy-tui/src/tui/overlay_help.rs`, `web/ui.ts:654`
  via `session.aboutText()`). Confirm no other caller assumes a fixed line count in `ABOUT_TEXT`
  (grep for `ABOUT_TEXT` usages beyond `about_text()` itself; expected: none, since every
  consumer treats it as an opaque string).

### 7. README — add the missing MS Store link

`README.md`'s `## Desktop app` section (locate via existing `.dmg`/`nsis`/release-asset wording,
not yet read verbatim in this session — the implementer must open `README.md`'s Desktop app
section, find where release links live, e.g. near line ~15-17's existing
`[Releases](https://github.com/superyngo/confy/releases)` reference) and add a line:
`- Windows: also listed on the [Microsoft Store](https://apps.microsoft.com/detail/9PLCJGQ3C654).`
Place it adjacent to the existing Windows/desktop release-asset bullet, not inside the VS Code
extension section (Step 6's `## VS Code extension` section already lists Marketplace/Open VSX and
should not be touched beyond what Step 6 covers in core, since README's VS Code section already
has both those links per the grep in this session).

## Critical files & anchors

- `web/samples.ts` (whole file) — full rewrite per Steps 1, 2, 4.
- `web/schema-sample.json` (whole file) — full rewrite per Step 3.
- `web/help-content.ts:158-161` — `helpAboutHTML`'s single-URL regex, widen to `/g` (Step 6).
- `crates/confy-core/src/session/state.rs:84-114` — `ABOUT_TEXT`/`ABOUT_TEXT_ZH_TW` consts, add
  4 lines each (Step 6).
- `README.md` (Desktop app section, exact line TBD by implementer read) — add MS Store bullet
  (Step 7).

## Verification

Build and manually exercise every claimed demo (no existing automated test covers sample
*content*, only `web/sample-strict-json.spec.mjs`'s `strict_json`-wiring mechanism, which is
untouched by this plan and must still pass):

1. `cd web && npm run typecheck && npm run build` — clean compile of the rewritten `samples.ts`.
2. `cd web && npm test` — existing plain-Node spec suite, incl. `sample-strict-json.spec.mjs`,
   still green (confirms `isPlainJson`/`strict_json` wiring is untouched and still fires for the
   new JSON sample content).
3. `cargo test -p confy-core` — confirms `ABOUT_TEXT`/`ABOUT_TEXT_ZH_TW` edits don't break any
   existing string-shape assertion (grep first for tests asserting on `ABOUT_TEXT` literal
   content/line count before editing; adjust any that hardcode the old text).
4. Serve `web/dist` locally (`cd web && node serve.mjs` or equivalent existing dev-serve script)
   and in a browser:
   - Load default (TOML) sample → confirm `[schema]` shows `editor`/`poll_ms`/`advanced` rows,
     `schema` and `schema.advanced` rows render **collapsed**, both carry the
     `has_descendant_violation` visual (dot/marker per `render.ts`'s existing indicator).
   - Press `/`, type `banana` → confirm ≥4 highlighted matches across `basics`/`servers`.
   - Press `f`, tick Bool+Null → confirm only `types.flag`/`types.nothing` remain (TOML: `nothing`
     absent per Step 1, confirm the type-filter row count reflects that).
   - Cycle the format pill to JSON → confirm: (a) the `showcase`/`about` `//` comments render
     with the red wavy `comment-advisory` underline, (b) a one-shot
     `web.host.json-comments-detected` toast fires on this load, (c) the `schema.editor`/
     `schema.poll_ms` schema violations/hints still populate (Detail panel shows `$ref`-resolved
     `description` for `editor`, `Pattern:`/`description` for `advanced.retry_pattern`) —
     this is the concrete E-2 regression check: comment-advisory toast and schema
     validation/hints must **both** be present simultaneously, confirming no notice/validation
     drop.
   - Cycle to YAML → confirm `pinned`/`alias_of_pinned` render read-only (no edit affordance),
     `flow_seq` shows `[A/F]` kind badge and `K` offers a block-conversion option.
   - Back on TOML, press `C` (convert) to JSON → confirm the warning list includes a datetime-loss
     warning naming `showcase.created`.
   - Open `?` → About tab → confirm all 4 new resource lines render as separate clickable links
     (inspect DOM: multiple `<a target="_blank">` elements, not just one).
5. `cd editors/vscode && npm run integration-test` — confirms the webview-embedded bundle (same
   `web/dist`) still boots and the extension's own About/Help surfaces (which reuse
   `help-content.ts`) aren't broken by the regex/content changes.

## Assumptions & contingencies

- **Collapse-by-default mechanism (Step 4).** Assumed: every non-root branch starts collapsed
  unless explicitly expanded by host startup code, so wrapping the invalid seed inside
  `schema.advanced` is suffient with no extra `Expand`/`SetExpanded` dispatch. If the grep in
  Step 4 finds a blanket "expand everything N levels" call fired after `loadSample`, the
  implementer must exclude `schema`/`schema.advanced` from that expansion rather than defeat the
  general mechanism (scoped exclusion, not a mechanism removal).
- **README Desktop-app section exact anchor line** is not yet read verbatim this session (only
  the VS Code section was). The implementer must locate the actual bullet list before inserting
  Step 7's line; if no such per-platform bullet list exists (e.g. Desktop app section is prose,
  not bullets), add the MS Store link as a new short sentence immediately after the existing
  Releases-link sentence, matching that section's actual prose style rather than inventing a list
  format.
- **`ABOUT_TEXT` test coverage.** Assumed no existing test hardcodes the const's exact line count
  or full text (typical for a static banner string); Step "Verification" #3 requires confirming
  this before considering Step 6 done — if such a test exists, update its expected string rather
  than skip/weaken the assertion.
